#!/usr/bin/env python3
"""Validate a YU-RIS `translate.json` (schema yuris-translate-1).

Checks structure, unique keys, counts, character-glossary consistency, speaker
prefixes, control-token preservation, line boundaries, length ratios, whether
each translation can be encoded by its game codec, and fixed-cell UI width
(full-width source labels must not gain half-width ASCII).

Stdlib only. Exit code 1 on errors, 0 on success (warnings allowed).

The runtime loader can extend the CP932 SJIS tunnel with new characters, so
`cp932` entries containing Chinese are accepted by default (`--no-cp932-tunnel`
restores strict CP932 checking for data-layer patches).

Usage:
  python validate_translate.py translate.json
  python validate_translate.py translate.json --strict
  python validate_translate.py translate.json --codec gbk --allow-unencodable
  python validate_translate.py translate.json --no-cp932-tunnel
"""

from __future__ import annotations

import argparse
import collections
import json
import re
import sys
from pathlib import Path

SCHEMA = "yuris-translate-1"
CONTROL_RE = re.compile(r"(?:\\[A-Za-z0-9]|%[-+ 0#]*[0-9]*(?:\.[0-9]+)?[sdifxX]|\$[A-Za-z_][A-Za-z0-9_]*)")
SPEAKER_RE = re.compile(r"^【([^】]*)】")
BRACKET_PAIRS = [
    ("【", "】"),
    ("「", "」"),
    ("『", "』"),
    ("（", "）"),
    ("(", ")"),
    ("“", "”"),
    ("‘", "’"),
    ("《", "》"),
]
CODECS = {"cp932", "gbk", "big5", "cp949", "cp1252", "utf8"}
# 固定布局/居中 UI:这些 context 的译文要尽量保持原标签的全角宽度类别。
UI_CONTEXTS = {
    "es.BT.NAME.SET",
    "es.DIALOG.YESNO.SET",
    "DIALOG",
    "MENU",
    "es._mes",
    "ES.SEL.SET",
}


class Report:
    def __init__(self, max_report: int) -> None:
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.max_report = max_report

    def error(self, message: str) -> None:
        self.errors.append(message)

    def warning(self, message: str) -> None:
        self.warnings.append(message)

    def dump(self) -> None:
        for message in self.errors[: self.max_report]:
            print(f"ERROR {message}")
        if len(self.errors) > self.max_report:
            print(f"ERROR ... {len(self.errors) - self.max_report} more errors")
        for message in self.warnings[: self.max_report]:
            print(f"WARN  {message}")
        if len(self.warnings) > self.max_report:
            print(f"WARN  ... {len(self.warnings) - self.max_report} more warnings")


def load(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def bracket_pair(text: str):
    for opening, closing in BRACKET_PAIRS:
        if text.startswith(opening) and text.endswith(closing) and len(text) >= len(opening) + len(closing):
            return opening, closing
    return None


def target_name(character: dict) -> str | None:
    """Canonical translated name: display inner if bracketed, else name_translation."""
    display = character.get("display") or ""
    display_translation = (character.get("display_translation") or "").strip()
    name_translation = (character.get("name_translation") or "").strip()
    pair = bracket_pair(display)
    if pair and display_translation:
        opening, closing = pair
        if display_translation.startswith(opening) and display_translation.endswith(closing):
            inner = display_translation[len(opening) : -len(closing)].strip()
            if inner:
                return inner
    return name_translation or display_translation or None


def build_glossary(characters: list[dict], report: Report):
    by_variant: dict[str, dict[str, str]] = {}
    global_map: dict[str, str] = {}
    for character in characters:
        name = (character.get("name") or "").strip()
        display = character.get("display") or ""
        variant = character.get("variant") or ""
        target = target_name(character)
        if target:
            if name:
                by_variant.setdefault(variant, {})[name] = target
                global_map.setdefault(name, target)
            pair = bracket_pair(display)
            if pair:
                inner = display[len(pair[0]) : -len(pair[1])].strip()
                if inner:
                    global_map.setdefault(inner, target)
                    by_variant.setdefault(variant, {}).setdefault(inner, target)
        if not name and not display:
            report.error(f"character {character.get('key')}: empty display and name")
        if not (character.get("name_translation") or "").strip():
            report.warning(f"character {character.get('key')}: empty name_translation")
        if display and not (character.get("display_translation") or "").strip():
            report.warning(f"character {character.get('key')}: empty display_translation")
    return by_variant, global_map


def check_character(character: dict, report: Report) -> None:
    key = character.get("key", "<no key>")
    for field in ("display_translation", "name_translation"):
        value = character.get(field)
        if not isinstance(value, str):
            report.error(f"character {key}: missing/invalid {field}")
            continue
        display = character.get("display") or ""
        if field == "display_translation" and value:
            pair = bracket_pair(display)
            if pair and not (value.startswith(pair[0]) and value.endswith(pair[1])):
                report.error(
                    f"character {key}: display_translation must keep {pair[0]}…{pair[1]}"
                )


def control_tokens(text: str) -> collections.Counter:
    return collections.Counter(CONTROL_RE.findall(text))


def check_encoding(
    text: str,
    codec: str,
    key: str,
    report: Report,
    allow: bool,
    cp932_tunnel: bool,
    tunnel_chars: set[str],
) -> None:
    if codec == "cp932" and cp932_tunnel:
        # 运行时装载器会为 CP932 表示不了的字符分配 SJIS 隧道码,并把扩展表
        # 交给渲染器;这里只统计全局唯一字符数,容量在末尾汇总检查。
        for character in text:
            if ord(character) >= 0x80:
                try:
                    character.encode("cp932")
                except UnicodeEncodeError:
                    tunnel_chars.add(character)
        return
    try:
        text.encode(codec)
    except UnicodeEncodeError as error:
        character = text[error.start] if error.start < len(text) else "?"
        message = (
            f"text {key}: U+{ord(character):04X} ({character!r}) is not encodable in {codec}"
        )
        if allow:
            report.warning(message)
        else:
            report.error(message)


def width_risk(source: str, translation: str) -> bool:
    """源标签没有半角 ASCII,译文却引入半角字母/数字/空格。"""
    def halfwidth_ascii(text: str) -> bool:
        return any(c.isascii() and (c.isalnum() or c == " ") for c in text)
    return not halfwidth_ascii(source) and halfwidth_ascii(translation)


def check_ui_width(source: str, translation: str, key: str, report: Report) -> None:
    if width_risk(source, translation):
        report.warning(
            f"{key}: UI text introduced half-width ASCII "
            "(may break fixed-cell centering; prefer full-width forms)"
        )


def check_text(
    entry: dict,
    glossary_by_variant: dict[str, dict[str, str]],
    glossary_any: dict[str, str],
    codec_override: str | None,
    allow_unencodable: bool,
    cp932_tunnel: bool,
    tunnel_chars: set[str],
    max_ratio: float,
    report: Report,
) -> None:
    key = entry.get("key", "<no key>")
    source = entry.get("text")
    translation = entry.get("translation")
    if not isinstance(source, str):
        report.error(f"text {key}: missing/invalid text")
        return
    if not isinstance(translation, str):
        report.error(f"text {key}: missing/invalid translation")
        return

    if not translation.strip():
        report.warning(f"text {key}: empty translation (original kept)")
        return

    source_tokens = control_tokens(source)
    target_tokens = control_tokens(translation)
    if source_tokens != target_tokens:
        missing = source_tokens - target_tokens
        extra = target_tokens - source_tokens
        report.error(
            f"text {key}: control tokens differ (missing={dict(missing)}, extra={dict(extra)})"
        )

    if ("\n" in translation or "\r" in translation) and not (
        "\n" in source or "\r" in source
    ):
        report.error(f"text {key}: translation adds a line break")

    # 外置剧本的行内/裸文本没有引号保护:这些序列会被 DSL 当注释/场景
    # 标记/命令前缀,运行时会拒绝替换并保留原文。
    if entry.get("source") == "scenario" and entry.get("kind") == "text":
        stripped = translation.lstrip()
        if "//" in translation or "/*" in translation:
            report.error(f"text {key}: bare scenario text contains // or /*")
        if stripped.startswith(("#", "\\")):
            report.error(f"text {key}: bare scenario text starts with # or backslash")

    speaker = SPEAKER_RE.match(source)
    if speaker:
        target_speaker = SPEAKER_RE.match(translation)
        if not target_speaker:
            report.error(f"text {key}: speaker prefix 【…】 was dropped")
        else:
            variant = entry.get("variant") or ""
            source_name = speaker.group(1).strip()
            target_name_value = target_speaker.group(1).strip()
            expected = glossary_by_variant.get(variant, {}).get(source_name)
            if expected is None:
                expected = glossary_any.get(source_name)
            if expected is None:
                report.warning(
                    f"text {key}: speaker {source_name!r} is not in the character glossary"
                )
            elif target_name_value != expected:
                report.error(
                    f"text {key}: speaker {target_name_value!r} != glossary {expected!r}"
                )

    codec = entry.get("codec") or "cp932"
    if codec_override is not None:
        codec = None if codec_override == "none" else codec_override
    if codec in CODECS:
        check_encoding(
            translation,
            codec,
            key,
            report,
            allow_unencodable,
            cp932_tunnel,
            tunnel_chars,
        )

    if source:
        ratio = len(translation) / len(source)
        if ratio > max_ratio:
            report.warning(
                f"text {key}: long translation ({ratio:.2f}x, limit {max_ratio:.2f}x)"
            )
        elif ratio < 0.25:
            report.warning(f"text {key}: suspiciously short ({ratio:.2f}x)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path, help="translate.json")
    parser.add_argument(
        "--codec",
        choices=["auto", "none", *sorted(CODECS)],
        default="auto",
        help="override per-entry codec; 'none' skips encoding checks",
    )
    parser.add_argument(
        "--allow-unencodable",
        action="store_true",
        help="downgrade encoding failures to warnings",
    )
    parser.add_argument(
        "--no-cp932-tunnel",
        action="store_true",
        help="require cp932 translations to be directly encodable (data-layer patch)",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="treat empty translations as errors",
    )
    parser.add_argument(
        "--max-ratio",
        type=float,
        default=2.0,
        help="length-ratio warning threshold (default 2.0)",
    )
    parser.add_argument("--max-report", type=int, default=100)
    args = parser.parse_args()

    report = Report(args.max_report)
    try:
        document = load(args.path)
    except (OSError, json.JSONDecodeError) as error:
        print(f"ERROR cannot read {args.path}: {error}")
        return 1

    if document.get("schema") != SCHEMA:
        report.error(f"schema must be {SCHEMA!r}, got {document.get('schema')!r}")
    characters = document.get("characters")
    text = document.get("text")
    choices = document.get("choices", [])
    literals = document.get("literals", [])
    meta = document.get("meta")
    if not isinstance(characters, list):
        report.error("characters must be an array")
        characters = []
    if not isinstance(text, list):
        report.error("text must be an array")
        text = []
    if not isinstance(choices, list):
        report.error("choices must be an array when present")
        choices = []
    if not isinstance(literals, list):
        report.error("literals must be an array when present")
        literals = []
    if not isinstance(meta, dict):
        report.error("meta must be an object")
        meta = {}

    counts = meta.get("counts") if isinstance(meta.get("counts"), dict) else {}
    if counts:
        if counts.get("characters") != len(characters):
            report.error(
                f"meta.counts.characters={counts.get('characters')} != {len(characters)}"
            )
        if counts.get("text") != len(text):
            report.error(f"meta.counts.text={counts.get('text')} != {len(text)}")
        if "choices" in counts and counts.get("choices") != len(choices):
            report.error(
                f"meta.counts.choices={counts.get('choices')} != {len(choices)}"
            )
        if "literals" in counts and counts.get("literals") != len(literals):
            report.error(
                f"meta.counts.literals={counts.get('literals')} != {len(literals)}"
            )
        word = sum(1 for entry in text if entry.get("source") == "word")
        scenario = sum(1 for entry in text if entry.get("source") == "scenario")
        if counts.get("word_units") != word:
            report.error(f"meta.counts.word_units={counts.get('word_units')} != {word}")
        if counts.get("scenario_units") != scenario:
            report.error(
                f"meta.counts.scenario_units={counts.get('scenario_units')} != {scenario}"
            )

    keys: list[str] = []
    for entry in characters + text + choices + literals:
        key = entry.get("key")
        if not isinstance(key, str) or not key:
            report.error("entry without a string key")
        else:
            keys.append(key)
    duplicates = [key for key, count in collections.Counter(keys).items() if count > 1]
    for key in duplicates:
        report.error(f"duplicate key: {key}")

    codec_override = None if args.codec == "auto" else args.codec
    tunnel_chars: set[str] = set()
    for character in characters:
        check_character(character, report)
        codec = character.get("codec") or "cp932"
        if codec_override is not None:
            codec = None if codec_override == "none" else codec_override
        if codec in CODECS:
            for field in ("display_translation", "name_translation"):
                value = character.get(field)
                if isinstance(value, str) and value.strip():
                    check_encoding(
                        value,
                        codec,
                        f"{character.get('key', '<character>')} {field}",
                        report,
                        args.allow_unencodable,
                        not args.no_cp932_tunnel,
                        tunnel_chars,
                    )
    glossary_by_variant, glossary_any = build_glossary(characters, report)

    for entry in choices:
        key = entry.get("key", "<no key>")
        translation = entry.get("translation")
        if translation is None:
            continue
        if not isinstance(translation, str):
            report.error(f"choice {key}: invalid translation")
            continue
        if not translation.strip():
            message = f"choice {key}: empty translation (original kept)"
            if args.strict:
                report.error(message)
            else:
                report.warning(message)
            continue
        check_ui_width(entry.get("text") or "", translation, f"choice {key}", report)
        codec = entry.get("codec") or "cp932"
        if codec_override is not None:
            codec = None if codec_override == "none" else codec_override
        if codec in CODECS:
            check_encoding(
                translation,
                codec,
                key,
                report,
                args.allow_unencodable,
                not args.no_cp932_tunnel,
                tunnel_chars,
            )
    for entry in literals:
        key = entry.get("key", "<no key>")
        translation = entry.get("translation")
        if not isinstance(translation, str):
            report.error(f"literal {key}: invalid translation")
            continue
        if not translation.strip():
            # 大量 literals 是内部 ID/调试串,空译文是正常选择;不报警。
            continue
        source = entry.get("text") or ""
        source_tokens = control_tokens(source)
        target_tokens = control_tokens(translation)
        if source_tokens != target_tokens:
            report.error(
                f"literal {key}: control tokens differ "
                f"(missing={dict(source_tokens - target_tokens)}, "
                f"extra={dict(target_tokens - source_tokens)})"
            )
        if ("\n" in translation or "\r" in translation) and not (
            "\n" in source or "\r" in source
        ):
            report.error(f"literal {key}: translation adds a line break")
        if entry.get("context") in UI_CONTEXTS:
            check_ui_width(source, translation, f"literal {key}", report)
        codec = entry.get("codec") or "cp932"
        if codec_override is not None:
            codec = None if codec_override == "none" else codec_override
        if codec in CODECS:
            check_encoding(
                translation,
                codec,
                key,
                report,
                args.allow_unencodable,
                not args.no_cp932_tunnel,
                tunnel_chars,
            )

    for entry in text:
        source = entry.get("text") or ""
        if not (entry.get("translation") or "").strip() and args.strict:
            report.error(f"text {entry.get('key')}: empty translation (--strict)")
        check_text(
            entry,
            glossary_by_variant,
            glossary_any,
            codec_override,
            args.allow_unencodable,
            not args.no_cp932_tunnel,
            tunnel_chars,
            args.max_ratio,
            report,
        )

    if tunnel_chars:
        if len(tunnel_chars) > 3400:
            report.warning(
                f"cp932 tunnel needs {len(tunnel_chars)} unique characters "
                "(official allocator capacity is about 3400; loader will fail)"
            )
        print(f"cp932 tunnel: {len(tunnel_chars)} unique characters")
    report.dump()
    print(
        f"summary: characters={len(characters)} choices={len(choices)} "
        f"literals={len(literals)} text={len(text)} "
        f"errors={len(report.errors)} warnings={len(report.warnings)}"
    )
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())
