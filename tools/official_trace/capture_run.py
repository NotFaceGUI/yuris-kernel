"""官方引擎「屏幕级」被动采集:不发送任何输入,窗口关闭即结束。

与 `capture_screen.py` 的区别:
- `capture_screen.py` 只采首屏(固定帧数);
- 本脚本按 `--interval` 连续截图,直到**窗口被关闭/进程退出**或到达
  `--duration`/`--max-shots` 上限。用于取证演出中途的画面(例如 FOUT 幕)。

**默认不发送任何输入**:鼠标/键盘完全交给操作者。要脚本化推进时才用
`--click` / `--click-every` 并显式加 `--send-input`。

产物(全部写到仓库外/`target`,不入库):
```
<out>/frame_0001.png ...
<out>/capture_index.json   每张截图的时间戳/运行秒数
```

只读观测:启动进程、截客户区;结束时 terminate。
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import subprocess
import sys
import time

user32 = ctypes.windll.user32

user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM), wt.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.ClientToScreen.argtypes = [wt.HWND, ctypes.POINTER(wt.POINT)]
user32.SetForegroundWindow.argtypes = [wt.HWND]


def find_engine_window(pid):
    found = []

    def cb(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid or not user32.IsWindowVisible(hwnd):
            return True
        rect = wt.RECT()
        if not user32.GetClientRect(hwnd, ctypes.byref(rect)):
            return True
        w, h = rect.right - rect.left, rect.bottom - rect.top
        if w >= 640 and h >= 480:
            found.append((hwnd, w, h))
        return True

    user32.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
    return found[0] if found else None


def grab_client(hwnd, width, height):
    """只取客户区:`PrintWindow(PW_RENDERFULLCONTENT)` 整窗渲染后按客户区偏移裁剪。

    旧实现用 `GetWindowDC`+`BitBlt` 抓客户区尺寸的整窗左上角 —— 抓到的是
    **标题栏+边框** 且底部被裁掉,所有像素比对都在错位图上做(2026-10-27 修正)。
    """
    import ctypes as ct
    from PIL import Image

    gdi32 = ct.windll.gdi32
    user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]

    window_rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(window_rect))
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    full_width = window_rect.right - window_rect.left
    full_height = window_rect.bottom - window_rect.top
    crop_x = max(0, origin.x - window_rect.left)
    crop_y = max(0, origin.y - window_rect.top)

    hdc = user32.GetDC(hwnd)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, full_width, full_height)
    old = gdi32.SelectObject(mem, bmp)
    ok = user32.PrintWindow(hwnd, mem, 2)  # PW_RENDERFULLCONTENT

    class BMI(ct.Structure):
        _fields_ = [
            ("biSize", ct.c_uint32),
            ("biWidth", ct.c_int32),
            ("biHeight", ct.c_int32),
            ("biPlanes", ct.c_uint16),
            ("biBitCount", ct.c_uint16),
            ("biCompression", ct.c_uint32),
            ("biSizeImage", ct.c_uint32),
            ("biXPelsPerMeter", ct.c_int32),
            ("biYPelsPerMeter", ct.c_int32),
            ("biClrUsed", ct.c_uint32),
            ("biClrImportant", ct.c_uint32),
        ]

    bi = BMI()
    bi.biSize = ct.sizeof(BMI)
    bi.biWidth = full_width
    bi.biHeight = -full_height
    bi.biPlanes = 1
    bi.biBitCount = 32
    buffer = ct.create_string_buffer(full_width * full_height * 4)
    lines = gdi32.GetDIBits(mem, bmp, 0, full_height, buffer, ct.byref(bi), 0)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(hwnd, hdc)
    if not (ok and lines):
        raise RuntimeError("PrintWindow 失败")
    data = bytearray(buffer.raw)
    data[0::4], data[2::4] = data[2::4], data[0::4]
    image = Image.frombytes("RGBA", (full_width, full_height), bytes(data))
    return image.crop((crop_x, crop_y, crop_x + width, crop_y + height)).convert("RGB")


def click(hwnd, x, y):
    point = wt.POINT(x, y)
    user32.ClientToScreen(hwnd, ctypes.byref(point))
    user32.SetCursorPos(point.x, point.y)
    time.sleep(0.03)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--interval", type=float, default=0.25, help="截图间隔(秒)")
    parser.add_argument("--duration", type=float, default=1800.0, help="最长运行时长(秒)")
    parser.add_argument("--max-shots", type=int, default=4000, help="最多保留多少张截图")
    parser.add_argument("--warmup", type=float, default=0.0, help="启动后静置(秒)")
    parser.add_argument(
        "--send-input",
        action="store_true",
        help="允许发送鼠标点击(默认完全被动,不碰鼠标)",
    )
    parser.add_argument(
        "--click",
        action="append",
        default=[],
        help="at_seconds:x,y —— 相对启动的秒数;可重复。需要 --send-input",
    )
    parser.add_argument("--click-every", default="", help="at0,interval:x,y 周期点击;需要 --send-input")
    args = parser.parse_args()

    os.makedirs(args.out, exist_ok=True)
    clicks = []
    if args.send_input:
        for spec in args.click:
            at, coord = spec.split(":", 1)
            x, y = coord.split(",")
            clicks.append((float(at), int(x), int(y)))
        if args.click_every:
            head, coord = args.click_every.split(":", 1)
            start, interval = head.split(",")
            x, y = coord.split(",")
            t = float(start)
            while t < args.duration:
                clicks.append((t, int(x), int(y)))
                t += float(interval)
        clicks.sort()

    process = subprocess.Popen([args.exe], cwd=args.cwd)
    started = time.time()
    width = height = 0
    index = []
    try:
        window = None
        for _ in range(60):
            window = find_engine_window(process.pid)
            if window is not None:
                break
            time.sleep(0.25)
        if window is None:
            print("engine window not found", file=sys.stderr)
            return 2
        hwnd, width, height = window
        print(f"window pid={process.pid} hwnd={hwnd} client={width}x{height}")
        index = []
        shot = 0
        next_click = 0
        stop_reason = "duration"
        while True:
            if process.poll() is not None:
                stop_reason = "process-exit"
                break
            elapsed = time.time() - started
            if elapsed >= args.duration:
                stop_reason = "duration"
                break
            if shot >= args.max_shots:
                stop_reason = "max-shots"
                break
            while next_click < len(clicks) and clicks[next_click][0] <= elapsed:
                _, x, y = clicks[next_click]
                click(hwnd, x, y)
                next_click += 1
            if elapsed >= args.warmup and user32.IsWindow(hwnd):
                try:
                    image = grab_client(hwnd, width, height)
                except Exception:
                    stop_reason = "window-gone"
                    break
                shot += 1
                path = os.path.join(args.out, f"frame_{shot:04d}.png")
                image.save(path)
                index.append(
                    {
                        "shot": shot,
                        "elapsed": round(elapsed, 3),
                        "clicks_sent": next_click,
                        "png": path,
                    }
                )
            time.sleep(args.interval)
        print(f"stop: {stop_reason} after {round(time.time() - started, 1)}s")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except Exception:
                process.kill()
        with open(os.path.join(args.out, "capture_index.json"), "w", encoding="utf-8") as fh:
            json.dump({"width": width, "height": height, "shots": index}, fh, ensure_ascii=False, indent=1)
            fh.write("\n")
    print(f"wrote {len(index)} shots to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
