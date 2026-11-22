#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 v555 构建矩阵:身份 + 反编译覆盖 + 分发表 + 语义族。

输入:
  --builds-json  pe_identity.py 的 builds.json
  --decomp-root  <workbench>/tmp/v555-decomp
  --sizes        handler-sizes.tsv(可选;没有则不判族)
输出:
  docs/formats/v555-build-matrix.tsv
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import sys
from pathlib import Path


# 构建 → 中性标签(样本真名只留在 AGENTS.md)。
BUILD_LABELS = {
    "b02a4dc04ae": "v555-4",
    "b18f932f632": "v555-4-backup",
    "b1baf88a871": "v555-1",
    "b1e3d85507b": "v555-pc",
    "b2d786cf3f0": "v555-pc-chs",
    "b3c02ab23bf": "v555-2",
    "b61a4e53b93": "v555-4-backup",
    "b8772c246fe": "v555-5",
    "baed7a05232": "v555-3",
}


def game_name(path: str, build_id: str) -> str:
    p = path.replace("\\", "/")
    if "<backup>" in p:
        return "original-backup"
    return BUILD_LABELS.get(build_id, "?")


def read_log(build_dir: Path):
    p = build_dir / "dispatch" / "dispatch.log"
    if not p.is_file():
        return None
    t = p.read_text(encoding="utf-8", errors="replace")
    out = {}
    for key, pat in (
        ("table_base", r"tableBase=0x([0-9a-f]+)"),
        ("default", r"default handler from loop: 0x([0-9a-f]+)"),
        ("explicit", r"explicitInRange=(-?\d+)"),
        ("distinct", r"distinct handlers=(\d+)"),
    ):
        m = re.search(pat, t)
        if m:
            out[key] = m.group(1)
    return out


def read_summary(build_dir: Path):
    p = build_dir / "functions" / "_summary.txt"
    if not p.is_file():
        return None
    d = {}
    for line in p.read_text(encoding="utf-8").splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            d[k] = v
    return d


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--builds-json", required=True)
    ap.add_argument("--decomp-root", required=True)
    ap.add_argument("--sizes", default="")
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)

    rows = json.loads(Path(args.builds_json).read_text(encoding="utf-8"))
    root = Path(args.decomp_root)
    sizes = {}
    if args.sizes and Path(args.sizes).is_file():
        for r in csv.DictReader(open(args.sizes, encoding="utf-8"), delimiter="\t"):
            sizes[r["name"]] = r

    seen = {}
    for row in rows:
        th = row.get("text_hash") or ""
        if not th:
            th = next((s.get("sha256", "") for s in row.get("sections", []) if s.get("name") == ".text"), "")
        bid = f"b{th[:10]}" if th else "bNA"
        seen.setdefault(bid, row)

    out = [
        "build_id\tgame\tsize\tproduct\tanalyzed\tfunctions\tok\tdispatch_base\tdefault\texplicit\tdistinct\tfamily\tnotes"
    ]
    for bid, row in sorted(seen.items()):
        d = root / bid
        summary = read_summary(d)
        disp = read_log(d)
        analyzed = "yes" if summary else "no"
        funcs = summary.get("total", "") if summary else ""
        ok = summary.get("ok", "") if summary else ""
        base = disp.get("table_base", "") if disp else ""
        default = disp.get("default", "") if disp else ""
        explicit = disp.get("explicit", "") if disp else ""
        distinct = disp.get("distinct", "") if disp else ""
        family = ""
        notes = row.get("packer_hint", "")
        if not disp:
            family = "packed/transformed"
        elif funcs.isdigit() and int(funcs) < 50 and base.startswith("ffffffff"):
            family = "packed-stub"
            notes = (notes + "; static=packer stub").strip("; ")
        else:
            t = sizes.get("TEXT", {})
            c = sizes.get("CSV", {})
            p = sizes.get("PRODUCT", {})
            if t.get(bid) == "1404" and c.get(bid) == "1682":
                family = "C-1"
            elif p.get(bid) == "4134":
                family = "B-PC"
            elif t.get(bid) == "837" and c.get(bid) == "1148":
                family = "A-2.1MB"
            else:
                family = "unknown"
        out.append(
            "\t".join(
                [
                    bid,
                    game_name(row["path"], bid),
                    str(row["size"]),
                    row.get("version_text", ""),
                    analyzed,
                    funcs,
                    ok,
                    base,
                    default,
                    explicit,
                    distinct,
                    family,
                    notes,
                ]
            )
        )
    Path(args.out).write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"wrote {args.out} ({len(out)-1} builds)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
