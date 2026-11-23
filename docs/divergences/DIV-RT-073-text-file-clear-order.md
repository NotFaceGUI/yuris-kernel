# DIV-RT-073 · v555 `TEXT[CLEAR]` 在文件成功装载后才清缓冲

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/other/handlers/text.rs`
- 关联: `DIV-RT-069`(行末清缓冲)、`DIV-RT-032`(`_TEXT`)
- 样本: `<v555-1>`、`<v555-pc>`、2.1MB 族

## 1. 现象

`TEXT[FILE=..., CLEAR=1]` 原先在**打开文件之前**清 `_TEXT`。
官方 `0x452a0c` 是:先打开文件,失败直接 `return -1`;**成功装载/切行后**
才判 `CLEAR` 并调用 `FUN_00469f9c` 清缓冲。语言回退(文件缺失 → 下一源)
时,官方保留旧文本,内核会先清空。

## 2. 证据

`b02a4dc04ae TEXT 0x452a0c`(反编译 `00452a0c_V555H_452a0c.c`):

```text
:61   iVar4 = FUN_0040bcdd();          // 打开 FILE
:62   if (iVar4 != 0) return 0xffffffff; // 失败: 直接返回 -1
...
:134  uVar9 = DAT_00662622;            // CLEAR 参数
:135  if (uVar9 != 0 &&
:136      args[CLEAR] == 1)
:137    state+0x15 = 1;
:138    FUN_00469f9c();                // 清 _TEXT/_TEXT_FLAG
```

`b1baf88a871 TEXT 0x4529d4` 同型:4 源打开/读取成功后才在
`LAB_00452b85` 执行同一 CLEAR 分支。

`FUN_00469f9c` 只清 `+0x18/+0x1c` 两块 0x2000 缓冲与
`+0x20..+0x2c` 四个游标,不动 LET 数组。

## 3. 修法

`other/handlers/text.rs`:

- 删除参数解析后的提前 `clear_text()`;
- 无 `FILE` 的 `CLEAR` 合法形态:仍执行 CLEAR;
- 缺 `LET` 的未取证形态:保持旧行为(先清后报、不打开文件);
- 有 `FILE` + `LET`:打开失败返回 `Yield`,**不清**;
- 打开成功后执行 CLEAR(官方在装载成功后必进入 CLEAR;与 LET 写入
  顺序无关,`LET` 目标只能是字符串数组,不会与文本缓冲别名);
- 该文件只由 `SemanticProfile::text == TextMode::FileArray`(精确 555)
  安装,0.495 的 `TEXT` 属性设置路径不受影响,无额外版本门控。

## 4. 回归

- `text_555_missing_file_does_not_clear_text_buffer`:
  `WORD("OLD")` → `TEXT[FILE=missing CLEAR=1]` 失败后
  `text_state().bytes == b"OLD"`,诊断 `rt.text.file`;
- `text_555_file_fills_let_string_array`(增加 `WORD("OLD")` + `CLEAR=1`,
  断言成功装载后 `_TEXT` 被清)/ `text_555_missing_file_yields_then_advances_to_next_command` 保持通过。
