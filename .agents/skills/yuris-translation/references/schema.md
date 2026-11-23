# translate.json schema (yuris-translate-1)

Produced by:

```bash
yu-cli export <project-root> <out-dir> --format translate-json
```

UTF-8, no BOM, one JSON object. Only `characters` and `text` are present; string
constants (`literals`), CSV tables, and human-readable copies are intentionally
not produced in this mode.

## Top level

```json
{
  "schema": "yuris-translate-1",
  "meta": { ... },
  "characters": [ ... ],
  "text": [ ... ]
}
```

### `meta` (read-only)

| Field | Meaning |
|---|---|
| `project_root`, `data_root` | absolute paths used for the export |
| `text_codec` | `cp932` / `gbk` / `big5` for compiled text |
| `text_codec_source` | `auto` / `cli` / `default` |
| `text_tunnel` | `true` = `sjis_ext.bin` was loaded and used to decode extended characters |
| `script_key`, `script_key_source` | YSTB XOR key and how it was resolved |
| `counts` | `characters`, `word_units`, `scenario_units`, `text` (= word + scenario) |

`counts` must match array lengths; the validator checks it.

## `characters[]`

One entry per character definition, in game-definition order.

```json
{
  "key": "character:base:data\\script\\userdefine\\キャラ名定義.txt:0",
  "variant": "base",
  "table": "data\\script\\userdefine\\キャラ名定義.txt",
  "index": 0,
  "display": "【優理】",
  "name": "優理",
  "codec": "cp932",
  "display_translation": "",
  "name_translation": ""
}
```

| Field | Meaning | Editable |
|---|---|---|
| `key` | stable coordinate: `character:{variant}:{table}:{index}` | no |
| `variant` | `base` / `cn` / `tw` (language build) | no |
| `table` | source script path of the definition table | no |
| `index` | 0-based definition order within `table` | no |
| `display` | name as shown, usually with brackets/honorific | no |
| `name` | raw name key used by the engine | no |
| `codec` | codec of this definition script | no |
| `display_translation` | translated display name, keep bracket marks | **yes** |
| `name_translation` | translated raw name | **yes** |

If `display` starts/ends with a mark pair (`【】`, `「」`, `『』`, `（ ）`, `()`,
`“”`), `display_translation` must keep the same pair.

## `text[]`

Two sources share the array.

### `source: "word"` (compiled `WORD` text)

```json
{
  "key": "word:s123:pc0",
  "source": "word",
  "script_id": 123,
  "script_path": "data\\script\\userscript\\example.txt",
  "pc": 0,
  "unit": 0,
  "line": 1,
  "codec": "cp932",
  "text": "優理「あ、こんにちは……」",
  "translation": ""
}
```

| Field | Meaning |
|---|---|
| `key` | `word:s{script_id}:pc{pc}` |
| `script_id` | `ystNNNNN.ybn` number |
| `pc` | instruction index inside that script (stable for a build) |
| `unit` | ordinal of the `WORD` within the script |
| `line` | 1-based text line (`RETURNCODE` count + 1) |
| `text` | original decoded text (UTF-8) |
| `translation` | translated text (fill this) |

### `source: "scenario"` (external scenario file)

```json
{
  "key": "scenario:cn:sc_cn_scenario/saku_01_00.txt:56:0",
  "source": "scenario",
  "variant": "cn",
  "file": "sc_cn_scenario/saku_01_00.txt",
  "line": 56,
  "seq": 0,
  "kind": "macro",
  "macro": "LC",
  "scene": "saku_01_00",
  "label": "RP_ona001",
  "codec": "gbk",
  "text": "【大姊姊】「真伤脑筋……」",
  "translation": ""
}
```

| Field | Meaning |
|---|---|
| `key` | `scenario:{variant}:{file}:{line}:{seq}` |
| `variant` | `base` / `cn` / `tw` |
| `file` | path inside the mount (`/` separators) |
| `line` | 1-based file line |
| `seq` | 0-based ordinal of the unit within that line (multiple macros per line) |
| `kind` | `macro` (a `\LC`/`\LE`/`\LT`-style line) or `text` (bare/inline text) |
| `macro` | macro name for `kind=macro`; leading command for inline text; `-` for bare lines |
| `scene`, `label` | nearest `#` scene/label markers (context only) |
| `text` | original decoded text (UTF-8) |
| `translation` | translated text (fill this) |

## Invariants

1. `key` values are unique across `characters` + `text`.
2. `meta.counts.characters == len(characters)`.
3. `meta.counts.word_units + meta.counts.scenario_units == len(text)`.
4. `codec` is metadata; never change it to “fix” encoding.
5. Do not add or remove entries. Empty `translation` means “keep original”.
6. Do not add fields the importer does not know; only fill the translation
   fields.
