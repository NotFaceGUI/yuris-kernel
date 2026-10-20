#!/usr/bin/env python3
"""官方引擎屏幕级 trace 采集器(schema v3, producer=official, level=screen)。

设计约束(任务书 H):
- 只读观测:启动官方 exe、截取窗口客户区、记录输入时间线;**不改二进制/游戏文件**。
- 采集器不进入内核;语料/截图写到仓库外的输出目录。
- 屏幕级记录只证明"画面摘要",不伪造事件级命令/参数。
- 帧锚定:每个截图是一个逻辑帧;时间线记录每一帧实际发送的输入。
- 输出 trace 的 header 不含绝对路径;manifest(单独文件)可含路径,便于复核。

用法:
  python tools/official_trace/capture_screen.py \
    --exe <官方 yu-ris.exe> --cwd <游戏/工程根> \
    --out <仓库外输出目录> --frames 12 --interval 0.35

输出:
  <out>/frame_0001.png ...       客户区截图(不入库)
  <out>/official_screen.jsonl    schema v3 trace(可直接给 diff_traces)
  <out>/input_timeline.json      帧号 -> 输入
  <out>/manifest.json            引擎/样本 hash、窗口尺寸、级别声明

屏幕摘要定义(官方与内核必须一致):
  pixels_sha256 = SHA-256(b"RGBA8" + width:u32 LE + height:u32 LE + RGBA bytes)
  display JSON = {"width":W,"height":H,"pixels_sha256":...}
  display_sha256 = SHA-256(display JSON 的规范文本,无空白、键序 width/height/pixels_sha256)
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import struct
import subprocess
import sys
import time

from PIL import Image, ImageGrab

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD),
        ("biWidth", wt.LONG),
        ("biHeight", wt.LONG),
        ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", wt.LONG),
        ("biYPelsPerMeter", wt.LONG),
        ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM), wt.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.IsWindowVisible.argtypes = [wt.HWND]
user32.IsWindowVisible.restype = wt.BOOL
user32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.ClientToScreen.argtypes = [wt.HWND, ctypes.POINTER(wt.POINT)]
user32.SetForegroundWindow.argtypes = [wt.HWND]
user32.ShowWindow.argtypes = [wt.HWND, ctypes.c_int]


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def find_engine_window(pid, timeout=15.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = []

        def callback(hwnd, _):
            owner = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value != pid or not user32.IsWindowVisible(hwnd):
                return True
            rect = wt.RECT()
            user32.GetClientRect(hwnd, ctypes.byref(rect))
            width = rect.right - rect.left
            height = rect.bottom - rect.top
            if width >= 320 and height >= 240:
                found.append((hwnd, width, height))
            return True

        user32.EnumWindows(
            ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(callback), 0
        )
        if found:
            found.sort(key=lambda item: item[1] * item[2], reverse=True)
            return found[0]
        time.sleep(0.2)
    return None


def grab_client(hwnd, width, height):
    """PrintWindow 整个窗口后按客户区偏移裁剪;失败再前台截图。"""
    window_rect = wt.RECT()
    client_rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(window_rect))
    user32.GetClientRect(hwnd, ctypes.byref(client_rect))
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
    ok = user32.PrintWindow(hwnd, mem, 2)
    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = full_width
    info.bmiHeader.biHeight = -full_height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = 0
    buffer = ctypes.create_string_buffer(full_width * full_height * 4)
    lines = gdi32.GetDIBits(mem, bmp, 0, full_height, buffer, ctypes.byref(info), 0)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(hwnd, hdc)
    if ok and lines:
        data = bytearray(buffer.raw)
        data[0::4], data[2::4] = data[2::4], data[0::4]
        image = Image.frombytes("RGBA", (full_width, full_height), bytes(data))
        return image.crop((crop_x, crop_y, crop_x + width, crop_y + height))
    user32.ShowWindow(hwnd, 9)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.6)
    return ImageGrab.grab(
        bbox=(origin.x, origin.y, origin.x + width, origin.y + height)
    ).convert("RGBA")


def empty_paths_digest():
    # 与 yu-trace::paths_summary 的零路径编码一致:5 个 (tag, present=0)。
    return sha256_bytes(bytes([value for tag in range(5) for value in (tag, 0)]))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--frames", type=int, default=12)
    parser.add_argument("--interval", type=float, default=0.35)
    parser.add_argument("--sample", default="data.ypf", help="sample file under --cwd")
    parser.add_argument("--producer-name", default="official-eris-screen")
    parser.add_argument("--producer-version", default="0.1.0")
    parser.add_argument("--no-send-input", action="store_true")
    parser.add_argument("--warmup", type=float, default=5.0)
    args = parser.parse_args()

    os.makedirs(args.out, exist_ok=True)
    sample_path = os.path.join(args.cwd, args.sample)
    sample_sha = None
    if os.path.isfile(sample_path):
        with open(sample_path, "rb") as handle:
            digest = hashlib.sha256()
            for chunk in iter(lambda: handle.read(1 << 20), b""):
                digest.update(chunk)
            sample_sha = digest.hexdigest()

    process = subprocess.Popen([args.exe], cwd=args.cwd)
    try:
        window = find_engine_window(process.pid)
        if window is None:
            print("engine window not found", file=sys.stderr)
            return 2
        hwnd, width, height = window
        print(f"window pid={process.pid} hwnd={hwnd} client={width}x{height}")
        if args.warmup > 0:
            time.sleep(args.warmup)
        frames = []
        timeline = []
        for index in range(1, args.frames + 1):
            image = grab_client(hwnd, width, height)
            png_path = os.path.join(args.out, f"frame_{index:04d}.png")
            image.save(png_path)
            pixels = image.tobytes()
            pixels_sha = sha256_bytes(
                b"RGBA8" + struct.pack("<II", width, height) + pixels
            )
            descriptor = {
                "width": width,
                "height": height,
                "pixels_sha256": pixels_sha,
            }
            descriptor_text = json.dumps(
                descriptor, ensure_ascii=False, separators=(",", ":")
            )
            display_sha = sha256_bytes(descriptor_text.encode("utf-8"))
            frames.append(
                {
                    "frame": index,
                    "level": "screen",
                    "display_sha256": display_sha,
                    "pass_count": 0,
                    "op_count": 0,
                    "executable_ops": 0,
                    "skipped_ops": 0,
                    "display": descriptor,
                    "png": png_path,
                }
            )
            # 首屏不发送输入;如需推进,未来由 --input-script 扩展。
            timeline.append({"frame": index, "cursor": [0, 0], "pressed": []})
            time.sleep(args.interval)
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except Exception:
                process.kill()

    # 输入摘要(与 yu-trace::input_summary_range 相同的规范字节序)。
    digest = hashlib.sha256()
    for item in timeline:
        digest.update(struct.pack("<Q", item["frame"]))
        digest.update(struct.pack("<q", item["cursor"][0]))
        digest.update(struct.pack("<q", item["cursor"][1]))
        pressed = item["pressed"]
        digest.update(struct.pack("<Q", len(pressed)))
        for name in pressed:
            encoded = name.encode("utf-8")
            digest.update(struct.pack("<Q", len(encoded)))
            digest.update(encoded)
    input_sha = digest.hexdigest()

    header = {
        "codec": "cp932",
        "producer": {
            "kind": "official",
            "name": args.producer_name,
            "version": args.producer_version,
            "sample_sha256": sample_sha,
        },
        "tables": [],
        "boot": {
            "entry": "SYSTEM_START",
            "fps": 60,
            "start_ms": 0,
            "frame_ms": 16,
            "script_key": "00000000",
            "debug_mode": False,
            "argv": {"len": 0, "sha256": sha256_bytes(b"")},
            "host": {
                "screen_color": 32,
                "media_readonly": False,
                "os_sound": True,
                "dx_mode": 0,
                "paths": {"len": 0, "sha256": empty_paths_digest()},
            },
        },
        "input": {"events": len(timeline), "sha256": input_sha},
        "clock": {"mode": "none", "now_ms": []},
        "kernel": "official-eris-screen (not observable at screen level)",
    }
    trace_path = os.path.join(args.out, "official_screen.jsonl")
    with open(trace_path, "w", encoding="utf-8", newline="\n") as out:
        out.write(
            json.dumps(
                {
                    "schema": 3,
                    "seq": 0,
                    "frame": None,
                    "sub": "meta",
                    "kind": "header",
                    "payload": header,
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
            + "\n"
        )
        for seq, item in enumerate(frames, start=1):
            payload = {
                "frame": item["frame"],
                "level": item["level"],
                "display_sha256": item["display_sha256"],
                "pass_count": item["pass_count"],
                "op_count": item["op_count"],
                "executable_ops": item["executable_ops"],
                "skipped_ops": item["skipped_ops"],
                "display": item["display"],
            }
            out.write(
                json.dumps(
                    {
                        "schema": 3,
                        "seq": seq,
                        "frame": item["frame"],
                        "sub": "frame",
                        "kind": "frame_description",
                        "payload": payload,
                    },
                    ensure_ascii=False,
                    separators=(",", ":"),
                )
                + "\n"
            )

    manifest = {
        "level": "screen",
        "event_level_l1_achieved": False,
        "note": (
            "screen-level oracle: pass_count/op_count/executable_ops/skipped_ops and "
            "boot config are not observable from outside the engine and are recorded as "
            "0/defaults; this trace must not be read as event-level L1."
        ),
        "exe": args.exe,
        "cwd": args.cwd,
        "sample": args.sample,
        "sample_sha256": sample_sha,
        "client_width": frames[0]["display"]["width"] if frames else None,
        "client_height": frames[0]["display"]["height"] if frames else None,
        "frame_count": len(frames),
        "interval_seconds": args.interval,
        "frames": [
            {"frame": item["frame"], "png": item["png"], "display_sha256": item["display_sha256"]}
            for item in frames
        ],
        "input_timeline": timeline,
        "trace": trace_path,
    }
    with open(os.path.join(args.out, "manifest.json"), "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, ensure_ascii=False, indent=2)
    with open(os.path.join(args.out, "input_timeline.json"), "w", encoding="utf-8") as handle:
        json.dump(timeline, handle, ensure_ascii=False, indent=2)
    print(f"wrote {trace_path} ({len(frames)} frames)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
