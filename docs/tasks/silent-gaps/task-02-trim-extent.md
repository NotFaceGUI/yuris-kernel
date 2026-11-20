# 任务 02 · TRIM 裁后尺寸回写

- 状态：取证完成（2026-11-17）；`SET` 缺失/越界已修，裁后矩形设计待确认
- 优先级：P2
- 关联：`DIV-GFX-063`、`DIV-GFX-066`、
  `docs/tasks/silent-state-audit-2026-11-17.md` §2.2
- 官方锚点：手册 `cg/cgact.html` 例 22（`TRIM=1 SET=15`，8=上/4=下/
  2=左/1=右）；官方执行体地址尚未定位，本任务先取证。

## 目标

`CGACT[TRIM]` 执行后，图层矩形 `+0x78/+0x7c` 与 `CGINFO[SX/SY]` 必须反映
裁后尺寸；所有透明边缘裁除的边界、全透明行为与官方一致。

## 现状

- soft `imageop::apply_trim` 已按位掩码裁纹理：
  只裁 alpha 全 0 的行/列，全透明时保守保持原样。
- `GfxState::cgact_image` 只在 `SIZE` 族与 `RESIZE` 更新
  `LayerKind::Image.size`；`TRIM` 没有回写矩形。
- `ImageAction::changes_extent()` 已存在（`RESIZE || TRIM`）但没有消费者。
- 当前 `.yst` 源语料未见 `TRIM` 调用；动态脚本/商业样本尚未普查。
- 与 RESIZE 不同：TRIM 的新尺寸依赖**像素 alpha 包围盒**，gfx 在命令
  时刻没有像素（资产解码、前序 `RECTPAINT/COPY/SIZE` 都在后端回放）。

## 先取证（不改代码）

1. 在 0.486/0.495 官方二进制定位 CGACT `TRIM` 分派与执行体：
   - 确认 `SET` 位映射是否与手册一致；
   - 确认裁后是否写 `+0x78/+0x7c`，写的是 `w-left-right` 还是含端点差；
   - 确认全透明（四边都裁空）时官方返回/尺寸/像素；
   - 确认 `TRIM` 前是否要求 `+0x86`（alpha 有效）或 layer kind。
2. 若 REA 有歧义，用官方引擎微场景（带透明边框的 1 张 PNG）逐像素/
   `CGINFO` 对拍；临时工程与截图用完删除。
3. 查当前游戏/ERIS 语料是否实际调用 `TRIM`（含打包 `data.ypf` 与动态宏），
   给出调用点/次数；0 次也要记录证据。

## 取证结果（2026-11-17）

### 官方执行体已定位

- **0.495 `FUN_0043c539`（0x43c539–0x43c8c7，911 B）**，CGACT 处理体在
  0x42e52e 调用；**0.479 `FUN_00436f69`（0x436f69–0x4372f1，905 B）**
  结构完全相同（仅层结构偏移不同：0.479 `+0x70/+0x74`，0.495
  `+0x78/+0x7c`）。行为跨版本一致。
- 参数：源层（EAX）、目标层（EDX）、原地/两层标志（CL）、掩码字节
  （栈参数）。
- **掩码**：`8=上/4=下/2=左/1=右`（0x43c684–0x43c6b7 连续
  `test al,8/4/2/1`，与手册例 22 一致）。
- **算法**：扫描源表面**所有像素**的 alpha 字节（`pixel[3] != 0`）求
  global bbox（min/max x/y）；未启用的边强制为整边；然后
  `newW = maxX-minX+1`、`newH = maxY-minY+1`（含端点）。
- **回写**：新表面 malloc → `FUN_0043c8d4` 复制 bbox → 释放旧表面 →
  `[目标+0x78/+0x7c] = newW/newH`，并把源 `[+0x86]` 复制给目标。
  原地且新尺寸==当前矩形时直接复用旧表面，不重分配。
- **全透明**：`SET=15` 时 bbox 全 -1 ⇒ `newW=newH=0`，官方分配 8 字节
  最小表面并把矩形写 0×0（**不是保持原样**）。`SET=0` 四边强制整边
  ⇒ 保持原尺寸（原地 no-op）。
- **`SET` 缺失**：0x42c998..0x42c9b6 显示缺失槽的值初始化为 0 ⇒
  掩码 0 = 四边都不裁 = 原地 no-op；**不是错误/未实现**。
- **`SET > 0xF`**：调用点 0x42e50b `cmp eax,0xf; ja` ⇒ 0x42f12d
  错误上报并返回 -1。
- **区域 `X/Y/X2/Y2`**：执行体没有区域参数；手册例 22 也没有。
  处理体是否拒绝区域尚未逐行取证，当前内核保守挂
  `gfx.imageop.region.unsupported`，不在本次改动范围。
- **`ID2`**：执行体有双层路径（EDX + CL），但手册例 22 未文档化；
  当前语料无 ID2 TRIM。调用点参数映射待补证，暂维持
  `gfx.imageop.id2.unsupported`。

### 语料普查（`census_params <root> CGACT:TRIM`）

| 工程 | 脚本数 | TRIM 组 | 位置/参数 |
|---|---:|---:|---|
| `project-eris`（0.495 官方编译） | 141 | 0 | — |
| <game-b>（v494） | 160 | 1 | s27 `data/script/eris/macro.yst` pc5803 line 8268，仅 `ID + TRIM`，**无 SET** |
| 0.486 样本 <game-d>_C4 | 260 | 0 | — |

⇒ 当前全部语料中唯一的 TRIM 就是「无 SET」；官方掩码 0 = no-op。
本次已按证据修复该路径（gfx 不再报 `gfx.imageop.set.missing`；
soft/wgpu 缺省按 0 执行），并补了 `SET>0xF` 的拒绝回归。

### 仍未实现（需设计确认）

非零 `SET` 的「裁后回写 `+0x78/+0x7c`」：gfx 命令时刻没有像素，且
TRIM 可能跟在 `BLUR`/`COPY` 等后端回放项之后（<game-b> 宏就是
BLUR→TRIM→BLUR5），从资产直接取 alpha 会看到**加工前**的边界，
不能作为终态。需要同步 CPU 表面回放或后端回报 + 同 tick 时序取舍，
见下方设计方向。

## 设计方向（取证后再选）

- **A. 同步像素 provider**：给 `GfxState` 注入只读表面/像素采样接口，
  `cgact_image` 在命令时刻计算 alpha 包围盒并回写矩形。最贴近官方
  “命令处理体内立即执行”，但要避免每帧大图拷贝，TRIM 低频可接受。
- **B. 后端回报**：`ImageOp` 增加 result 通道，渲染后把裁后尺寸回写；
  但同一 tick 内脚本随后的 `CGINFO` 仍会读到旧值，不能直接满足官方
  同为命令时刻的语义；若采用，必须在任务书/台账明确这一时序差异。
- **C. 两阶段提交**：把 TRIM 的像素与矩形都推迟到可计算阶段，并让
  `CGINFO` 在结果未就绪时返回明确诊断，而不是静默旧值；这仍是过渡态，
  不能当终态。
- 禁止用“常见图片边框”猜公式；必须先有官方证据。

## 待办（非零 SET，2026-11-17 登记）

> 触发条件：出现**真实使用 `TRIM=1 SET≠0`** 的样本/脚本，或主人明确要求
> 提前实现。当前三个工程普查均为 0 次非零 SET。

- [ ] **P1 · 同步 CPU 表面回放**：在 engine/host 侧按命令序重放该层的
  pending ops（至少 `BLUR*`/`RECTPAINT*`/`COPY/COPY2`/`SIZE*`/
  `RESIZE`/`TRANSITION*`），得到与后端一致的命令时刻表面；扫描 alpha
  bbox 后写回 `LayerKind::Image.size`。需先盘点 `TextureStore`/快照/
  离屏组的 CPU 可解析范围，不能假设所有源都可读。
- [ ] **P1 · 全透明语义**：0.495 `SET=15` 全透明 → `newW=newH=0`、
  8 字节最小表面、矩形 0×0；当前 soft `apply_trim` 与 wgpu
  `trim_bounds` 都保持原样，与 REA 不符。需与矩形回写同批修。
- [ ] **P1 · 链式 TRIM**：<game-b> `macro.yst` s27 pc5803 是
  `BLUR → TRIM(无 SET) → BLUR5`；非零 SET 版本必须验证 TRIM 看到的是
  **BLUR 后**的像素，不能用资产原图 bbox。
- [ ] **P2 · partial mask + 全透明**：官方未 clamp，enabled 边的 -1
  会推出 `w+1/h+1` 等越界尺寸；决定“照抄公式”还是显式拒绝，二者都
  必须写清证据与取舍，不得静默。
- [ ] **P2 · ID2 / 区域参数**：执行体有双层路径（EDX+CL），手册例 22
  无 ID2；处理体对 `X/Y/X2/Y2` 是否拒绝尚未逐行取证。补调用点映射后
  再决定实现/维持 Unimplemented。
- [ ] **P2 · 回归**：256×256 四边 10px → 236×236；单边 mask；
  `SET=15` 全透明 → 0×0；TRIM 后 `CGINFO`/`ONMOUSE` 同 tick 一致；
  BLUR→TRIM 链。
- [ ] **P3 · 台账**：更新 `DIV-GFX-063`/`DIV-GFX-066` 与 `PROGRESS.md`。

## 必做回归

1. 256×256 图，四边各 10px 全透明，`TRIM=1 SET=15` →
   `CGINFO SX/SY = 236×236`（或 REA 确认的端点口径）。
2. 只裁上/左（`SET=8|2`）→ 对应边尺寸正确，其他边不变。
3. 全透明图：按官方行为（0 尺寸/报错/保持）断言；不得沿用“保守 no-op”
   除非官方证据支持。
4. `TRIM` 后 `CGINFO[ONMOUSE]` 命中范围与新矩形一致。
5. 现有 `all_layer_filter_actions_are_executable` 与 soft/wgpu 图像加工
   测试全绿；`cargo build --workspace --all-targets`。

## 验收

- 官方语义有证据锚点；实现后 `CGINFO`/命中/绘制尺寸三者一致。
- 更新 `DIV-GFX-063`、`DIV-GFX-066`、`PROGRESS.md`；若时序无法同 tick
  对齐，必须在台账写清限制并保留 open/过渡态，不得静默。
- 若语料 0 调用且无官方证据，可只登记证据与后续方案，不强行改代码。

## 提交建议

```text
fix(gfx): TRIM 裁后回写图层矩形,统一 CGINFO 与命中尺寸
```
