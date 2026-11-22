#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""最小 YSCM(ysc.ybn)解析器:命令名/参数名/参数元数据。

只用于取证工具链(生成 opcode→命令名、参数槽表);
运行期解析仍以 yu-format::yscm 为唯一入口。
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class Arg:
    name: str
    flag: int
    kind: int


@dataclass
class Command:
    opcode: int
    name: str
    argc: int
    args: list[Arg] = field(default_factory=list)


@dataclass
class CommandTable:
    version: int
    commands: list[Command]
    messages: list[bytes]
    key_table: bytes


def _u32(b: bytes, off: int) -> int:
    return struct.unpack_from("<I", b, off)[0]


def parse(buf: bytes) -> CommandTable:
    if len(buf) < 0x10 or buf[:4] != b"YSCM":
        raise ValueError("not YSCM")
    version = _u32(buf, 0x04)
    count = _u32(buf, 0x08)
    at = 0x10
    commands: list[Command] = []
    for opcode in range(count):
        end = buf.index(b"\x00", at)
        name = buf[at:end].decode("latin1", "replace")
        argc = buf[end + 1]
        at = end + 2
        args: list[Arg] = []
        for _ in range(argc):
            aend = buf.index(b"\x00", at)
            aname = buf[at:aend].decode("latin1", "replace")
            flag = buf[aend + 1]
            kind = buf[aend + 2]
            args.append(Arg(aname, flag, kind))
            at = aend + 3
        commands.append(Command(opcode, name, argc, args))
    messages: list[bytes] = []
    for _ in range(37):
        end = buf.index(b"\x00", at)
        messages.append(buf[at:end])
        at = end + 1
    key_table = buf[at : at + 0x100]
    return CommandTable(version, commands, messages, key_table)


def load(path: str | Path) -> CommandTable:
    return parse(Path(path).read_bytes())


def write_names_tsv(table: CommandTable, out: str | Path) -> None:
    lines = ["opcode\tname\targc\tparams"]
    for c in table.commands:
        params = ",".join(f"{a.name}:{a.kind}" for a in c.args)
        lines.append(f"{c.opcode}\t{c.name}\t{c.argc}\t{params}")
    Path(out).write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser()
    ap.add_argument("ysc", help="ysc.ybn")
    ap.add_argument("--out", default="")
    args = ap.parse_args()
    t = load(args.ysc)
    print(f"version={t.version} commands={len(t.commands)}")
    for c in t.commands:
        print(f"{c.opcode:3} {c.name:14} argc={c.argc:2} params={','.join(a.name for a in c.args)}")
    if args.out:
        write_names_tsv(t, args.out)
        print(f"wrote {args.out}")
