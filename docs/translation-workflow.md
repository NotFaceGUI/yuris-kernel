# 翻译工作流(YU-RIS)

> 目标:用一份 `translate.json` 承载**角色表 + 正文**的原文与译文;翻译阶段不碰
> 游戏数据,导出/校验/注入各自独立。
>
> 游戏名/路径一律用 `<project>` 占位;真名只在 `AGENTS.md`。

## 1. 导出

```bash
yu-cli export "<project>" "<out>" --format translate-json
```

产物只有一个 `<out>/translate.json`(UTF-8,无 BOM):

- `characters`:角色显示名/本名 + 空译文槽;
- `text`:编译脚本 `WORD` 正文 + 外置剧本正文,每条带稳定 key;
- 不含 `literals.csv`、`text.csv`、人读副本。

字段与坐标见
[`.agents/skills/yuris-translation/references/schema.md`](../.agents/skills/yuris-translation/references/schema.md)。

## 2. 翻译(AI skill)

仓库自带一个可分发的翻译 skill:

```text
.agents/skills/yuris-translation/
├── SKILL.md                     # 触发词/流程/角色统一/剧情规则
├── references/schema.md         # translate.json 字段规范
├── references/style-guide.md    # 例子:名字、说话人、控制符、编码
└── scripts/validate_translate.py
```

- 在 ZCode 中,项目内 `.agents/skills/` 会被自动发现;也可整目录复制到其它
  AI 工具的 skills 目录,或复制到 `~/.agents/skills/` 供全局使用。
- Skill 的硬规则:
  - 先翻 `characters`,建立 `(variant, name) -> 译文名` 词汇表;
  - 正文里所有 `【名字】`/`名字「…」` 必须用词汇表的同一译名;
  - 只填 `translation` / `display_translation` / `name_translation`;
  - 不改 `key`/`meta`/`codec`/坐标,不合并/拆分行;
  - 保留 `\n`/`\r`/`%s`/`$VAR` 等控制符,保留 `【】「」『』（）` 等标记;
  - `cn` = 简体(通常 GBK),`tw` = 繁体(通常 Big5);`cp932` 条目翻中文
    需要 GBK/Big5 补丁或 CP932 载体字/隧道方案,不能静默产出不可编码文本。

## 3. 校验

```bash
python .agents/skills/yuris-translation/scripts/validate_translate.py \
  "<out>/translate.json"
```

可选参数:

| 参数 | 作用 |
|---|---|
| `--codec auto|cp932|gbk|big5|none` | 覆盖每条目的 codec;`none` 跳过编码检查 |
| `--allow-unencodable` | 编码失败降为 warning |
| `--strict` | 空译文视为 error |
| `--max-ratio 2.0` | 译文/原文长度比 warning 阈值 |

检查项:

- schema / 数组类型 / `meta.counts` 与条目数一致;
- `key` 全局唯一;
- 角色表译文非空(警告)、`display` 的括号对在译文中保留;
- 正文说话人前缀与角色词汇表一致;
- 控制符多重集一致、不新增换行;
- 按条目 codec 检查可编码性;
- 长度比异常 warning。

退出码:有 ERROR = 1,仅 warning = 0。

## 4. 后续注入

当前仓库提供导出与校验;把译文写回游戏有两条路线,尚未接线:

1. **运行时 hook**(计划):按 `key` 在排版前替换字符串;显示 codec/tunnel
   由引擎现有 `PatchKind` 决定。优先做,因为不改游戏数据。
2. **数据层回写**(开源工具主流):把译文按坐标写回 YSTB 参数区/外置剧本,
   重新封包。适合发永久补丁,需要额外的写入/封包与字节级回归。

## 5. 编码速查

| 条目 codec | 目标语言 | 注意 |
|---|---|---|
| `gbk` | 简体中文 | 避免繁体专用字、生僻扩展汉字 |
| `big5` | 繁体中文 | 避免简体专用字、emoji |
| `cp932` | 日文原版 | 中文字符通常不可编码;需 GBK/Big5 补丁或载体字/隧道方案 |
| `cp932` + `text_tunnel` | 日文原版(扩展字) | 隧道表只覆盖**既有**映射,不能编码任意新中文 |

`translate.json` 本身始终是 UTF-8;最终注入时按条目的 `codec` 编码。校验脚本
会把第一条不可编码字符报出来。
