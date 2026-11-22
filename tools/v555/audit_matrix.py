#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成逐命令审计矩阵:官方分发表类别 × 当前 HandlerKind × 语料使用。

用法:
  python -I tools/v555/audit_matrix.py \
    --official docs/formats/v555-command-handlers-b02a4dc04ae.tsv \
    --current docs/formats/v555-current-handlers-4.tsv \
    --usage docs/formats/v555-command-usage-all.tsv \
    --out docs/formats/v555-audit-matrix.tsv
"""

from __future__ import annotations

import argparse
import csv
from pathlib import Path

# 官方 real 处理体,但当前按域事件/空操作实现时可接受的命令(注释里写理由)。
MATCH_STUB = {"LABEL", "SAVEINFO", "VAR", "PLUG", "ERRORINFO"}
MATCH_DOMAIN = {
    "FILEACT": "SaveHost 同步 COPY/DELETE",
    "SOUNDINFO": "SoundHost::query 专用路径写 LET",
}
PARTIAL_DOMAIN = {
    "MENUINFO": "官方写 LET;当前只发 Domain(Menu)",
    "FLASHINFO": "官方写 LET;当前只发 Domain(Flash)",
    "MOVIE2INFO": "官方写 LET;当前只发 Domain(Movie)",
}
NOTES = {
    "ALIAS": "v555 共享空操作 0x423168(已修:Noop)",
    "CSV": "v555 真实解析器 0x43c3a0(已修:Csv)",
    "TEXT": "2.1MB 族 837B / <v555-1> 1404B(4 源回退);当前实现按共享核心",
    "TEXTINFO": "官方 0x452d54 只装载参数并返回 0(已修:空签名 no-op)",
    "PRODUCT": "逐构建 COM 集合不同;COM=23/34/12 已补,21000/10201/10205/1001 仍宿主挂账",
    "CSVINFO": "2.1MB 族 615B / <v555-1> 1143B;当前 op_csvinfo 覆盖 LINE/DATA/SIZE",
    "SOUNDINFO": "2.1MB 族 2431B / <v555-1> 2031B;SoundHost::query 路径",
    "FILEACT": "2.1MB 族 2363B(<v555-4> 2491B)/ <v555-1> 2287B;SaveHost 消费",
    "VARINFO": "6 构建 2775B 相同;当前 Varinfo",
    "SAVE": "6 构建 4886B 相同;当前 Domain(Save)+MEM",
    "LOAD": "6 构建 2806B(约)相同;当前 Domain(Save)+MEM",
    "CGINFO": "6 构建 ~3552B 相同;当前 Cginfo",
}


def load(path: str, key: str = "opcode"):
    return {
        int(r[key]): r
        for r in csv.DictReader(open(path, encoding="utf-8"), delimiter="\t")
    }


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--official", required=True)
    ap.add_argument("--current", required=True)
    ap.add_argument("--usage", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)

    off = load(args.official)
    cur = load(args.current)
    use = {
        r["command"]: int(r["total"])
        for r in csv.DictReader(open(args.usage, encoding="utf-8"), delimiter="\t")
    }
    rows = [
        "opcode\tname\tofficial_class\tofficial_handler\tcurrent_kind\tverdict\tused_groups\tnote"
    ]
    mismatch = []
    for i in range(121):
        o = off[i]
        c = cur[i]
        cls = o["class"]
        kind = c["handler_kind"]
        name = o["name"]
        used = use.get(name, 0)
        note = NOTES.get(name, "")
        if cls == "default":
            verdict = "match" if kind == "ScriptError" else "MISMATCH"
        elif cls == "shared-stub":
            verdict = "match" if kind in ("Noop", "NoopEval") else "MISMATCH"
        elif kind in ("Unknown", "ScriptError"):
            verdict = "MISMATCH"
        elif kind in ("Noop", "NoopEval") and name not in MATCH_STUB:
            verdict = "PARTIAL"
        elif kind.startswith("Domain"):
            if name in MATCH_DOMAIN:
                verdict = "match-host"
                note = (note + "; " if note else "") + MATCH_DOMAIN[name]
            elif name in PARTIAL_DOMAIN:
                verdict = "PARTIAL"
                note = (note + "; " if note else "") + PARTIAL_DOMAIN[name]
            else:
                verdict = "match-domain"
        elif name in MATCH_STUB:
            verdict = "match-stub"
        else:
            verdict = "match"
        rows.append(
            f"{i}\t{name}\t{cls}\t{o['handler']}\t{kind}\t{verdict}\t{used}\t{note}"
        )
        if verdict == "MISMATCH":
            mismatch.append((i, name, cls, o["handler"], kind, used))
    Path(args.out).write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(f"wrote {args.out}; mismatches={len(mismatch)}")
    for m in mismatch:
        print("  ", m)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
