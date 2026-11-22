#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""比较两个 v555 构建的同一 opcode 处理体反编译文本(地址归一化)。

输入:
  --all-tsv   join_dispatch.py 的合并表(含每构建的 handler 地址)
  --build-a/b 构建目录名(与 all-tsv 的列前缀一致)
  --functions-a/b 两个构建的 functions/ 目录

输出 TSV: opcode name handler_a handler_b ratio differing_lines diff_file
低相似度的 diff 写到 --diff-dir(默认首个 functions 目录的 ../handler-diffs)。
"""

from __future__ import annotations

import argparse
import csv
import difflib
import re
from pathlib import Path

ADDR = re.compile(r"0x[0-9a-fA-F]+")
FUN = re.compile(r"(?:FUN|V555H)_[0-9a-fA-F]+")
DAT = re.compile(r"(?:DAT|_DAT)_[0-9a-fA-F]+")
SPACE = re.compile(r"\s+")


def load_index(functions_dir: Path) -> dict[str, str]:
    idx = functions_dir / "_index.tsv"
    out: dict[str, str] = {}
    if not idx.is_file():
        return out
    for line in idx.read_text(encoding="utf-8", errors="replace").splitlines():
        p = line.split("\t")
        if len(p) >= 4 and p[0].startswith("0"):
            # entry 可能是 00452a0c 或 0x00452a0c
            key = p[0].lower().replace("0x", "")
            out[key] = p[3]
    return out


def normalize(text: str) -> list[str]:
    text = FUN.sub("FUN_X", text)
    text = DAT.sub("DAT_X", text)
    text = ADDR.sub("0xADDR", text)
    return [SPACE.sub(" ", l).strip() for l in text.splitlines() if l.strip()]


def find_file(index: dict[str, str], handler: str) -> Path | None:
    key = handler.lower().replace("0x", "")
    name = index.get(key)
    return name


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--all-tsv", required=True)
    ap.add_argument("--build-a", required=True)
    ap.add_argument("--build-b", required=True)
    ap.add_argument("--functions-a", required=True)
    ap.add_argument("--functions-b", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--diff-dir", default="")
    ap.add_argument("--threshold", type=float, default=0.97)
    args = ap.parse_args(argv)

    fa, fb = Path(args.functions_a), Path(args.functions_b)
    ia, ib = load_index(fa), load_index(fb)
    diff_dir = Path(args.diff_dir) if args.diff_dir else fa.parent / "handler-diffs"
    diff_dir.mkdir(parents=True, exist_ok=True)

    rows = list(csv.DictReader(open(args.all_tsv, encoding="utf-8"), delimiter="\t"))
    out = ["opcode\tname\thandler_a\thandler_b\tratio\tdiffering_lines\tdiff_file"]
    low = 0
    for r in rows:
        op = int(r["opcode"])
        name = r["name"]
        ha = r[f"{args.build_a}.handler"]
        hb = r[f"{args.build_b}.handler"]
        # 只有 real 体才有独立语义;default/shared 体不比较。
        if r[f"{args.build_a}.class"] != "real" or r[f"{args.build_b}.class"] != "real":
            continue
        na = find_file(ia, ha)
        nb = find_file(ib, hb)
        if not na or not nb:
            out.append(f"{op}\t{name}\t{ha}\t{hb}\t0\t0\tMISSING_FILE")
            low += 1
            continue
        la = normalize((fa / na).read_text(encoding="utf-8", errors="replace"))
        lb = normalize((fb / nb).read_text(encoding="utf-8", errors="replace"))
        ratio = difflib.SequenceMatcher(None, la, lb).ratio()
        diff_lines = sum(1 for _ in difflib.unified_diff(la, lb, lineterm=""))
        diff_file = ""
        if ratio < args.threshold:
            low += 1
            diff_file = f"{op:03d}_{name}.diff"
            with open(diff_dir / diff_file, "w", encoding="utf-8") as fh:
                fh.writelines(difflib.unified_diff(la, lb, fromfile=na, tofile=nb, lineterm="\n"))
        out.append(f"{op}\t{name}\t{ha}\t{hb}\t{ratio:.4f}\t{diff_lines}\t{diff_file}")
    Path(args.out).write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"wrote {args.out}; low-similarity handlers={low}; diffs={diff_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
