# TODO / 待办总账(2026-10-22 审查后更新)

> 本文件是**扫描结果**,不是新的进度真相。真相仍在 `PROGRESS.md` 与各台账。
> 扫描对象:`docs/gfx-unimplemented-todo.md`、`crates/yu-rt/docs/coverage-matrix.md`、
> `docs/divergences/*.md`、`crates/*/docs/divergences/README.md`、
> `docs/backend-soft-review.md`、代码内 `TODO(DIV-*)` 标记、`PROGRESS.md` 成果
> J1/L1/M1/M2/J2/J3/J4/J5/J6。
>
> **2026-10-22 改动审查**:新增 `docs/audit-2026-10-22.md`(4 项确定错误、
> 4 项文档失真、3 项工程回归)。本次审查的 P0 项已写入 §2.0;`backend-soft-review.md`
> 的 P1/P2 计数已过期(多数已由 J2/J3/J4 关闭),不再作为待办来源。

## 0. 口径

- **未取证** = 三层证据(手册 / 真实脚本 / REA)缺层 ⇒ 按 A5 **禁止实现**;
- **未实现** = 证据已够但代码未做,或卡在契约裁决;
- **部分** = 主路径已实现,边缘路径/字段仍缺;
- **非缺口** = 域事件(归宿主)、声明类命令、已裁决/已撤回项。

## 1. 总量

| 来源 | 条目 | 分布 |
|---|---:|---|
| `docs/gfx-unimplemented-todo.md` | 50 | fixed 11(001/006/007/017/019/029/043/047/049/050 + 013 子项) / unimplemented 19 / open 15 / partial 2 / note 2 / blocked 3 / decision 1 / withdrawn 1 |
| `coverage-matrix.md`(命令) | 119 | 已实现 35 / 域事件 34 / 未取证 27 / 未实现 7 / 部分 4 / 声明类 12 |
| `docs/divergences/`(跨 crate) | 12 + 2 索引 | fixed 5(含 1 带保留)/ partial 1 / explained 2 / **open 3** / withdrawn 1 |
| `crates/yu-rt/docs/divergences/` | 42(DIV-RT-001..042) | 状态行格式不统一,无法机读(见 §6) |
| `crates/yu-gfx/docs/divergences/` | 42(DIV-GFX 镜像) | 与 gfx TODO 同表 |
| `docs/backend-soft-review.md` | 2 P1 + 9 P2 | P1-1 已修;P1-2 未修;P2-6 已随数据入口清理修复;其余待复核/未修 |
| 代码 `TODO(DIV-*)` | 21 处 | 全部对应 gfx TODO 行 |

## 2. 可立即修(证据已够,或已知 bug)

### 2.0 2026-10-22 审查新增(P0)

1. **A1 首帧 `COPY ID=:/W0/` 失去源**(实测 `first_frame`:
   `skip copy gfx.copy.source.offscreen_missing :/W0/CGS100`)。
   主组直绘(DIV-GFX-008)后主组没有离屏 pass,而 COPY 仍按 Offscreen 找源。
   需在“回退本帧子层自合成”与“显式 `gfx.copy.source.no_backbuffer` + 挂账”
   之间裁决。见 `audit-2026-10-22.md` §A1。
2. **A3 `Engine::restart` 不清渲染器状态**:`last_screen`/`backbuffer`/
   `snapshots` 跨重启保留;`GfxState.copy_seq` 归零后新 `CopyId` 与旧快照
   冲突 ⇒ 冻结早退命中旧像素。需 `reset()` 或 `CopyId` 代际隔离。
3. **A2 `image_probe_order` 空值仍内置兜底**(默认 `[png,bmp,jpg,gif,psb]`
   + 一条 Info),违反 A5“缺表 = 拒绝启动/Unimplemented”。DIV-GFX-049
   标 `fixed` 属结论高于实现,应改回 `open` 或按 A5 实装。
4. **C1 `cases/*.json` 全部过期**:`case first-screen` 实测 17 ops vs 期望 18
   ⇒ FAIL。需 `case --all --update` 并把 `case --all` 列为门禁。

### 2.1 既有条目

1. **后端 P1-2:MODE 2 的 DIFFUSE RGB** — **J4 已修**:顶点色=层 A 灰度
   (asm `0x414d09..0x414d21`),MODE 2/3/5/6/7/9 先按 `T·A` 归约再 blend。
2. **后端 P2-2:零尺寸 Offscreen pass** 未累加 `ops_total`,破坏
   `ops_total == drawn + skipped` 不变量(当前代码有断言,需复核触发路径)。
3. **后端 P2-3/P2-4:Group transition 静默忽略 / Transition 一律跳过** —
   **J2 已修**:两端执行 `Op::Transition`;配对缺失回退普通层。
4. **后端 P2-5:`first_frame` 未注入 HostPaths**、丢弃 `tick_at` report、
   名字误导为“官方首帧” — 注入路径、检查 errors、改名/注明。
5. **后端 P2-8:后端未消费 `fixed_state()`/`blend_table_json()`** —
   用状态表驱动/交叉检查 blend,避免第二套手写公式。
6. **后端 P2-9:`image_probe_order` 只在构造时生效** — frame 内变化不刷新。
7. **J-10:COPY/COPY2 已按 EVID-T-11/12/15 实现**(区域/时序/生存期/交错序);
   软/硬后端执行,`j2-micro-copy2` 三方向量一致。`TEXCOPY/TEXSSCOPY` 仍 open。
8. **J-08:主组直绘/子组 RT alpha 行为已 Confirmed**,卡契约裁决(DIV-GFX-008/041);
   title 稳定帧不依赖该差异(J2 报告)。
9. **按钮状态后缀**:**J3 已修**。`yu-rt` 实现 `FILEINFO[EXIST]→LET`
   (EVID-J3-01),title `asset.missing=0`;DIV-GFX-043 fixed。
10. **`first_frame` 同名 example 互相覆盖**(soft/wgpu 都叫 `first_frame`) —
    改 example 名或分 target dir,J1 已实测到该坑(J2 验收用 `touch` 规避)。

## 3. 需先取证/裁决(禁止直接实现)

### 3.1 契约裁决(证据已够,等人拍板)

- **主组直绘 / 子组 RT alpha**(DIV-GFX-008/041):修正会改变所有半透明层;
  J2 用 title 平坦区域对拍确认 title 不依赖该差异,保持挂账。
- **COPY 时序**:EVID-J-11/12/13 + J2 的 EVID-T-11/12/15 已闭合;
  `CopyTiming::CommandCurrent`、快照 `CopyId` 冻结、命令交错序已实现。
- **COPY 属性是否含 Z**(DIV-GFX-017):REA 属性块未见 Z,但调用点未定位;
  当前保持复制 Z(Likely),DIV-GFX-017 记 fixed(重新锚定后决策保持)。

### 3.2 未取证(缺手册页或 REA 语义)

- gfx:MODE 3/5/6/7/9(001)、着色器 4/8/10..18(002)、FX 仿射族(003)、
  多部件(004)、MID(005)、过渡推进(006)、附加槽位(011)、顶点 DIFFUSE(019)、
  资源图尺寸(024)、INFO 多字段(025/026/038)、CGACT 多图层(031)、
  FONT 字段(035/036)、RASTER/FSX(039)、半像素(042)、IDNO/TIDNO(027/030)、
  SWAP(032)、窗口 host(037)。
- rt(静态频度高者):`F_INT` 337、`F_FLT` 72、`F_STR` 64、`DEBUGLIST` 5、
  `YMVINFO` 4、`YMV` 3、`YMVEND` 2、`ERROR` 1、`MATH3D` 1;手册有页但语义未取:
  `DIALOG` 17、`TRAY` 2、`CLIPACT` 1、`CLIPINFO`/`LABEL`/`MACRO`/`REGINFO` 0。
- rt 部分:INFO 类(1598/129/3 次)是“未安装 GraphicsHost”的普查口径,装了宿主
  即可;`LET` 剩 2 次 = `@_SAVE_PATH` 宿主注入 + `_TEXT_FLAG` 写入。
- format:`DIV-002` NUL-XOR 方言(无真样本)、`DIV-007/009` YSTB v2
  (仅当要支持老游戏)、`DIV-003` GBK 码表仍未落地。
- trace:`DIV-L-002`(script 60 返回后的任务相位/调度)open;官方 L1 `args=[]`
  参数级不可观测,当前只到命令顺序级。

## 4. 非缺口(不要修)

- 34 个域事件命令(CG/WINDOW/SOUND/SAVE/FILE/MENU/EMOTE/MOVIE/FLASH/INPUT…):
  VM 只发事件,语义归宿主/子会话。
- 12 个声明类(G_INT/G_STR/…):不按命令执行,初值走 `VARACT[INIT]`。
- DIV-GFX-014(同 Z 顺序,decision)、DIV-H-001(withdrawn)、
  DIV-006/008(explained)。

## 5. 建议修复顺序

| 顺序 | 动作 | 阻塞关系 |
|---|---|---|
| P0 | **A1 主组 COPY 无后缓冲源**:回退本帧子层自合成,或显式 `no_backbuffer` 挂账 | 首帧 golden/case 全依赖 |
| P0 | **A3 渲染器 `reset()` / `CopyId` 代际隔离** | 重启后像素错 |
| P0 | 解 boot/golden 回归:给 gfx/后端示例与测试注入 `debug_mode=true`(语料是 debug 构建,DIV-L-001 证据),重跑 census;不回再二分 `1365918` | 不解决则所有像素验收失真 |
| P1 | **C1 `case --all --update` + 把 `case --all` 写进门禁** | 当前用例等于无门禁 |
| P1 | **A2 `image_probe_order` 按 A5 拒绝启动/全层 Unimplemented** | 违反 A5 原文 |
| P1 | 后端 P1-2 / P2-2/3/4/5/8/9 + example 重名 | 影响报告不变量与可诊断性 |
| P1 | 组 RT 子组 alpha(DIV-GFX-041,契约裁决后) | 依赖裁决 |
| P2 | 文字桥:DIV-GFX-012/033/034/035/040(`yu-font` 已就绪) | 依赖 M2 的 TEXT 状态 |
| P2 | 资产元数据 provider(DIV-GFX-024)→ CGINFO SX/SY | 已部分由 J3 落地(图像头),完整 provider 仍缺 |
| P2 | **A4 `REVERSE`+`RECTPAINT` 组合补两端测试或删分支** | 未对拍 |
| P2 | **C2 `eprintln!` 移出 `src/`;C3 示例注入 HostPaths** | 纪律/环境等价 |
| P3 | 未取证命令取证 campaign(按静态频度) | 每个都要三层证据 |
| P3 | YSTB v2 / NUL-XOR 方言(仅按目标游戏需要) | 需真样本 |
| P3 | DIV-L-002 任务相位对齐 | 非渲染 |

## 6. 维护问题(扫描本身暴露的)

1. **状态格式三套**:gfx TODO 用英文(unimplemented/open/partial),
   coverage matrix 用 emoji(✅/🔁/⛔/➖),rt divergences 多数没有“状态:”行
   (脚本解析 42 条全为 `?`)。建议统一头部模板,并在本文件生成索引。
2. **工作区 49 个未提交改动**横跨 J/M/L/P/K2 五条线;gfx 回归未定前,
   golden 相关文件不要与 rt 改动同 commit。
3. 本文件是快照;条目状态以各自台账/PROGRESS 为准,扫描脚本重跑后更新。
4. **2026-10-22 审查发现的文档失真**(详见 `audit-2026-10-22.md` §2):
   - `dx_mode` 在 README/PROGRESS/shader-parity 里被写成“引擎双路径”,
     实际是内核扩展(引擎 effect 始终创建,触发条件是配对/TA);
   - README 模块表仍说 MODE 3/5/6/7/9 “后端未支持”,实际两端已支持;
   - `render-parity §12.6` 仍说“组目标复制 Unimplemented”,实际已实现;
   - `backend-soft-review.md` 的 P1/P2 结论多数已修但文件无状态头;
   - `title-motion-ledger §9.6` 的 blocked 结论已被 §9.7 推翻,两节并列。
5. **代码 `eprintln!` 仍在 `src/`**(wgpu `passes.rs`/`lib.rs` 的
   `YURIS_DBG_SIZEMISS` 分支与统计输出),违反“`src/` 零 stdout”纪律。

## 7. 复现扫描

```bash
# gfx TODO 表
grep -c '^| DIV-GFX-' docs/gfx-unimplemented-todo.md
# 命令矩阵状态
grep '^| `' crates/yu-rt/docs/coverage-matrix.md | awk -F'|' '{print $6}' | sort | uniq -c
# DIV 文件状态
grep -h '状态' docs/divergences/DIV-*.md
# 代码标记
grep -rho 'TODO(DIV-[A-Z]*-[0-9]*' crates --include='*.rs' | sort | uniq -c
```
