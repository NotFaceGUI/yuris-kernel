#!/usr/bin/env python3
"""官方 0.495 声音层树探针（只读）。

用途：启动官方引擎（工程根目录），等首屏稳定后 `ReadProcessMemory` 读出
声音层树、前缀字符串与 ID 字符分类表，作为 `docs/sound-ledger.md` N0-14 /
`docs/game-settings-ledger.md` §1.2 的运行时证据。

证据口径（0.495，sha256 4f5eefa3…）：
  [0x801718]  声音 ID 前缀指针（初始化从 .data:0x80bf54 复制 "S0"）
  [0x801708]  声音层表根节点
  节点 +0x00 首子 / +0x0c 下一兄弟 / +0x10 父 / +0x14 名字 / +0x18 名字长
       +0x30 容器标记 / +0x34 声音对象（+0x106 E，+0x148 VOLUME）
  0x689b80    256 字节字符分类表（运行时只有 '.' '/' ':' 非零）

用法：
  YURIS_SAMPLE_ERIS_ROOT=<project-eris> python tools/rea/sound_tree_probe.py [等待秒数]
缺样本时显式报错（不空跑）。
"""

import ctypes
import os
import struct
import subprocess
import sys
import time
from ctypes import wintypes as w

PROCESS_VM_READ = 0x10
PROCESS_QUERY_INFORMATION = 0x400


def main() -> int:
    root = os.environ.get("YURIS_SAMPLE_ERIS_ROOT")
    if not root:
        print("SKIP: YURIS_SAMPLE_ERIS_ROOT 未设置（缺样本环境必须显式报错）", file=sys.stderr)
        return 2
    exe = os.path.join(root, "yu-ris.exe")
    if not os.path.isfile(exe):
        print(f"SKIP: {exe} 不存在", file=sys.stderr)
        return 2
    wait_s = float(sys.argv[1]) if len(sys.argv) > 1 else 6.0

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = w.HANDLE
    k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    k32.ReadProcessMemory.argtypes = [
        w.HANDLE,
        w.LPCVOID,
        w.LPVOID,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
    ]
    k32.ReadProcessMemory.restype = w.BOOL

    proc = subprocess.Popen([exe], cwd=root)
    try:
        return probe(k32, proc, wait_s)
    finally:
        # 异常/早退也必须收掉引擎进程,不留后台实例。
        proc.kill()
        proc.wait(timeout=5)


def probe(k32, proc, wait_s):
    time.sleep(wait_s)
    handle = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, proc.pid)
    if not handle:
        print(f"OpenProcess 失败: {ctypes.get_last_error()}", file=sys.stderr)
        return 1

    def rd(addr, n):
        buf = ctypes.create_string_buffer(n)
        got = ctypes.c_size_t(0)
        if not k32.ReadProcessMemory(handle, ctypes.c_void_p(addr), buf, n, ctypes.byref(got)):
            return None
        return buf.raw[: got.value]

    def u32(addr):
        b = rd(addr, 4)
        return struct.unpack("<I", b)[0] if b and len(b) == 4 else None

    def cstr(addr, n=96):
        b = rd(addr, n)
        if b is None:
            return None
        z = b.find(bytes([0]))
        return b[:z] if z >= 0 else b

    table = rd(0x689B80, 256)
    print("char_table@689b80:", [(i, table[i]) for i in range(256) if table and table[i]])
    prefix = u32(0x801718)
    print("prefix:", cstr(prefix) if prefix else None)

    seen = set()

    def walk(node, depth, maxdepth):
        while node and node not in seen and depth <= maxdepth:
            seen.add(node)
            name_ptr = u32(node + 0x14)
            name = cstr(name_ptr) if name_ptr else b""
            sound = u32(node + 0x34)
            flag = rd(node + 0x30, 1)
            vol = u32(sound + 0x148) if sound else None
            en = rd(sound + 0x106, 1) if sound else None
            print(
                "  " * depth
                + f"{hex(node)} name={name!r} flag={flag.hex() if flag else None} "
                + f"vol={vol} E={en.hex() if en else None} parent={hex(u32(node + 0x10) or 0)}"
            )
            first = u32(node)
            if first:
                walk(first, depth + 1, maxdepth)
            node = u32(node + 0x0C)

    root_node = u32(0x801708)
    print("root:", hex(root_node) if root_node else None)
    walk(root_node, 0, 4)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
