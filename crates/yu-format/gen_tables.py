#!/usr/bin/env python3
# coding: utf-8
"""yu-format 编解码码表生成器(一次性工具;产物已入库,改码表须重跑本脚本)。

数据源 = Windows 本体 API(kernel32):
  - 解码表: MultiByteToWideChar(932/936, MB_ERR_INVALID_CHARS, ...)
  - 编码表: WideCharToMultiByte(932/936, ...)
理由: YU-RIS 引擎运行在 Windows 上,文本边界走系统码页;以系统行为为真值,
避免 Python 自带 codec 的非标准扩充(cp932 把 0xA0/0xFD-0xFF 映到 PUA)。

产物:
  src/codec/table_cp932.rs
  src/codec/table_gbk.rs

用法: python gen_tables.py   (在 crates/yu-format 目录下;仅 Windows 可用)
"""

import ctypes
from ctypes import wintypes

_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

_CP_ACP = 0
_MB_ERR_INVALID_CHARS = 0x00000008

_LPCCH = ctypes.POINTER(ctypes.c_char)
_LPSTR = ctypes.POINTER(ctypes.c_char)
_LPWSTR = ctypes.POINTER(ctypes.c_wchar)
_LPBOOL = ctypes.POINTER(ctypes.c_int)

_MultiByteToWideChar = _kernel32.MultiByteToWideChar
_MultiByteToWideChar.argtypes = [
    wintypes.UINT, wintypes.DWORD, _LPCCH, ctypes.c_int,
    _LPWSTR, ctypes.c_int,
]
_MultiByteToWideChar.restype = ctypes.c_int

_WideCharToMultiByte = _kernel32.WideCharToMultiByte
_WideCharToMultiByte.argtypes = [
    wintypes.UINT, wintypes.DWORD, _LPWSTR, ctypes.c_int,
    _LPSTR, ctypes.c_int, _LPCCH, _LPBOOL,
]
_WideCharToMultiByte.restype = ctypes.c_int


def m2w(cp: int, raw: bytes) -> str | None:
    """字节 → UTF-16 串;非法序列返回 None(严格模式)。"""
    buf = ctypes.create_unicode_buffer(16)
    n = _MultiByteToWideChar(cp, _MB_ERR_INVALID_CHARS, raw, len(raw), buf, 16)
    if n <= 0:
        return None
    return buf[:n]


def w2m(cp: int, ch: str) -> bytes | None:
    """单个 UTF-16 字符 → 字节;不可映射返回 None。"""
    out = ctypes.create_string_buffer(8)
    n = _WideCharToMultiByte(cp, 0, ch, 1, out, 8, None, None)
    if n <= 0:
        return None
    return out.raw[:n]


def collect(cp: int) -> dict:
    singles: dict[int, int] = {}      # byte -> code unit
    leads: set[int] = set()
    doubles: dict[int, dict[int, int]] = {}   # lead -> {trail -> unit}

    for b in range(0x100):
        ch = m2w(cp, bytes([b]))
        if ch is not None and len(ch) == 1:
            singles[b] = ord(ch)
        else:
            leads.add(b)
    # ASCII 恒等(两个码页一致,不依赖 API 回传的形态)
    for b in range(0x80):
        singles[b] = b

    for lead in sorted(leads):
        row: dict[int, int] = {}
        for trail in range(0x100):
            if trail == 0x00:
                continue
            ch = m2w(cp, bytes([lead, trail]))
            if ch is not None and len(ch) == 1 and ord(ch) != 0:
                row[trail] = ord(ch)
        if row:
            doubles[lead] = row

    lead_lo, lead_hi = min(doubles), max(doubles)
    all_trails = [t for row in doubles.values() for t in row]
    trail_lo, trail_hi = min(all_trails), max(all_trails)

    # 反向表:字符 → 字节(用 WideCharToMultiByte 的裁决 = 系统首选编码)
    enc: dict[int, int] = {}
    units = set(singles.values())
    for row in doubles.values():
        units.update(row.values())
    alias = 0
    for u in sorted(units):
        if u < 0x10000:
            ch = chr(u)
        else:  # 两个码页均为 BMP,防御分支
            continue
        raw = w2m(cp, ch)
        if raw is None or len(raw) == 0:
            continue
        back = m2w(cp, raw)
        assert back is not None and ord(back[0]) == u, (hex(u), raw.hex())
        if len(raw) == 1:
            enc[u] = raw[0]
        elif len(raw) == 2:
            enc[u] = raw[0] | (raw[1] << 8)
        else:
            raise AssertionError(raw.hex())
    # 一致性:decode 表内每个字符都能编码;别名 = 多字节码 → 同一字符
    have = set(enc)
    dup = len(units) - len(units & have)
    assert dup == 0, f"{dup} 个字符不可编码"

    return {
        "singles": singles,
        "doubles": doubles,
        "lead_lo": lead_lo, "lead_hi": lead_hi,
        "trail_lo": trail_lo, "trail_hi": trail_hi,
        "enc": enc,
        "units": len(units),
    }


def render(name: str, cp: int, data: dict, evidence: str) -> str:
    singles = data["singles"]
    doubles = data["doubles"]
    enc = data["enc"]
    lo, hi = data["lead_lo"], data["lead_hi"]
    tlo, thi = data["trail_lo"], data["trail_hi"]
    width = thi - tlo + 1
    rows = hi - lo + 1

    # 双字节解码矩阵
    lines = []
    total_double = 0
    for lead in range(lo, hi + 1):
        row = doubles.get(lead, {})
        total_double += len(row)
        cells = ", ".join(
            "0x%04X" % row.get(trail, 0) for trail in range(tlo, thi + 1)
        )
        lines.append("    [%s]," % cells)
    # 单字节(非 ASCII):显式列出全部映射,不猜区间性质
    high = {b: singles[b] for b in range(0x80, 0x100) if b in singles}
    # 编码别名(双字节码映射到已被更小编码占用的字符)
    alias_count = 0
    for lead in range(lo, hi + 1):
        for trail, unit in doubles.get(lead, {}).items():
            code = lead | (trail << 8)
            if enc.get(unit, code) != code:
                alias_count += 1

    out = []
    out.append("// 生成物,勿手改 —— 由 crates/yu-format/gen_tables.py 生成。")
    out.append("// 数据源 = Windows kernel32 码页 %d(%s)。" % (cp, evidence))
    out.append("// 解码:ROWS[lead-%#04x][trail-%#04x],0 = 非法;trail = %#04x..=%#04x。"
               % (lo, tlo, tlo, thi))
    if high:
        out.append("// 单字节 0x80.. 映射(其余 = 非法):%s" % ", ".join(
            "%#04x→U+%04X" % (b, u) for b, u in sorted(high.items())))
    out.append("// 编码别名(多码一符,编码取系统首选):%d 条双字节码。" % alias_count)
    out.append("")
    out.append("/// 单字节 0x80.. 的解码(缺省 = 非法)。索引 = 字节 - 0x80。")
    out.append("pub(super) static SINGLE_HIGH: [u16; 0x80] = [")
    cells = []
    for b in range(0x80, 0x100):
        cells.append("0x%04X" % singles.get(b, 0))
    for i in range(0, len(cells), 12):
        out.append("    " + ", ".join(cells[i:i + 12]) + ",")
    out.append("];")
    out.append("")
    out.append("pub(super) const LEAD_LO: u8 = 0x%02X;" % lo)
    out.append("pub(super) const LEAD_HI: u8 = 0x%02X;" % hi)
    out.append("pub(super) const TRAIL_LO: u8 = 0x%02X;" % tlo)
    out.append("pub(super) const TRAIL_HI: u8 = 0x%02X;" % thi)
    out.append("/// 双字节解码表(行 = 前导字节,列 = 后随字节;0 = 非法)。")
    out.append("pub(super) static ROWS: [[u16; %d]; %d] = [" % (width, rows))
    out.extend(lines)
    out.append("];")
    out.append("")
    out.append("/// 审计常量:双字节条目数、编码别名数(生成时核对)。")
    out.append("pub(super) const DOUBLE_ENTRIES: usize = %d;" % total_double)
    out.append("pub(super) const ALIASES: usize = %d;" % alias_count)
    # 编码覆盖表:扫描序首选(行主序)与系统首选(WideCharToMultiByte)的差集
    overrides = []
    for lead in range(lo, hi + 1):
        for trail, unit in doubles.get(lead, {}).items():
            win = enc.get(unit)
            if win is not None and win != (lead | (trail << 8)):
                # 该字符的系统首选不是本码 → 查该字符的扫描序码是否就是本码
                overrides.append((unit, win))
    uniq = {}
    for unit, win in overrides:
        uniq.setdefault(unit, win)
    out.append("/// 编码覆盖:(字符, 系统首选码)。扫描序首选与此不同的全部字符。")
    out.append("pub(super) const ALIAS_OVERRIDES: [(u16, u16); %d] = [" % len(uniq))
    for unit in sorted(uniq):
        out.append("    (0x%04X, 0x%04X)," % (unit, uniq[unit]))
    out.append("];")
    return "\n".join(out) + "\n"


def main():
    import os
    here = os.path.dirname(os.path.abspath(__file__))
    dst = os.path.join(here, "src", "codec")
    tables = [
        ("table_cp932.rs", 932, "Windows CP932 / Shift-JIS(NEC・IBM 选定行)"),
        ("table_gbk.rs", 936, "Windows CP936 / GBK"),
    ]
    for fname, cp, evidence in tables:
        data = collect(cp)
        text = render(fname, cp, data, evidence)
        path = os.path.join(dst, fname)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print("%s: singles=%d double_entries=%d aliases=%d rows=%dx%d" % (
            fname,
            sum(1 for b in range(0x80, 0x100)
                if b in data["singles"] and b not in data["doubles"]),
            sum(len(r) for r in data["doubles"].values()),
            sum(1 for lead in range(data["lead_lo"], data["lead_hi"] + 1)
                for trail, unit in data["doubles"].get(lead, {}).items()
                if data["enc"].get(unit) != (lead | (trail << 8))),
            data["lead_hi"] - data["lead_lo"] + 1,
            data["trail_hi"] - data["trail_lo"] + 1,
        ))


if __name__ == "__main__":
    main()
