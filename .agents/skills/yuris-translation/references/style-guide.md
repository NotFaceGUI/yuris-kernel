# Translation style guide (YU-RIS)

This is the working style for `translate.json`. It is intentionally concrete; when
the user's project glossary or style sheet conflicts, the user's sheet wins.

## Target variants

| `variant` | Language | Typical codec | Punctuation |
|---|---|---|---|
| `cn` | Simplified Chinese | `gbk` | full-width `，。！？` |
| `tw` | Traditional Chinese | `big5` | full-width `，。！？` |
| `base` | original (usually Japanese) | `cp932` | keep source style |

Do not mix Simplified into `tw` or Traditional into `cn`. If a `cn` line contains
Traditional-only glyphs (or vice versa), fix the variant, not the codec.

## Character names (角色统一)

Build the glossary first and never deviate:

```text
base: Big Sister  -> 大姐姐 (cn) / 大姊姊 (tw)
base: 優理        -> 优理   (cn) / 優理   (tw)
```

Rules with examples:

| Source | Good `cn` | Bad |
|---|---|---|
| `【優理】` | `【优理】` | `优理` (marks dropped) |
| `優理「あ、こんにちは……」` | `优理「啊，你好……」` | `優理「啊，你好……」` (name not localized) |
| `【Big Sister】「……」` | `【大姐姐】「……」` | `【Big Sister】「……」` (untranslated name) |
| `【大姊姊】` (tw source) | `【大姊姊】` | `【大姐姐】` (wrong variant) |

- If `display == name`, translate both fields to the same string.
- If `display` has an honorific (`優理ちゃん`), translate the honorific once and
  keep it in `display_translation`; `name_translation` stays the bare name.
- When a speaker is not in `characters`, write the chosen spelling in a sidecar
  `glossary.md`; use exactly that spelling in every line.

## Plot text

### Speaker prefix

Keep the prefix structure and translate only its words:

```text
source: 【優理】「そんなことないよ」
cn:     【优理】「才没有那种事呢」
tw:     【優理】「才沒有那種事呢」
```

If the source uses `名字「…」` without `【】`, keep that style:

```text
source: 優理「そんなことないよ」
cn:     优理「才没有那种事呢」
```

Never move the speaker into the dialogue or add a speaker where none exists.

### Control tokens

Preserve every token exactly, in the same order:

| Source fragment | Must appear in translation |
|---|---|
| `\n`, `\r`, `\t` | same escape |
| `%s`, `%d`, `%02d` | same specifier |
| `$NAME`, `$123` | same reference |
| `\C`, `\R`-style single-letter escapes | same escape |

Example:

```text
source: 名前は%02d番です\n$PLAYER_NAME
good:   名字是%02d号\n$PLAYER_NAME
bad:    名字是2号\n玩家名        (token changed)
```

### Line boundaries

- One entry = one displayed line. Do not merge two `scenario` entries even if
  they form one sentence; translate each as a coherent fragment.
- Do not split one entry into multiple lines unless the source already contains
  a newline token.
- If a sentence spans entries, use the neighboring entries for context, then
  produce translations that read in sequence.

### Length and layout

The game lays out text by character width. Long translations overflow the text
box, especially in `WORD` lines with a fixed window.

- Aim for ≤ 1.5× the source character count.
- > 2× is reported by the validator; rewrite shorter or split at a natural
  punctuation point (but keep one entry = one line).
- Do not pad short lines with spaces or decorative characters.

### Tone and terminology

- Translate meaning, not word order. Keep the character's voice: rough/refined,
  polite/casual, childish/mature.
- Keep recurring terms (places, organizations, skills, items) identical across
  the whole file; add them to the sidecar glossary if they are not characters.
- SFX/onomatopoeia: use a Chinese equivalent of similar length; keep recurring
  SFX consistent.
- Numbers, dates, counters: follow Chinese conventions unless a token must stay
  verbatim (see control tokens).
- Adult content: faithful, plain translation; no censorship unless requested.

## Encoding pitfalls

`translate.json` is UTF-8, but the game entry has a `codec`:

- `gbk` — Simplified Chinese. Avoid Traditional-only glyphs and rare CJK ext-B+
  characters; check with `text.encode("gbk")`.
- `big5` — Traditional Chinese. Avoid Simplified-only forms, emoji, and rare
  characters; check with `text.encode("big5")`.
- `cp932` — Japanese Shift-JIS. Most Chinese characters **cannot** be encoded.
  A Chinese translation on a `cp932` entry requires a GBK/Big5 patch or a
  carrier-font/tunnel project; do not silently produce text the final encoder
  will reject.
- `sjis_ext.bin` (`meta.text_tunnel: true`) maps the **existing** extended
  characters to Unicode. It does not give you arbitrary new Chinese glyphs.

The validator reports the first unencodable character per entry. Fix the wording
or switch the project's codec route; never change the `codec` field.

## Duplicates and recurring lines

The same source line can appear under several keys (different scenes/scripts).
Translate them consistently:

- Prefer reusing the earlier translation for identical source text.
- If context changes the meaning, translate per scene but keep names and terms
  from the glossary.
- Never leave one duplicate translated and another empty unless you explain why.

## Quick self-check before writing back

- [ ] Every `characters` entry has non-empty `name_translation` (and
      `display_translation` if `display` is non-empty).
- [ ] `display_translation` keeps the original bracket pair.
- [ ] Every speaker prefix uses the glossary spelling.
- [ ] Control tokens match the source (validator checks the multiset).
- [ ] No line merged/split; no new newline unless the source had one.
- [ ] No Simplified in `tw`, no Traditional in `cn`.
- [ ] Validator: 0 errors; every warning explained.
