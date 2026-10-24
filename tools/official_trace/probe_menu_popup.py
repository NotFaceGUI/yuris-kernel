#!/usr/bin/env python3
"""官方 0.495 菜单弹窗取证(只读):启动官方 exe,SendInput 点击,截客户区。

目的(DIV-RT-050):确认官方在「選択肢 → メニュー」时弹窗背后是什么。
不修改二进制、不写游戏文件;产物只落 target/tmp。

用法:
  python tools/official_trace/probe_menu_popup.py --exe <yu-ris.exe> --cwd <工程根> \
     --out target/tmp/official/menu-popup
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

from PIL import Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM), wt.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.ClientToScreen.argtypes = [wt.HWND, ctypes.POINTER(wt.POINT)]

INPUT_MOUSE = 0
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_ABSOLUTE = 0x8000


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wt.LONG),
        ("dy", wt.LONG),
        ("mouseData", wt.DWORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.POINTER(wt.ULONG)),
    ]


class INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [("mi", MOUSEINPUT)]

    _anonymous_ = ("u",)
    _fields_ = [("type", wt.DWORD), ("u", _U)]


def send_mouse(flags, x=0, y=0):
    screen_w = user32.GetSystemMetrics(0)
    screen_h = user32.GetSystemMetrics(1)
    inp = INPUT()
    inp.type = INPUT_MOUSE
    inp.mi.dx = int(x * 65535 / max(1, screen_w - 1))
    inp.mi.dy = int(y * 65535 / max(1, screen_h - 1))
    inp.mi.dwFlags = flags | (MOUSEEVENTF_ABSOLUTE if flags & MOUSEEVENTF_MOVE else 0)
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))


def click(client_x, client_y, hold=0.12):
    send_mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, client_x, client_y)
    time.sleep(0.05)
    send_mouse(MOUSEEVENTF_LEFTDOWN, client_x, client_y)
    time.sleep(hold)
    send_mouse(MOUSEEVENTF_LEFTUP, client_x, client_y)


def find_window(pid, timeout=20.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = []

        def cb(hwnd, _):
            owner = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value != pid or not user32.IsWindowVisible(hwnd):
                return True
            rect = wt.RECT()
            user32.GetClientRect(hwnd, ctypes.byref(rect))
            w = rect.right - rect.left
            h = rect.bottom - rect.top
            if w >= 320 and h >= 240:
                found.append((hwnd, w, h))
            return True

        user32.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(cb), 0)
        if found:
            found.sort(key=lambda i: i[1] * i[2], reverse=True)
            return found[0]
        time.sleep(0.2)
    return None


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG),
        ("biPlanes", wt.WORD), ("biBitCount", wt.WORD), ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD), ("biXPelsPerMeter", wt.LONG),
        ("biYPelsPerMeter", wt.LONG), ("biClrUsed", wt.DWORD), ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


def grab_client(hwnd, width, height):
    window_rect = wt.RECT()
    client_rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(window_rect))
    user32.GetClientRect(hwnd, ctypes.byref(client_rect))
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    fw = window_rect.right - window_rect.left
    fh = window_rect.bottom - window_rect.top
    cx = max(0, origin.x - window_rect.left)
    cy = max(0, origin.y - window_rect.top)
    hdc = user32.GetDC(hwnd)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, fw, fh)
    old = gdi32.SelectObject(mem, bmp)
    ok = user32.PrintWindow(hwnd, mem, 2)
    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = fw
    info.bmiHeader.biHeight = -fh
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = 0
    buf = ctypes.create_string_buffer(fw * fh * 4)
    lines = gdi32.GetDIBits(mem, bmp, 0, fh, buf, ctypes.byref(info), 0)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(hwnd, hdc)
    if not (ok and lines):
        # D3D 窗口的 PrintWindow 常返回黑帧:改从屏幕客户区直接抓。
        try:
            from PIL import ImageGrab
            return ImageGrab.grab(
                bbox=(origin.x, origin.y, origin.x + width, origin.y + height),
                all_screens=True,
            ).convert("RGBA")
        except Exception:
            return None
    data = bytearray(buf.raw)
    data[0::4], data[2::4] = data[2::4], data[0::4]
    img = Image.frombytes("RGBA", (fw, fh), bytes(data))
    return img.crop((cx, cy, cx + width, cy + height))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--cwd", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--start-click", default="217,281")
    ap.add_argument("--menu-click", default="750,583")
    ap.add_argument("--menu-delay", type=float, default=6.0,
                    help="点完スタート后等待多少秒再点メニュー")
    ap.add_argument("--shots", type=float, default=4.0,
                    help="点メニュー后持续截图秒数")
    args = ap.parse_args()

    os.makedirs(args.out, exist_ok=True)
    proc = subprocess.Popen([args.exe], cwd=args.cwd)
    try:
        win = find_window(proc.pid)
        if not win:
            print("window not found", file=sys.stderr)
            return 2
        hwnd, width, height = win
        print(f"hwnd={hwnd} client={width}x{height} pid={proc.pid}")
        user32.ShowWindow(hwnd, 9)
        user32.SetForegroundWindow(hwnd)
        time.sleep(2.5)

        sx, sy = (int(v) for v in args.start_click.split(","))
        origin = wt.POINT(0, 0)
        user32.ClientToScreen(hwnd, ctypes.byref(origin))
        kx = width / 800.0
        ky = height / 600.0
        click(origin.x + int(sx * kx), origin.y + int(sy * ky))
        print("clicked start")
        time.sleep(args.menu_delay)
        mx, my = (int(v) for v in args.menu_click.split(","))
        click(origin.x + int(mx * kx), origin.y + int(my * ky))
        print("clicked menu")

        n = 0
        deadline = time.time() + args.shots
        while time.time() < deadline:
            n += 1
            img = grab_client(hwnd, width, height)
            if img is not None:
                img.save(os.path.join(args.out, f"menu_{n:03d}.png"))
            time.sleep(0.35)
        print(f"captured {n} frames")
        return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
