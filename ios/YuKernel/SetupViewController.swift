import UIKit

/// 数据入口选择页。
///
/// iOS 没有 Android 那种任意目录浏览:游戏数据由用户经「文件」App /
/// Finder 放进 app 的 `Documents/`(Info.plist 开 `UIFileSharingEnabled` +
/// `LSSupportsOpeningDocumentsInPlace`),本页扫描并调用 `probe_root` 识别。
final class SetupViewController: UIViewController {
    private struct Entry {
        let name: String
        let url: URL
        let detail: String
    }

    private let table = UITableView(frame: .zero, style: .insetGrouped)
    private let hintLabel = UILabel()
    private let statusLabel = UILabel()
    private let versionLabel = UILabel()
    private var entries: [Entry] = []
    private var scanning = false

    private var documentsURL: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    }

    private var appSupportURL: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("YuKernel", isDirectory: true)
    }

    private var logsURL: URL {
        let base = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Logs", isDirectory: true)
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        title = "YuKernel"
        view.backgroundColor = .systemGroupedBackground
        navigationItem.rightBarButtonItem = UIBarButtonItem(
            title: "刷新",
            style: .plain,
            target: self,
            action: #selector(refresh)
        )

        hintLabel.text = """
        1. 用「文件」App 或 Finder 把游戏目录拷进「我的 iPhone → YuKernel」
        2. 回到本页点右上角「刷新」
        3. 选择识别到的数据入口(含 data.ypf / data/ / pac/*.ypf)
        """
        hintLabel.numberOfLines = 0
        hintLabel.font = .preferredFont(forTextStyle: .footnote)
        hintLabel.textColor = .secondaryLabel

        statusLabel.font = .preferredFont(forTextStyle: .subheadline)
        statusLabel.textColor = .secondaryLabel
        statusLabel.text = "尚未扫描"

        versionLabel.font = .preferredFont(forTextStyle: .caption2)
        versionLabel.textColor = .tertiaryLabel
        versionLabel.text = "\(YuKernel.version) · ABI \(YuKernel.abiVersion)"

        table.dataSource = self
        table.delegate = self
        table.register(UITableViewCell.self, forCellReuseIdentifier: "cell")
        table.backgroundColor = .clear
        // 让表格吃掉纵向剩余空间(标签按 intrinsic 高度)。
        table.setContentHuggingPriority(.defaultLow, for: .vertical)
        table.setContentCompressionResistancePriority(.defaultLow, for: .vertical)

        let stack = UIStackView(arrangedSubviews: [hintLabel, statusLabel, table, versionLabel])
        stack.axis = .vertical
        stack.alignment = .fill
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            stack.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
            stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -12),
        ])
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        scan()
    }

    @objc private func refresh() {
        scan()
    }

    private func scan() {
        guard !scanning else { return }
        scanning = true
        statusLabel.text = "扫描中…"
        let documents = documentsURL
        DispatchQueue.global(qos: .userInitiated).async {
            let found = Self.discover(in: documents)
            DispatchQueue.main.async {
                self.entries = found
                self.scanning = false
                self.statusLabel.text = found.isEmpty
                    ? "未找到数据入口:确认游戏目录已拷进 Documents/"
                    : "找到 \(found.count) 个数据入口"
                self.table.reloadData()
            }
        }
    }

    private static func discover(in documents: URL) -> [Entry] {
        var result: [Entry] = []

        func probe(_ url: URL, fallbackName: String) -> Entry? {
            let json = YuKernel.probeRoot(url.path)
            guard
                let data = json.data(using: .utf8),
                let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                object["ok"] as? Bool == true
            else { return nil }
            let kind = object["kind"] as? String ?? "?"
            let packs = (object["packs"] as? NSNumber)?.intValue ?? 0
            let updates = (object["updates"] as? NSNumber)?.intValue ?? 0
            let detail = "\(kind) · \(packs) 包 · \(updates) 增量层"
            return Entry(name: fallbackName, url: url, detail: detail)
        }

        // 用户也可能把游戏文件直接铺在 Documents 根目录。
        if let root = probe(documents, fallbackName: "Documents 根目录") {
            result.append(root)
        }

        let keys: [URLResourceKey] = [.isDirectoryKey, .isHiddenKey]
        let children = (try? FileManager.default.contentsOfDirectory(
            at: documents,
            includingPropertiesForKeys: keys,
            options: [.skipsHiddenFiles]
        )) ?? []

        for child in children.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
            let values = try? child.resourceValues(forKeys: Set(keys))
            guard values?.isDirectory == true else { continue }
            if let entry = probe(child, fallbackName: child.lastPathComponent) {
                result.append(entry)
            }
        }
        return result
    }

    private func start(_ entry: Entry) {
        let saveRoot = appSupportURL
            .appendingPathComponent("Saves", isDirectory: true)
            .appendingPathComponent(entry.name, isDirectory: true)
        let logFile = logsURL.appendingPathComponent("yu-kernel.log")
        do {
            try FileManager.default.createDirectory(at: saveRoot, withIntermediateDirectories: true)
            try FileManager.default.createDirectory(at: logsURL, withIntermediateDirectories: true)
        } catch {
            presentError("创建存档/日志目录失败: \(error.localizedDescription)")
            return
        }

        // 多 GB 游戏数据不该进 iCloud 备份。
        var game = entry.url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? game.setResourceValues(values)

        let controller = GameViewController(
            dataRoot: entry.url,
            saveRoot: saveRoot,
            logFile: logFile
        ) { [weak self] in
            self?.navigationController?.popToRootViewController(animated: true)
        }
        navigationController?.pushViewController(controller, animated: true)
    }

    private func presentError(_ message: String) {
        let alert = UIAlertController(title: "错误", message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "好", style: .default))
        present(alert, animated: true)
    }
}

extension SetupViewController: UITableViewDataSource, UITableViewDelegate {
    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        entries.count
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "cell", for: indexPath)
        let entry = entries[indexPath.row]
        var content = cell.defaultContentConfiguration()
        content.text = entry.name
        content.secondaryText = entry.detail
        cell.contentConfiguration = content
        cell.accessoryType = .disclosureIndicator
        return cell
    }

    func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        start(entries[indexPath.row])
    }
}
