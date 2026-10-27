"""按层指针 dump 官方层对象(`+0x80..+0x240`),取过渡的部件/分块表。

用途:补齐 `FUN_00414ba4` 的网格数据——`+0xa6`(部件数)、`+0xa7`(走不走网格)、
`+0xb0`(分块高度表)、`+0x130`(部件宽度表),以及 `+0x80/+0x82/+0x84`(=A/T/TA)。
这些字段由引擎运行时填进层对象,yssd 记录里没有。

做法:调试器在 `FUN_00414ba4` 入口(默认 RVA `0x14ba4`)下一个 `cc`,
命中时读 `EAX` = 层指针,把这段内存原样打印;然后恢复字节、单步、重新布防。

**只读**:唯一的写是断点字节 `cc` 与恢复原字节。

产物(仓库外/`target`,不入库):`<out>/layer_dump.jsonl`
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import struct
import sys
import time

k = ctypes.windll.kernel32

IMAGE_BASE_DEFAULT = 0x400000
TRANSITION_RVA = 0x14BA4  # FUN_00414ba4 - 0x400000
TICK_COUNTER_RVA = 0x401268  # 0x801268: 每 tick 的 u64 计数器(心跳用)
INT3 = bytes([0xCC])

CREATE_THREAD_DEBUG_EVENT = 2
CREATE_PROCESS_DEBUG_EVENT = 3
EXIT_PROCESS_DEBUG_EVENT = 5
EXCEPTION_DEBUG_EVENT = 1
EXCEPTION_BREAKPOINT = 0x80000003
EXCEPTION_SINGLE_STEP = 0x80000004
EXCEPTION_WX86_BREAKPOINT = 0x4000001F
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001

CONTEXT_i386 = 0x00010000
CTX_FLAGS = CONTEXT_i386 | 1 | 2  # CONTROL | INTEGER
EFLAGS_TF = 0x100


class CONTEXT(ctypes.Structure):
    _fields_ = [
        ("ContextFlags", wt.DWORD),
        ("Dr0", wt.DWORD), ("Dr1", wt.DWORD), ("Dr2", wt.DWORD), ("Dr3", wt.DWORD),
        ("Dr6", wt.DWORD), ("Dr7", wt.DWORD),
        ("FloatSave", ctypes.c_byte * 112),
        ("SegGs", wt.DWORD), ("SegFs", wt.DWORD), ("SegEs", wt.DWORD), ("SegDs", wt.DWORD),
        ("Edi", wt.DWORD), ("Esi", wt.DWORD), ("Ebx", wt.DWORD), ("Edx", wt.DWORD),
        ("Ecx", wt.DWORD), ("Eax", wt.DWORD),
        ("Ebp", wt.DWORD), ("Eip", wt.DWORD), ("SegCs", wt.DWORD), ("EFlags", wt.DWORD),
        ("Esp", wt.DWORD), ("SegSs", wt.DWORD),
        ("ExtendedRegisters", ctypes.c_byte * 512),
    ]


class STARTUPINFOA(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD), ("lpReserved", wt.LPSTR), ("lpDesktop", wt.LPSTR),
        ("lpTitle", wt.LPSTR), ("dwX", wt.DWORD), ("dwY", wt.DWORD),
        ("dwXSize", wt.DWORD), ("dwYSize", wt.DWORD),
        ("dwXCountChars", wt.DWORD), ("dwYCountChars", wt.DWORD),
        ("dwFillAttribute", wt.DWORD), ("dwFlags", wt.DWORD),
        ("wShowWindow", wt.WORD), ("cbReserved2", wt.WORD),
        ("lpReserved2", ctypes.c_void_p), ("hStdInput", wt.HANDLE),
        ("hStdOutput", wt.HANDLE), ("hStdError", wt.HANDLE),
    ]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", wt.HANDLE), ("hThread", wt.HANDLE),
                ("dwProcessId", wt.DWORD), ("dwThreadId", wt.DWORD)]


class DEBUG_EVENT(ctypes.Structure):
    class _U(ctypes.Union):
        class _EXCEPTION(ctypes.Structure):
            _fields_ = [("ExceptionRecord", ctypes.c_byte * 80),
                        ("dwFirstChance", wt.DWORD)]

        class _CREATE_THREAD(ctypes.Structure):
            _fields_ = [("hThread", wt.HANDLE), ("lpThreadLocalBase", ctypes.c_void_p),
                        ("lpStartAddress", ctypes.c_void_p)]

        class _CREATE_PROCESS(ctypes.Structure):
            _fields_ = [("hFile", wt.HANDLE), ("hProcess", wt.HANDLE),
                        ("hThread", wt.HANDLE), ("lpBaseOfImage", ctypes.c_void_p),
                        ("dwDebugInfoFileOffset", wt.DWORD),
                        ("nDebugInfoSize", wt.DWORD),
                        ("lpThreadLocalBase", ctypes.c_void_p),
                        ("lpStartAddress", ctypes.c_void_p),
                        ("lpImageName", ctypes.c_void_p),
                        ("fUnicode", wt.WORD)]

        _fields_ = [("Exception", _EXCEPTION), ("CreateThread", _CREATE_THREAD),
                    ("CreateProcessInfo", _CREATE_PROCESS)]

    _fields_ = [("dwDebugEventCode", wt.DWORD), ("dwProcessId", wt.DWORD),
                ("dwThreadId", wt.DWORD), ("u", _U)]


k.CreateProcessA.argtypes = [wt.LPCSTR, wt.LPSTR, ctypes.c_void_p, ctypes.c_void_p,
                             wt.BOOL, wt.DWORD, ctypes.c_void_p, wt.LPCSTR,
                             ctypes.POINTER(STARTUPINFOA), ctypes.POINTER(PROCESS_INFORMATION)]
k.CreateProcessA.restype = wt.BOOL
k.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), wt.DWORD]
k.ContinueDebugEvent.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD]
k.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                ctypes.POINTER(ctypes.c_size_t)]
k.WriteProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                 ctypes.POINTER(ctypes.c_size_t)]
k.Wow64GetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
k.Wow64SetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
k.FlushInstructionCache.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_size_t]
k.DebugActiveProcessStop.argtypes = [wt.DWORD]
k.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]


def read_mem(hproc, address, size):
    buffer = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t()
    ok = k.ReadProcessMemory(hproc, ctypes.c_void_p(address), buffer, size, ctypes.byref(got))
    if not ok or got.value != size:
        return None
    return buffer.raw


def write_mem(hproc, address, data):
    got = ctypes.c_size_t()
    buffer = ctypes.create_string_buffer(data, len(data))
    ok = k.WriteProcessMemory(hproc, ctypes.c_void_p(address), buffer, len(data),
                              ctypes.byref(got))
    return bool(ok) and got.value == len(data)


def read_u32(hproc, address):
    data = read_mem(hproc, address, 4)
    return struct.unpack("<I", data)[0] if data else None


def read_u16(data, offset):
    if not (0 <= offset <= len(data) - 2):
        return None
    return struct.unpack_from("<h", data, offset)[0]


def read_i32(data, offset):
    if not (0 <= offset <= len(data) - 4):
        return None
    return struct.unpack_from("<i", data, offset)[0]


def read_u8(data, offset):
    return data[offset] if 0 <= offset < len(data) else None


def parse_layer(raw):
    """按 `FUN_00414ba4` 用到的偏移解析(基准 = 层指针)。"""
    return {
        "A": read_u16(raw, 0x80),
        "T": read_u16(raw, 0x82),
        "TA": read_u16(raw, 0x84),
        "b86": read_u8(raw, 0x86),
        "surface": read_i32(raw, 0x90),
        "mode": read_u8(raw, 0xA4),
        "parts": read_u8(raw, 0xA6),
        "grid": read_u8(raw, 0xA7),
        "rect_raw": [read_i32(raw, off) for off in (0x1B0, 0x1B8, 0x1C0, 0x1C8, 0x1D0, 0x1D8)],
        "rot_raw": [read_i32(raw, off) for off in (0x1F8, 0x1FC, 0x200)],
        "texture0": read_i32(raw, 0x21C),
        "blocks_table": [read_i32(raw, 0xB0 + i * 4) for i in range((0x130 - 0xB0) // 4)],
        "parts_table": [read_i32(raw, 0x130 + i * 4) for i in range((0x21C - 0x130) // 4)],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--rva", default=hex(TRANSITION_RVA))
    parser.add_argument("--max-hits", type=int, default=2000)
    parser.add_argument("--duration", type=float, default=900.0)
    parser.add_argument("--layer-span", type=int, default=0x240)
    args = parser.parse_args()

    rva = int(args.rva, 0)
    os.makedirs(args.out, exist_ok=True)
    startup = STARTUPINFOA()
    startup.cb = ctypes.sizeof(STARTUPINFOA)
    info = PROCESS_INFORMATION()
    ok = k.CreateProcessA(args.exe.encode("mbcs"), None, None, None, False,
                          0x00000002,  # DEBUG_ONLY_THIS_PROCESS
                          None, args.cwd.encode("mbcs"), ctypes.byref(startup),
                          ctypes.byref(info))
    if not ok:
        print(f"CreateProcess failed: {k.GetLastError()}", file=sys.stderr)
        return 2
    hproc, pid = info.hProcess, info.dwProcessId
    print(f"pid={pid} breakpoint RVA={hex(rva)} (passive)", flush=True)

    threads = {}
    armed = set()
    originals = {}
    pending = {}
    hits = 0
    started = time.time()
    next_beat = started + 5.0
    base_abs = IMAGE_BASE_DEFAULT
    stop_reason = "duration"
    out_path = os.path.join(args.out, "layer_dump.jsonl")
    sink = open(out_path, "w", encoding="utf-8")
    while True:
        if time.time() - started > args.duration:
            stop_reason = "duration"
            break
        event = DEBUG_EVENT()
        if not k.WaitForDebugEvent(ctypes.byref(event), 200):
            continue
        code = event.dwDebugEventCode
        status = DBG_CONTINUE
        if code == CREATE_PROCESS_DEBUG_EVENT:
            process = event.u.CreateProcessInfo
            base = (ctypes.cast(process.lpBaseOfImage, ctypes.c_void_p).value
                    if process.lpBaseOfImage else IMAGE_BASE_DEFAULT)
            base_abs = base
            threads[event.dwThreadId] = process.hThread
            address = base + rva
            current = read_mem(hproc, address, 1)
            if current and current != INT3 and write_mem(hproc, address, INT3):
                armed.add(address)
                originals[address] = current
                k.FlushInstructionCache(hproc, ctypes.c_void_p(address), 1)
                print(f"armed {hex(address)} base={hex(base)}", flush=True)
        elif code == CREATE_THREAD_DEBUG_EVENT:
            threads[event.dwThreadId] = event.u.CreateThread.hThread
        elif code == EXIT_PROCESS_DEBUG_EVENT:
            stop_reason = "process-exit"
            k.ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status)
            break
        elif code == EXCEPTION_DEBUG_EVENT:
            record = ctypes.cast(ctypes.byref(event.u.Exception.ExceptionRecord),
                                 ctypes.POINTER(wt.DWORD))
            exception = record.contents.value
            hthread = threads.get(event.dwThreadId)
            ctx = CONTEXT()
            ctx.ContextFlags = CTX_FLAGS
            got = bool(hthread) and bool(
                k.Wow64GetThreadContext(hthread, ctypes.byref(ctx)))
            if exception == EXCEPTION_SINGLE_STEP:
                rearm = pending.pop(event.dwThreadId, None)
                if got and rearm is not None:
                    write_mem(hproc, rearm, INT3)
                    k.FlushInstructionCache(hproc, ctypes.c_void_p(rearm), 1)
                    ctx.EFlags &= ~EFLAGS_TF
                    k.Wow64SetThreadContext(hthread, ctypes.byref(ctx))
                status = DBG_CONTINUE
            elif exception in (EXCEPTION_BREAKPOINT, EXCEPTION_WX86_BREAKPOINT):
                # 一律吞掉,否则进程会在首个系统断点处退出。
                site = ctx.Eip - 1 if got else None
                if got and site in armed:
                    write_mem(hproc, site, originals[site])
                    ctx.Eip = site
                    ctx.EFlags |= EFLAGS_TF
                    k.Wow64SetThreadContext(hthread, ctypes.byref(ctx))
                    pending[event.dwThreadId] = site
                    layer = ctx.Eax
                    raw = read_mem(hproc, layer, args.layer_span)
                    if raw:
                        stack = [read_u32(hproc, ctx.Esp + 4 + i * 4) for i in range(6)]
                        row = {
                            "i": hits,
                            "t": round(time.time() - started, 3),
                            "layer": hex(layer),
                            "stack_args": stack,
                            "raw": raw.hex(),
                            "fields": parse_layer(raw),
                        }
                        sink.write(json.dumps(row, ensure_ascii=False) + "\n")
                        sink.flush()
                        f = row["fields"]
                        print(
                            f"[{hits}] layer={hex(layer)} A={f['A']} T={f['T']} TA={f['TA']}"
                            f" mode={f['mode']} parts={f['parts']} grid={f['grid']}"
                            f" blocks={f['blocks_table'][:6]}"
                            f" partsTbl={f['parts_table'][:6]} args={stack}",
                            flush=True,
                        )
                    hits += 1
                    if hits >= args.max_hits:
                        for address in list(armed):
                            write_mem(hproc, address, originals[address])
                            k.FlushInstructionCache(hproc, ctypes.c_void_p(address), 1)
                        armed.clear()
                        print("breakpoint removed; free running", flush=True)
                status = DBG_CONTINUE
            else:
                status = DBG_EXCEPTION_NOT_HANDLED
        k.ContinueDebugEvent(event.dwProcessId, event.dwThreadId, status)
    sink.close()
    print(f"stop: {stop_reason} hits={hits} -> {out_path}", flush=True)
    try:
        k.DebugActiveProcessStop(pid)
    except Exception:
        pass
    k.TerminateProcess(hproc, 0)
    return 0


if __name__ == "__main__":
    sys.exit(main())
