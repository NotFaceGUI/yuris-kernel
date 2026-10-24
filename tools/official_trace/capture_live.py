#!/usr/bin/env python3
"""官方引擎只读连续抓帧(交给用户手动点击)。

找到标题含 `デバッグモード`(或 YU-RIS)的官方窗口,按固定间隔抓客户区,
文件名带序号与耗时,便于事后定位"点进菜单"那一帧。不改二进制/不写游戏文件。

用法:
  python tools/official_trace/capture_live.py --out target/tmp/official/live \
      --seconds 90 --interval 0.4
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

from PIL import ImageGrab

user32 = ctypes.windll.user32

user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM), wt.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
user32.ClientToScreen.argtypes = [wt.HWND, ctypes.POINTER(wt.POINT)]


def list_windows():
    out = []

    def cb(hwnd, _):
        if not user32.IsWindowVisible(hwnd):
            return True
        rect = wt.RECT()
        user32.GetClientRect(hwnd, ctypes.byref(rect))
        w = rect.right - rect.left
        h = rect.bottom - rect.top
        if w < 640 or h < 480:
            return True
        buf = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(hwnd, buf, 512)
        origin = wt.POINT(0, 0)
        user32.ClientToScreen(hwnd, ctypes.byref(origin))
        out.append((hwnd, w, h, origin.x, origin.y, buf.value))
        return True

    user32.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
    return out


def pick(windows, exclude=()):
    for w in windows:
        title = w[5]
        if any(x in title for x in exclude):
            continue
        if "デバッグ" in title or "YU-RIS" in title.upper() or "yu-ris" in title:
            return w
    # 官方 800x600 逻辑、可能被 DPI 缩放:挑面积最大、标题非空的
    cands = [w for w in windows if w[5].strip() and not any(x in w[5] for x in exclude)]
    cands.sort(key=lambda i: i[1] * i[2], reverse=True)
    return cands[0] if cands else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--seconds", type=float, default=90.0)
    ap.add_argument("--interval", type=float, default=0.4)
    ap.add_argument("--min-width", type=int, default=640)
    ap.add_argument("--max-width", type=int, default=900)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--exclude-title", action="append", default=[],
                    help="标题包含该子串的窗口不抓(可多次)")
    args = ap.parse_args()

    if args.list:
        for w in list_windows():
            print(w)
        return 0

    os.makedirs(args.out, exist_ok=True)
    started = time.time()
    target = None
    index = 0
    last_note = 0.0
    while time.time() - started < args.seconds:
        if target is None:
            target = pick(list_windows(), tuple(args.exclude_title))
            if target is None:
                time.sleep(0.3)
                continue
            print(f"window hwnd={target[0]} {target[1]}x{target[2]} title={target[5]!r}", flush=True)
        hwnd, w, h, ox, oy, _ = target
        if not user32.IsWindow(hwnd):
            target = None
            continue
        index += 1
        try:
            img = ImageGrab.grab(bbox=(ox, oy, ox + w, oy + h), all_screens=True)
        except Exception as exc:  # 窗口最小化/被移动时跳过
            print(f"grab failed: {exc}", flush=True)
            time.sleep(args.interval)
            continue
        img.save(os.path.join(args.out, f"live_{index:05d}.png"))
        elapsed = time.time() - started
        if elapsed - last_note > 5.0:
            last_note = elapsed
            print(f"t={elapsed:.1f}s frames={index}", flush=True)
        time.sleep(args.interval)
    print(f"done frames={index}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
