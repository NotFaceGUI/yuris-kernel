#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""从 Ghidra/旁线导出的初始化器汇编中提取 v555 命令分发表。

输入是 `re_asm/<init>.asm` 这类逐指令文本(形如
`004633fc  MOV dword ptr [0x0078b1a0],0x423168`)。
初始化器通常先循环把 121 槽填成默认体,再逐槽覆盖。

输出:
  - handlers.tsv: opcode, name, argc, handler, class, detail
    class = real / shared-stub / default
  - handlers.txt: 纯地址列表(供 Ghidra DecompAddrs 使用)
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

# -I(隔离模式)不会把脚本目录放进 sys.path;显式加入以支持同目录 import。
sys.path.insert(0, str(Path(__file__).resolve().parent))
import yscm as yscm_mod

MOV_ABS = re.compile(
    r"MOV\s+dword\s+ptr\s+\[0x([0-9A-Fa-f]+)\],0x([0-9A-Fa-f]+)", re.IGNORECASE
)
MOV_ABS_SPACE = re.compile(
    r"MOV\s+dword\s+ptr\s+\[\s*0x([0-9A-Fa-f]+)\s*\],\s*0x([0-9A-Fa-f]+)", re.IGNORECASE
)
# 循环默认: MOV dword ptr [EAX + 0x78b1a0],0x45c63c
MOV_LOOP = re.compile(
    r"MOV\s+dword\s+ptr\s+\[[A-Za-z0-9]+\s*\+\s*0x([0-9A-Fa-f]+)\],\s*0x([0-9A-Fa-f]+)",
    re.IGNORECASE,
)
LOOP_BOUND = re.compile(r"CMP\s+[A-Za-z0-9]+,0x([0-9A-Fa-f]+)", re.IGNORECASE)


def parse_asm(path: Path, table_base: int):
    text = path.read_text(encoding="utf-8", errors="replace")
    assigns: dict[int, int] = {}
    default = None
    for line in text.splitlines():
        m = MOV_ABS.search(line) or MOV_ABS_SPACE.search(line)
        if m:
            dst = int(m.group(1), 16)
            src = int(m.group(2), 16)
            if table_base <= dst < table_base + 0x1000:
                assigns[(dst - table_base) // 4] = src
            continue
        m = MOV_LOOP.search(line)
        if m and int(m.group(1), 16) == table_base:
            # EAX + table_base -> 默认体;只在 default 为空时取第一个。
            if default is None:
                default = int(m.group(2), 16)
    # 由显式赋值推断表长上界;默认 121 槽由 CMP EAX,0x1e4 给出。
    count = 0
    for m in LOOP_BOUND.finditer(text):
        v = int(m.group(1), 16)
        if v in (0x1E4, 0x1E8):
            count = v // 4
            break
    if not count:
        count = max(assigns) + 1 if assigns else 121
    return assigns, default, count


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--asm", required=True, help="分发表初始化器的逐指令汇编")
    ap.add_argument("--ysc", required=True, help="对应工程的 ysc.ybn")
    ap.add_argument("--table-base", required=True, type=lambda s: int(s, 0))
    ap.add_argument("--count", type=int, default=0, help="槽数(默认从汇编 CMP 推断)")
    ap.add_argument("--default", type=lambda s: int(s, 0), default=0)
    ap.add_argument("--out-tsv", required=True)
    ap.add_argument("--out-list", default="")
    args = ap.parse_args(argv)

    assigns, default_from_asm, count = parse_asm(Path(args.asm), args.table_base)
    if args.count:
        count = args.count
    default = args.default or default_from_asm or 0
    table = yscm_mod.load(args.ysc)
    names = {c.opcode: c for c in table.commands}

    handlers = [default] * count
    explicit = [False] * count
    for idx, addr in assigns.items():
        if 0 <= idx < count:
            handlers[idx] = addr
            explicit[idx] = True

    # 共享体统计:出现次数 >=5 的非默认地址视为共享 stub(仍需反编译确认)。
    freq = Counter(handlers)
    rows = ["opcode\tname\targc\thandler\tclass\texplicit\tcount"]
    for i in range(count):
        c = names.get(i)
        name = c.name if c else ""
        argc = c.argc if c else 0
        h = handlers[i]
        if h == default:
            cls = "default"
        elif freq[h] >= 5:
            cls = "shared-stub"
        else:
            cls = "real"
        rows.append(f"{i}\t{name}\t{argc}\t0x{h:08x}\t{cls}\t{int(explicit[i])}\t{freq[h]}")
    Path(args.out_tsv).write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(
        f"table_base=0x{args.table_base:08x} count={count} default=0x{default:08x} "
        f"explicit={sum(explicit)} distinct={len(freq)} -> {args.out_tsv}"
    )
    print("class counts:", dict(Counter(r.split("\t")[4] for r in rows[1:])))
    if args.out_list:
        uniq = sorted({h for h in handlers if h})
        Path(args.out_list).write_text("\n".join(f"{h:08x}" for h in uniq) + "\n")
        print(f"handler addresses={len(uniq)} -> {args.out_list}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
