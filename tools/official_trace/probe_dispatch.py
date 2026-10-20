#!/usr/bin/env python3
"""0.495 命令分派只读探针(事件级 L1 的起点,当前未打通)。

方法:64 位 Python 以 DEBUG_ONLY_THIS_PROCESS 启动 32 位官方引擎,清 PEB
反调试点,在命令分派 `0x4044db: call dword ptr [ecx + edi*4]` 下 int3;
命中时读 `[0x80171c]` 当前任务、`task+0x3c` 脚本号、`EDI` pc、
`[ECX+EDI*4]` handler。输出 JSONL 供后续经 `0x7ace20` 命令表反查命令名。

现状(2026-10-20):标题画面 25 s 内 0 命中;即使延迟 3 s 布防,
`ReadProcessMemory(0x400000/0x401000/0x4044db)` 仍返回 error 299
(ERROR_PARTIAL_COPY)。这说明断点尚未真正落下,**不是**"官方没有事件";
不得把 0 命中结果当作事件级 trace。下一步见 README §4。

用法:
  python tools/official_trace/probe_dispatch.py     --exe <官方 yu-ris.exe> --cwd <工程根>     --out target/tmp/official/dispatch_hits.jsonl --seconds 15 --click-after 5
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import sys
import time

EXE = r"D:\Art\ai\projects\YurisDroid\workbench\project-eris\yu-ris.exe"
CWD = r"D:\Art\ai\projects\YurisDroid\workbench\project-eris"
OUT = r"D:\Art\ai\projects\YurisDroid\workbench\tmp\official_probe\debug_hits.jsonl"
BP_RVA = 0x44DB
GLOBAL_TASK = 0x80171C
IMAGE_BASE = 0x400000

k = ctypes.windll.kernel32
DEBUG_ONLY_THIS_PROCESS = 2
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001
EXCEPTION_DEBUG_EVENT = 1
CREATE_PROCESS_DEBUG_EVENT = 3
EXIT_PROCESS_DEBUG_EVENT = 5
CREATE_THREAD_DEBUG_EVENT = 2
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


class EXCEPTION_RECORD(ctypes.Structure):
    _fields_ = [
        ("ExceptionCode", wt.DWORD),
        ("ExceptionFlags", wt.DWORD),
        ("ExceptionRecord", ctypes.c_void_p),
        ("ExceptionAddress", ctypes.c_void_p),
        ("NumberParameters", wt.DWORD),
        ("ExceptionInformation", ctypes.c_size_t * 15),
    ]


class EXCEPTION_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("ExceptionRecord", EXCEPTION_RECORD), ("dwFirstChance", wt.DWORD)]


class CREATE_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [
        ("hFile", wt.HANDLE),
        ("hProcess", wt.HANDLE),
        ("hThread", wt.HANDLE),
        ("lpBaseOfImage", ctypes.c_void_p),
        ("lpBaseOfDll", ctypes.c_void_p),
        ("dwDebugInfoFileOffset", wt.DWORD),
        ("fUnicode", wt.WORD),
        ("nDebugInfoSize", wt.DWORD),
        ("lpThreadLocalBase", ctypes.c_void_p),
        ("lpStartAddress", ctypes.c_void_p),
        ("lpImageName", ctypes.c_void_p),
        ("fUnicode2", wt.WORD),
    ]


class CREATE_THREAD_DEBUG_INFO(ctypes.Structure):
    _fields_ = [
        ("hThread", wt.HANDLE),
        ("lpThreadLocalBase", ctypes.c_void_p),
        ("lpStartAddress", ctypes.c_void_p),
    ]


class EXIT_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("dwExitCode", wt.DWORD)]


class LOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [
        ("hFile", wt.HANDLE),
        ("lpBaseOfDll", ctypes.c_void_p),
        ("hSection", wt.HANDLE),
        ("dwDebugInfoFileOffset", wt.DWORD),
        ("nDebugInfoSize", wt.DWORD),
        ("lpImageName", ctypes.c_void_p),
        ("fUnicode", wt.WORD),
    ]


class UNLOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("lpBaseOfDll", ctypes.c_void_p)]


class OUTPUT_DEBUG_STRING_INFO(ctypes.Structure):
    _fields_ = [
        ("lpDebugStringData", ctypes.c_void_p),
        ("fUnicode", wt.WORD),
        ("nDebugStringLength", wt.WORD),
    ]


class RIP_INFO(ctypes.Structure):
    _fields_ = [("dwError", wt.DWORD), ("dwType", wt.DWORD)]


class _U(ctypes.Union):
    _fields_ = [
        ("Exception", EXCEPTION_DEBUG_INFO),
        ("CreateThread", CREATE_THREAD_DEBUG_INFO),
        ("CreateProcessInfo", CREATE_PROCESS_DEBUG_INFO),
        ("ExitProcess", EXIT_PROCESS_DEBUG_INFO),
        ("LoadDll", LOAD_DLL_DEBUG_INFO),
        ("UnloadDll", UNLOAD_DLL_DEBUG_INFO),
        ("DebugString", OUTPUT_DEBUG_STRING_INFO),
        ("RipInfo", RIP_INFO),
    ]


class DEBUG_EVENT(ctypes.Structure):
    _fields_ = [
        ("dwDebugEventCode", wt.DWORD),
        ("dwProcessId", wt.DWORD),
        ("dwThreadId", wt.DWORD),
        ("u", _U),
    ]


class FLOATING_SAVE_AREA(ctypes.Structure):
    _fields_ = [
        (name, wt.DWORD)
        for name in (
            "ControlWord",
            "StatusWord",
            "TagWord",
            "ErrorOffset",
            "ErrorSelector",
            "DataOffset",
            "DataSelector",
        )
    ] + [("RegisterArea", ctypes.c_ubyte * 80), ("Cr0NpxState", wt.DWORD)]


class CONTEXT(ctypes.Structure):
    _fields_ = [
        ("ContextFlags", wt.DWORD),
        ("Dr0", wt.DWORD),
        ("Dr1", wt.DWORD),
        ("Dr2", wt.DWORD),
        ("Dr3", wt.DWORD),
        ("Dr6", wt.DWORD),
        ("Dr7", wt.DWORD),
        ("FloatSave", FLOATING_SAVE_AREA),
        ("SegGs", wt.DWORD),
        ("SegFs", wt.DWORD),
        ("SegEs", wt.DWORD),
        ("SegDs", wt.DWORD),
        ("Edi", wt.DWORD),
        ("Esi", wt.DWORD),
        ("Ebx", wt.DWORD),
        ("Edx", wt.DWORD),
        ("Ecx", wt.DWORD),
        ("Eax", wt.DWORD),
        ("Ebp", wt.DWORD),
        ("Eip", wt.DWORD),
        ("SegCs", wt.DWORD),
        ("EFlags", wt.DWORD),
        ("Esp", wt.DWORD),
        ("SegSs", wt.DWORD),
        ("ExtendedRegisters", ctypes.c_ubyte * 512),
    ]


class STARTUPINFO(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD),
        ("lpReserved", wt.LPWSTR),
        ("lpDesktop", wt.LPWSTR),
        ("lpTitle", wt.LPWSTR),
        ("dwX", wt.DWORD),
        ("dwY", wt.DWORD),
        ("dwXSize", wt.DWORD),
        ("dwYSize", wt.DWORD),
        ("dwXCountChars", wt.DWORD),
        ("dwYCountChars", wt.DWORD),
        ("dwFillAttribute", wt.DWORD),
        ("dwFlags", wt.DWORD),
        ("wShowWindow", wt.WORD),
        ("cbReserved2", wt.WORD),
        ("lpReserved2", ctypes.c_void_p),
        ("hStdInput", wt.HANDLE),
        ("hStdOutput", wt.HANDLE),
        ("hStdError", wt.HANDLE),
    ]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("hProcess", wt.HANDLE),
        ("hThread", wt.HANDLE),
        ("dwProcessId", wt.DWORD),
        ("dwThreadId", wt.DWORD),
    ]


k.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), wt.DWORD]
k.WaitForDebugEvent.restype = wt.BOOL
k.ContinueDebugEvent.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD]
k.ContinueDebugEvent.restype = wt.BOOL
k.CreateProcessW.argtypes = [
    wt.LPCWSTR,
    wt.LPWSTR,
    ctypes.c_void_p,
    ctypes.c_void_p,
    wt.BOOL,
    wt.DWORD,
    ctypes.c_void_p,
    wt.LPCWSTR,
    ctypes.POINTER(STARTUPINFO),
    ctypes.POINTER(PROCESS_INFORMATION),
]
k.CreateProcessW.restype = wt.BOOL
k.ReadProcessMemory.argtypes = [
    wt.HANDLE,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_size_t,
    ctypes.POINTER(ctypes.c_size_t),
]
k.ReadProcessMemory.restype = wt.BOOL
k.WriteProcessMemory.argtypes = [
    wt.HANDLE,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_size_t,
    ctypes.POINTER(ctypes.c_size_t),
]
k.WriteProcessMemory.restype = wt.BOOL
k.Wow64GetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
k.Wow64GetThreadContext.restype = wt.BOOL
k.Wow64SetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
k.Wow64SetThreadContext.restype = wt.BOOL
k.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k.OpenProcess.restype = wt.HANDLE
k.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]
k.TerminateProcess.restype = wt.BOOL
ntdll = ctypes.windll.ntdll
ntdll.NtQueryInformationProcess.argtypes = [
    wt.HANDLE,
    ctypes.c_uint,
    ctypes.c_void_p,
    ctypes.c_uint,
    ctypes.POINTER(ctypes.c_uint),
]
ntdll.NtQueryInformationProcess.restype = ctypes.c_long


def hide_debugger(hproc):
    peb = ctypes.c_uint64(0)
    status = ntdll.NtQueryInformationProcess(
        hproc, 26, ctypes.byref(peb), ctypes.sizeof(peb), None
    )
    if status == 0 and peb.value:
        address = peb.value
        got = ctypes.c_size_t(0)
        k.WriteProcessMemory(hproc, ctypes.c_void_p(address + 2), b"\x00", 1, ctypes.byref(got))
        k.WriteProcessMemory(hproc, ctypes.c_void_p(address + 0x68), b"\x00" * 4, 4, ctypes.byref(got))
        return True
    return False


def read_mem(hproc, address, size):
    buf = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t(0)
    if not k.ReadProcessMemory(hproc, ctypes.c_void_p(address), buf, size, ctypes.byref(got)):
        return None
    return buf.raw[: got.value]


def write_mem(hproc, address, data):
    got = ctypes.c_size_t(0)
    return bool(k.WriteProcessMemory(hproc, ctypes.c_void_p(address), data, len(data), ctypes.byref(got)))


def get_ctx(hthread):
    ctx = CONTEXT()
    ctx.ContextFlags = CTX_FLAGS
    if not k.Wow64GetThreadContext(hthread, ctypes.byref(ctx)):
        return None
    return ctx


def set_ctx(hthread, ctx):
    ctx.ContextFlags = CTX_FLAGS
    return bool(k.Wow64SetThreadContext(hthread, ctypes.byref(ctx)))


def find_window(pid):
    found = []

    def callback(hwnd, _):
        owner = wt.DWORD()
        user32 = ctypes.windll.user32
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid:
            return True
        rect = wt.RECT()
        user32.GetClientRect(hwnd, ctypes.byref(rect))
        if user32.IsWindowVisible(hwnd) and rect.right >= 320 and rect.bottom >= 240:
            found.append(hwnd)
        return True

    user32 = ctypes.windll.user32
    user32.EnumWindows(
        ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)(callback), 0
    )
    return found[0] if found else None


def send_click(hwnd):
    user32 = ctypes.windll.user32
    user32.SetForegroundWindow(hwnd)
    # 标题画面的「スタート」大致在客户区 (410, 275);先按 Enter。
    user32.PostMessageW(hwnd, 0x0100, 0x0D, 0)
    user32.PostMessageW(hwnd, 0x0101, 0x0D, 0)
    user32.PostMessageW(hwnd, 0x0201, 1, (275 << 16) | 410)
    user32.PostMessageW(hwnd, 0x0202, 0, (275 << 16) | 410)


def main():
    global EXE, CWD, OUT, BP_RVA
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default=EXE)
    parser.add_argument("--cwd", default=CWD)
    parser.add_argument("--out", default=OUT)
    parser.add_argument("--bp-rva", default=hex(BP_RVA))
    parser.add_argument("--seconds", type=float, default=25.0)
    parser.add_argument("--max-hits", type=int, default=400)
    parser.add_argument("--click-after", type=float, default=6.0)
    args = parser.parse_args()
    EXE, CWD, OUT = args.exe, args.cwd, args.out
    if not EXE:
        parser.error("--exe is required")
    BP_RVA = int(args.bp_rva, 0)

    startup = STARTUPINFO()
    startup.cb = ctypes.sizeof(startup)
    info = PROCESS_INFORMATION()
    if not k.CreateProcessW(
        EXE, None, None, None, False, DEBUG_ONLY_THIS_PROCESS, None, CWD,
        ctypes.byref(startup), ctypes.byref(info),
    ):
        print("CreateProcess failed", ctypes.GetLastError())
        return 2
    print("pid", info.dwProcessId)
    threads = {info.dwThreadId: info.hThread}
    hproc = info.hProcess
    base = 0
    reloc = 0
    breakpoints = {}
    pending = {}
    out = open(OUT, "w", encoding="utf-8")
    hits = 0
    started = time.time()
    stop = None
    clicked = False
    arm_pending = False
    arm_at = 0.0
    while True:
        if time.time() - started > args.seconds:
            stop = "timeout"
            break
        event = DEBUG_EVENT()
        if not k.WaitForDebugEvent(ctypes.byref(event), 500):
            if arm_pending and time.time() >= arm_at:
                address = BP_RVA + reloc
                for probe in (address, IMAGE_BASE + reloc, IMAGE_BASE + 0x1000 + reloc):
                    raw = read_mem(hproc, probe, 1)
                    print("probe", hex(probe), raw.hex() if raw else None, "err", ctypes.GetLastError())
                original = read_mem(hproc, address, 1)
                if original and write_mem(hproc, address, b"\xcc"):
                    breakpoints[address] = original[0]
                    arm_pending = False
                    print("bp", hex(address))
                else:
                    print("arm still failing err", ctypes.GetLastError())
            if not clicked and time.time() - started >= args.click_after:
                hwnd = find_window(info.dwProcessId)
                print("window", hwnd)
                if hwnd:
                    send_click(hwnd)
                    print("sent click/enter")
                clicked = True
            continue
        pid = event.dwProcessId
        tid = event.dwThreadId
        code = event.dwDebugEventCode
        status = DBG_CONTINUE
        if code == CREATE_PROCESS_DEBUG_EVENT:
            proc = event.u.CreateProcessInfo
            base = proc.lpBaseOfImage or 0
            reloc = base - IMAGE_BASE
            opened = k.OpenProcess(0x1F0FFF, False, pid)
            hproc = opened or proc.hProcess
            print("base", hex(base), "reloc", hex(reloc), "hide", hide_debugger(hproc), "hproc", hex(hproc))
            arm_pending = True
            arm_at = time.time() + 3.0
            print("defer breakpoint for 3s")
        elif code == CREATE_THREAD_DEBUG_EVENT:
            threads[tid] = event.u.CreateThread.hThread
        elif code == EXCEPTION_DEBUG_EVENT:
            if arm_pending:
                address = BP_RVA + reloc
                for probe in (address, IMAGE_BASE + reloc, IMAGE_BASE + 0x1000 + reloc):
                    raw = read_mem(hproc, probe, 1)
                    print("probe", hex(probe), raw.hex() if raw else None, "err", ctypes.GetLastError())
                original = read_mem(hproc, address, 1)
                if original and write_mem(hproc, address, b"\xcc"):
                    breakpoints[address] = original[0]
                    arm_pending = False
                    print("bp", hex(address))
                else:
                    print("arm still failing err", ctypes.GetLastError())
            record = event.u.Exception.ExceptionRecord
            exception = record.ExceptionCode
            if exception in (EXCEPTION_BREAKPOINT, EXCEPTION_WX86_BREAKPOINT):
                hthread = threads.get(tid)
                ctx = get_ctx(hthread) if hthread else None
                hit = None
                if ctx:
                    for candidate in (ctx.Eip - 1, ctx.Eip):
                        if candidate in breakpoints:
                            hit = candidate
                            break
                if hit is not None:
                    write_mem(hproc, hit, bytes([breakpoints.pop(hit)]))
                    ctx.Eip = hit
                    ctx.EFlags |= EFLAGS_TF
                    set_ctx(hthread, ctx)
                    pending[tid] = hit
                    task_raw = read_mem(hproc, GLOBAL_TASK + reloc, 4)
                    task = int.from_bytes(task_raw, "little") if task_raw else 0
                    script = None
                    obj = None
                    candidate_bytes = None
                    if task:
                        raw = read_mem(hproc, task + 0x3C, 2)
                        script = int.from_bytes(raw, "little") if raw else None
                        raw = read_mem(hproc, task + 0x34, 4)
                        obj = int.from_bytes(raw, "little") if raw else None
                        if obj and ctx:
                            raw = read_mem(hproc, obj + 0x20 + ctx.Edi * 4, 4)
                            candidate_bytes = raw.hex() if raw else None
                    handler = None
                    if ctx:
                        raw = read_mem(hproc, ctx.Ecx + ctx.Edi * 4, 4)
                        handler = int.from_bytes(raw, "little") if raw else None
                    record_obj = {
                        "i": hits,
                        "t": round(time.time() - started, 3),
                        "pc": ctx.Edi if ctx else None,
                        "script": script,
                        "task": hex(task) if task else None,
                        "handler": hex(handler) if handler else None,
                        "obj": hex(obj) if obj else None,
                        "cand": candidate_bytes,
                        "regs": {
                            name: hex(getattr(ctx, name))
                            for name in ("Eax", "Ebx", "Ecx", "Edx", "Esi", "Edi", "Ebp", "Esp")
                        }
                        if ctx
                        else None,
                    }
                    out.write(json.dumps(record_obj, separators=(",", ":")) + "\n")
                    out.flush()
                    hits += 1
                    if hits <= 20 or hits % 100 == 0:
                        print(record_obj)
                    if hits >= args.max_hits:
                        stop = "cap"
                        break
                else:
                    status = DBG_EXCEPTION_NOT_HANDLED
            elif exception in (EXCEPTION_SINGLE_STEP, EXCEPTION_WX86_SINGLE_STEP):
                address = pending.pop(tid, None)
                if address is not None:
                    hthread = threads.get(tid)
                    ctx = get_ctx(hthread) if hthread else None
                    if ctx:
                        ctx.EFlags &= ~EFLAGS_TF
                        set_ctx(hthread, ctx)
                    original = read_mem(hproc, address, 1)
                    if original and write_mem(hproc, address, b"\xcc"):
                        breakpoints[address] = original[0]
                else:
                    status = DBG_EXCEPTION_NOT_HANDLED
        elif code == EXIT_PROCESS_DEBUG_EVENT:
            stop = "exit"
            k.ContinueDebugEvent(pid, tid, status)
            break
        if stop:
            k.ContinueDebugEvent(pid, tid, status)
            break
        k.ContinueDebugEvent(pid, tid, status)
    out.close()
    k.TerminateProcess(hproc, 1)
    print("done", stop, "hits", hits)
    return 0


if __name__ == "__main__":
    sys.exit(main())
