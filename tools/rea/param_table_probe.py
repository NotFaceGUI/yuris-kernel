#!/usr/bin/env python3
"""官方 0.495 命令参数描述符探针（只读）。

用途:读出运行期 yscm 投影表里指定命令的参数描述符,给出
「槽号 → 参数名 id / 值类型 / 标志位」,用来把 CG/CGACT 处理体里
`[0x684160 + name_id] → slot → [slot*8 + 0x683740]` 的读取点还原成参数名。

证据口径（0.495，sha256 4f5eefa3…）:
  [0x80171c]            运行期 yscm 投影结构指针
  struct +0x2c          u8[命令数]   每命令参数个数
  struct +0x30          指针[命令数] 每命令参数描述符数组
  struct +0x34          值数据基址（字符串/常量块）
  描述符 12 字节: +0 名字 id(u8) +1 未知 +2 值类型(u8) +3 标志(u8)
                  +4 值偏移(u32) +8 值长度(u32)
  [0x684160 + name_id] = 槽号（每命令执行前 memset 100 字节再填）
  [slot*8 + 0x683740]  = 值（低 32 位）；+4 = 高 32 位

用法:
  YURIS_SAMPLE_ERIS_ROOT=<project-eris> python tools/rea/param_table_probe.py [等待秒] [命令号...]
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
YSCM_STRUCT = 0x80171C


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
    cmds = [int(a, 0) for a in sys.argv[2:]] or [1, 2]

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
        return probe(k32, proc, wait_s, cmds)
    finally:
        # 异常/早退也必须收掉引擎进程,不留后台实例。
        proc.kill()
        proc.wait(timeout=5)


def probe(k32, proc, wait_s, cmds):
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

    base = u32(YSCM_STRUCT)
    if not base:
        print("yscm 投影结构指针为空（引擎可能未进入脚本）", file=sys.stderr)
        return 1
    counts_ptr = u32(base + 0x2C)
    desc_ptr_array = u32(base + 0x30)
    print(f"yscm@{base:#x} counts@{counts_ptr:#x} descs@{desc_ptr_array:#x}")
    if not counts_ptr or not desc_ptr_array:
        return 1
    allc = rd(counts_ptr, 256)
    print("counts[0..40]:", list(allc[:40]) if allc else None)
    for cmd in cmds:
        tp = u32(0x801740 + cmd * 4)
        cp = u32(0x801744 + cmd * 4)
        if tp:
            arr = rd(tp, 256)
            nz = [(i, arr[i]) for i in range(256) if arr[i]]
            print(f"cmd {cmd} type_class@{tp:#x}: {nz}")
        if cp:
            arr = rd(cp, 256)
            nz = [(i, arr[i]) for i in range(256) if arr[i]]
            print(f"cmd {cmd} checker@{cp:#x}: {nz}")
    for cmd in cmds:
        n = rd(counts_ptr + cmd, 1)
        p = u32(desc_ptr_array + cmd * 4)
        if not n or not p:
            print(f"cmd {cmd}: 读不到描述符")
            continue
        count = n[0]
        print(f"== cmd {cmd}: {count} params, descs@{p:#x} ==")
        raw = rd(p, count * 12)
        if raw is None:
            print("  描述符读取失败")
            continue
        for i in range(count):
            name_id, unk, vtype, flags, voff, vlen = struct.unpack_from("<BBBBII", raw, i * 12)
            print(
                f"  slot {i:3}  name_id={name_id:3} ({name_id:#04x})  unk={unk} "
                f"type={vtype} flags={flags} off={voff:#x} len={vlen}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
