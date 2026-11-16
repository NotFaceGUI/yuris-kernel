import UIKit

/// 游戏画面:在 CPU 帧阶段用 `CALayer.contents` 贴 `CGImage`。
///
/// 接 Metal 时把 `layerClass` 改成 `CAMetalLayer.self`,在 `viewDidAppear`
/// 里把 `frameView.layer as! CAMetalLayer` 交给 `YuKernel.attachLayer(...)`。
final class FrameView: UIView {
    override class var layerClass: AnyClass { CALayer.self }

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .black
        isOpaque = true
        layer.backgroundColor = UIColor.black.cgColor
        layer.contentsGravity = .resizeAspect
        layer.magnificationFilter = .linear
        layer.minificationFilter = .linear
        layer.contentsScale = UIScreen.main.scale
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }
}

final class GameViewController: UIViewController {
    private let dataRoot: URL
    private let saveRoot: URL
    private let logFile: URL
    private let onExit: () -> Void

    private let frameView = FrameView()
    private var displayLink: CADisplayLink?
    private var frameData = Data()
    private var gameSize = CGSize.zero
    private var didStart = false
    private var dialogPollCounter = 0
    private var presentedDialogs = Set<UInt64>()

    init(dataRoot: URL, saveRoot: URL, logFile: URL, onExit: @escaping () -> Void) {
        self.dataRoot = dataRoot
        self.saveRoot = saveRoot
        self.logFile = logFile
        self.onExit = onExit
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        title = "游戏"
        navigationItem.leftBarButtonItem = UIBarButtonItem(
            title: "退出",
            style: .plain,
            target: self,
            action: #selector(confirmExit)
        )
        frameView.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(frameView)
        NSLayoutConstraint.activate([
            frameView.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
            frameView.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
            frameView.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            frameView.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor),
        ])
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(willResignActive),
            name: UIApplication.willResignActiveNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(didBecomeActive),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        guard !didStart else { return }
        didStart = true
        do {
            _ = try YuKernel.initialize(dataRoot: dataRoot, saveRoot: saveRoot, logFile: logFile)
        } catch {
            showFatal(error.localizedDescription)
            return
        }
        UIApplication.shared.isIdleTimerDisabled = true
        let link = CADisplayLink(target: self, selector: #selector(step(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 30, maximum: 60, preferred: 60)
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        if isMovingFromParent || isBeingDismissed {
            stop()
            _ = YuKernel.shutdown()
        }
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    // MARK: - 帧循环

    @objc private func step(_ link: CADisplayLink) {
        let code = YuKernel.tick()
        if code == 1 {
            finish(title: "游戏结束", message: "主脚本已走完。")
            return
        }
        if code == 4 {
            stop()
            onExit()
            return
        }
        if code < 0 {
            finish(
                title: "运行失败",
                message: YuKernel.lastError.isEmpty ? "tick 返回 \(code)" : YuKernel.lastError
            )
            return
        }

        let (width, height) = YuKernel.frameSize()
        guard width > 0, height > 0 else { return }
        gameSize = CGSize(width: width, height: height)
        let pixels = width * height * 4
        if frameData.count != pixels {
            frameData = Data(count: pixels)
        }
        let copied = YuKernel.copyFrame(into: &frameData)
        if copied == pixels, let image = makeImage(width: width, height: height) {
            frameView.layer.contents = image
        }
        pollDialogs()
    }

    private func makeImage(width: Int, height: Int) -> CGImage? {
        guard let provider = CGDataProvider(data: frameData as CFData) else { return nil }
        return CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }

    private func stop() {
        displayLink?.invalidate()
        displayLink = nil
        UIApplication.shared.isIdleTimerDisabled = false
    }

    private func finish(title: String? = nil, message: String? = nil) {
        stop()
        _ = YuKernel.shutdown()
        guard let title, presentedViewController == nil else {
            onExit()
            return
        }
        let alert = UIAlertController(title: title, message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "返回", style: .default) { [weak self] _ in
            self?.onExit()
        })
        present(alert, animated: true)
    }

    private func showFatal(_ message: String) {
        stop()
        _ = YuKernel.shutdown()
        let alert = UIAlertController(title: "启动失败", message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "返回", style: .default) { [weak self] _ in
            self?.onExit()
        })
        // 可能从 DIALOG 的按钮回调里进来:那条 alert 正在 dismiss,
        // 立刻 present 会被 UIKit 丢掉;等一轮主循环,必要时先收掉旧 alert。
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            if let presented = self.presentedViewController {
                presented.dismiss(animated: false) {
                    self.present(alert, animated: true)
                }
            } else {
                self.present(alert, animated: true)
            }
        }
    }

    @objc private func confirmExit() {
        let alert = UIAlertController(
            title: "退出游戏",
            message: "未保存的进度会丢失。",
            preferredStyle: .alert
        )
        alert.addAction(UIAlertAction(title: "继续", style: .cancel))
        alert.addAction(UIAlertAction(title: "退出", style: .destructive) { [weak self] _ in
            self?.finish()
        })
        present(alert, animated: true)
    }

    // MARK: - 生命周期

    @objc private func willResignActive() {
        displayLink?.isPaused = true
        _ = YuKernel.suspend()
    }

    @objc private func didBecomeActive() {
        _ = YuKernel.resume()
        displayLink?.isPaused = false
    }

    // MARK: - 触摸

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let point = gamePoint(touches.first) else { return }
        _ = YuKernel.setCursor(x: Int(point.x), y: Int(point.y))
        _ = YuKernel.setInput("MOUSE_L", true)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let point = gamePoint(touches.first) else { return }
        _ = YuKernel.setCursor(x: Int(point.x), y: Int(point.y))
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        _ = YuKernel.setInput("MOUSE_L", false)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        _ = YuKernel.setInput("MOUSE_L", false)
    }

    /// 视图是 aspect-fit;把触点反算成游戏视口像素。
    private func gamePoint(_ touch: UITouch?) -> CGPoint? {
        guard let touch, gameSize.width > 0, gameSize.height > 0 else { return nil }
        let bounds = frameView.bounds.size
        guard bounds.width > 0, bounds.height > 0 else { return nil }
        let scale = min(bounds.width / gameSize.width, bounds.height / gameSize.height)
        guard scale > 0 else { return nil }
        let drawn = CGSize(width: gameSize.width * scale, height: gameSize.height * scale)
        let origin = CGPoint(
            x: (bounds.width - drawn.width) / 2,
            y: (bounds.height - drawn.height) / 2
        )
        let point = touch.location(in: frameView)
        let x = (point.x - origin.x) / scale
        let y = (point.y - origin.y) / scale
        guard x >= 0, y >= 0, x < gameSize.width, y < gameSize.height else { return nil }
        return CGPoint(x: x, y: y)
    }

    // MARK: - DIALOG

    private func pollDialogs() {
        dialogPollCounter += 1
        if dialogPollCounter < 15 { return }
        dialogPollCounter = 0

        let json = YuKernel.pendingDialogs
        if json.isEmpty {
            presentedDialogs.removeAll()
            return
        }
        guard
            let data = json.data(using: .utf8),
            let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
            let dialogs = root["dialogs"] as? [[String: Any]]
        else { return }

        let current = Set(dialogs.compactMap { ($0["id"] as? NSNumber)?.uint64Value })
        presentedDialogs.formIntersection(current)
        guard presentedViewController == nil else { return }

        for dialog in dialogs {
            let id = (dialog["id"] as? NSNumber)?.uint64Value ?? 0
            if presentedDialogs.contains(id) { continue }
            present(dialog: dialog, id: id)
            break
        }
    }

    private func present(dialog: [String: Any], id: UInt64) {
        let caption = dialog["caption"] as? String ?? ""
        let text = dialog["text"] as? String ?? ""
        let roles = dialog["buttons"] as? [String] ?? ["OK"]
        let isInput = dialog["input"] as? Bool ?? false

        let alert = UIAlertController(
            title: caption.isEmpty ? nil : caption,
            message: text,
            preferredStyle: .alert
        )

        var inputField: UITextField?
        if isInput {
            alert.addTextField { field in
                field.text = dialog["default_input"] as? String
                field.placeholder = "输入"
            }
            inputField = alert.textFields?.first
        }

        for (index, role) in roles.enumerated() {
            let title = role.isEmpty ? "按钮 \(index + 1)" : role
            let style: UIAlertAction.Style = role.uppercased() == "CANCEL" ? .cancel : .default
            alert.addAction(UIAlertAction(title: title, style: style) { [weak self] _ in
                guard let self else { return }
                let button = UInt8(index + 1)
                let input = isInput ? (inputField?.text ?? "") : nil
                let code = YuKernel.answerDialog(id: id, button: button, text: input)
                self.presentedDialogs.remove(id)
                if code != 0 {
                    self.showFatal(YuKernel.lastError)
                }
            })
        }

        presentedDialogs.insert(id)
        present(alert, animated: true)
    }
}
