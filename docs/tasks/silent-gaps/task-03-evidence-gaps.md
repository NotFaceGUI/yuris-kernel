# 任务 03 · 小证据缺口收口

- 状态：open
- 优先级：P3（逐项做，不要一次全改）
- 关联：`docs/tasks/silent-state-audit-2026-11-17.md` §2.4/2.5
- 原则：每项独立取证、独立结论；证据不足就保留 open，**不要为了收口编语义**。

## 3.1 目录形 `CG[FILE=<…/>]` 装载失败的矩形

现状：目录形键在 `apply_cg_inner` 退化成 `SolidBlack1x1` 透明空白面
（DIV-GFX-084）；若脚本没有再给 `SX/SY`，当前实现保留旧矩形。
官方 `FUN_0042b144` 非空分支 → `FUN_0040c7c5`；解码失败路径返回 -1，
但 `+0x78/+0x7c` 在失败前后是否保持 0/旧值尚未逐分支确认。

要做：
1. REA 0.486/0.495 的 `FUN_0040c7c5` 失败返回路径（`FUN_0040c9b0`
   返回 -1、各解码器失败、`FUN_004648b4` 日志分支），确认层矩形写入点。
2. 或官方微场景：对一个不存在的目录形/非法文件发 `CG[FILE=...]`，
   查 `CGINFO[SX/SY]` 与 `+0x78/+0x7c`。
3. 按证据改 `apply_cg_inner` 的失败分支，或保持并写明“官方也未写”的
   结论；加回归。

验收：目录形失败后 `CGINFO SX/SY` 与官方一致；DIV-GFX-084 补记。

## 3.2 `TRIM` 全透明

现状：`apply_trim` 全透明时直接保持原图（保守 no-op）。
官方在四边都裁空时可能得到 0×0、报错或保持；任务 02 的取证顺带确认。
若任务 02 暂未做，本项只登记，不单独改。

验收：官方结论 + 测试（或明确 open 原因）；不得把“保守 no-op”写成
`fixed`。

## 3.3 `SIZE=1` 单轴

现状：`apply_layer_params` 对图像层只给 `SX` 或只给 `SY` 时，另一轴取
当前尺寸；引擎 `FUN_0043772c` 对单轴是否保留 `+0x7c/+0x78` 尚未逐分支
确认。源语料 0 次单轴用法。

要做：
1. REA `FUN_0043772c` 参数读取/尺寸写入，确认缺省轴。
2. 若官方缺省轴 = 0/报错/保持，按证据改；无证据保留并记录。
3. 加最小测试。

验收：单轴行为有证据；`CGINFO`/绘制一致；无猜测公式。

## 3.4 资产换图同帧尺寸（验证 2026-11-17 修复）

现状：`apply_cg_inner` 换图分支对 `CG[FILE=asset]` 已存在层会把
`Image.size` 置为新资产尺寸（`asset_sizes` 已知时）或 `None`（由后续
资产头回退）。需要验证：
- 同 tick 内 `CG[FILE=new]` 后立刻 `CGINFO[SX/SY]`，官方返回新图尺寸；
- 内核在资产头未注入时返回 `Unimplemented`/等待回填，而不是旧尺寸；
- 资产头已注入时返回新尺寸。

要做：
1. 用 `note_asset_size`/测试夹具分别模拟“已注入/未注入”两种顺序。
2. 若未注入时脚本同 tick 查询会拿到错误值/诊断，评估是否需要在
   `CG[FILE]` 命令时同步解析资产头（可能与现有预加载架构冲突），
   给出方案；无方案则记录为过渡限制，不静默。
3. 加回归 `cg_file_asset_replacement_updates_size`（名称可调整）。

验收：两种顺序都有测试；`CGINFO` 不再静默返回旧矩形；DIV-GFX-060/085
补记。

## 取证进展（2026-11-17）

### 3.4 资产换图同帧尺寸 —— 已验（测试）

- 已注入资产头：`CG[FILE=new]` 后 `Image.size=新尺寸`，`CGINFO SX/SY`
  直接返回新尺寸。
- 未注入资产头：`Image.size=None`，`CGINFO[SX/SY]` 返回
  `gfx.info.asset_size`（不返回旧图矩形）；`set_asset_size` 后返回新尺寸，
  与 VM 的 `INFO_ASSET_SIZE → MFILE → note_asset_size → 重试` 链一致。
- 回归：`cg_file_asset_replacement_updates_size`、
  `cg_file_asset_replacement_before_header_injection_is_not_old_size`
  （`cargo test -p yu-gfx --test parity_fixes cg_file_asset` → 2 passed）。
- 台账：DIV-GFX-060 §7 补记。

### 3.1 目录形 `CG[FILE]` —— 部分取证

- 0.495 空串分支已复核：`0x40c5f0` 函数把层 `+0x86=0`、
  `+0x78=+0x7c=1`，分配 1×1 表面并清零四字节（与 DIV-GFX-060 一致）。
- 非空/非法文件的装载器不是直接的 `call`，而是经函数指针/加载器表进入；
  失败路径（`FUN_0040c9b0`/解码器返回 -1）与 `+0x78/+0x7c` 是否写值
  尚未逐分支定位。下一步：找加载器表调用点，或做官方微场景
  （非法 `FILE` + `CGINFO SX/SY`）。**仍 open**。

### 3.3 `SIZE=1` 单轴 —— 锚点需修正

- `FUN_0043772c` 不是 `CG[SX/SY]` 的参数应用体，而是 **CGACT `SIZE/SIZE2`
  的重采样执行体**：入口把 `in_stack_0000001c/0x20`（目标宽/高）**除以
  100**，再按目标尺寸 malloc/写 `+0x78/+0x7c`（0x43772c–0x437c17）。
- 单轴缺省发生在 CGACT 处理体对 SX/SY（及 FSX/FSY）槽的读取/默认，
  0x42d0xx–0x42d293 一带；尚未逐行确认缺省轴是 0、当前值还是报错。
- 结论：**仍 open**，但任务书里的 `FUN_0043772c` 锚点应改为“SIZE 像素
  执行体”，单轴默认需另找处理体锚点。

### 3.2 `TRIM` 全透明 —— 证据已由任务 02 给出

- 0.495 `FUN_0043c539`：`SET=15` 全透明时 bbox 全 -1 ⇒ `newW=newH=0`，
  分配 8 字节最小表面、矩形写 0×0；当前 soft/wgpu 保守保持原样，
  实现跟随任务 02 的“非零 SET 矩形回写”待办。**不得标 fixed**。

## 全组验证

```bash
cargo test -p yu-gfx --test parity_fixes
cargo test -p yu-gfx
cargo test -p yu-backend-soft
cargo build --workspace --all-targets
```

## 结论格式

每项完成后在 `docs/tasks/silent-state-audit-2026-11-17.md` 和对应
`DIV-GFX-*` 台账写：
- 证据锚点（REA 地址/官方截图/数据字节）；
- 结论：`fixed` / `fixed(no-manual)` / `aligned-stub` /
  `unsupported-by-official` / 继续 `open`；
- 测试名与结果。

## 提交建议

按项拆提交，例如：

```text
fix(gfx): 目录形 CG[FILE] 失败时按官方矩形语义处理
fix(gfx): SIZE 单轴缺省按官方执行体回写
test(gfx): 资产换图同帧尺寸查询覆盖注入前/后
```
