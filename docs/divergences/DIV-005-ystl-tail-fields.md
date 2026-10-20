# DIV-005 · yst_list.ybn 尾字段定性:旁线笔记的"脚本文件号"被否定

- 状态: **fixed**(2026-10-19,按官方 ERIS 产物逐字节确证并钉进回归)
- 优先级: 高
- 发现日期 / 发现方式: 2026-10-19,任务书 B 对 ERIS 语料 `yst_list.ybn`
  全量解析时,发现按旁线口径解释的尾字段与磁盘文件编号矛盾。

## 复现

- 数据: 官方 YSCom 0.495 编译的 ERIS 产物 `ysbin/yst_list.ybn`(9,130 字节,141 条)。
- 锚点: 记录区从 `+0x0C` 起(头部仅 12 字节);步长 = `0x1C + nameLen`。

## 事实(逐字节实测)

```text
记录:{ u32 index, u32 nameLen, byte[name], u32 filetimeHi, u32 filetimeLo,
       u32 varCount, u32 labelCount, u32 textCount }
```

- 旁线 `<side-repo>/docs/format-ybn.md` §4 / `format-ybn-loader.md` §3 把
  `+0x18+nameLen` 的 u32 记作"脚本文件号(引擎按出现顺序收集)"。
- **否定证据**:该字段在 141 条记录中只有 14 个不同值(值域 0..=151),
  无法承担 136 个不同脚本的编号;而 `index` 字段恰为 0..140 的升序,
  与磁盘 `yst%05d.ybn` 文件号一一对应(0..140 中缺 26/27/69/74/108 五个号,
  恰为五条"无脚本产物"条目——纯声明文件 global.yst / macro.yst /
  stdlib\macro.yst / userdefine\global.txt / userdefine\macro.txt,
  其 varCount/labelCount 为哨兵 `0xFFFFFFFF`)。
- RxYuris `YSTL.cpp` `YSTL_V5` 的解释(高/低时间戳 + 变量数 + 标签数 +
  文本数)与数据吻合;其中 **labelCount 与 ysl 逐条互证:136 个有产物脚本
  全部相等**(1941 个标签按 script_no 分组计数)。
- 头部同理修正:旁线称"+0x00 byte[8] 未知前缀",实为 `YSTL` 魔数 + 版本;
  记录区起点是 `+0x0C`(即头部只有 12 字节),与 YSLB 的 0x400 索引块起点
  口径一致(`+0x0C`)。

## 根因

旁线笔记按 Ghidra 伪代码的"最后一个 int 字段"命名,未与真实数据的值域
分布做互证;`[待验证]` 标记一直未回填。

## 修法

`yu-format::ystl` 按 12 字节头 + 五 u32 尾字段实现;`Entry::has_script()`
暴露哨兵语义;引擎若确以"出现顺序"装载脚本(而非字段值),与本布局不冲突。

## 回归

- `cargo test -p yu-format --test corpus_eris`(需 `YURIS_SAMPLE_ERIS_YSBIN`):
  `yst_list ↔ yst 文件 ↔ ysl` 三方一致性断言(141 条 / 5 哨兵 / labelCount
  逐条相等)全部通过;原始输出见 `PROGRESS.md` 成果 B2。
