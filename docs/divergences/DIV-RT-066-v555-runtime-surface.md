# DIV-RT-066 · v555 商业分支运行期面(TEXT[FILE]/MEM/PRODUCT/词法器)

- 状态:**fixed(no-manual,2026-11-20)**;`_STR` 读侧与 `PRODUCT` 未取证 COM
  仍为**过渡态**(非终态,见 §2 的 P0-1/COM 行)
- 影响面:`yu-rt`(handlers/vm/sysvar/scenario)、`yu-save`、`yu-engine`/
  desktop 生命周期
- 证据:`docs/formats/v555-commercial.md`、`docs/formats/v555-gap-2026-11-19.md`、
  `docs/formats/v555-sysvar-abi-measured.tsv`;处理体地址均以 `<v555-4>`
  为锚(商业 exe,静态 RE + 运行 trace)
- 原则:v555 的**同名命令参数表不同**一律按工程 `ysc.ybn` 的参数名分派,
  不用散落版本号;同名同参数、语义随版本变的(仅 `VARINFO[SJISCODE]`)
  走 `SemanticProfile` 单门控

## 1. 已实现面

| 能力 | 落点 | 语义 / 锚点 |
|---|---|---|
| `TEXT[FILE,LET,CLEAR]` | `handlers/text.rs::op_text_file` | 555 `TEXT` 同名不同义:文件 ASCII 小写 + `/`→`\`(不补扩展名) → 追加 CRLF → 按 CRLF/CR/LF 切行 → 每行加 `0x0D` 写 `LET` 字符串一维数组 → 末尾两空元素;`CLEAR` 清文本缓冲。处理体 `0x452a0c`;打不开返回 -1(内核 Yield + `rt.text.file`) |
| `SAVE[MEM=n]` | `handlers/file_io.rs::op_save`、`yu-save::SaveState::save_mem` | `MEM` 优先于 `FILE`;容器与文件存档同型(YSSD/0x1e0),写稀疏内存槽;`MEM=99999` 是独立打包分支,显式挂账。处理体 `0x44b874` |
| `LOAD[MEM=n]` | `op_load`、`SaveState::load_block_mem/load_image_mem` | 同槽读取;MEM 的 DNO 0 基(文件仍 1 基);块类型 0 写 `LET`、2 按 var_id 恢复、1 图像。处理体 `0x4447c8`;缺槽报 `sv.mem.missing` |
| `PRODUCT` | `handlers/product.rs::op_product` | `COM=29`→LET=0;`21/24` VFS 枚举→`_DROPFILE*`;`22` 词法器;`23` 最近 `TEXT[FILE]` 行数;`51/52/80/81/100/110` no-op。处理体 `0x459670` |
| `_DROPFILE`/`_DROPFILE_NUM` | `sysvar.rs`、`Vm::enumerate_drop_files` | `PRODUCT[COM=21/24]` 按已挂载 VFS(`src`→`aux`)枚举直接子文件;上限 0x2000;未枚举=空表 |
| v555 DSL 词法器 | `yu-rt/src/scenario.rs` | `PRODUCT[COM=22]`:`#`/`\`/`(`/`)`/`,`/`@`/`$` = 0x39/0x33..0x38,标识符/数字 = 0x20,`//` 注释丢弃;消费方 `es_scenario_parser.yst` 按 0x20/0x39 分类 |
| `VARINFO[SJISCODE]` | `handlers/variables.rs` | `SET` 首字符码:空串 0、单字节字节值、lead byte `(lead<<8)|trail`(`0x4554a9/0x455b9c`);**门控**:`SemanticProfile.varinfo_sjis`(495 及更早 `Inert`,555 起 `FirstChar`) |
| `VARINFO[STRFIRST]` | 同上 | `SET` 字符串一维数组 `[NO,NO2]` 闭区间字典序最小元素下标,ASCII 大小写不敏感(`0x455200`) |
| `VARINFO[NO2]` | 同上 | `STRFIRST` 的结束下标从属参数;单独出现按对齐 no-op(不 fatal) |
| 无窗口文本推进 | `vm.rs::text_reveal_complete` | 555 启动定义脚本在 `TEXT[ID]` 前跑 `WORD`+`RETURNCODE`:无文本窗口即视为完成(官方 `FUN_00404914` 同判),避免启动链永久停住 |
| 生命周期 | `Scheduler::live_tasks` / `Engine::live_tasks` / desktop | 555 主任务启动链结束自然走完后 title/scenario 子任务仍在;关闭/退出条件改为 `main_finished && live_tasks == 0` |

## 2. 显式未实现 / 可排查点(不猜值)

| 面 | 行为 | 诊断 |
|---|---|---|
| `_STR(宏帧,参数号)` | 不返回空串(否则宏参数全 0 的假象);读侧返回 Unimplemented | `rt.sysvar.unimplemented`,detail 明确写 P0-1 / v555-gap §P0-1 |
| `SAVE/LOAD[MEM=99999]` | 已实现:`SAVE` 打包 `FUN_0044d7a8` → 下一次 `SAVE[FILE]` 写 DNO=1024;`LOAD[FILE DNO=1024]` 读回。`LOAD[MEM=99999]` 官方无分支 | 见 DIV-RT-056/071/075 |
| `PRODUCT` 已取证未实现 COM | `26/33/34/36/37/38/10201/10205/21000/21031/0x2776..79` 逐 COM no-op + 挂账 | `report_unimplemented_variant("PRODUCT","COM=…")`,每 COM 一次 |
| `PRODUCT` 未列出的 COM | 官方处理体未知;no-op + Info 一次 | `rt.cmd.aligned_stub`,`PRODUCT.COM=…` |
| `PRODUCT[COM=22]` 位置/长度字段 | 文本/类型码已输出;位置/长度未对拍(调用方当前不用) | `rt.cmd.aligned_stub`,`PRODUCT.COM=22.FIELDS`,只报一次 |
| `PRODUCT[COM=22]` 输出数组未定位 | `PINT2/PINT3` 未回填且 ysv 无唯一 `STR[1024]+INT[1024]` 对:只返回 token 数,不猜数组 | `rt.cmd.unimplemented` 变体 `COM=22/vars`,每变体一次 |
| `TEXT[FILE]` 打不开 | 官方返回 -1,调用方回退;内核 Yield + 可恢复诊断 | `rt.text.file` |
| 图形 P0(`CGINFO`/按钮纹理/过渡/背景 CG) | 本轮不实现;既有 `gfx.*` 诊断保留 | 见 `docs/formats/v555-gap-2026-11-19.md` §3 P0-2..P0-4 |

## 3. 跨版本隔离

- `TEXT/SAVE/LOAD/PRODUCT` 的 555 形态按**参数名**分派:老工程 ysc 没有
  `FILE`(TEXT)/`MEM`(SAVE/LOAD)/`PRODUCT`,分支天然不可达。
- `VARINFO[SJISCODE]` 是唯一「同名同参数、语义随版本变」的项:
  `SemanticProfile` 由 `ysc.ybn` 头版本派生一次;0.495 保持
  `aligned-stub`,v555 计算首字符码。0.495 ERIS 语料 `SJISCODE` 使用 0 次,
  新增门控不影响老工程。
- 存档容器版本与脚本引擎版本**正交**:v555 容器仍为 `0x1E0`,复用
  `SaveProfile` 的现有行;`MEM` 只增加命令面,不改块布局。

## 4. 验证

```text
cargo test -p yu-rt -p yu-save --no-default-features
  → lib 128 / consistency 4 / corpus 11 / corpus_golden 4 / corpus_v2 3 /
    fixture 140 / golden 3 / yu-save 31,全部 ok
```

新增回归:
`fixture/control::gosub_keeps_pint_and_pstr_groups_apart`、
`fixture/variables::{varinfo_sjiscode_is_inert_on_495_projects,
varinfo_sjiscode_returns_first_char_code,
varinfo_strfirst_returns_smallest_index_case_insensitively}`、
`fixture/text::text_555_file_fills_let_string_array`、
`fixture/file_save::save_and_load_mem_use_memory_slots_not_files`、
`fixture/product::{product_com29_writes_mode_bit_zero_to_let,
product_com21_enumerates_vfs_directory_into_dropfiles}`、
`yu-save::state::tests::mem_save_roundtrips_container_without_disk`、
`vm::handlers::text::text_file_tests::*`、`scenario::tests::*`。

v555 实机(headless)结果与仍存缺口见 `PROGRESS.md` 的 v555 成果条;
`_STR`/图形 P0 未取证前不宣称「标题可见/能玩」。
