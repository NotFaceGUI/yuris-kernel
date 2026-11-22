#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 builds.json 里的 555 exe 复制成 ASCII 路径,供 Ghidra 导入。

Ghidra 的 Windows 启动器在非 UTF-8 控制台下无法 canonicalize 含 U+30FB 等
字符的路径;统一用 build_id.exe 的 ASCII 副本,证据锚点仍指原路径。

用法:
  python -I tools/v555/stage_exes.py \
    --builds <workbench>/tmp/v555-decomp/builds.json \
    --out-dir <workbench>/tmp/v555-decomp/exes
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--builds", required=True)
    ap.add_argument("--out-dir", required=True)
    args = ap.parse_args(argv)

    rows = json.loads(Path(args.builds).read_text(encoding="utf-8"))
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    seen: dict[str, dict] = {}
    for row in rows:
        th = row.get("text_hash") or ""
        if not th:
            th = next(
                (s.get("sha256", "") for s in row.get("sections", []) if s.get("name") == ".text"),
                "",
            )
        bid = f"b{th[:10]}" if th else "bNA"
        # 同 build 取第一个路径;优先非“破解/汉化/enigma/bind”的副本?
        # 这里只保留第一个,来源路径写进 sidecar,人工可换。
        seen.setdefault(bid, row)
    for bid, row in sorted(seen.items()):
        src = Path(row["path"])
        dst = out / f"{bid}.exe"
        shutil.copy2(src, dst)
        (out / f"{bid}.source.txt").write_text(str(src) + "\n", encoding="utf-8")
        print(f"{bid}  {src.stat().st_size:>10}  -> {dst}")
    print(f"staged {len(seen)} build(s) -> {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
