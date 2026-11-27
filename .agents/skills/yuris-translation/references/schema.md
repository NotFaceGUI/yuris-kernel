# translate.json schema (yuris-translate-1)

Produced by:

```bash
yu-cli export <project-root> <out-dir> --format translate-json
```

UTF-8, no BOM, one JSON object. `characters`, `choices`, `literals`, and `text`
are present in the JSON; CSV tables and human-readable copies are not produced in
this mode.

## Top level

```json
{
  "schema": "yuris-translate-1",
  "meta": { ... },
  "characters": [ ... ],
  "choices": [ ... ],
  "literals": [ ... ],
  "text": [ ... ]
}
```

### `meta` (read-only)

| Field | Meaning |
|---|---|
| `project_root`, `data_root` | absolute paths used for the export |
| `text_codec` | `cp932` / `gbk` / `big5` / `cp949` / `cp1252` / `utf8` for compiled text |
| `text_codec_source` | `auto` / `cli` / `default` |
| `text_tunnel` | `true` = `sjis_ext.bin` was loaded and used to decode extended characters |
| `script_key`, `script_key_source` | YSTB XOR key and how it was resolved |
| `counts` | `characters`, `choices`, `literals`, `word_units`, `scenario_units`, `text` (= word + scenario) |

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
  "script_id": 77,
  "pc": 3,
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
| `script_id`, `pc` | runtime call site of `es.CHAR.NAME` (used to replace the two string args) | no |
| `codec` | codec of this definition script | no |
| `display_translation` | translated display name, keep bracket marks | **yes** |
| `name_translation` | translated raw name | **yes** |

If `display` starts/ends with a mark pair (`【】`, `「」`, `『』`, `（ ）`, `()`,
`“”`), `display_translation` must keep the same pair.

## `choices[]`

One entry per string argument of `GOSUB("ES.SEL.SET"|"ES.CSEL.SET", …)`.
The runtime replaces the argument at the call site before the callee stores it,
so the option label is translated without touching layout parameters.

```json
{
  "key": "choice:s123:pc15:arg0",
  "script_id": 123,
  "pc": 15,
  "arg": 0,
  "codec": "cp932",
  "text": "キャラクターの読込・表示",
  "translation": ""
}
```

| Field | Meaning | Editable |
|---|---|---|
| `key` | `choice:s{script_id}:pc{pc}:arg{arg}` | no |
| `script_id` | script containing the `GOSUB` | no |
| `pc` | instruction index of the `GOSUB` | no |
| `arg` | 0-based evaluation order after the label window | no |
| `text` | original option label | no |
| `translation` | translated option label | **yes** |

## `literals[]`

Script string constants (`0x4D` records), including buttons, menus, dialogs,
messages, and internal IDs. `context` is the display/consumer context:

- `GOSUB` → the called label (`es.BT.NAME.SET`, `ES.SEL.SET`, …);
- other commands → the command name (`DIALOG`, `MENU`, `LET`, `CG`, …).

```json
{
  "key": "literal:s112:pc13:w1:s0",
  "script_id": 112,
  "script_path": "data\\script\\userdesign\\jp_0036.txt",
  "pc": 13,
  "command": "GOSUB",
  "context": "es.BT.NAME.SET",
  "window": 1,
  "segment": 0,
  "codec": "cp932",
  "text": "■スタート",
  "translation": ""
}
```

| Field | Meaning | Editable |
|---|---|---|
| `key` | `literal:s{script_id}:pc{pc}:w{window}:s{segment}` | no |
| `script_id`, `pc` | script and instruction | no |
| `window` | 0-based window index in the command | no |
| `segment` | 0-based quoted-segment index inside that window payload | no |
| `context` | display API or command consuming the string | no |
| `text` | original decoded constant | no |
| `translation` | translated constant | **yes** |

Translate only display/message entries. Internal IDs (`■SAVE`, `■Page01`,
asset paths, layer names, `es.BT.KEY.SET` values, `ES.SEL.GO` actions) must stay
empty. Fixed-cell UI labels must preserve the source width class; see
`references/style-guide.md`.

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

1. `key` values are unique across `characters` + `choices` + `literals` + `text`.
2. `meta.counts.characters == len(characters)`,
   `meta.counts.choices == len(choices)`, and
   `meta.counts.literals == len(literals)` (when present).
3. `meta.counts.word_units + meta.counts.scenario_units == len(text)`.
4. `codec` is metadata; never change it to “fix” encoding.
5. Do not add or remove entries. Empty `translation` means “keep original”.
6. Do not add fields the importer does not know; only fill the translation
   fields.

## Runtime import

```bash
yu-cli run <project-root> --translate <out-dir>/translate.json
yu-cli serve <project-root> --translate <out-dir>/translate.json
yu-cli run <project-root> --mode desktop --translate <out-dir>/translate.json
```

Android: `run-config.json` / intent extra `translate` = absolute path.

Behavior:

- `WORD` entries are keyed by `(script_id, pc)`; `text`/`choices` coordinates as
  above; external scenario lines are rewritten before the v555 `LET` array is
  filled.
- `literals` rewrite the matched quoted segment at command-cache build time
  `(script_id, pc, window, segment)`; all later evaluations see the translated
  string. Internal IDs left empty stay byte-identical.
- Entries not present in the file keep the original text; literals/UI strings
  not exported here are not touched.
- GBK/Big5 projects encode translations directly. CP932 projects encode what
  CP932 can represent directly and allocate new SJIS tunnel codes for the rest,
  extending the game's own `sjis_ext.bin` mappings; the renderer receives the
  extended table.
- A translation with a line break inside a `scenario` entry is rejected at load
  time (the DSL is one physical line per entry).
