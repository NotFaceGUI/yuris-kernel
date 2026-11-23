---
name: yuris-translation
description: Translate YU-RIS games from the `translate.json` produced by `yu-cli export --format translate-json`. Use this whenever the user asks to translate or 汉化 a YU-RIS game, fill/repair a translate.json, keep 角色名统一 (character-name consistency), translate 剧情/台词 (scenario or dialogue), or validate a translation before importing it. It covers character glossary rules, plot translation style, control-token preservation, and per-codec (CP932/GBK/Big5) encoding checks.
---

# YU-RIS Translation

Translate a YU-RIS game from a single `translate.json`. The file is produced by the
project's exporter; treat it as the **only** translation surface — do not edit game
scripts, packs, or exported CSV files.

## 0. Input

```bash
yu-cli export <project-root> <out-dir> --format translate-json
# => <out-dir>/translate.json   (schema: yuris-translate-1)
```

`translate.json` contains four arrays:

- `characters`: the in-game character-name table (`display` + `name`);
- `choices`: option labels passed to `ES.SEL.SET` / `ES.CSEL.SET`;
- `literals`: script string constants (buttons, menus, dialogs, messages, and
  internal IDs — see §2.5);
- `text`: compiled `WORD` text plus external-scenario text.

Every entry has a stable `key`. Translation fields are:

| Entry | Fields you fill |
|---|---|
| `characters[i]` | `display_translation`, `name_translation` |
| `choices[i]` | `translation` |
| `literals[i]` | `translation` (only display/message entries) |
| `text[i]` | `translation` |

If an entry is left empty, the game keeps the original text. Never edit `key`,
`meta`, `schema`, `codec`, coordinates, or counts.

## 1. Inspect `meta` first

Read `meta.text_codec`, `meta.text_tunnel`, and each entry's `codec` before writing
a single character:

- `cn` variant → **Simplified Chinese**, target codec usually `gbk`.
- `tw` variant → **Traditional Chinese**, target codec usually `big5`.
- `cp932` → original Japanese. The runtime loader can encode Chinese with
  CP932 where possible and allocate new SJIS tunnel codes for the rest
  (extending the game's own `sjis_ext.bin`). So a CP932 project is a valid
  target for a Chinese translation **when the game is played through the
  runtime `--translate` hook**; the validator accepts it by default.
- GBK/Big5 projects encode directly and have no tunnel fallback: any character
  the codec cannot encode is a hard error.
- For a **data-layer patch** (rewriting packs instead of runtime hook), CP932 can
  only carry characters already present in its table/`sjis_ext.bin`; validate
  with `--no-cp932-tunnel`.

## 2. Character names first (角色统一)

Translate `characters` before `text`, then build a glossary in memory:

```text
(variant, source name) -> canonical translated name
```

Rules:

1. One canonical translation per `name`. All later speaker prefixes must use it.
2. `display` may carry brackets/honorific (`【優理】`, `優理ちゃん`). Keep the
   bracket style; translate only the inner text. `display_translation` must keep
   the same opening/closing marks.
3. If `display == name` (common in older works), `display_translation` and
   `name_translation` should be the same string unless the user asks otherwise.
4. Do not translate names phonetically differently in different lines. If a
   character is missing from `characters`, create a sidecar glossary note
   (`glossary.md` next to `translate.json`) and reuse that spelling everywhere.
5. `cn` and `tw` are separate variants: use Simplified for `cn`, Traditional for
   `tw`; never copy one variant's names into the other.
6. Preserve honorific information when it matters to the relationship
   (`さん`/`ちゃん`/`先輩` → natural Chinese such as 小姐/先生/前辈/小…, or keep the
   Japanese suffix if the project's style guide says so). Be consistent per
   character, not per line.
7. The glossary is the source of truth for speaker prefixes:
   `【Big Sister】「…」` → `【大姐姐】「…」` (cn) / `【大姊姊】「…」` (tw).

## 2.5 UI literals and choices (按钮/菜单/选项)

`choices` are the select-menu option labels. `literals` are script string
constants; `context` tells you where the string is consumed:

| `context` | Meaning | Action |
|---|---|---|
| `es.BT.NAME.SET` | button display name | translate visible Japanese labels |
| `DIALOG` / `MENU` / `es.DIALOG.YESNO.SET` | dialog/menu text | translate |
| `es._mes` | in-game message/error text | translate user-facing messages; leave debug-only text if unsure |
| `ES.SEL.SET` | option label (duplicated in `choices`) | translate via `choices`; keep both consistent if you fill both |
| `LET` / `STR` / `IF` | may build a later dialog/message | translate only human sentences, never identifiers |
| `CG` / `CGACT` / `CGINFO` / `TEXT` / `es.SP.*` / `es.BT.KEY.SET` / `ES.SEL.GO` | asset, layer, key, or action IDs | **do not translate** |

Rules:

1. Buttons/options are part of the translation surface; leaving a visible
   Japanese label empty is a defect unless it is an internal ID or a
   pre-rendered image button (see below).
2. Keep the original **width class**. Fixed-cell YU-RIS UIs center by full-width
   character cells. If the source label is all full-width, write Latin letters,
   digits, and spaces in full-width too:
   - `Ｅ－ｍｏｔｅ機能紹介` → `Ｅ－ｍｏｔｅ功能介绍` (not `E-mote 功能介绍`)
   - `β４機能紹介` → `β４功能介绍`
   - `テスト１` → `测试１`
   Mixing half-width ASCII into a full-width label shifts the text off-center.
3. Do not translate names that are clearly internal (`■SAVE`, `■Page01`,
   `■Check1`, `■グループ01`, asset paths, `CG`/layer IDs). They are keys, not
   prose; changing them can break lookups.
4. Pre-rendered image buttons (common for the bottom system bar: `セーブ`,
   `ロード`, `クイックセーブ`, …) contain no script text. Text translation
   cannot change their pixels; they need an asset replacement or a button-system
   change. Do not fake entries for them.

## 3. Plot translation (剧情/台词)

Translate `choices` first where they occur, then `text` entries line by line:

- Use the surrounding entries in the same `file`/`scene` (`scenario`) or the same
  `script_path` (`word`) as context. Read before and after; do not translate a
  line in isolation when it is a continuation.
- Keep the original meaning, tone, and character voice. Do not add exposition,
  merge lines, split lines, or reorder entries. One entry = one displayed line.
- For `choices`, translate the option as a button label: short, natural, and
  consistent with the character glossary when a name appears.
- Keep speaker prefixes and quotation marks exactly as in the source:
  `【…】`, `「…」`, `『…』`, `（…）`, `“…”`. Translate the words inside; keep the
  marks and the prefix name from the glossary.
- Preserve control tokens verbatim: `\n`, `\r`, `\t`, `\\xNN`-style escapes,
  `%s`/`%d`/`%02d`-style format specifiers, and `$VARIABLE` references. Count
  them; do not translate, add, or drop them.
- Do not translate ASCII asset IDs, command names, file names, or numbers that
  are data rather than prose.
- Chinese punctuation: use full-width punctuation consistently (`，。！？`), keep
  the source's ellipsis/dash style if it carries rhythm; do not mix half/full
  width per line.
- Onomatopoeia/SFX: use an established Chinese equivalent of similar length;
  keep recurring SFX consistent across scenes.
- Keep line length close to the original. Aim for ≤ 1.5× the source character
  count; > 2× risks UI overflow and is reported by the validator.
- Erotic/adult content: translate plainly and faithfully; do not censor unless
  the user asks.

## 4. Write back

- Keep the file UTF-8, same schema, same entry order, same keys.
- Fill only the translation fields listed above.
- Do not add comments or trailing text to the JSON.
- If a translation is intentionally left empty, record why in your final report
  (the game will show the original).

Runtime use (no game data is modified):

```bash
yu-cli run <project-root> --translate <out-dir>/translate.json
```

Android: `translate` in `run-config.json` / the `translate` intent extra.

## 5. Validate before reporting

Run the bundled validator:

```bash
python scripts/validate_translate.py <out-dir>/translate.json
```

Options:

```text
--codec cp932|gbk|big5|auto   override the per-entry codec (default: use file)
--allow-unencodable           downgrade encoding failures to warnings
--no-cp932-tunnel             require direct CP932 encoding (data-layer patch)
--strict                      treat empty translations as errors (default: warning)
--max-ratio 2.0               length-ratio warning threshold
```

Fix every **ERROR**. Warnings are decisions you must explain:

- empty translation (original kept);
- speaker prefix not found in the character glossary;
- length ratio outside bounds;
- character translation missing a bracket mark.

## 6. Report

Return a short summary:

```text
characters translated: N/M
choices translated: N/M
literals translated: N/M (display/message only)
text translated: N/M (word X, scenario Y)
glossary: N names (cn/tw)
validator: 0 errors, K warnings (list the important ones)
codec: gbk (cn) / big5 (tw) / cp932 (runtime tunnel)
```

## 7. Common mistakes

- Editing `key`, `meta`, `codec`, or coordinates → breaks the importer/hook.
- Translating `【】`/`「」` marks or dropping them → speaker/name parsing breaks.
- Using two different Chinese names for the same `name` → breaks 角色统一.
- Mixing Simplified into `tw` or Traditional into `cn`.
- Mixing half-width ASCII into a full-width UI label (breaks fixed-cell centering).
- Translating internal IDs (`■SAVE`, `■Page01`, layer/asset names) instead of
  display labels.
- Writing emoji/rare CJK that the target codec cannot encode (validator catches;
  CP932 runtime tunnel has a finite carrier space).
- Merging two `scenario` entries with the same `line` but different `seq` → the
  line-level importer sees a shifted array.
- “Improving” the source text instead of translating it.

## References

- `references/schema.md` — full `translate.json` field and key specification.
- `references/style-guide.md` — worked examples for names, speakers, control
  tokens, and codec pitfalls.
- `scripts/validate_translate.py` — structural + glossary + codec validator
  (Python 3, stdlib only).
