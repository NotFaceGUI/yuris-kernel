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

- `characters`:角色显示名/本名 + 运行期调用坐标(`script_id`/`pc`)+ 空译文槽;
- `choices`:`ES.SEL.SET`/`ES.CSEL.SET` 的选项字符串 + 调用坐标;
- `literals`:脚本字符串常量(按钮/菜单/对话框/提示 + 内部 ID),带
  `context`(`es.BT.NAME.SET`/`DIALOG`/`CG` 等)用于区分显示文本与资源/图层 ID;
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
- `key` 在 characters + choices + text 内全局唯一;
- 角色表译文非空(警告)、`display` 的括号对在译文中保留;
- 正文说话人前缀与角色词汇表一致;
- 控制符多重集一致、不新增换行;裸剧本正文不得引入 `//`/`/*`/行首 `#`/`\`;
- 按条目 codec 检查可编码性(CP932 默认按运行期隧道估算;`--no-cp932-tunnel`
  恢复严格检查);角色名、选项、literals 同样检查;
- 固定布局 UI 标签若原文全角、译文引入半角 ASCII(字母/数字/空格)则 warning
  (会破坏按全角字格居中);
- 长度比异常 warning。

退出码:有 ERROR = 1,仅 warning = 0。

## 4. 运行时注入(已接线)

```bash
yu-cli run "<project>" --translate "<out>/translate.json"
yu-cli serve "<project>" --translate "<out>/translate.json"
yu-cli run "<project>" --mode desktop --translate "<out>/translate.json"
```

Android 在 `run-config.json`(或 intent extra)里给 `"translate": "<绝对路径>"`。

替换范围(只替换文件内条目,文件外文本不动):

| 条目 | 入口 | 坐标 |
|---|---|---|
| `text.source=word` | `op_word` | `(script_id, pc)` |
| `text.source=scenario` | v555 `TEXT[FILE]` 写 LET 之前 | `(file, line, seq)` |
| `characters` | `GOSUB` 字符串参数 | `(script_id, pc, arg 0/1)` |
| `choices` | `GOSUB` 字符串参数 | `(script_id, pc, arg)` |
| `literals` | 脚本字符串常量(`0x4D` 记录) | `(script_id, pc, window, segment)` |

编码:

- 目标 codec = 当前补丁的显示 codec(`--patch auto|original|gbk|big5|tunnel`)。
- GBK/Big5 直接编码;不可编码字符装载期报错。
- CP932 优先直接编码,其余字符分配到 SJIS 隧道码;装载器保留游戏自带
  `sjis_ext.bin` 映射、扫描脚本体/变量初值/外置剧本里已出现的双字节对后
  追加新码,并把扩展表同时交给 VM 与渲染器。隧道码仍是脚本可按前导字节
  步进的双字节,`STRLEN`/`_strmid`/剧本 lexer 不需要换 codec。
- 翻译模式自动打开系统字体缺字回退(`tuning.font_fallback`)。

启动日志会打印 `translate: words=… scenario_lines=… characters=… choices=…
tunnel=+… bytes=…`;`nativeStatus` 返回命中/未命中统计。

数据层回写(改写封包)仍是另一条路线,未接线;本文件只覆盖运行时 hook。

已知限制:

- 只替换 `translate.json` 内条目;UI 字面量、动态 `_` 文本、`DIALOG` 等
  不在文件内,保持原文。
- CP932 隧道容量约 3400 个唯一非 CP932 字符(官方分配算法);不足装载期报错。
- 行内/裸剧本文本不能含 `//`、`/*`,也不能以 `#` 或 `\` 开头(会改变 DSL
  结构);运行时会拒绝该条替换并保留原行。
- `literals` 只应翻译显示类条目;`■SAVE`/`■Page01`/图层与资源名等内部 ID
  不能翻,否则会破坏查找。
- 固定字格 UI 要保持全角宽度类别;半角 `E-mote`/数字/空格会让按钮文字偏移。
- 预渲染图片按钮(常见于底部系统栏 `セーブ/ロード/...`)不在脚本文本里,
  文本 hook 改不了,需要替换 UI 素材。
- v555 预烘焙字形页(`fnt_s20_nNN`)覆盖的字形不随隧道扩展;这类构建的
  译文仍受原字库限制。
- 译文会进入 `$_TEXT`/回想/存档;换翻译表后旧存档里的隧道码可能失效。
- 多语文件只有一套目标 codec;运行时若不做全局 codec 切换,不能同时正确
  显示 cn/tw 两套译文。

## 5. 编码速查

| 条目 codec | 目标语言 | 注意 |
|---|---|---|
| `gbk` | 简体中文 | 避免繁体专用字、生僻扩展汉字 |
| `big5` | 繁体中文 | 避免简体专用字、emoji |
| `cp932` | 日文原版 | 运行时 hook 会直接用 CP932 能表示的字,其余分配新隧道码(容量约 3400 个唯一非 CP932 字符) |
| `cp932` + `text_tunnel` | 日文原版(扩展字) | 既有映射保留,新中文继续追加;数据层补丁仍受既有表限制 |

`translate.json` 本身始终是 UTF-8;最终注入时按**当前补丁的显示 codec**编码,
条目 `codec` 只用于多语条目的优先级选择。校验脚本会把第一条不可编码字符报出来
(CP932 默认按运行期隧道估算容量)。
