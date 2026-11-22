# DIV-RT-068 · v555 过期窗口长度与脚本字符串自定界

- 状态: **fixed(no-manual,2026-11-22)**
- 影响面: `crates/yu-rt/src/{expr,tables,vm}.rs`、
  `crates/yu-engine/src/patch.rs`
- 关联: `DIV-RT-067`(555 剧本词法器)、`DIV-RT-017`(变量空间)
- 样本: `<v555-sample>`(`<v555-sample>`,YU-RIS 0.555,汉化数据)

## 1. 现象

`yu-cli run <v555-sample> --frames 1 --backend soft` 自动编码时首错:

```text
FATAL [vm] rt.expr.malformed @ data\script\userdefine\...txt pc3:
      字符串字面量未闭合
```

`--patch gbk` 推进到 pc13,又报:

```text
rt.expr.malformed: 操作数越界: op=0x4d 偏移 0 声明 12 字节,窗口共 11
```

同一份数据官方引擎可启动(发行包即此形态),所以两处都是内核判定比官方严。

## 2. 根因一:双主 exe 的 lead 表冲突

工程根同时有两只主 exe(非 helper):

| exe | 256 B 静态 lead 表(@文件偏移 1681088) |
|---|---|
| `<main-exe>` | CP932 |
| `<chs-exe>` | **GBK** |

数据是汉化后的 GBK(`data\script\userdefine\...` 的脚本文本与一个文件名),
配 `<chs-exe>` 使用。旧 `static_codec_for_v555` 在“多主 exe 编码
不一致”时直接返回 `None` → `Auto` 回退默认 CP932。

pc3 实测字节:

```text
4d 04 00 22 d1 e0 22      // "姓"(GBK d1 e0)
```

- GBK:`d1 e0` 一个字,`22` 正常闭合;
- CP932:`d1` 单字节,`e0` 是前导字节,把结束引号 `22` 当尾字节吞掉
  → 字符串永不闭合。

**修复**:冲突时不再静默回退。`probe_codec_by_script_literals` 用工程
自己的 `0x4d` 字面量给候选表打分(未闭合 +8、结尾悬空前导 +12;窗口内
被截断的记录整条跳过)。唯一显著低分者生效,否则维持“不猜”。评分依据
是数据本身,不认游戏名/exe 名/语言名。

## 3. 根因二:窗口记录 length 过期(汉化变长替换)

s273(`data\script\userdefine\...`,汉化新增文件)pc13 的 `PSTR2`:

```text
窗口记录: offset=1035  length=11
窗口内容: 4d 0c 00 22 d0 a1 ca b1 ba f2 b5
          op  len=12   └── 只落进窗口 8 字节 ──┘
```

字符串记录自声明 12 字节负载(整条 15 字节),窗口只有 11。
日文原名 3 字(CP932 6 字节,整条 11 字节)与旧 length 吻合;
汉化名 5 字(GBK 10 字节)——工具更新了记录头,没有同步窗口记录。
窗口末尾最后一个字节 `b5` 是 GBK 前导字节、无尾字节,即“被截断”的
物理证据。s273 pc216 还有同型的 `4d 04 00 22 99 e1 91`(记录/窗口都
比真实字面量短)。

旧 `ScriptFile::window_payload` 严格按 `WindowRec.length` 切片,
`expr::decode` 读到声明长度就越界报错。

**修复(精确 555 门控,`SemanticProfile::window_payload =
RecordDelimited`)**:

1. `ScriptFile::window_payload_with_tail` 给出 `(window, tail)`,
   `tail` 从窗口末尾延伸到共享池内**下一个窗口 offset**(没有则池尾);
2. `expr::window_literal_unclosed` 静态标记“首记录为 `0x4d` 且字面量
   不在声明长度内闭合”的窗口;
3. `expr::decode_self_delimited` 只对这类窗口按界定符重解:
   字面量在声明长度内已闭合的记录**逐字节保持原样**;未闭合时读到
   `window + tail` 的结束引号,补读上限 = 下一个窗口/池尾;非字符串
   记录仍按声明长度严格切;
4. 补读成功的窗口发一条 Verbose
   `rt.window.length_stale`(含声明长度/实际字节/参数槽),每站点一次。

跳转槽等“首记录不是 `0x4d`”的窗口不进入该路径,避免把目标组号当载荷。

## 4. 边界与残留

- 只影响精确 `ysc_version == 555`;0.495/0.479 的 `window_payload` 与
  `Auto` 行为不变(已实测 `kind=original source=default`)。
- 补读绝不越过共享池内下一个窗口的 offset;找不到界定符时保留原严格
  错误,不猜文本内容。
- 官方 555 求值器是否**正式**忽略 `0x4d` 记录长度、以界定符为边界,
  尚未有独立 REA 逐指令证据;本档案的“官方能运行发行数据”是发行包
  层面的反证。若后续 REA 推翻,回退口径只有一个:恢复严格切片,
  把汉化数据作为 Data Profile 处理。
- 字符串记录**不是**窗口首记录、且记录长度过期的形态未覆盖(当前
  语料未出现);出现时按同一方法扩展,不提前实现。

## 5. 验收

```bash
# 自动编码启动(不再需要 --patch gbk),到标题/logo 无 Fatal
yu-cli run <v555-sample> --frames 90 --backend soft \
  --shot <tmp>/sample-f90.png --log-level warn
# 结果:frames ticked: 90;无 FATAL/UNIMP;截图 = Whirlpool logo

# 旧版 Auto 不变
yu-cli run <project-eris> --frames 1 --backend soft --log-level info
# patch: kind=original source=default execution=cp932 ...
```

测试:

- `crates/yu-rt/src/expr.rs::self_delimited_tests`(4 条):
  pc13 补读、pc216 悬空前导补读、结构正常窗口与严格解码逐字节一致、
  无尾部时不猜。
- `crates/yu-engine/src/patch.rs::static_codec_scan_tests`:
  `literal_scores_pick_the_table_that_closes_gbk_text`、
  `script_names_map_to_script_ids`;冲突 + 无数据仍
  `main_exe_conflict_is_none`。
- `cargo test -p yu-engine --lib` 70 passed;
  `cargo test -p yu-rt --test fixture --no-default-features` 144 passed;
  `cargo test -p yu-gfx` 通过。
