# DIV-006 · GO/GOSUB 目标标签在 ysl 中缺失:ERIS 样板的可选扩展点

- 状态: **explained**(官方行为已定性;非本内核缺陷,无需修)
- 优先级: 中
- 发现日期 / 发现方式: 2026-10-19,任务书 B "标签闭环"验收对 ERIS 全语料
  做 GO/GOSUB 目标 → ysl 可解析性检查时发现。

## 复现

- 数据: 官方 YSCom 0.495 编译的 ERIS 全部 136 个 `yst%05d.ybn` + `ysl.ybn`(1941 标签)。
- 锚点: yst00010(es_First.yst)的 `\_gosub(ES.ST2.DEFINE)`(源码 `///x` 标记行)。

## 实测数字

- GO/GOSUB 静态引用共 **10,410** 处:10,174 处可在 ysl 解析;
  137 处为动态目标(`H`/`V`/数值载荷,运行期求值,静态不可辨);
  **99 处(76 组、69 个不同标签)引用了 ysl 中不存在的标签**。
- 缺失标签构成(钉死于 `crates/yu-format/tests/fixtures/pinned_unresolved.rs`):
  - `ES.<画面>.VoicePlay.<N>` 音声再生钩子(~60 组):ES.CONFIG / ES.SAVE /
    ES.LOAD / ES.QSAVE / ES.MENU / ES.AUTO / ES.SKIP / ES.TITLE … 各界面的
    语音重放回调;
  - 更新/扩展钩子:`ES.ST2.DEFINE`、`ES.EMOTE.VAR.DEFINE`、
    `ES.SAVECONF.MAIN`、`es.Original.CfgInit`、`es.Original.TitleEnd`、
    `ES.CONFIGINITCONF.VoicePlay.1` 等;
  - 宏拼接前缀:`ES.CONFIG.VoicePlay.` / `es.EF.COM.`(运行期拼标签名)。

## 官方行为

- **YSCom 编译器不校验 GO/GOSUB 目标存在性**:源码中存在
  `\_gosub(ES.ST2.DEFINE)`(`ERIS/es_First.yst` 实证)而全工程无
  `#ES.ST2.DEFINE` 定义,官方编译器照样产出 yst/ysl 并验收通过。
- **引擎容忍缺失标签**:该引用位于启动必经的 `ES.FIRST.LOOP`,官方引擎
  实测仍启动到标题画面——与手册/脚本侧的 `ERROR[COM_XX=n]` 按命令族设
  错误等级机制一致(缺标签按 GOSUB 族错误等级降级为可继续)。
  错误等级本身是脚本可见语义,须由运行核建模(`yu-rt` 范围)。

## 内核行为

`yu-format` 如实报告:静态可辨的引用进 `DecodedScript.labels`;是否解析、
解析失败如何降级 = `yu-rt` 的命令语义,不在格式层。语料回归把 76 组钉死,
新语料出现清单外缺失即报错(跨版本对照的哨兵)。

## 根因

ERIS 样板把"游戏侧可选提供的钩子"直接写成 GOSUB 调用;SDK 样例工程自身
未提供这些标签,官方工具链/引擎双双重试放行。这不是数据损坏,是官方
生态的既定协作方式(汉化/二次开发亦可用同法挂接)。

## 回归

- `cargo test -p yu-format --test corpus_eris`(需 `YURIS_SAMPLE_ERIS_YSBIN`):
  resolved=10174 / dynamic=137 / unresolved=99(76 组)与钉死清单逐组一致。
- 原始输出见 `PROGRESS.md` 成果 B2。
