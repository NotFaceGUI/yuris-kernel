#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""v555 构建身份普查(只读目标文件)。

用途:
  1. 递归扫描样本根下的 .exe;
  2. 读 PE 头 / ProductVersion / 段表 / 导入 DLL;
  3. 对各段原始字节做 SHA-256,按 .text 哈希给构建聚类;
  4. 输出 TSV(默认 docs/formats/v555-builds.tsv)与可选 JSON。

不执行目标、不写目标目录;所有输出由 --out/--json 指定。

用法:
  python -I tools/v555/pe_identity.py \
    --root "<games-555>" \
    --out docs/formats/v555-builds.tsv

注意:版本资源可能被汉化/重打包改动,所以 ProductVersion 只作筛选线索,
唯一性判定以段哈希为准。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

RT_VERSION = 16


def _u16(b: bytes, off: int) -> int:
    return struct.unpack_from("<H", b, off)[0]


def _u32(b: bytes, off: int) -> int:
    return struct.unpack_from("<I", b, off)[0]


def _cstr(b: bytes, off: int, limit: int = 4096) -> str:
    end = b.find(b"\x00", off, min(len(b), off + limit))
    if end < 0:
        end = min(len(b), off + limit)
    return b[off:end].decode("latin1", "replace")


def _utf16z(b: bytes, off: int, limit: int = 4096) -> str:
    end = off
    while end + 1 < len(b) and end < off + limit:
        if b[end] == 0 and b[end + 1] == 0:
            break
        end += 2
    return b[off:end].decode("utf-16-le", "replace")


@dataclass
class Section:
    name: str
    virtual_size: int
    virtual_address: int
    raw_size: int
    raw_ptr: int
    characteristics: int
    sha256: str

    @property
    def is_executable(self) -> bool:
        return bool(self.characteristics & 0x20000000)


@dataclass
class PeInfo:
    path: str
    size: int
    sha256: str
    machine: int
    timestamp: int
    image_base: int
    entry_rva: int
    sections: list[Section] = field(default_factory=list)
    product_version: tuple[int, int, int, int] | None = None
    file_version: tuple[int, int, int, int] | None = None
    product_name: str = ""
    file_description: str = ""
    imports: list[str] = field(default_factory=list)
    version_strings: dict[str, str] = field(default_factory=dict)
    packer_hint: str = ""

    @property
    def text_hash(self) -> str:
        for s in self.sections:
            if s.name == ".text" and s.sha256:
                return s.sha256
        for s in self.sections:
            if s.is_executable and s.sha256:
                return s.sha256
        return ""

    @property
    def version_text(self) -> str:
        return ".".join(str(x) for x in (self.product_version or ()))


def parse_pe(path: Path) -> PeInfo:
    data = path.read_bytes()
    info = PeInfo(
        path=str(path),
        size=len(data),
        sha256=hashlib.sha256(data).hexdigest(),
        machine=0,
        timestamp=0,
        image_base=0,
        entry_rva=0,
    )
    if len(data) < 0x40 or data[:2] != b"MZ":
        info.packer_hint = "not-pe"
        return info
    pe = _u32(data, 0x3C)
    if pe + 24 > len(data) or data[pe : pe + 4] != b"PE\x00\x00":
        info.packer_hint = "bad-pe"
        return info
    info.machine = _u16(data, pe + 4)
    info.timestamp = _u32(data, pe + 8)
    nsec = _u16(data, pe + 6)
    opt_size = _u16(data, pe + 20)
    opt = pe + 24
    if opt + 2 > len(data):
        return info
    magic = _u16(data, opt)
    is_pe32_plus = magic == 0x20B
    info.image_base = struct.unpack_from("<Q" if is_pe32_plus else "<I", data, opt + 24)[0]
    info.entry_rva = _u32(data, opt + 16)
    dd = opt + (112 if is_pe32_plus else 96)
    nrva = _u32(data, opt + (108 if is_pe32_plus else 92))
    resource_rva = 0
    resource_size = 0
    import_rva = 0
    if nrva >= 3 and dd + 16 <= len(data):
        resource_rva = _u32(data, dd + 2 * 8)
        resource_size = _u32(data, dd + 2 * 8 + 4)
    if nrva >= 2 and dd + 16 <= len(data):
        import_rva = _u32(data, dd + 1 * 8)

    sec_base = opt + opt_size
    for i in range(nsec):
        off = sec_base + i * 40
        if off + 40 > len(data):
            break
        name = data[off : off + 8].rstrip(b"\x00").decode("latin1", "replace")
        vsize, vaddr, raw_size, raw_ptr = struct.unpack_from("<IIII", data, off + 8)
        chars = _u32(data, off + 36)
        raw = data[raw_ptr : raw_ptr + raw_size] if raw_ptr and raw_size else b""
        info.sections.append(
            Section(
                name=name,
                virtual_size=vsize,
                virtual_address=vaddr,
                raw_size=raw_size,
                raw_ptr=raw_ptr,
                characteristics=chars,
                sha256=hashlib.sha256(raw).hexdigest() if raw else "",
            )
        )

    def rva_to_off(rva: int) -> int | None:
        for s in info.sections:
            span = max(s.virtual_size, s.raw_size)
            if s.virtual_address <= rva < s.virtual_address + span:
                delta = rva - s.virtual_address
                if delta < s.raw_size:
                    return s.raw_ptr + delta
        return None

    if import_rva:
        off = rva_to_off(import_rva)
        if off is not None:
            seen = set()
            for _ in range(512):
                if off + 20 > len(data):
                    break
                name_rva = _u32(data, off + 12)
                if name_rva == 0:
                    break
                noff = rva_to_off(name_rva)
                if noff is None:
                    break
                name = _cstr(data, noff, 512)
                if name:
                    seen.add(name)
                off += 20
            info.imports = sorted(seen)[:64]

    if resource_rva:
        try:
            parse_version_resource(data, info, rva_to_off, resource_rva)
        except Exception as exc:  # 资源损坏不应让普查整体失败
            info.packer_hint = f"version-resource-error:{type(exc).__name__}"

    hints = []
    names = {s.name.lower() for s in info.sections}
    for marker in (
        "upx0",
        "upx1",
        ".aspack",
        ".adata",
        ".packed",
        ".vmp0",
        ".themida",
        ".enigma1",
        ".enigma2",
        ".bind",
        ".ierdata",
        ".rsrc a",
    ):
        if marker in names:
            hints.append(marker.lstrip("."))
    if any("Shieldkernel32" in x or "OriginShield" in x for x in info.imports):
        hints.append("origin-shield")
    if not info.product_version and not info.version_strings:
        hints.append("no-version-resource")
    info.packer_hint = ",".join(hints)
    return info


def _version_dir(data: bytes, base: int, off: int, depth: int = 0, type_id: int = -1):
    """Yield (top_level_type_id, data_rva, data_size) from an IMAGE_RESOURCE_DIRECTORY.

    资源树是 type -> name -> language 三层;只有第一层的 id 是资源类型
    (RT_VERSION=16),后两层是名字/语言。旧实现把叶子的 id 当类型,导致
    永远找不到版本资源。
    """
    if depth > 4 or off + 16 > len(data):
        return
    named = _u16(data, off + 12)
    ids = _u16(data, off + 14)
    for i in range(named + ids):
        ent = off + 16 + i * 8
        if ent + 8 > len(data):
            return
        name_field = _u32(data, ent)
        child = _u32(data, ent + 4)
        is_dir = bool(child & 0x80000000)
        child_off = base + (child & 0x7FFFFFFF)
        # 第一层:direntry 的 name/id 字段就是类型 id(不允许字符串名)。
        this_type = name_field if depth == 0 else type_id
        if is_dir:
            yield from _version_dir(data, base, child_off, depth + 1, this_type)
        else:
            # IMAGE_RESOURCE_DATA_ENTRY
            if child_off + 16 > len(data):
                continue
            data_rva = _u32(data, child_off)
            data_size = _u32(data, child_off + 4)
            yield this_type, data_rva, data_size


def parse_version_resource(data: bytes, info: PeInfo, rva_to_off, resource_rva: int) -> None:
    rbase = rva_to_off(resource_rva)
    if rbase is None:
        return
    # 只取 type=16 的第一个叶节点。
    target = None
    for name_id, data_rva, data_size in _version_dir(data, rbase, rbase):
        if name_id == RT_VERSION:
            target = (data_rva, data_size)
            break
    if target is None:
        return
    off = rva_to_off(target[0])
    if off is None:
        return
    blob = data[off : off + target[1]]
    if len(blob) < 40:
        return
    # VS_VERSIONINFO 根节点
    w_length = _u16(blob, 0)
    w_value_length = _u16(blob, 2)
    w_type = _u16(blob, 4)
    key = _utf16z(blob, 6, 128)
    pos = 6 + (len(key) + 1) * 2
    pos = (pos + 3) & ~3
    if key == "VS_VERSION_INFO" and pos + 52 <= len(blob):
        sig = _u32(blob, pos)
        if sig == 0xFEEF04BD:
            fv_ms = _u32(blob, pos + 8)
            fv_ls = _u32(blob, pos + 12)
            pv_ms = _u32(blob, pos + 16)
            pv_ls = _u32(blob, pos + 20)
            info.file_version = (fv_ms >> 16, fv_ms & 0xFFFF, fv_ls >> 16, fv_ls & 0xFFFF)
            info.product_version = (pv_ms >> 16, pv_ms & 0xFFFF, pv_ls >> 16, pv_ls & 0xFFFF)
    # 扫 UTF-16 键值对(ProductVersion/FileDescription/ProductName 等)
    text = blob.decode("utf-16-le", "ignore")
    for key_name in ("ProductVersion", "FileVersion", "ProductName", "FileDescription", "CompanyName"):
        idx = text.find(key_name + "\x00")
        if idx >= 0:
            tail = text[idx + len(key_name) + 1 :]
            # String 节点的 key 后面通常先跟一个 NUL(UTF-16 双零),再是值。
            val = tail.lstrip("\x00").split("\x00", 1)[0]
            if val:
                info.version_strings[key_name] = val
    info.product_name = info.version_strings.get("ProductName", "")
    info.file_description = info.version_strings.get("FileDescription", "")
    if info.product_version is None and "ProductVersion" in info.version_strings:
        parts = []
        for p in info.version_strings["ProductVersion"].replace(",", ".").split("."):
            try:
                parts.append(int(p.strip()))
            except ValueError:
                parts.append(0)
        while len(parts) < 4:
            parts.append(0)
        info.product_version = tuple(parts[:4])  # type: ignore[assignment]


def iter_exes(root: Path, min_size: int) -> Iterable[Path]:
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in {".git", "__pycache__"}]
        for fn in filenames:
            if not fn.lower().endswith(".exe"):
                continue
            p = Path(dirpath) / fn
            try:
                if p.stat().st_size >= min_size:
                    yield p
            except OSError:
                continue


def build_tsv(infos: list[PeInfo]) -> str:
    lines = [
        "\t".join(
            [
                "build_id",
                "cluster",
                "path",
                "size",
                "sha256",
                "product_version",
                "file_version",
                "product_name",
                "machine",
                "timestamp",
                "text_sha256",
                "sections",
                "imports",
                "packer_hint",
            ]
        )
    ]
    # 按 .text 哈希聚类;ID 直接取哈希前缀,跨轮次稳定。
    for info in infos:
        th = info.text_hash
        bid = f"b{th[:10]}" if th else "bNA"
        cid = f"C{th[:10]}" if th else "CNA"
        sections = ",".join(
            f"{s.name}:{s.raw_size}:{s.sha256[:12] or '-'}" for s in info.sections
        )
        lines.append(
            "\t".join(
                [
                    bid,
                    cid,
                    info.path,
                    str(info.size),
                    info.sha256,
                    info.version_text,
                    ".".join(str(x) for x in (info.file_version or ())),
                    info.product_name,
                    f"0x{info.machine:04x}",
                    str(info.timestamp),
                    info.text_hash,
                    sections,
                    ",".join(info.imports),
                    info.packer_hint,
                ]
            )
        )
    return "\n".join(lines) + "\n"


def to_dict(info: PeInfo) -> dict:
    return {
        "path": info.path,
        "size": info.size,
        "sha256": info.sha256,
        "text_hash": info.text_hash,
        "product_version": info.product_version,
        "file_version": info.file_version,
        "version_text": info.version_text,
        "product_name": info.product_name,
        "file_description": info.file_description,
        "machine": info.machine,
        "timestamp": info.timestamp,
        "imports": info.imports,
        "packer_hint": info.packer_hint,
        "version_strings": info.version_strings,
        "sections": [s.__dict__ for s in info.sections],
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True, help="样本根(递归扫 .exe)")
    ap.add_argument("--out", default="docs/formats/v555-builds.tsv")
    ap.add_argument("--json", default="")
    ap.add_argument("--min-size", type=int, default=1_000_000)
    ap.add_argument(
        "--all-exes",
        action="store_true",
        help="不过滤 ProductVersion;默认只保留 product_version 首字段为 555 的 exe",
    )
    args = ap.parse_args(argv)

    # Windows 控制台默认 GBK,日文路径会触发 UnicodeEncodeError;不影响 TSV。
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(errors="replace")
        except Exception:
            pass

    root = Path(args.root)
    if not root.is_dir():
        print(f"root is not a directory: {root}", file=sys.stderr)
        return 2
    infos: list[PeInfo] = []
    scanned = 0
    for p in iter_exes(root, args.min_size):
        scanned += 1
        try:
            info = parse_pe(p)
        except Exception as exc:
            print(f"skip {p}: {exc}", file=sys.stderr)
            continue
        pv = info.product_version or (0, 0, 0, 0)
        if args.all_exes or pv[1] == 555 or pv[0] == 555:
            infos.append(info)
            print(
                f"{info.version_text or '?':<12} {info.size:>10} "
                f"{info.text_hash[:12] or '-':<12} {p}"
            )
    infos.sort(key=lambda x: (x.text_hash, x.path))
    out = build_tsv(infos)
    outp = Path(args.out)
    outp.parent.mkdir(parents=True, exist_ok=True)
    outp.write_text(out, encoding="utf-8")
    print(f"\nscanned {scanned} exe(s), kept {len(infos)}, wrote {outp}")
    if args.json:
        jsonp = Path(args.json)
        jsonp.parent.mkdir(parents=True, exist_ok=True)
        jsonp.write_text(
            json.dumps([to_dict(info) for info in infos], ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        print(f"wrote {jsonp}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
