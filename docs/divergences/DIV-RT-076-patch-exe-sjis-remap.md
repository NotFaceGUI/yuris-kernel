# DIV-RT-076 · 汉化 exe 的 `VARINFO[SJISCODE]` 前导码补丁兼容

- 状态: **fixed(no-manual,2026-11-24)**
- 影响面: `crates/yu-rt/src/vm.rs`(启动自检)、
  `crates/yu-rt/src/vm/handlers/variables.rs`(`VARINFO[SJISCODE]`)、
  `crates/yu-rt/src/code.rs`(诊断码)
- 关联: `DIV-RT-066`(555 运行期面)、`DIV-RT-067`(555 剧本词法器)、
  `DIV-RT-069`(`RETURNCODE` 清缓冲)
- 样本: `<v555-pieces>`(YU-RIS 0.555,汉化版双 exe)

## 1. 现象

`<v555-pieces>` 汉化版进正文后:

- 剧情**自动快速推进**,不点鼠标也一句接一句;
- 经常**连文本框都不出现**,部分场景有框无字;
- 日文原版(`original` + 原包 `sc.ypf`)**正文正常**。

运行期 `YU_TEXT_TRACE`(日文原版对照):

```text
日文: v6205=4  s6120=14 s6123=8  draws=3 glyphs=3
汉化: v6205=0  s6120=0  s6123=0  draws=0 glyphs=0
```

即汉化版的剧本 parser **从未把正文交给文本任务**(`$6208/$6209/$6120/$6123`
全程为空),不是字体、文本框或 `CGACT[TEXT]` 绘制问题。

## 2. 根因:汉化包依赖 exe 二进制补丁改 `SJISCODE`

1. `<v555-pieces>` 与 `<v555-2>` 的 `ysc.ybn` **sha256 相同**
   (`7ee03ebb…`),`es_texttask.yst`/`es_text.yst` 框架同构;排除
   “555 ABI 不同”。
2. 但两者**编译进 `bn.ypf` 的 `es_scenario_lexer.yst` 不同**:
   - `<v555-2>`:前导范围 `129..198 || 201..252`(**GBK**);
   - `<v555-pieces>`:前导范围 `129..159 || 224..252`(**CP932**)。
3. 汉化包只替换 `scenario/*.txt` 与图片,没有重编译词法器。汉化版 exe
   用二进制补丁把 `VARINFO[SJISCODE]` 的返回值改成 CP932 范围内的伪码,
   让仍按 CP932 范围判词的 lexer 把 GBK 双字节汉字归成 `0x20` 词:
   - VA `0x455b10`:
     `shl eax,8; movzx ecx,[edx+1]; add eax,ecx` → `mov eax,0x824F`;
   - 同 exe 另有前导表补丁(VA `0x59c160` 起 64 字节置 1)。
4. 内核按原版语义返回真实 GBK 码(如 `B5 C4` → `0xB5C4`),lead=`0xB5`
   不在 CP932 范围 ⇒ lexer 不认汉字 ⇒ parser 不调用
   `es.ScenarioAddText` ⇒ 文本任务空转、场景 parser 继续推进。

## 3. 修法:按数据自检,不按游戏名

启动期 `Vm::detect_legacy_cp932_lexer`:

- 只在 `SemanticProfile.varinfo_sjis == FirstChar`(精确 v555)且执行
  codec 非 CP932(GBK/Big5)时启动;
- 扫描脚本路径含 `lexer` 者(找不到再退 `scenario`),用表达式整数
  常量判断词法器形态:
  - 同一脚本集齐 `129/159/224/252` 且不含 GBK 对 ⇒ CP932;
  - 集齐 `129/198/201/252` ⇒ GBK(原生中文词法器,不启用);
  - 两套都出现 ⇒ 形态不明,不猜。
- 仅 CP932 形态时置 `sjis_lead_remap = Some(0x824F)`;
  `VARINFO[SJISCODE]` 对“当前 codec 的前导字节”返回该伪码,
  其它情况(空串/单字节/未启用)保持原语义;
- 启动时发一条 `rt.sysvar.compat` Info,记录命中的词法器路径。

不加载、不执行任何汉化 DLL;只读取工程脚本/数据。

## 4. 边界

- 只覆盖 **“非 CP932 文本 + CP932 词法器”** 这一族补丁(GBK/Big5
  同型);原生 GBK 词法器(`<v555-2>`)与 CP932 工程完全不变。
- 0.495/0.479 等旧版本 `varinfo_sjis == Inert`,不进入自检。
- UIF(`uif_config.json` 字符替换/字体伪装)、Nepgear
  (`Nepgear.ini` + `Nepgear.chs` 文件重定向)是**另一层**汉化框架,
  本轮不涉及;它们改的是文本替换/资源挂载,不是 `SJISCODE` 语义。
- 补丁 exe 若还改了 `SJISCODE` 之外的语义(字体、文件、UI),按各自
  证据另立档案。

## 5. 验收

- `<v555-pieces>` 汉化版(修复后)运行 trace:
  `s6208=26 s6123=14 v6205=4 draws=3 glyphs=3`;
  行等待态 `v6205=204 v6240=1780 v6124=45 v6125=42`,文本停留在
  `ES.GAMEMAIN.TIP.MESW.TX` 层等待点击。
- 日文原版对照仍正常(同上 `v6205=4`,draws/glyphs>0)。
- 单测:
  - `vm::tests::lead_range_style_distinguishes_cp932_and_gbk_lexers`
  - `vm::tests::int_immediate_decodes_all_integer_widths`
- 回归: `cargo test -p yu-rt --lib` 161 passed;
  `cargo test -p yu-rt --test fixture --no-default-features` 160 passed;
  `cargo test -p yu-engine --lib` 89 passed。
