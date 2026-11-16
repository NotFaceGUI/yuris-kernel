import Foundation
import QuartzCore

/// C ABI 的 Swift 包装。
///
/// 设计口径与 `android/NATIVE_API.md` 一致:C ABI 只加函数/JSON 键,
/// Swift 壳不复制引擎逻辑。字符串统一走「两次调用」:先问所需字节数,
/// 再给缓冲;不跨语言传递所有权。
enum YuKernelError: LocalizedError {
    case message(String)

    var errorDescription: String? {
        if case let .message(text) = self { return text }
        return nil
    }
}

enum YuKernel {
    static var version: String {
        guard let pointer = yu_ios_version() else { return "yu-host-ios ?" }
        return String(cString: pointer)
    }

    static var abiVersion: UInt32 {
        yu_ios_abi_version()
    }

    /// 启动会话。`backend` 骨架固定 `soft`;Metal 上屏待接。
    @discardableResult
    static func initialize(
        dataRoot: URL,
        saveRoot: URL,
        logFile: URL,
        backend: String = "soft",
        entry: String? = nil
    ) throws -> String {
        var payload: [String: Any] = [
            "data_root": dataRoot.path,
            "save_root": saveRoot.path,
            "backend": backend,
            "audio": true,
            "quiet": false,
            "log_level": "info",
            "log_file": logFile.path,
        ]
        if let entry, !entry.isEmpty {
            payload["entry"] = entry
        }
        let data = try JSONSerialization.data(withJSONObject: payload)
        guard let json = String(data: data, encoding: .utf8) else {
            throw YuKernelError.message("初始化 JSON 编码失败")
        }
        let code = json.withCString { yu_ios_init_json($0) }
        guard code == 0 else {
            throw YuKernelError.message(lastError.isEmpty ? "初始化失败(\(code))" : lastError)
        }
        return status
    }

    static var status: String { text { yu_ios_status($0, $1) } }
    static var lastError: String { text { yu_ios_last_error($0, $1) } }
    static var pendingDialogs: String { text { yu_ios_pending_dialogs($0, $1) } }
    static var fonts: String { text { yu_ios_list_fonts($0, $1) } }

    static func probeRoot(_ path: String) -> String {
        path.withCString { pointer in
            text { out, cap in yu_ios_probe_root(pointer, out, cap) }
        }
    }

    static func tick() -> Int32 { yu_ios_tick() }

    static func frameSize() -> (width: Int, height: Int) {
        var width: UInt32 = 0
        var height: UInt32 = 0
        yu_ios_frame_size(&width, &height)
        return (Int(width), Int(height))
    }

    /// 把当前帧 RGBA 拷进 `data`;返回写入字节数(-1 无帧 / -2 缓冲不足)。
    static func copyFrame(into data: inout Data) -> Int {
        let capacity = data.count
        guard capacity > 0 else { return -1 }
        let copied = data.withUnsafeMutableBytes { raw -> Int64 in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else { return -2 }
            return yu_ios_copy_frame(base, capacity)
        }
        return Int(copied)
    }

    static func setInput(_ button: String, _ pressed: Bool) -> Int32 {
        button.withCString { yu_ios_set_input($0, pressed ? 1 : 0) }
    }

    static func setCursor(x: Int, y: Int) -> Int32 {
        yu_ios_set_cursor(Int64(x), Int64(y))
    }

    static func answerDialog(id: UInt64, button: UInt8, text: String?) -> Int32 {
        if let text {
            return text.withCString { yu_ios_answer_dialog(id, button, $0) }
        }
        return yu_ios_answer_dialog(id, button, nil)
    }

    static func suspend() -> Int32 { yu_ios_suspend() }
    static func resume() -> Int32 { yu_ios_resume() }
    static func lifecycle() -> Int32 { yu_ios_lifecycle() }
    static func takeShutdownRequest() -> Int32 { yu_ios_take_shutdown_request() }
    static func requestResize(width: Int, height: Int) -> Int32 {
        yu_ios_request_resize(UInt32(width), UInt32(height))
    }
    static func shutdown() -> Int32 { yu_ios_shutdown() }

    /// 预留的 Metal 上屏入口;当前返回 -3(未接线)。
    static func attachLayer(_ layer: CAMetalLayer, width: Int, height: Int) -> Int32 {
        let pointer = Unmanaged.passUnretained(layer).toOpaque()
        return yu_ios_attach_layer(pointer, UInt32(width), UInt32(height))
    }

    private static func text(_ body: (UnsafeMutablePointer<CChar>?, Int) -> Int32) -> String {
        let needed = body(nil, 0)
        guard needed > 0 else { return "" }
        var buffer = [CChar](repeating: 0, count: Int(needed))
        let written = buffer.withUnsafeMutableBufferPointer { raw in
            body(raw.baseAddress, raw.count)
        }
        guard written > 0 else { return "" }
        return String(cString: buffer)
    }
}
