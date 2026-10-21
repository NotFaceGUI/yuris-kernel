#!/usr/bin/env python3
"""官方 0.495 引擎事件级(L1)只读采集器。

方法:64 位 Python 以 DEBUG_ONLY_THIS_PROCESS 启动 32 位官方引擎,在两条
命令分派路径上各下一个 int3:

    0x4044db  `call dword ptr [ecx + edi*4]`   (0x8015b0 == 0 的路径)
    0x404547  `call dword ptr [ecx + edi*4]`   (0x8015b0 != 0 的路径)

命中时 `EDI` = 本脚本命令序号(pc,与内核 `Cmd.pc` 同语义),`ECX` = 本脚本
的处理器表基址;读 `[ECX+EDI*4]` 得到 handler,再经全局表 `0x7ace20`
(119 项,下标 = ysc.ybn opcode)反查命令 id。若 handler 被多个 opcode 共享,
id 显式留空并在 `command_table.json` 记录候选,不猜。

红线:
- 不修改官方二进制、不写游戏文件;
- 唯一的内存写是断点字节 `cc` 与恢复原字节(观测所需);
- **不做反调试绕过**(旧版本的 PEB BeingDebugged/NtGlobalFlag 清零已删除)。

根因勘误(2026-10-21):
旧版本把 `--bp-rva 0x44db` 直接当绝对地址用,实际落下的是 `0x44db`
(未映射)而不是 `0x400000+0x44db`;`ReadProcessMemory(0x44db)` 返回
`ERROR_PARTIAL_COPY(299)`,`WriteProcessMemory(0x44db)` 返回 998,断点从未
落下。旧日志里的 "err 299" 还是成功调用后未清零的陈旧 GetLastError。
本版本统一用 `模块基址 + RVA`,并在每次 API 调用前 `SetLastError(0)`。

用法:
  python tools/official_trace/probe_dispatch.py \
    --exe <官方 yu-ris.exe> --cwd <工程根> \
    --ysc <ysc.ybn> --out-dir target/tmp/official/events \
    --seconds 12 --arm-after 6.5 --input-at 8 --input both \
    --trace-out target/tmp/official/events/official_events.jsonl
截图不在此工具内:断点暂停时 PrintWindow 会等待被挂起的 UI 线程而死锁;
屏幕级截图仍由 capture_screen.py 单独采集。

输出(全部写 --out-dir,不入库):
  hits.jsonl             逐命中原始记录(含 handler/ecx/tick/线程)
  command_table.json     运行时 119 项 handler 表与歧义分组
  manifest.json          样本/引擎 hash、参数、计数、失败原因
  <trace-out>            schema v3 trace(producer=official, vm/command)
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import struct
import sys
import time

k = ctypes.WinDLL("kernel32", use_last_error=True)
ntdll = ctypes.WinDLL("ntdll", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)

IMAGE_BASE_DEFAULT = 0x400000
BP_RVA_DEFAULT = (0x44DB, 0x4547)
TASK_GLOBAL_RVA = 0x40171C      # 0x80171c
TICK_COUNTER_RVA = 0x401268     # 0x801268: per-tick 64-bit counter
DISPATCH_TABLE_RVA = 0x3ACE20   # 0x7ace20
DISPATCH_TABLE_LEN = 119        # ysc.ybn 0.495 命令数

DEBUG_ONLY_THIS_PROCESS = 2
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001
CREATE_PROCESS_DEBUG_EVENT = 3
CREATE_THREAD_DEBUG_EVENT = 2
EXIT_PROCESS_DEBUG_EVENT = 5
EXCEPTION_DEBUG_EVENT = 1
EXCEPTION_BREAKPOINT = 0x80000003
EXCEPTION_SINGLE_STEP = 0x80000004
EXCEPTION_WX86_BREAKPOINT = 0x4000001F
EXCEPTION_WX86_SINGLE_STEP = 0x4000001E

CONTEXT_i386 = 0x00010000
CONTEXT_CONTROL = CONTEXT_i386 | 1
CONTEXT_INTEGER = CONTEXT_i386 | 2
CONTEXT_DEBUG_REGISTERS = CONTEXT_i386 | 0x10
CTX_FLAGS = CONTEXT_CONTROL | CONTEXT_INTEGER | CONTEXT_DEBUG_REGISTERS
EFLAGS_TF = 0x100


class STARTUPINFO(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD), ("lpReserved", wt.LPWSTR), ("lpDesktop", wt.LPWSTR),
        ("lpTitle", wt.LPWSTR), ("dwX", wt.DWORD), ("dwY", wt.DWORD),
        ("dwXSize", wt.DWORD), ("dwYSize", wt.DWORD), ("dwXCountChars", wt.DWORD),
        ("dwYCountChars", wt.DWORD), ("dwFillAttribute", wt.DWORD),
        ("dwFlags", wt.DWORD), ("wShowWindow", wt.WORD), ("cbReserved2", wt.WORD),
        ("lpReserved2", ctypes.c_void_p), ("hStdInput", wt.HANDLE),
        ("hStdOutput", wt.HANDLE), ("hStdError", wt.HANDLE),
    ]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", wt.HANDLE), ("hThread", wt.HANDLE),
                ("dwProcessId", wt.DWORD), ("dwThreadId", wt.DWORD)]


class FLOATING_SAVE_AREA(ctypes.Structure):
    _fields_ = [(name, wt.DWORD) for name in (
        "ControlWord", "StatusWord", "TagWord", "ErrorOffset",
        "ErrorSelector", "DataOffset", "DataSelector")] + [
        ("RegisterArea", ctypes.c_ubyte * 80), ("Cr0NpxState", wt.DWORD)]


class CONTEXT(ctypes.Structure):
    _fields_ = [
        ("ContextFlags", wt.DWORD), ("Dr0", wt.DWORD), ("Dr1", wt.DWORD),
        ("Dr2", wt.DWORD), ("Dr3", wt.DWORD), ("Dr6", wt.DWORD), ("Dr7", wt.DWORD),
        ("FloatSave", FLOATING_SAVE_AREA), ("SegGs", wt.DWORD), ("SegFs", wt.DWORD),
        ("SegEs", wt.DWORD), ("SegDs", wt.DWORD), ("Edi", wt.DWORD), ("Esi", wt.DWORD),
        ("Ebx", wt.DWORD), ("Edx", wt.DWORD), ("Ecx", wt.DWORD), ("Eax", wt.DWORD),
        ("Ebp", wt.DWORD), ("Eip", wt.DWORD), ("SegCs", wt.DWORD), ("EFlags", wt.DWORD),
        ("Esp", wt.DWORD), ("SegSs", wt.DWORD), ("ExtendedRegisters", ctypes.c_ubyte * 512),
    ]


class EXCEPTION_RECORD(ctypes.Structure):
    _fields_ = [("ExceptionCode", wt.DWORD), ("ExceptionFlags", wt.DWORD),
                ("ExceptionRecord", ctypes.c_void_p), ("ExceptionAddress", ctypes.c_void_p),
                ("NumberParameters", wt.DWORD),
                ("ExceptionInformation", ctypes.c_size_t * 15)]


class EXCEPTION_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("ExceptionRecord", EXCEPTION_RECORD), ("dwFirstChance", wt.DWORD)]


class CREATE_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("hFile", wt.HANDLE), ("hProcess", wt.HANDLE), ("hThread", wt.HANDLE),
                ("lpBaseOfImage", ctypes.c_void_p), ("lpBaseOfDll", ctypes.c_void_p),
                ("dwDebugInfoFileOffset", wt.DWORD), ("fUnicode", wt.WORD),
                ("nDebugInfoSize", wt.DWORD), ("lpThreadLocalBase", ctypes.c_void_p),
                ("lpStartAddress", ctypes.c_void_p), ("lpImageName", ctypes.c_void_p),
                ("fUnicode2", wt.WORD)]


class CREATE_THREAD_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("hThread", wt.HANDLE), ("lpThreadLocalBase", ctypes.c_void_p),
                ("lpStartAddress", ctypes.c_void_p)]


class EXIT_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("dwExitCode", wt.DWORD)]


class LOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("hFile", wt.HANDLE), ("lpBaseOfDll", ctypes.c_void_p),
                ("hSection", wt.HANDLE), ("dwDebugInfoFileOffset", wt.DWORD),
                ("dwDebugInfoSize", wt.DWORD), ("lpImageName", ctypes.c_void_p),
                ("fUnicode", wt.WORD)]


class UNLOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("lpBaseOfDll", ctypes.c_void_p)]


class OUTPUT_DEBUG_STRING_INFO(ctypes.Structure):
    _fields_ = [("lpDebugStringData", ctypes.c_void_p), ("fUnicode", wt.WORD),
                ("nDebugStringLength", wt.WORD)]


class RIP_INFO(ctypes.Structure):
    _fields_ = [("dwError", wt.DWORD), ("dwType", wt.DWORD)]


class _U(ctypes.Union):
    _fields_ = [("Exception", EXCEPTION_DEBUG_INFO),
                ("CreateThread", CREATE_THREAD_DEBUG_INFO),
                ("CreateProcessInfo", CREATE_PROCESS_DEBUG_INFO),
                ("ExitProcess", EXIT_PROCESS_DEBUG_INFO),
                ("LoadDll", LOAD_DLL_DEBUG_INFO), ("UnloadDll", UNLOAD_DLL_DEBUG_INFO),
                ("DebugString", OUTPUT_DEBUG_STRING_INFO), ("RipInfo", RIP_INFO)]


class DEBUG_EVENT(ctypes.Structure):
    _fields_ = [("dwDebugEventCode", wt.DWORD), ("dwProcessId", wt.DWORD),
                ("dwThreadId", wt.DWORD), ("u", _U)]


k.CreateProcessW.argtypes = [wt.LPCWSTR, wt.LPWSTR, ctypes.c_void_p, ctypes.c_void_p,
                             wt.BOOL, wt.DWORD, ctypes.c_void_p, wt.LPCWSTR,
                             ctypes.POINTER(STARTUPINFO), ctypes.POINTER(PROCESS_INFORMATION)]
k.CreateProcessW.restype = wt.BOOL
k.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k.OpenProcess.restype = wt.HANDLE
k.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k.ReadProcessMemory.restype = wt.BOOL
k.WriteProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                 ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k.WriteProcessMemory.restype = wt.BOOL
k.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), wt.DWORD]
k.WaitForDebugEvent.restype = wt.BOOL
k.ContinueDebugEvent.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD]
k.ContinueDebugEvent.restype = wt.BOOL
k.Wow64GetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
k.Wow64GetThreadContext.restype = wt.BOOL
k.Wow64SetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
k.Wow64SetThreadContext.restype = wt.BOOL
k.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]
k.TerminateProcess.restype = wt.BOOL
k.CloseHandle.argtypes = [wt.HANDLE]
k.IsWow64Process.argtypes = [wt.HANDLE, ctypes.POINTER(wt.BOOL)]
k.IsWow64Process.restype = wt.BOOL
user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM), wt.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.GetClientRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.ClientToScreen.argtypes = [wt.HWND, ctypes.POINTER(wt.POINT)]
user32.IsWindowVisible.argtypes = [wt.HWND]
user32.GetForegroundWindow.restype = wt.HWND
user32.SetForegroundWindow.argtypes = [wt.HWND]
user32.SetForegroundWindow.restype = wt.BOOL
user32.GetWindowThreadProcessId.restype = wt.DWORD
user32.AttachThreadInput.argtypes = [wt.DWORD, wt.DWORD, wt.BOOL]
user32.AttachThreadInput.restype = wt.BOOL
user32.BringWindowToTop.argtypes = [wt.HWND]
user32.BringWindowToTop.restype = wt.BOOL
user32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
user32.GetCursorPos.argtypes = [ctypes.POINTER(wt.POINT)]
user32.mouse_event.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_size_t]
user32.keybd_event.argtypes = [wt.BYTE, wt.BYTE, wt.DWORD, ctypes.c_size_t]
user32.ClientToScreen.argtypes = [wt.HWND, ctypes.POINTER(wt.POINT)]
user32.PrintWindow.argtypes = [wt.HWND, wt.HDC, wt.UINT]
def last_error():
    return ctypes.get_last_error()


def clear_error():
    ctypes.set_last_error(0)


def read_mem(hproc, address, size):
    buf = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t(0)
    clear_error()
    ok = bool(k.ReadProcessMemory(hproc, ctypes.c_void_p(address), ctypes.byref(buf),
                                  size, ctypes.byref(got)))
    return {"ok": ok, "got": got.value, "err": last_error(),
            "bytes": buf.raw[:got.value] if ok else b""}


def write_mem(hproc, address, data):
    buf = ctypes.create_string_buffer(bytes(data), len(data))
    got = ctypes.c_size_t(0)
    clear_error()
    ok = bool(k.WriteProcessMemory(hproc, ctypes.c_void_p(address), ctypes.byref(buf),
                                   len(data), ctypes.byref(got)))
    return {"ok": ok, "got": got.value, "err": last_error()}


def read_u32(hproc, address):
    got = read_mem(hproc, address, 4)
    return int.from_bytes(got["bytes"], "little") if got["ok"] and got["got"] == 4 else None


def read_u16(hproc, address):
    got = read_mem(hproc, address, 2)
    return int.from_bytes(got["bytes"], "little") if got["ok"] and got["got"] == 2 else None


def read_u64(hproc, address):
    got = read_mem(hproc, address, 8)
    return int.from_bytes(got["bytes"], "little") if got["ok"] and got["got"] == 8 else None


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

        user32.EnumWindows(ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(callback), 0)
        if found:
            found.sort(key=lambda item: item[1] * item[2], reverse=True)
            return found[0]
        time.sleep(0.2)
    return None


def ensure_foreground(hwnd):
    """返回 (是否前台, 尝试记录)。不盲点:前台不是目标窗口时由调用方放弃输入。"""
    notes = []
    before = user32.GetForegroundWindow()
    notes.append(f"foreground_before={before}")
    if before == hwnd:
        return True, notes
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.3)
    if user32.GetForegroundWindow() == hwnd:
        notes.append("set_foreground_window=ok")
        return True, notes
    fg_tid = user32.GetWindowThreadProcessId(before, None) if before else 0
    target_tid = user32.GetWindowThreadProcessId(hwnd, None)
    attached = bool(user32.AttachThreadInput(fg_tid, target_tid, True)) if fg_tid else False
    notes.append(f"attach_thread_input={attached} err={last_error()}")
    user32.BringWindowToTop(hwnd)
    user32.SetForegroundWindow(hwnd)
    if attached:
        user32.AttachThreadInput(fg_tid, target_tid, False)
    time.sleep(0.3)
    if user32.GetForegroundWindow() == hwnd:
        notes.append("attach_workaround=ok")
        return True, notes
    # Windows 前台锁:模拟一次 Alt 键再试(不影响目标进程内存)。
    user32.keybd_event(0x12, 0, 0, 0)
    user32.SetForegroundWindow(hwnd)
    user32.keybd_event(0x12, 0, 0x0002, 0)
    time.sleep(0.3)
    after = user32.GetForegroundWindow()
    notes.append(f"alt_workaround_foreground={after}")
    return after == hwnd, notes


def send_input(hwnd, mode, x, y, method, force_foreground=True):
    """返回 (已发送列表, 备注, 待释放状态)。

    force_foreground=False 时只检查前台,不调用 SetForegroundWindow/
    AttachThreadInput —— 调试暂停期间这些调用会等待被挂起的 UI 线程。
    """
    if method == "post":
        user32.SetForegroundWindow(hwnd)
        sent = []
        if mode in ("enter", "both"):
            user32.PostMessageW(hwnd, 0x0100, 0x0D, 0)
            user32.PostMessageW(hwnd, 0x0101, 0x0D, 0)
            sent.append("POST:VK_RETURN")
        if mode in ("click", "both"):
            lparam = (y << 16) | (x & 0xFFFF)
            user32.PostMessageW(hwnd, 0x0200, 0, lparam)
            user32.PostMessageW(hwnd, 0x0201, 1, lparam)
            user32.PostMessageW(hwnd, 0x0202, 0, lparam)
            sent.append(f"POST:MOUSE_L@{x},{y}")
        return sent, [], None
    notes = []
    if force_foreground:
        ok, notes = ensure_foreground(hwnd)
        if not ok:
            notes.append("input_skipped=not_foreground")
            return [], notes, None
    else:
        fg = user32.GetForegroundWindow()
        notes.append(f"foreground_at_input={fg}")
        if fg != hwnd:
            notes.append("input_skipped=not_foreground(no-force-while-armed)")
            return [], notes, None
    sent = []
    pending = None
    if mode in ("click", "both"):
        origin = wt.POINT(x, y)
        user32.ClientToScreen(hwnd, ctypes.byref(origin))
        saved = wt.POINT(0, 0)
        user32.GetCursorPos(ctypes.byref(saved))
        user32.SetCursorPos(origin.x, origin.y)
        time.sleep(0.05)
        user32.mouse_event(0x0002, 0, 0, 0, 0)
        sent.append(f"SENDINPUT:MOUSE_L_DOWN@{x},{y}")
        notes.append(f"cursor_saved={saved.x},{saved.y}")
        pending = {"kind": "click", "saved": (saved.x, saved.y), "x": x, "y": y}
    if mode == "enter":
        user32.keybd_event(0x0D, 0, 0, 0)
        time.sleep(0.03)
        user32.keybd_event(0x0D, 0, 0x0002, 0)
        sent.append("SENDINPUT:ENTER")
    return sent, notes, pending


def release_input(pending):
    if pending is None:
        return []
    done = []
    if pending.get("kind") == "click":
        user32.mouse_event(0x0004, 0, 0, 0, 0)
        done.append("SENDINPUT:MOUSE_L_UP")
        saved = pending.get("saved")
        if saved:
            user32.SetCursorPos(saved[0], saved[1])
            done.append(f"cursor_restored={saved[0]},{saved[1]}")
    return done


def parse_ysc(path):
    """解析 ysc.ybn:返回 (version, [(id, name)])。格式见 crates/yu-format/src/yscm.rs。"""
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:4] != b"YSCM":
        raise ValueError(f"{path}: not YSCM")
    version = struct.unpack_from("<I", data, 4)[0]
    count = struct.unpack_from("<I", data, 8)[0]
    at = 0x10
    commands = []
    for index in range(count):
        end = data.index(b"\x00", at)
        name = data[at:end].decode("ascii", "replace")
        argc = data[end + 1]
        at = end + 2
        for _ in range(argc):
            end = data.index(b"\x00", at)
            at = end + 3
        commands.append((index, name))
    return version, commands


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def empty_paths_digest():
    return sha256_bytes(bytes([value for tag in range(5) for value in (tag, 0)]))


def build_trace(hits, commands, sample_sha, args, click_tick=None, tick_offset=None):
    """把命中记录写成 schema v3 trace(producer=official, vm/command)。"""
    names = {index: name for index, name in commands}
    records = []
    for hit in hits:
        # schema 要求 vm/command 带 frame;script/pc 取不到时跳过而不是填哨兵。
        if hit.get("tick") is None or hit.get("script") is None or hit.get("pc") is None:
            continue
        raw_id = hit.get("cmd")
        if raw_id is None:
            name = ""
        else:
            name = names.get(raw_id, "")
        frame = hit.get("tick")
        if frame is not None and tick_offset is not None:
            frame = max(0, frame - tick_offset) if tick_offset else frame
        records.append({
            "schema": 3,
            "seq": len(records) + 1,
            "frame": frame,
            "sub": "vm",
            "kind": "command",
            "payload": {
                "script": hit["script"],
                "pc": hit["pc"],
                "name": name,
                "args": [],
            },
        })
    input_events = 0 if click_tick is None else 1
    input_bytes = b""
    if click_tick is not None:
        input_bytes = (struct.pack("<Q", click_tick)
                       + struct.pack("<q", args.click_xy[0])
                       + struct.pack("<q", args.click_xy[1])
                       + struct.pack("<Q", 1)
                       + struct.pack("<Q", len(b"MOUSE_L")) + b"MOUSE_L")
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
            "debug_mode": args.debug_mode == "true",
            "argv": {"len": 0, "sha256": sha256_bytes(b"")},
            "host": {
                "screen_color": 32,
                "media_readonly": False,
                "os_sound": True,
                "dx_mode": 0,
                "paths": {"len": 0, "sha256": empty_paths_digest()},
            },
        },
        "input": {"events": input_events, "sha256": sha256_bytes(input_bytes)},
        "clock": {"mode": "tick", "now_ms": []},
        "kernel": "official-0.495 event probe (not the kernel)",
    }
    lines = [json.dumps({"schema": 3, "seq": 0, "frame": None, "sub": "meta",
                         "kind": "header", "payload": header},
                        ensure_ascii=False, separators=(",", ":"))]
    lines.extend(json.dumps(record, ensure_ascii=False, separators=(",", ":"))
                 for record in records)
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default=os.environ.get("YURIS_OFFICIAL_EXE"),
                        help="官方 yu-ris.exe;也可用 YURIS_OFFICIAL_EXE")
    parser.add_argument("--cwd", default=os.environ.get("YURIS_OFFICIAL_CWD"),
                        help="工程根;也可用 YURIS_OFFICIAL_CWD")
    parser.add_argument("--ysc", default=os.environ.get("YURIS_OFFICIAL_YSC"),
                        help="ysc.ybn 路径(用于 opcode→命令名)")
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--trace-out", default=None)
    parser.add_argument("--sample", default="data.ypf")
    parser.add_argument("--bp-rva", default="0x44db,0x4547")
    parser.add_argument("--trigger-rva", default="0x5ab63",
                        help="0x45ab63 = WM_LBUTTONDOWN 命中非透明 UI 像素后的写点;"
                             "先只挂它,点中后再挂分派断点(不拖慢标题)")
    parser.add_argument("--seconds", type=float, default=20.0)
    parser.add_argument("--arm-after", type=float, default=0.0,
                        help="延迟布防(秒):让引擎先跑完启动,再在稳定态下断点")
    parser.add_argument("--max-hits", type=int, default=200000)
    parser.add_argument("--input-at", type=float, default=None)
    parser.add_argument("--input", choices=["enter", "click", "both"], default="both")
    parser.add_argument("--input-method", choices=["sendinput", "post"], default="sendinput")
    parser.add_argument("--click-xy", default="410,275")
    parser.add_argument("--producer-name", default="official-eris-events")
    parser.add_argument("--producer-version", default="0.2.0")
    parser.add_argument("--debug-mode", choices=["true", "false"], default="true",
                        help="官方引擎运行模式;本语料窗口标题实测为【デバッグモード】")
    args = parser.parse_args()
    if not args.exe or not args.cwd:
        parser.error("--exe/--cwd required (or set YURIS_OFFICIAL_EXE/YURIS_OFFICIAL_CWD)")
    args.click_xy = tuple(int(part) for part in args.click_xy.split(","))
    bps = [int(part, 0) for part in args.bp_rva.split(",") if part.strip()]
    trigger_rva = int(args.trigger_rva, 0) if args.trigger_rva else 0
    trigger_addr = None
    os.makedirs(args.out_dir, exist_ok=True)

    commands = []
    ysc_version = None
    if args.ysc:
        ysc_version, commands = parse_ysc(args.ysc)
    names = {index: name for index, name in commands}

    sample_path = os.path.join(args.cwd, args.sample)
    sample_sha = sha256_file(sample_path) if os.path.isfile(sample_path) else None
    manifest = {
        "level": "event-l1",
        "method": "CreateProcess(DEBUG_ONLY_THIS_PROCESS) + int3 at dispatch sites",
        "exe": args.exe,
        "cwd": args.cwd,
        "exe_sha256": sha256_file(args.exe),
        "sample": args.sample,
        "sample_sha256": sample_sha,
        "ysc": args.ysc,
        "ysc_version": ysc_version,
        "ysc_commands": len(commands),
        "ysc_sha256": sha256_file(args.ysc) if args.ysc and os.path.isfile(args.ysc) else None,
        "debug_mode": args.debug_mode == "true",
        "debug_mode_evidence": (
            "official window title decoded from CP932 = 【デバッグモード】; "
            "save/data/config/projectconfig.txt DEBUGMODE=0 is not the runtime value"
        ),
        "bp_rva": [hex(bp) for bp in bps],
        "anti_debug_bypass": "none (read-only; PEB not modified)",
        "started": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "notes": [
            "pc = EDI at dispatch, same semantics as kernel Cmd.pc (command index).",
            "command id = index in [0x7ace20 + id*4] whose handler equals [ECX+EDI*4].",
            "shared handler => cmd=null and name empty (not guessed).",
            "frame = engine per-tick counter at [0x801268] (u64), read at the hit.",
        ],
    }

    startup = STARTUPINFO()
    startup.cb = ctypes.sizeof(startup)
    info = PROCESS_INFORMATION()
    clear_error()
    ok = k.CreateProcessW(args.exe, None, None, None, False, DEBUG_ONLY_THIS_PROCESS,
                          None, args.cwd, ctypes.byref(startup), ctypes.byref(info))
    if not ok:
        manifest["failure"] = f"CreateProcess failed err={last_error()}"
        with open(os.path.join(args.out_dir, "manifest.json"), "w", encoding="utf-8") as h:
            json.dump(manifest, h, ensure_ascii=False, indent=2)
        print(manifest["failure"])
        return 2
    manifest["pid"] = info.dwProcessId
    hproc = info.hProcess
    threads = {info.dwThreadId: info.hThread}
    base = IMAGE_BASE_DEFAULT
    originals = {}
    armed = set()
    pending = {}
    hits = []
    hit_file = open(os.path.join(args.out_dir, "hits.jsonl"), "w", encoding="utf-8")
    handler_table = None
    handler_groups = None
    click_tick = None
    input_sent = []
    input_done = False
    input_pending = None
    input_down_tick = None
    input_down_at = None
    input_hold_ticks = 0
    trigger_hit = None
    trigger_disabled = False
    started = time.time()
    stop = None
    window = None

    def ensure_window():
        nonlocal window
        if window is None:
            found = find_engine_window(info.dwProcessId)
            if found:
                window = found
        return window

    def read_dispatch_table():
        nonlocal handler_table, handler_groups
        if handler_table is not None:
            return
        table_bytes = read_mem(hproc, base + DISPATCH_TABLE_RVA, DISPATCH_TABLE_LEN * 4)
        if not (table_bytes["ok"] and table_bytes["got"] == DISPATCH_TABLE_LEN * 4):
            return
        values = [int.from_bytes(table_bytes["bytes"][i * 4:(i + 1) * 4], "little")
                  for i in range(DISPATCH_TABLE_LEN)]
        # 引擎启动早期该表还是 0;至少要有多个不同的非零 handler 才接受。
        if len({value for value in values if value != 0}) < 10:
            return
        handler_table = values
        groups = {}
        for index, handler in enumerate(values):
            groups.setdefault(handler, []).append(index)
        handler_groups = groups
        print("dispatch table", hex(base + DISPATCH_TABLE_RVA),
              "unique", len(groups), flush=True)

    def arm_address(rva):
        address = base + rva
        if address in armed:
            return
        original = read_mem(hproc, address, 1)
        if original["ok"] and original["got"] == 1:
            originals[address] = original["bytes"]
            if write_mem(hproc, address, b"\xcc")["ok"]:
                armed.add(address)
                print("armed", hex(address), original["bytes"].hex(), flush=True)
            else:
                print("arm failed", hex(address), last_error(), flush=True)

    def arm():
        read_dispatch_table()
        for rva in bps:
            arm_address(rva)

    while True:
        elapsed = time.time() - started
        if elapsed > args.seconds:
            stop = "timeout"
            break
        if args.input_at is not None and not input_done and elapsed >= args.input_at:
            input_done = True
            found = ensure_window()
            if found:
                click_tick = read_u64(hproc, base + TICK_COUNTER_RVA)
                input_sent, input_notes, input_pending = send_input(
                    found[0], args.input, *args.click_xy, args.input_method,
                    force_foreground=not armed)
                manifest["input_notes"] = input_notes
                if input_pending:
                    input_down_tick = read_u64(hproc, base + TICK_COUNTER_RVA)
                    input_down_at = time.time()
                print("input", input_sent, "tick", click_tick, flush=True)
            else:
                input_sent = ["window-not-found"]
        if input_pending is not None:
            current_tick = read_u64(hproc, base + TICK_COUNTER_RVA)
            held = time.time() - input_down_at if input_down_at else 0.0
            progressed = (current_tick is not None and input_down_tick is not None
                          and current_tick > input_down_tick)
            if progressed or held > 5.0:
                released = release_input(input_pending)
                input_sent.extend(released)
                manifest["input_notes"].extend(released)
                if progressed:
                    input_hold_ticks = current_tick - input_down_tick
                manifest["input_hold_ticks"] = input_hold_ticks
                manifest["input_hold_seconds"] = round(held, 3)
                print("release", released, "held_ticks", input_hold_ticks, flush=True)
                input_pending = None
        if (not trigger_rva and not armed and elapsed >= args.arm_after):
            arm()
        event = DEBUG_EVENT()
        if not k.WaitForDebugEvent(ctypes.byref(event), 100):
            if armed:
                read_dispatch_table()
            continue
        code = event.dwDebugEventCode
        status = DBG_CONTINUE
        if code == CREATE_PROCESS_DEBUG_EVENT:
            proc = event.u.CreateProcessInfo
            base = proc.lpBaseOfImage or IMAGE_BASE_DEFAULT
            threads[event.dwThreadId] = proc.hThread
            manifest["module_base"] = hex(base)
            trigger_addr = base + trigger_rva if trigger_rva else None
            wow = wt.BOOL()
            k.IsWow64Process(hproc, ctypes.byref(wow))
            manifest["is_wow64"] = bool(wow.value)
            if trigger_rva:
                arm_address(trigger_rva)
            elif args.arm_after <= 0:
                arm()
        elif code == CREATE_THREAD_DEBUG_EVENT:
            threads[event.dwThreadId] = event.u.CreateThread.hThread
        elif code == EXCEPTION_DEBUG_EVENT:
            record = event.u.Exception.ExceptionRecord
            exception = record.ExceptionCode
            if exception in (EXCEPTION_BREAKPOINT, EXCEPTION_WX86_BREAKPOINT):
                hthread = threads.get(event.dwThreadId)
                ctx = CONTEXT()
                ctx.ContextFlags = CTX_FLAGS
                got_ctx = bool(hthread) and bool(k.Wow64GetThreadContext(hthread, ctypes.byref(ctx)))
                site = ctx.Eip - 1 if got_ctx else None
                if got_ctx and site is not None and site == trigger_addr:
                    write_mem(hproc, site, originals[site])
                    ctx.Eip = site
                    ctx.EFlags |= EFLAGS_TF
                    k.Wow64SetThreadContext(hthread, ctypes.byref(ctx))
                    pending[event.dwThreadId] = site
                    trigger_disabled = True
                    trigger_hit = {
                        "t": round(time.time() - started, 3),
                        "tick": read_u64(hproc, base + TICK_COUNTER_RVA),
                        "site": hex(site),
                        "window_index_plus1": ctx.Eax,
                        "lparam": read_u32(hproc, ctx.Esp + 0x30),
                    }
                    manifest["trigger"] = trigger_hit
                    print("TRIGGER left-button-down", trigger_hit, flush=True)
                    arm()
                elif got_ctx and site in armed:
                    write_mem(hproc, site, originals[site])
                    ctx.Eip = site
                    ctx.EFlags |= EFLAGS_TF
                    k.Wow64SetThreadContext(hthread, ctypes.byref(ctx))
                    pending[event.dwThreadId] = site
                    read_dispatch_table()
                    task = read_u32(hproc, base + TASK_GLOBAL_RVA)
                    script = read_u16(hproc, task + 0x3C) if task else None
                    tick = read_u64(hproc, base + TICK_COUNTER_RVA)
                    handler = read_u32(hproc, ctx.Ecx + ctx.Edi * 4)
                    candidates = None
                    cmd = None
                    if handler is not None and handler_groups is not None:
                        candidates = handler_groups.get(handler)
                        if candidates is not None and len(candidates) == 1:
                            cmd = candidates[0]
                    row = {
                        "i": len(hits),
                        "t": round(time.time() - started, 3),
                        "tid": event.dwThreadId,
                        "site": hex(site),
                        "tick": tick,
                        "pc": ctx.Edi,
                        "script": script,
                        "task": hex(task) if task else None,
                        "ecx": hex(ctx.Ecx),
                        "handler": hex(handler) if handler is not None else None,
                        "cmd": cmd,
                        "cmd_candidates": candidates,
                        "cmd_name": names.get(cmd, "") if cmd is not None else "",
                    }
                    hits.append(row)
                    hit_file.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
                    hit_file.flush()
                    if len(hits) <= 5 or len(hits) % 1000 == 0:
                        print("hit", row["i"], "tick", tick, "s", script, "pc", ctx.Edi,
                              "cmd", cmd, row["cmd_name"], flush=True)
                    if len(hits) >= args.max_hits:
                        stop = "cap"
                        break
                else:
                    status = DBG_EXCEPTION_NOT_HANDLED
            elif exception in (EXCEPTION_SINGLE_STEP, EXCEPTION_WX86_SINGLE_STEP):
                site = pending.pop(event.dwThreadId, None)
                if site is not None:
                    hthread = threads.get(event.dwThreadId)
                    ctx = CONTEXT()
                    ctx.ContextFlags = CTX_FLAGS
                    if hthread and k.Wow64GetThreadContext(hthread, ctypes.byref(ctx)):
                        ctx.EFlags &= ~EFLAGS_TF
                        k.Wow64SetThreadContext(hthread, ctypes.byref(ctx))
                    write_mem(hproc, site, b"\xcc")
                else:
                    status = DBG_EXCEPTION_NOT_HANDLED
        elif code == EXIT_PROCESS_DEBUG_EVENT:
            manifest["exit_code"] = event.u.ExitProcess.dwExitCode
            stop = "exit"
            k.ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status)
            break
        k.ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status)
        if stop:
            break
    hit_file.close()

    # 收尾:把断点字节恢复(进程随后终止),不把修改留在运行中的引擎里。
    for address in list(armed):
        write_mem(hproc, address, originals[address])
    k.TerminateProcess(hproc, 1)
    k.CloseHandle(hproc)

    if handler_table is not None:
        with open(os.path.join(args.out_dir, "command_table.json"), "w", encoding="utf-8") as h:
            json.dump({
                "table_rva": hex(DISPATCH_TABLE_RVA),
                "length": DISPATCH_TABLE_LEN,
                "handlers": [hex(value) for value in handler_table],
                "shared_handlers": {hex(handler): [names.get(index, "") for index in indices]
                                    for handler, indices in handler_groups.items()
                                    if len(indices) > 1},
            }, h, ensure_ascii=False, indent=2)

    manifest.update({
        "stop": stop,
        "hits": len(hits),
        "unique_cmd_hits": sum(1 for hit in hits if hit["cmd"] is not None),
        "ambiguous_cmd_hits": sum(1 for hit in hits if hit["cmd"] is None
                                  and hit["cmd_candidates"]),
        "unknown_handler_hits": sum(1 for hit in hits if not hit["cmd_candidates"]),
        "scripts": sorted({hit["script"] for hit in hits if hit["script"] is not None}),
        "ticks": {"first": hits[0]["tick"] if hits else None,
                  "last": hits[-1]["tick"] if hits else None},
        "click_tick": click_tick,
        "input_sent": input_sent,
        "finished": time.strftime("%Y-%m-%dT%H:%M:%S"),
    })
    if args.trace_out:
        trace = build_trace(hits, commands, sample_sha, args, click_tick=click_tick)
        os.makedirs(os.path.dirname(os.path.abspath(args.trace_out)), exist_ok=True)
        with open(args.trace_out, "w", encoding="utf-8", newline="\n") as h:
            h.write(trace)
        manifest["trace"] = args.trace_out
        manifest["trace_sha256"] = sha256_bytes(trace.encode("utf-8"))
        manifest["trace_records"] = trace.count(chr(10)) - 1
        print("wrote", args.trace_out)
    with open(os.path.join(args.out_dir, "manifest.json"), "w", encoding="utf-8") as h:
        json.dump(manifest, h, ensure_ascii=False, indent=2)
    print("stop", stop, "hits", len(hits))
    return 0


if __name__ == "__main__":
    sys.exit(main())
