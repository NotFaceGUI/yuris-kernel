# DIV-RT-055 · `ysv` 串初值被按字面存字节,表达式初值把字节码拼进了资源路径

- 状态: fixed
- 优先级: 高
- 发现日期 / 发现方式: 2026-10-30 / 商业包 0.481 引擎运行期诊断
  (`WARN [vfs] vfs.path @ V $w B &, B )…+0x0: NUL byte in lookup key`)

## 复现

- 游戏位置: 0.481 商业包(引擎 exe sha256 `331c586a…`),
  多包入口 `pac/`(base 7 + update 2)。
- 操作序列: `yu-cli run <root> --frames 600 --log-level info`(headless)。
- 锚点:
  - 诊断出现时机 = 引导期(启动摘要之前),共 5 条,前缀字节完全一致
    `56 03 00 24 77 04 42 01 00 26 2c 00 00 42 01 00 06 29 01 00 00`,
    后缀分别是 `cg/face/face_back_{na,off,on,onover,over}.png`。
  - 该前缀 = `ysv.ybn` 里 `slot=1413`(`kind=2` 脚本 0 局部串)的
    **初值字节**:表达式 `$1143[38,6]`。
  - 同一形态还有 `slot=1412`(`$1143[38,21]`),被脚本拼进声音路径:
    修复前是 `se/` + 字节码 + `sys001`,修复后是 `se/sys001` →
    资产 `se/sys001.ogg` 命中(5097 字节)。

## 官方行为

0.481 引擎 `FUN_00456e50`(ysv 记录应用;`0x456e50`,调用点 `0x456f61`):

```c
uVar2 = *(ushort *)(DAT_008098ec + 8);       // 记录数
puVar11 = (ushort *)(DAT_008098ec + 10);     // 记录游标
pvVar6 = _malloc(0x1000);                    // 求值缓冲
...
if (bVar4) {                                 // 本条记录可见(掩码/种类过滤)
  /* kind / ty / dims / 各维大小 写入变量槽 */
  if (ty == 1)      FUN_004658e0(slot, *(u32*)puVar11, *(u32*)(puVar11+2));
  else if (ty == 2) FUN_00465800(slot, ...);
  else if (ty == 3) {                        // 串初值
    uVar3 = *puVar11;                                    // u16 长度
    uVar8 = (*DAT_008097e0)(pvVar6, puVar11 + 1, uVar3); // ← 窗口求值器
    FUN_00465654(slot, pvVar6, uVar8);                   // 写变量
  }
}
```

要点:**数值初值直写,只有串初值过求值器**。`DAT_008097e0` 在静态镜像里
是 0(`0x8097e0` 起 256 字节全 0),运行期才赋值;全模块 9 处引用都是
`CALL dword ptr [0x008097e0]`,参数形态统一为 `(缓冲, 参数字节, 长度)`,
即 YSTB 参数窗口求值器 —— 与命令参数走同一个入口。

因此 `0x4d` 字面量(`M` + u16 长度 + 引号串)只是求值的**特例**;
`$1143[38,21]` 这类表达式初值必须在装载时求值。

`S_STR` 不能补上这个缺口:0.495 分发表把它映射到 `0x4634d8`
(`xor eax,eax; ret`,见 `crates/yu-rt/src/vm/handlers.rs` 的
`NOOP_COMMANDS_0495`),官方运行期根本不执行该赋值。

## 内核行为

修复前:`crates/yu-rt/src/tables.rs` 的 `decode_str_init` 只认「单条
`0x4d`」,其余形态**原样返回字节**,于是 `$1412/$1413` 的值就是那段
字节码;脚本 `LET $1351[$1382][i] = $1412 + $55[i]`(`s0 pc1124/1125`,
`es_button.yst`)把它拼到资源名前,VFS 查表时撞到 `0x00` 报
`NUL byte in lookup key`。

## 二分记录

- [x] L1 译码一致? 是 —— 同一 `ysv.ybn` 记录、同一初值字节,`yu-format`
      逐字节闭合。
- [x] L2 执行/事件流一致? 否 —— 变量装载后的值不同(字节码 vs 空串)。
- [x] L3 帧层状态一致? 帧像素未变(那些 CG 名在本包里本就不存在),
      但 5 条非法路径告警消失。
- [x] L4 帧间编排一致? 是。
- 定位结论: 变量初值投影层(ysv → 运行值),不在编码/版本/格式层。

## 根因

串初值不是「一段文本」,而是**一段 YSTB 参数数据**;内核把「解码不了的
形态」当成「原样字节」,把字节码本身当成了字符串值。

## 修法

- `crates/yu-rt/src/tables.rs`:`VarRecord::str_init_needs_eval()` ——
  只有三种形态不求值:空串初值、单条 `0x4d` 字面量、**解不成合法表达式**
  的字节(维持既有「原样字节」口径,不猜)。
- `crates/yu-rt/src/vm.rs`:`Vm::ysv_init_value()` + `Vm::eval_init_expr()`,
  在 `apply_global_initial_values` / `build_globals_restore` /
  `apply_script_locals` 三处统一走求值;求值失败留痕后按空串建槽,
  不再把字节码继续传播。
- 结果类型按串口径归一(`ParamTy::Str.coerce`),与参数槽同一套。

## 回归

- 对拍用例:`crates/yu-rt/tests/fixture.rs::ysv_str_init_expression_is_evaluated_not_stored_raw`
  (全局表达式初值 / 脚本 0 局部表达式初值 / 数组元素字面量初值三路断言)。
- 验证命令与原始结果:

```text
$ cargo test --release -p yu-rt --test fixture ysv_str_init_expression
test ysv_str_init_expression_is_evaluated_not_stored_raw ... ok
test result: ok. 1 passed; 0 failed

$ ./target/release/yu-cli.exe run <root> --frames 600 --log-level info
(修复前) 5  vfs.path   ← 非法路径
(修复后) 0  vfs.path   ← 诊断汇总里不再出现,且无新增诊断

$ probe_asset <root> se/sys001.ogg
se/sys001.ogg  HIT 5097 bytes   ← 声音路径从「se/ + 字节码 + sys001」纠正为「se/sys001」
```
