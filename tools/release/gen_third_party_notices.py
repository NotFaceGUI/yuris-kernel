#!/usr/bin/env python3
"""生成 THIRD-PARTY-NOTICES.md。

数据源是 `cargo metadata --locked`：只收录第三方包（有 `source` 的），
工作区自身 crate 由 LICENSE / NOTICE 覆盖，不在此表重复。

用法：
    python tools/release/gen_third_party_notices.py
    python tools/release/gen_third_party_notices.py --metadata target/meta.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = REPO_ROOT / "THIRD-PARTY-NOTICES.md"

COPYRIGHT_RE = re.compile(r"(?i)^\s*copyright\s*(?:\(c\)|©)?\s*(?:19|20)\d\d")
COPYRIGHT_SKIP_RE = re.compile(
    r"(?i)(notice that is included|license to reproduce|name of copyright owner|"
    r"<year>|<owner>|\[yyyy\]|copyright and related rights)"
)


def load_metadata(path: Path | None) -> dict:
    if path is not None:
        return json.loads(path.read_text(encoding="utf-8"))
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked"]
    proc = subprocess.run(
        cmd, cwd=REPO_ROOT, check=False, capture_output=True, text=True
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit("cargo metadata 失败；请先确保依赖已拉取并可离线解析")
    return json.loads(proc.stdout)


def crate_source(pkg: dict) -> str:
    repo = pkg.get("repository") or pkg.get("homepage")
    if repo:
        return repo
    source = pkg.get("source") or ""
    if source.startswith("registry+"):
        return f"https://crates.io/crates/{pkg['name']}/{pkg['version']}"
    return source


def license_files(pkg: dict) -> list[Path]:
    root = Path(pkg["manifest_path"]).parent
    files: list[Path] = []
    declared = pkg.get("license_file")
    if declared:
        path = Path(declared)
        files.append(path if path.is_absolute() else root / path)
    for pattern in ("LICENSE*", "COPYING*", "NOTICE*"):
        files.extend(root.glob(pattern))
    return sorted({f for f in files if f.is_file()})


def copyright_lines(pkg: dict) -> list[str]:
    lines: list[str] = []
    for path in license_files(pkg):
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        for raw in text.splitlines():
            line = " ".join(raw.split())
            if not COPYRIGHT_RE.match(line) or COPYRIGHT_SKIP_RE.search(line):
                continue
            if line not in lines:
                lines.append(line)
    return lines[:4]


def escape_cell(text: str) -> str:
    return text.replace("|", "\\|")


def render(metadata: dict) -> str:
    packages = [p for p in metadata["packages"] if p.get("source")]
    packages.sort(key=lambda p: (p["name"].lower(), p["version"]))

    lock_path = REPO_ROOT / "Cargo.lock"
    lock_sha = hashlib.sha256(lock_path.read_bytes()).hexdigest() if lock_path.exists() else "unknown"

    out: list[str] = []
    out.append("# 第三方许可与声明")
    out.append("")
    out.append("<!-- 本文件由 tools/release/gen_third_party_notices.py 生成，请勿手工编辑。 -->")
    out.append("")
    out.append(
        "> 生成依据：`cargo metadata --locked`（`Cargo.lock` sha256 "
        f"`{lock_sha}`）；第三方 crate 共 **{len(packages)}** 个。"
    )
    out.append(">")
    out.append(
        "> 本项目自身代码以 **GPL-3.0-or-later** 发布，见 [`LICENSE`](LICENSE) 与 "
        "[`NOTICE`](NOTICE)。本文件随发行产物一并分发；许可证全文见 "
        "[`third_party/licenses/`](third_party/licenses/)。"
    )
    out.append("")
    out.append("## 1. 随仓库分发的第三方组件")
    out.append("")
    out.append("| 组件 | 位置 | 许可 | 许可文本 |")
    out.append("| --- | --- | --- | --- |")
    out.append(
        "| swash（本地 fork） | `third_party/swash/` | Apache-2.0 OR MIT | "
        "`third_party/swash/LICENSE-APACHE`、`LICENSE-MIT` |"
    )
    out.append(
        "| egui 默认字体（`epaint_default_fonts`） | crate 依赖 | "
        "OFL-1.1 AND Ubuntu-font-1.0 | `third_party/licenses/OFL-1.1.txt`、"
        "`third_party/licenses/Ubuntu-font-1.0.txt` |"
    )
    out.append(
        "| libpng（`libpng-sys` vendored） | crate 依赖 | Libpng | "
        "`third_party/licenses/Libpng.txt` |"
    )
    out.append(
        "| `yu-emote/` 子模块 | 独立仓库 | MIT OR Apache-2.0 | "
        "见子模块 `LICENSE-*`；不属于本工作区构建 |"
    )
    out.append("")
    out.append("## 2. Rust crate 依赖清单")
    out.append("")
    out.append("| Crate | Version | License | Source |")
    out.append("| --- | --- | --- | --- |")
    for pkg in packages:
        license_text = pkg.get("license") or (
            "见包内 LICENSE 文件" if pkg.get("license_file") else "未声明"
        )
        out.append(
            "| {name} | {version} | {license} | {source} |".format(
                name=escape_cell(pkg["name"]),
                version=escape_cell(pkg["version"]),
                license=escape_cell(license_text),
                source=escape_cell(crate_source(pkg)),
            )
        )
    out.append("")
    out.append("## 3. 版权声明")
    out.append("")
    out.append(
        "以下版权行摘自各包自带的 `LICENSE*` / `COPYING*` 文件；"
        "未列出的包请以其上游许可文件为准。"
    )
    out.append("")
    for pkg in packages:
        lines = copyright_lines(pkg)
        if not lines:
            continue
        out.append(f"- **{pkg['name']} {pkg['version']}** — " + "；".join(lines))
    out.append("")
    out.append("## 4. 上游 NOTICE 文件")
    out.append("")
    notice_count = 0
    for pkg in packages:
        for path in license_files(pkg):
            if not path.name.upper().startswith("NOTICE"):
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="ignore").strip()
            except OSError:
                continue
            if not text:
                continue
            notice_count += 1
            out.append(f"### {pkg['name']} {pkg['version']} — `{path.name}`")
            out.append("")
            out.append("```text")
            out.append(text)
            out.append("```")
            out.append("")
    if notice_count == 0:
        out.append("（未发现依赖携带 NOTICE 文件。）")
        out.append("")
    out.append("## 5. 说明")
    out.append("")
    out.append("- SPDX 表达式中的 `OR` 表示双许可；本项目按宽松项使用。")
    out.append(
        "- `LicenseRef-NSL-1.0 OR GPL-3.0-or-later` 的包按 GPL-3.0-or-later "
        "选项使用，不适用 NSL 条款。"
    )
    out.append(
        "- 表中没有自带许可文件的包，以 crate 元数据中的 `license` 字段为准；"
        "完整依赖图固定在 `Cargo.lock`。"
    )
    out.append(
        "- 清单覆盖工作区全部依赖（含开发/测试与平台条件依赖），"
        "单个发行包实际链接的子集可能更小。"
    )
    out.append(
        "- 本文件是发布合规清单，不构成法律意见；如用作商业分发，请自行复核。"
    )
    out.append("")
    return "\n".join(out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, help="预先生成的 cargo metadata JSON")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    metadata = load_metadata(args.metadata)
    text = render(metadata)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {args.output} ({len(text)} bytes)")


if __name__ == "__main__":
    main()
