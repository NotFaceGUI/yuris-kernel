#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把各构建的 dispatch_raw.tsv 与 ysc 命令名对齐,输出合并审计表。

每个构建目录需有:
  dispatch/dispatch_raw.tsv   opcode, handler, explicit
  dispatch/dispatch.log       含 `default handler from loop: 0x...`

分类:
  default      = 与初始化器循环默认体相同
  shared-stub  = 非 default 且被 >=5 个槽共用(555 的 0x423168 型空操作)
  real         = 其余
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import yscm as yscm_mod


def load_build(build_dir: Path):
    raw = build_dir / "dispatch" / "dispatch_raw.tsv"
    log = build_dir / "dispatch" / "dispatch.log"
    if not raw.is_file():
        raise SystemExit(f"missing {raw}")
    default = 0
    if log.is_file():
        m = re.search(r"default handler from loop: 0x([0-9a-fA-F]+)", log.read_text(encoding="utf-8"))
        if not m:
            m = re.search(r"default handler from frequency: 0x([0-9a-fA-F]+)", log.read_text(encoding="utf-8"))
        if m:
            default = int(m.group(1), 16)
    rows = {}
    for line in raw.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("opcode"):
            continue
        op, handler, explicit = line.split("\t")
        rows[int(op)] = (int(handler, 16), explicit == "1")
    freq = Counter(h for h, _ in rows.values() if h != default)
    shared = {h for h, n in freq.items() if n >= 5}
    return default, rows, shared


def classify(handler: int, default: int, shared: set[int]) -> str:
    if handler == default:
        return "default"
    if handler in shared:
        return "shared-stub"
    return "real"


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ysc", required=True)
    ap.add_argument("--build", action="append", required=True, help="build dir; repeatable")
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)

    table = yscm_mod.load(args.ysc)
    names = {c.opcode: c for c in table.commands}
    builds = []
    for b in args.build:
        path = Path(b)
        default, rows, shared = load_build(path)
        builds.append((path.name, default, rows, shared))

    header = ["opcode", "name", "argc"]
    for name, _, _, _ in builds:
        header += [f"{name}.class", f"{name}.handler"]
    header.append("consistent")
    lines = ["\t".join(header)]
    for i in range(len(table.commands)):
        c = names.get(i)
        classes = []
        handlers = []
        for _, default, rows, shared in builds:
            h, _ = rows.get(i, (0, False))
            classes.append(classify(h, default, shared))
            handlers.append(f"0x{h:08x}")
        consistent = "yes" if len(set(classes)) == 1 else "NO"
        row = [str(i), c.name if c else "", str(c.argc if c else 0)]
        for cls, h in zip(classes, handlers):
            row += [cls, h]
        row.append(consistent)
        lines.append("\t".join(row))
    Path(args.out).write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {args.out} ({len(builds)} builds)")
    diff = [l for l in lines[1:] if l.endswith("\tNO")]
    print(f"class differences: {len(diff)}")
    for l in diff:
        print("  " + l)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
