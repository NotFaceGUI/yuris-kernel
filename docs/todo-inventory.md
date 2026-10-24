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
>
> **2026-10-24 全项目审查对账(AU5)**:本快照整体落后于 P1–P8/AU4/N/P4/P5
> 落地后的现状;本轮审查事实源 = `docs/audit-2026-10-24.md`。本次已就地勘误:
> §3.2 GBK 码表、§5 行8/9/11/12;rt 侧勘误见
> `crates/yu-rt/docs/coverage-matrix.md`(FILEINFO/FONTINFO/WINDOWINFO/LET 四行)
> 与 `docs/symptom-ledger.md`(S2/S3d/S4/S7 销账)。§1 计数与 §4/§5 未逐行
> 重写,以各 crate 台账为准。
>
> **2026-10-24 B1(存档闭环)**:DIV-RT-042 fixed——`ArgValue.reference` +
> `SaveHost` 同步契约;SAVE 预约/刷盘、LOAD 写回落地(`save-load-loop`
> 用例);`sv.write.refused` 门禁语义收窄为「只读根拒绝」;既读色
> (`seld.sd`)确认 es 脚本自 LOAD,零内核新机制。详见 DIV-RT-042 条目
> 2026-10-24 收口段。

## 0. 口径

- **未取证** = 三层证据(手册 / 真实脚本 / REA)缺层 ⇒ 按 A5 **禁止实现**;
- **未实现** = 证据已够但代码未做,或卡在契约裁决;
- **部分** = 主路径已实现,边缘路径/字段仍缺;
- **非缺口** = 域事件(归宿主)、声明类命令、已裁决/已撤回项。

## 1. 总量

| 来源 | 条目 | 分布 |
|---|---:|---|
| `docs/gfx-unimplemented-todo.md` | 50 | fixed 11(001/006/007/017/019/029/043/047/049/050 + 013 子项) / unimplemented 19 / open 15 / partial 2 / note 2 / blocked 3 / decision 1 / withdrawn 1 |
| `coverage-matrix.md`(命令) | 119 | 已实现 35 / 域事件 31(另 3 条声音命令由 `yu-sound` 接线) / 未取证 27 / 未实现 7 / 部分 4 / 声明类 12 |
| `docs/divergences/`(跨 crate) | 12 + 2 索引 | fixed 5(含 1 带保留)/ partial 1 / explained 2 / **open 3** / withdrawn 1 |
| `crates/yu-rt/docs/divergences/` | 42(DIV-RT-001..042) | 状态行格式不统一,无法机读(见 §6) |
| `crates/yu-gfx/docs/divergences/` | 42(DIV-GFX 镜像) | 与 gfx TODO 同表 |
| `docs/backend-soft-review.md` | 2 P1 + 9 P2 | P1-1 已修;P1-2 未修;P2-6 已随数据入口清理修复;其余待复核/未修 |
| 代码 `TODO(DIV-*)` | 21 处 | 全部对应 gfx TODO 行 |

## 2. 可立即修(证据已够,或已知 bug)

### 2.0 2026-10-22 审查项(AU1 已修)

1. **A1 首帧 `COPY ID=:/W0/` 失去源**(`DIV-GFX-051`)**→ fixed(AU1)**:
   首帧无后缓冲时用 `frame.clear` 合成后缓冲初值(设备初始化清屏色)。
2. **A3 `Engine::restart` 不清渲染器状态**(`DIV-HOST-001`)**→ fixed(AU1)**:
   两后端加 `reset()`(清 `last_screen`/`backbuffer`/`snapshots`/`image_cache`/
   `uploaded_overrides`),`Engine::restart` 调用;
   `crates/yu-cli/tests/restart.rs` 回归(先跑到 title 再重启,同帧 SHA 必须一致)。
3. **A2 `image_probe_order` 空值内置兜底**(`DIV-GFX-049`)**→ fixed(AU1)**:
   改为拒绝构造,宿主显式注入才允许。
4. **A4 `REVERSE`/`HORIZONTAL` 组合分支**(`DIV-GFX-052`)**→ fixed(AU1)**。
5. **C1 `cases/*.json` 过期** **→ fixed(AU1)**:`case --all --update` 重生成
   (85 ops / 0 skipped),三条用例全绿。
6. **C2 `src/` 的 `eprintln!`** **→ 已清**(AU1 复核时已无命中)。
7. **C3 示例/CLI 未注入 HostPaths** **→ fixed(AU1)**:7 个示例 + `yu-cli`
   的 `boot()` 注入 `$_SYSTEM_PATH/$_SAVE_PATH/$_APPDATA_PATH/$_PERSONAL_PATH`
   (`$_PROJECT_PATH` 手册无页,保持 `None`)。
8. **新增发现并修复**:
   - `DIV-GFX-053`:复制源层同帧被 `CGEND` 删除 → 命令时刻固定源纹理;
   - `DIV-GFX-054`:COPY 区域缺省坐标应为整幅(不是 0)。

### 2.0a 2026-10-22 输入收口(AU4 已修)

1. **`CGINFO[ONMOUSE*]` 未实现**,导致 `es.BT.MOUSECHECK` 恒不命中、
   标题/配置/确认弹窗全部点不动 → **fixed(AU4)**:`yu-gfx` 加
   `TextureSampler`;按 REA `0x43de01/0x43dec8/0x43df7f` 实现
   `ONMOUSE`(RGB)/`ONMOUSE2`(alpha)/`ONMOUSE3`(alpha+RGB);快照层按
   `snapshot_files` 源资产采样。组/离屏仍 Unimplemented(`DIV-GFX-055`)。
2. **`INPUT`/`MOUSE` 只发 DomainCall** → **fixed(AU4)**:内核按
   `0x446564`/`0x44aaa0` 应用源开关与光标设置;`FUN_0041a4b8` 的整源冻结
   语义已实现;脚本写回按 `FUN_00447014` 钳制。
3. **文本推进硬编码 `KEY_ENTER/KEY_CTRL`** → **fixed(AU4)**:改读
   `TEXT[CLICKBUTTON1..4]`/`SKIPBUTTON1..4]` 的按钮码(`FUN_004046fc`)。
4. **`_KEYLAST/_MOUSELAST` 读取挂号** → **fixed(AU4)**:REA
   `FUN_00427350` case 0x91/0x92 + `FUN_0045a3b4`(DIV-RT-033)。
5. **桌面宿主缺滚轮/右中键/光标模式/关闭路由** → **fixed(AU4)**:
   `MouseWheel→MOUSE_U/D` 脉冲;`MOUSE[MODE/TIME/MOVE/CLICK/WHEEL/CURSOR]`
   映射到窗口光标;`CloseRequested` 推 `@_WINDOW_EXEC_CLOSE=1`(二次关闭
   宿主兜底强制退出)。
6. **平台键表在桌面/Web 各写一份** → **fixed(AU4)**:新增
   `yu-platform-input`,键位/光标策略唯一;Web 只送原始 `key/pointer/wheel`
   事件,由 native 映射。
7. **极短点击被时间线折叠** → **fixed(AU4)**:`Engine` 记录按下将落在哪个
   帧;若抬起早于该帧,延后一帧(`deferred_release`)。
8. **回归**:`cases/title-config-back.json`(标题→設定→戻る→标题);
   `crates/yu-cli/tests/input_settings.rs`(启动设置);
   `yu-platform-input` 3 个映射单测。

### 2.1 既有条目

1. **后端 P1-2:MODE 2 的 DIFFUSE RGB** — **J4 已修**:顶点色=层 A 灰度
   (asm `0x414d09..0x414d21`),MODE 2/3/5/6/7/9 先按 `T·A` 归约再 blend。
2. **后端 P2-2:零尺寸 Offscreen pass** 未累加 `ops_total` — **已复核关闭
   (AU2)**:soft(`render.rs` 零尺寸分支)与 wgpu(`passes.rs` 规划期)
   都逐 op 计入 `ops_total` + `group.size.zero` 跳过,不变量两端成立。
3. **后端 P2-3/P2-4:Group transition 静默忽略 / Transition 一律跳过** —
   **J2 已修**:两端执行 `Op::Transition`;配对缺失回退普通层。
4. **后端 P2-5:`first_frame` 未注入 HostPaths**、丢弃 `tick_at` report、
   名字误导为“官方首帧” — **AU1 已修**(C3):7 个示例 + `yu-cli boot()`
   注入宿主路径。
5. **后端 P2-8:后端未消费 `fixed_state()`/`blend_table_json()`** —
   **AU2 已修**:soft `blend_pixel` 的 stage 归约提取为 `stage_reduce`
   (单一事实来源),新增交叉检查测试
   `blend_pixel_matches_generic_d3d9_equation_from_state_table`:用
   `fixed_state()` 的因子/运算做通用 D3D9 方程求值,12 点网格 × 8 模式
   逐通道对拍;可执行集 = 状态表集合也一并断言。公式与状态表任何一侧
   单独改动都会在测试失败,不再漂移到像素。
6. **后端 P2-9:`image_probe_order` 只在构造时生效** — **已作废(AU2)**:
   A2 修复后探测序只在 `GfxState::from_tables` 装载或宿主显式注入,
   `GfxState` 无 setter,帧内不存在变更口。
7. **J-10:COPY/COPY2 已按 EVID-T-11/12/15 实现**(区域/时序/生存期/交错序);
   软/硬后端执行,`j2-micro-copy2` 三方向量一致。`TEXCOPY/TEXSSCOPY` 仍 open。
8. **J-08:主组直绘/子组 RT alpha 行为已 Confirmed**,卡契约裁决(DIV-GFX-008/041);
   title 稳定帧不依赖该差异(J2 报告)。
9. **按钮状态后缀**:**J3 已修**。`yu-rt` 实现 `FILEINFO[EXIST]→LET`
   (EVID-J3-01),title `asset.missing=0`;DIV-GFX-043 fixed。
10. **`first_frame` 同名 example 互相覆盖**(soft/wgpu 都叫 `first_frame`)—
    **已修**:wgpu 侧已改名 `wgpu_first_frame` / `wgpu_frame_walk`
    (examples 目录实测无重名)。

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
- rt(AU5 勘误:此行多为旧口径):`F_INT`/`F_FLT`/`F_STR`/`DEBUGLIST` **已由
  DIV-RT-043 收口**(脚本错误槽/no-op);`MATH3D` **已实现**(DIV-GFX-046);
  `DIALOG` **已实现**(DIV-RT-044)。剩余:`YMV/YMVINFO/YMVEND`(宿主媒体,
  路线图 B14)、`ERROR` 消费者(046,路线图 B10)、`TRAY/CLIPACT 写/REGINFO`
  (宿主能力,路线图 B7)、`CLIPINFO/LABEL/MACRO`。全量清零计划见
  `docs/tasks/roadmap-batches.md` B10。
- rt 部分:INFO 类(1598/129/3 次)是“未安装 GraphicsHost”的普查口径,装了宿主
  即可;`LET` 剩 2 次 = `@_SAVE_PATH` 宿主注入 + `_TEXT_FLAG` 写入。
- format:`DIV-002` NUL-XOR 方言(无真样本)、`DIV-007/009` YSTB v2
  (仅当要支持老游戏)。~~`DIV-003` GBK 码表仍未落地~~ **码表已落地**
  (`yu-format/src/codec/table_gbk.rs` + 全表往返测试 `full_table_roundtrip_gbk`);
  DIV-003 剩余 = `yu-vfs/src/dir.rs` DirSource 把 Unicode 文件名折回字节键。
- trace:`DIV-L-002`(script 60 返回后的任务相位/调度)open;官方 L1 `args=[]`
  参数级不可观测,当前只到命令顺序级。

## 4. 非缺口(不要修)

- 31 个域事件命令(CG/WINDOW/SAVE/FILE/MENU/EMOTE/MOVIE/FLASH/INPUT…):
  VM 只发事件,语义归宿主/子会话。`SOUND`/`SOUNDINFO`/`SOUNDEND` 已由
  `yu-sound` 宿主接线(见 `docs/sound-ledger.md`),不再计入域事件缺口。
  `MIDI`/`MIDIEND`/`MIDIINFO`/`CDDA` 保持 Unimplemented(0.495 处理体为空桩,
  手册无页;见 `docs/sound-ledger.md` N0-10)。
  **例外**:`EMOTE` 的**渲染语义**是独立子系统(E-mote 运行时/PSB/自带
  58 个 shader),不是 VM 域事件本身的缺口;见 `docs/emote-parity.md` 与
  `docs/tasks/task-emote-runtime.md`。
- 12 个声明类(G_INT/G_STR/…):不按命令执行,初值走 `VARACT[INIT]`。
- DIV-GFX-014(同 Z 顺序,decision)、DIV-H-001(withdrawn)、
  DIV-006/008(explained)。

## 5. 建议修复顺序

| 顺序 | 动作 | 阻塞关系 |
|---|---|---|
| ~~P0~~ | ~~A1 主组 COPY 无后缓冲源~~ **AU1 已修**:`frame.clear` 合成后缓冲初值(DIV-GFX-051 fixed;官方启动瞬间截图仍待采集) | ~~首帧 golden/case 全依赖~~ |
| ~~P0~~ | ~~A3 渲染器 `reset()`~~ **AU1 已修**:两后端 reset + restart 接线 + `yu-cli/tests/restart.rs` 真语料回归(DIV-HOST-001 fixed) | ~~重启后像素错~~ |
| P0 | 解 boot/golden 回归:给 gfx/后端示例与测试注入 `debug_mode=true`(语料是 debug 构建,DIV-L-001 证据),重跑 census;不回再二分 `1365918`。**AU2 复核注**:title 对拍(600 帧两端 0 差异 + 官方平坦区域 max 2)当前不受此阻塞;该 P0 只影响 census 口径是否与官方 debug 构建一致,保留待办 | 不解决则 census 口径存疑 |
| ~~P1~~ | ~~C1 `case --all --update`~~ **AU1 已修**(85 ops / 0 skipped,3/3 PASS);门禁保持提交前手跑,CI 挂接待宿主 | ~~当前用例等于无门禁~~ |
| ~~P1~~ | ~~A2 `image_probe_order`~~ **AU1 已修**:空表在 `from_tables` 拒绝构造(DIV-GFX-049 fixed) | ~~违反 A5 原文~~ |
| P1 | ~~后端 P1-2 / P2-2/3/4/5/8/9 + example 重名~~ **全部关闭**(P1-2=J4,P2-3/4=J2,P2-5=AU1 C3,P2-2/P2-8 复核与交叉检查=AU2,P2-9 作废=AU2,重名已改) | — |
| P1 | 组 RT 子组 alpha(DIV-GFX-041,契约裁决后) | 依赖裁决 |
| ~~P2~~ | ~~文字桥:DIV-GFX-012/033/034/035/040~~ **主体已 fixed(P2/P7 续 3 对账)**:012/034/040 落地;剩余 = 033 reveal(等 H-P2-02 取证)、035 尾项(宿主字体清单/BNO/PITCH/LANG,见 036) | ~~依赖 M2 的 TEXT 状态~~(已落地) |
| P2 | **E1 E-mote 运行时(M0→M4)** | M0/M1 完成;M2 pose/参数表/Z 顺序、M3 plan/soft/composite mask 完成(2026-10-22);剩余参数化 deformer mesh、wgpu 管线、物理控制器/M4 对拍 + 内核接线。**AU5 对账**:以 submodule `d0f03a1` 为准(物理未求值、嵌套深度仍 32);`emote-parity.md` §7/§8 宣称项在该 checkout 不可复核,已加失真警告 |
| P2 | 资产元数据 provider(DIV-GFX-024)→ CGINFO SX/SY | 已部分由 J3 落地(图像头),完整 provider 仍缺 |
| ~~P2~~ | ~~**A4 `REVERSE`+`RECTPAINT` 组合补两端测试或删分支**~~ **已修(AU2)**:跨后端用例 `reverse_after_rectpaint_matches_soft` 逐像素一致(DIV-GFX-052) | — |
| ~~P2~~ | ~~**C2 `eprintln!` 移出 `src/`;C3 示例注入 HostPaths**~~ **已修(AU1)**:`src/` 无 stdout 宏(§6.5);7 个示例 + `yu-cli boot()` 已注入宿主路径 | — |
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
5. ~~**代码 `eprintln!` 仍在 `src/`**~~ **已清(AU1 复核)**:AU2 复跑
   `grep -rn "println!|eprintln!|dbg!" crates/*/src` 仅命中
   `yu-cli/output.rs` 的纪律注释本身,`src/` 无 stdout 宏。

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
