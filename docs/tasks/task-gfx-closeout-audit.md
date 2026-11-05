# 任务书 · yu-gfx 剩余项收口审计(第 0 步:只重建事实,不改代码)

- 日期:2026-11-05
- 范围:`crates/yu-gfx/src`、`crates/yu-render/{soft,wgpu}`、相关台账
- 本文件状态:**待主人过目**。未过目前不进入实现波次。
- 纪律:`AGENTS.md` §0/§4;零 git 写操作;不改他人未提交改动。

---

## 0. 基线(改动前,2026-11-05 本机)

环境:`TEMP/TMP=<side-repo>/workbench/tmp`;
`YURIS_SAMPLE_ERIS_ROOT=<side-repo>/workbench/project-eris`;
`YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/data/ysbin`。

| 项 | 命令 | 结果 |
|---|---|---|
| 构建 | `cargo build --workspace --all-targets` | 通过(exit 0) |
| gfx 测试 | `cargo test -p yu-gfx --all-targets` | 155 passed / 0 failed |
| 覆盖普查 | `cargo run -p yu-rt --example coverage_census -- 600 8` | 命令 7,735,437;逻辑帧 602;语料 FNV `64d15de36b4fff30`;与 `coverage-matrix.md` 基线一致 |
| 标题 skip | `probe_skips <project-eris> --frames 600`(soft) | **无 skip**;结束帧 445 层,无 `Unimplemented` |
| 毒化层 | `probe_layer_status <project-eris> --frames 600` | f200/f400/f600 毒化层 0 |
| 演出探针 | `scenario_probe <project-eris> cases/symptom-enter-story.json --backend soft` | 470 帧:final_ops 33;final_skipped 0;skipped_total 0;gfx 诊断 0(仅 EMOTE deferred/SYSTEM aligned-stub) |
| 用例 | `yu-cli case <project-eris> --all` | **基线已红**:`first-screen`、`record-events-off`、`save-load-loop`、`save-title-flow`、`symptom-enter-story`、`symptom-ingame-config` FAIL(其余仍在跑;本轮未改任何代码) |

> 结论:**当前标题路径不存在整层消失**;整层丢弃风险集中在演出/精灵路径
> (`TEXCOPY`)与商业语料未覆盖的参数组合(MID/资源图尺寸/RID/多部件)。

---

## 1. 当前真实状态表(以代码为准,不以旧台账为准)

> **注(2026-11-05 实现后)**:本表是开工前的快照。005/007/022/025/030/032
> 与 `CGINFO[TEX]` 已在第 1/2 波落地,最新状态见 §9;其余行仍有效。

图例:
- **代码状态**:`exec` 可执行 / `skip-layer` 整层被 `LayerStatus::Unimplemented` 挡住 /
  `skip-op` 只跳过该 op / `diag` 只诊断不改可见行为 / `n/a` 不可达。
- **命中**:ERIS 官方脚本计数(源 `<project-eris>` 解包脚本;
  运行期以 600 帧 probe 为准)。
- **官方**:`Y` 做 / `N` 不做 / `?` 未取证。
- **建议**:五种终态之一,或 `open(evidence)` 取证任务。

### A. 整层丢弃 / 画面缺失

| ID | 旧台账 | 代码真实状态 | 命中 | 官方 | 缺什么 | 证据 | 风险 / 波次 |
|---|---|---|---|---|---|---|---|
| 069 §5.1 仿射公式 | fixed(绑定+缩放)/ open 半像素 | `exec`:普通层 `FBX/FBY` 缩放已接 soft/wgpu;`rz/ex/ey/sd/md/F` 只发 recoverable 诊断,不丢层 | 立绘路径每帧 | Y(仅 X/Y+FBX/FBY) | `-0.5` 与 16.16 像素口径 → 042/064 | `FUN_00414ba4`;DIV-069 §5 | 文档已改;像素残留归 042 |
| 005 `MID` 外部掩码 | unimplemented | **`skip-layer`**:`apply_layer_params` 标 `gfx.mask.unsupported` → `op_for_layer` 返回 `Op::Mask` → 两后端按状态跳过**整层** | `MID=` **0** | Y:遍历时解析 `+0x210` 并查层表(`0x413e9b..0x413ecf`) | 掩码取值通道(α/亮度)、尺寸/采样;生产绘制消费点未逐行还原 | `FUN_00413dc8` 111-119;`FUN_0042f809` 405-406 | 第 1 波:先“基图照画 + 掩码字段单列诊断”;通道取证后补齐 |
| 004 多部件层 | unimplemented | **`skip-layer`**,但 `Layer.parts` **全仓无写入点**(仅默认空/COPY 克隆)⇒ 运行期不可达 | 0 | Y:按 `+0xa6/+0xa7` 网格、`+0xb0/+0x130` 每格宽高、`+0x21c` 基址逐部件 `SetTexture` | PSB/多部件资产 provider + tile 表来源 | `FUN_00414ba4` 155-390;DIV-069 §2.2.1 | 不可达;`open(evidence)`,不排代码波次 |
| 022 资源图 `SX/SY` | unimplemented | **`skip-layer`**:任何非 `SolidBlack1x1/None` 纹理 + 显式 `image_size` 标 `gfx.image.size.unsupported` | 非空 `FILE`+`SX` **0**;`FILE=""`+`SX` 大量 | Y:`FUN_0043772c` 按 SX/SY 生成缩放纹理并写 `+0x78/+0x7c` | 重采样滤波(点/线性)、裁剪口径 | `FUN_0043772c`;`render-parity §3.4` | 第 1 波:去整层 skip,按目标矩形缩放;滤波待 REA |
| 007 `TEXCOPY/TEXSSCOPY` | fixed(COPY/COPY2);Tex* 仍 unimplemented | **`skip-layer`**:`cgact_copy` 无条件把 dest 纹理换成 `Snapshot`,再标 `Unimplemented` ⇒ 目标层永久不画 | **3 处自复制**(`es_Sprite.yst:5604/8884/8956`,`ID==ID2`) | ? handler 未定位;但官方引擎在这些层上照常显示 ⇒ 自复制至少不是“隐藏层” | action 68/69 的执行体/时序 | 语料 + `0x42c4a4` CGACT 处理体(未反编译) | **第 1 波最高优先**;先 REA 定位,再实现/对齐 |
| 028 图像层子层目标 | open | `exec`:图像节点子树按引擎后序扁平输出(`emit_image_subtree`) | 未知;不丢层 | Y:遍历对任意节点先递归子层(`0x413e1a`) | 目标 RT 的独立语义(若有) | `FUN_00413dc8`;D16 | 建议 `fixed`;剩余目标 RT 归 041 |
| 048 组上的 TID 转场 | open(Likely) | `exec`:输出 `Op::Transition{from: Offscreen(组), to: 配对}` | 组 TID **0** | Y(层表统一解析) | 官方微场景 | `group_transition_op`;`render-parity §8` | `open(evidence)`,不挡主路径 |
| 026 组/离屏 `ONMOUSE*` | partial | 图像层 `ONMOUSE/2/3/COLOR` **已实现**;组直接返回 `INFO_UNSUPPORTED` | 13 次 ONMOUSE 全为图像层;组 **0** | Y:采样 `[层+0x90]` 表面 | 组的已合成像素面 provider(需后端上一帧/CPU 镜像) | `query_onmouse`;REA `0x43de01..` | 低命中;`open(evidence)`,B7/宿主面 |
| 025 INFO 多字段 | unimplemented | `diag`:多字段直接 `INFO_MULTI`,不写 LET | 多字段 **0**(ERIS 全为单字段) | ? 处理体未反编译 | 官方字段顺序 | `single_flag` | `open(evidence)`,低风险 |
| 038 INFO 缺失目标非 EXIST | unimplemented | `diag`:返回 `INFO_MISSING`,不写 LET | 待 scenario 诊断计数 | ? | 缺失返回语义 | `query_cginfo` 1614-1628 | `open(evidence)`,低风险 |
| 024 资产元数据 provider | partial | 图像头/组/快照/影片可答;未注入资产仍 `INFO_ASSET_SIZE` | 标题资产齐全 | Y | PSB/E-mote 头 provider | `query_layer_size` | `open(evidence)`;E-mote 域 `deferred(master)` |
| 031 `ID3/ID4` 非 TRANSITION | partial | `diag`:非 TRANSITION 且给 ID3/ID4 只报未取证;TRANSITION 路径已实现 | ID3 仅 1 处 TRANSITION;ID4 **0** | ? | 多图层动作语义 | `apply_cgact` | `open(evidence)`,不排代码波次 |
| — `CGACT[TRANSITION2]` | 未登记 | **`skip-layer`**:`first_image_action` 识别为 `Other("transition2")`,但 `cgact_transition` 只看 `TRANSITION`,图像分支无该动作 ⇒ `Unimplemented` + 层被标不可执行 | **0** | ?(ysc 表有该槽) | 是否是 `TRANSITION` 的变体、参数差异 | ysc 参数表;`IMAGE_ACTIONS`;`apply_cgact` | 低命中;`open(evidence)`,与 031 同族 |
| 030 `CG[TIDNO]` | open | `diag`:原样留档,不改可见行为 | **0** | Y(应同 IDNO 拼接) | 与 TID 的组合规则 | `state.rs` TIDNO 分支 | 第 3 波;REA 后可 `fixed(no-manual)` |
| 018 子窗口叠放 | unimplemented | gfx 只输出窗口状态/离屏 pass;叠放归宿主 | 0(无 window host) | Y(宿主域) | 宿主窗口实现 | `render-parity §3.3` | 非 gfx 收口范围;B7 |

### B. 合成 / 过渡 / 文本

| ID | 旧台账 | 代码真实状态 | 命中 | 官方 | 缺什么 | 证据 | 风险 / 波次 |
|---|---|---|---|---|---|---|---|
| 041 子组 RT alpha | partial | `exec` 但 RGBA8 直通;组 RT 清透明黑;父级按组 A 合成 | 标题结束帧 2 pass(含 offscreen);未逐像素对拍 | Y:微场景证明**子组 RT alpha 不参与合成**(K4 `A=128` → `(0,128,0)`;根层同参 → `(16,160,48)`) | 生产 RT 格式/清屏色/写掩码 | `EVID-J-08`;`docs/fidelity-ledger-j.md` | **第 2 波**;高风险,需 micro5 复跑 + 标题 A/B |
| 065 过渡覆盖度 | fixed(2026-11-05 psC) | `exec`:soft/wgpu 均走 psC;`to_base` R 通道 | 标题/立绘转场 | Y | — | DIV-065 §13;PROGRESS TRANS-PSC | 已闭;台账需销 045 |
| 045 TA≠128 残差 | open(低风险) | psC 公式已按 TA 解析;40 格表 ≤2/255 | 标题 T=256/TA=128 | Y | — | DIV-065 §13 | **建议随 065 改 fixed**;台账未同步 |
| 072 §1-8 加工链→COPY→CGEND | open | COPY 的 `source_texture` 只记基图;源被同帧 CGEND 时中间加工可能不重放 | 存档缩略图链 | Y(命令时刻取当前像素) | 把 COPY 消费者纳入图像链发射闭包 | DIV-072 §1-8;EVID-T-15 | 第 2 波;可构造红绿用例 |
| 072 §1-9 / 076 文字/图像命令序 | open | 图像/复制 pass 全部前置,文本层末合成;执行图像加工会清目标层文字 | 外字页行首 `」` 曾复现 | Y(命令即写表面) | 文本烘焙按 seq 进入表面命令序 | DIV-072 §9;DIV-076 | **与文本线交叉**;第 2 波需主人确认认领 |
| 056/057 自动文本行高/基线/无层叠加 | open(Likely) | 行高=SY、基线=ascent、无层=屏幕覆盖;均 Likely | ERIS 正文不走自动层 | ? | 官方文本截图/最小脚本 | DIV-056/057;H-P2-03 | 文本线会话可能已认领;本任务不重复实现 |
| 075 待显示行缓冲 vs 图层表面 | open | 累积显示缓冲 + flag 换行 | 0(600 帧无文本) | Y(`+0x38` 行完成清空、`'P'` 清表面、宽度折行) | 官方最小脚本 + 截图 | DIV-075 | 文本线 |

### C. 像素 / 数值

| ID | 旧台账 | 代码真实状态 | 命中 | 官方 | 缺什么 | 证据 | 风险 / 波次 |
|---|---|---|---|---|---|---|---|
| 009 颜色空间 | open | `exec`:存储值空间混合(sRGB8 编码),无线性化 | 全帧 | Y(D3D9 默认,未开 sRGB write) | 官方色彩阶梯截图 | 全树无 `D3DRS_SRGBWRITEENABLE`;`render-parity §11` | 第 3 波:截图后 `fixed(no-manual)` |
| 010 帧清屏 | open | `exec`:`[0,0,0,255]` | 全帧 | Y(首屏黑) | 官方首帧 PNG 逐像素 | `Clear::default_black`;DIV-010 | 第 3 波:首帧截图后闭 |
| 042 半像素/quad 相位 | open | `exec`:顶点位置直接用 `X/Y`,未加 `-0.5`;1px 顶/边与官方不同 | 所有图层 | Y:`x0=X-0.5`、`y0=Y-0.5` | 两后端统一 quad 原点 + 16.16 口径 | `FUN_00414ba4`;DIV-069 §5.1 | 第 3 波;改两后端共式 |
| 060 `CG[FILE=""]` 1×1 字节 | open(evidence) | `SolidBlack1x1 = [0,0,0,255]`;REA 空串路径写四字节 0 | `FILE=""` 大量 | ? 手册“黑图” vs REA 全 0 | 官方微场景中心像素(考虑 `+0x86`) | DIV-060 | 第 3/4 波;不得凭 REA 单层改 |
| 064 浮点坐标规约 | partially-confirmed | `exec`:f64 一路到顶点,亚像素连续 | 立绘每帧 | Y(官方位置连续) | 精确 f64→i64 指令未定位(行为已对齐) | DIV-064;`MATH3D` trace | 建议 `fixed(no-manual)`;补回归即可 |
| 063 无手册滤镜尾项 | partial(no-manual) | 全族已实现;`XOR/AND` 只做 RGB、`RESIZE` 不重采样、`MEIDO` ×byte/128 | MEIDO 18 / RESIZE 16 / XOR·AND 0 | ? | XOR/AND alpha、RESIZE 滤波、MEIDO 舍入 | DIV-063 §2 | 第 3 波;低风险(语料只用 MEIDO/RESIZE) |
| 016 `CG` 无 FILE | open | `exec`:get-or-create 无纹理层;遍历跳过 | `CG` 无 FILE 有 | ? 手册“层不存在=错误” | 处理体重锚定 | DIV-GFX-016;A8 | `open(evidence)`,可见无差异 |
| 032 `SWAP/SWAP2` 模式对应 | open | `exec`:SWAP=换图+坐标+属性,SWAP2=换图+坐标;与 `FUN_0042f57d` 模式 1/2 同构 | `CGACT SWAP` **0**(脚本 SWAP 是数值交换宏) | Y | 调用点确认 | `FUN_0042f57d` 61-112 | 第 3 波;REA 后 `fixed` |
| 067 资产预载并行 | note | 实现层并行,不影响像素 | — | — | — | DIV-067 | 保持 note |
| 011 extra 参数分类 | fixed(no-manual) | `LINT/LINT2/TSX/TSY/MFILE/CASH/DXDRAW/MIPMAP/DXBUF` 留档;`RID` 仍整层 skip | RID **0** | Y(资源 provider) | RID 取图 | DIV-069 §2 | 011 本身已闭;RID 单列在 024 |

### D. 已终态(无需再“修”)

| ID | 终态 | 备注 |
|---|---|---|
| 001 | fixed | MODE 3/5/6/7/9 状态表;两端执行 |
| 002 | unsupported-by-official | `FUN_00414ba4` 无分支;内核归约通常路径 |
| 003 | fixed | 绑定 + 普通层缩放 |
| 006 | fixed | 过渡配对/TA 门控 |
| 008 | fixed | 主组直绘 |
| 012 | fixed | 文本字形/两条路径 |
| 014 | decision | 同 Z 取创建序(手册称不確定;确定性决策) |
| 015 | note | 首屏逻辑帧号;非语义缺口 |
| 017 | fixed | COPY 属性范围 |
| 019 | fixed | 顶点 DIFFUSE = A 灰度 |
| 020 | fixed(主路径) | `GfxHost` 已接;残余字段分别归 024/025/026/036 |
| 021 | fixed | 字节 ID |
| 023 | note | 复制/图像日志 16,384 上限 + dropped_ops |
| 027 | fixed(no-manual) | IDNO 十进制 |
| 029 | fixed | pass 交错序 |
| 033/034 | fixed | 显示节奏 / 跨帧文本 |
| 035 | partial(BNO fixed) | PITCH/LANG 只存值 |
| 036 | partial | NUM/LANG/NAME(NO=n) 已答;未设置 NAME/PITCH/X/Y 仍缺 |
| 037 | partial | 窗口 host 字段 |
| 039 | fixed(no-manual) | 双口径 |
| 040 | fixed | TEXTINFO |
| 043 | fixed | FILEINFO EXIST |
| 044 | withdrawn | 非引擎命令 |
| 046 | fixed | SIZE 缩放 |
| 047 | fixed | WebGPU 颜色因子 |
| 049 | fixed | 扩展序 |
| 050 | fixed(集合) | 4 个 blob/pass 全接;MODE 公式归 002 |
| 051–055 | fixed | 首帧源/REVERSE/快照冻结/区域缺省/文本合成 |
| 058/059 | fixed | 文本快照/空 NAME/缓存 |
| 061/062/066/068 | fixed/note | 动画 sprite/CPU 镜像/帧契约/存档时刻 |
| 069 | fixed(绑定+缩放) | 绘制公式见 042 |
| 070 | fixed | `CGINFO[COLOR]` 已实现(注:仓库根 `DIV-GFX-070` 是 COLOR,不是仿射公式) |
| 071 | fixed | WINDOW 读写配对 |
| 073/074 | fixed | YMV/MOVIE |
| 076 | 外字链 fixed(no-manual);通用命令序 open | 见 072 §9 |

---

## 2. 与旧台账的差异清单(必须先改档案再写码)

1. **DIV-GFX-026**:旧表写“`COLOR` 等仍 unsupported”;代码已实现
   `COLOR/ONMOUSE/2/3`。真实残余 = 组/离屏取样面。
2. **DIV-GFX-036**:旧表写 `NUM/LANG/NAME` 全缺;代码已实现
   `NUM/LANG/NAME(NO=n)`。真实残余 = 未设置 `NAME` 默认名、`PITCH/X/Y`。
3. **DIV-GFX-069**:提示词称“`state.rs:6030` 仍标 `gfx.affine.unverified` 并跳过”;
   现值不存在——只有 `gfx.affine.effect-unverified`(recoverable),层照画。
4. **DIV-GFX-045**:提交 `eb672e1` 已随 psC 定稿关闭;`gfx-unimplemented-todo.md`
   仍写 `open(低风险)`。需改 `fixed`。
5. **DIV-GFX-065**:已是 `fixed`;提示词“硬边界拟合”是 2026-11-05 前的旧状态。
6. **DIV-GFX-028**:代码已按引擎后序扁平绘制图像层子层;旧表仍 `open`。
   建议 `fixed`(目标 RT 的独立语义若有,归 041)。
7. **DIV-GFX-013**:soft/wgpu 全族已实现(含 BLUR*/DIFFUSION/WHIRLPOOL/RASTER*);
   真实残余只剩 063 的 `XOR/AND` alpha、`RESIZE` 滤波、`MEIDO` 舍入。
8. **DIV-GFX-004**:`Part` 无写入点,运行期不可达;旧表“`Part.raw` 原样保存”
   不等于“有缺口挡住实际层”。
9. **行号漂移**:提示词中的 `state.rs:6030/5848/4602/5870/6095/3092/1649/5023`
   均与现行代码不符;以函数名(`apply_layer_params`、`cgact_copy`、
   `query_onmouse`、`single_flag`)为准。
10. **`DIV-GFX-070` 编号冲突**:`DIV-GFX-069 §5.1` 把仿射公式称作
    `DIV-GFX-070`,但仓库根 `DIV-GFX-070` 实际是 `CGINFO[COLOR]`。
    仿射像素口径应统一引用 `DIV-GFX-042/064/069`,不再新造 `070`。

---

## 3. 波次计划(每波独立可编译、可回退)

### 第 1 波:整层丢弃 / 可执行性(先做“层不消失”)

1. `DIV-GFX-007` `TEXCOPY/TEXSSCOPY`
   - 先 REA:在 `0x42c4a4` CGACT 处理体内定位 slot 68/69 的执行体;
   - 若为纹理句柄复制:实现 `CopyKind::TexCopy` 的同层/跨层语义;
   - 自复制(`ID==ID2`)在两种语义下都不应是“隐藏层”;
   - 验收:构造 `CG[层 A]→TEXCOPY(ID=A,ID2=A)` 后帧内仍有该层、像素与无命令一致;
     跨层形态按 REA 期望断言。
2. `DIV-GFX-005` `MID`
   - 去整层 skip:基图按正常层绘制,掩码字段单独 `gfx.mask.unverified`(recoverable);
   - 并行取证:掩码通道/尺寸/采样;实现 `Op::Mask` 真正合成;
   - 验收:带 MID 的层不再整层消失;取证后像素与官方微场景一致。
3. `DIV-GFX-022` 资源图 + `SX/SY`
   - 去整层 skip:按目标矩形绘制(与 `SIZE` 共用缩放路径);
   - 滤波口径待 REA;先用与 `SIZE2` 一致的双线性并在台账标 `Likely`;
   - 验收:非空 FILE + SX/SY 的层可见,尺寸正确;像素待官方样本。
4. `DIV-GFX-004/024/026/031/025/038`
   - 不写猜测代码;统一登记 `open(evidence)`,列明所需样本/REA 点。
   - 其中 026 若宿主能提供组像素面,再排 B7。

### 第 2 波:合成 / 过渡 / 命令序

1. `DIV-GFX-041` 子组 RT
   - 先复跑 `micro5`,确认“RT alpha 不参与合成”的完整语义(清屏色/写掩码);
   - 设计:子组 RT 的 alpha 语义与主组直绘分开;两后端同式;
   - 验收:micro5 四个采样点与官方逐像素一致;标题/演出 A/B 不新增黑框。
2. `DIV-GFX-072 §1-8` 加工链 → COPY → CGEND
   - 把 COPY 消费者纳入图像链发射闭包;COPY 优先取 `texture_overrides`;
   - 验收:红/绿对照用例,soft/wgpu 一致。
3. `DIV-GFX-072 §1-9` / `DIV-GFX-076` 文字/图像命令序
   - 与文本线会话确认认领范围;若归本任务,按 seq 把文本烘焙进层表面;
   - 验收:ERIS 外字页行首前缀保留;`CGACT[TEXT]→MERGE2` 与官方截图一致。
4. `DIV-GFX-048` 组 TID
   - 微场景(YSCom 离线 CLI 编译);确认后 `fixed` 或修正 `group_transition_op`。

### 第 3 波:像素 / 数值

1. `DIV-GFX-042` `-0.5` 半像素:抽共用 `quad_origin()`;soft/wgpu 同式;
   先加几何断言,再等官方 1px 边缘截图。
2. `DIV-GFX-060` 空 `FILE` 1×1:官方微场景中心像素裁定;
   若 `+0x86=0` 时 alpha 被忽略,则记录该前提,不盲改 `SolidBlack1x1`。
3. `DIV-GFX-009/010`:官方首帧/色彩阶梯截图;能复采即闭,否则 `open(evidence)`。
4. `DIV-GFX-064`:行为已对齐,补回归后改 `fixed(no-manual)`。
5. `DIV-GFX-063` 尾项:`XOR/AND` alpha、`RESIZE` 滤波、`MEIDO` 舍入。
6. `DIV-GFX-016/030/032/025/038`:REA 定位处理体后逐个闭。

### 第 4 波:B8 取证任务(不猜)

- 官方首帧(010)、色彩阶梯(009)、1px 边缘(042)、空 FILE(060)、
  多部件 PSB(004)、MID 掩码(005)、组像素面(026)、文本行距/页(056/057/075)、
  子组 RT(041)、组 TID(048)。
- 每个任务写清:采集脚本、最小脚本/微场景、期望输出、验收标准;
  缺失样本显式计数,禁止空跑。

---

## 4. 需要主人裁决

1. **041 子组 RT**:EVID-J-08 只证明“alpha 不参与合成”;是否批准按
   “RT 不透明黑 + 组 A 一次合成”先实现,再用 micro5 + 标题 A/B 验证?
   (若错误会引入黑框,风险最高。)
2. **072 §9 / 076 通用命令序**:与文本线会话重叠;本任务是否认领?
   若不认领,只做 §1-8(COPY 链)。
3. **004/022/024 资源 provider**:多部件/资源图尺寸需要 PSB/E-mote 资产头;
   E-mote 家族仍 `deferred(master)`。是否只做“去整层 skip + 标 Likely”,
   等 provider 到位再像素对齐?
4. **基线红**:`case --all` 的 5 个 FAIL 在本轮改动前已存在(疑似近期
   转场/仿射改动后未重生成 golden)。是否允许在收口完成后按官方证据
   重生成 golden?未获授权前不动 golden。
5. **009/010/064 的闭账口径**:是否接受“REA + 既有官方微场景/首帧台账”
   作为 `fixed(no-manual)`,还是必须重新采集官方截图?

---

## 5. 基线原始输出(摘要)

```text
cargo build --workspace --all-targets
  Finished (exit 0)

cargo test -p yu-gfx --all-targets
  155 passed; 0 failed

coverage_census 600 8
  语料 data.ypf 31501753 bytes, FNV-1a64 64d15de36b4fff30
  静态 136 yst / 36974 组 / 80102 窗口
  命令 7,735,437;逻辑帧 602;事件丢弃 0

probe_skips --frames 600 (soft)
  逐帧 skip 汇总: (无 skip)
  结束帧: 445 层;全部 status=Executable

probe_layer_status --frames 600
  f200/f400/f600: 毒化层=0 累计毒化=0
  汇总: 毒化图层 0 个

yu-cli case --all
  first-screen            FAIL (ops expected 85 / actual 75)
  record-events-off       FAIL (同上)
  save-load-loop          FAIL (ops expected 122 / actual 33;
                                 display/png/save_state sha 均不同)
  save-title-flow         FAIL (ops expected 96 / actual 13)
  symptom-enter-story     FAIL (ops expected 122 / actual 33)
  symptom-ingame-config   FAIL (ops expected 404 / actual 38)
  (其余用例结果待补;本表随运行追加)
```

---

## 6. 第 1 波执行记录(2026-11-05)

主人令「开始推进」后先做“整层不消失”,本波已落地:

### 6.1 `TEXCOPY/TEXSSCOPY`(DIV-GFX-007)

REA 新事实(官方 0.495,sha256 `4f5eefa3…`,REA 已开):

```text
CGACT 处理体 0x42c4a4:
  0x42e67f  movzx eax, byte [0x6841a4]   ; slot 68 TEXCOPY
  0x42e686  jne 0x42e6b9 → call 0x43ccd8 ; FUN_0043ccd8
  0x42e68a  movzx eax, byte [0x6841a5]   ; slot 69 TEXSSCOPY
  0x42e6a7  call 0x43cd6c                ; FUN_0043cd6c
```

- `FUN_0043ccd8` = **PASTESPRITE**:把源/目标层指针、目标 `+0x78/+0x7c`、
  `FCX/FCY`、`+0xa5/+0xa8/+0xac` 写全局,再调 `FUN_0040af1c`(5490 B,
  按 `+0xa6/+0xa7` 网格与 `+0xb0/+0x130` tile 表粘贴 sprite);
  失败串 `YS_PASTESPRITE Error.`。
- `FUN_0043cd6c` = **GETSS**:BeginScene/采样器过滤后,
  `FUN_00414ba4()` + `FUN_0043d308()`,按全局宽高 `malloc` 目标表面并写
  `[目标+0x90]/+0x78/+0x7c`;失败串 `GETSS SetRenderTarget Error 1.`。

结论:**两者都不是纹理 blit**。旧实现换空快照并毒化目标层是错的。
现实现:保留目标层原纹理与状态,`CopyOp` 只留审计、`frame()` 不发射,
发 recoverable `gfx.copy.unsupported`;像素语义仍 `open(evidence)`。
档案:`docs/divergences/DIV-GFX-007-texcopy-paste-sprite.md`。

### 6.2 `MID`(DIV-GFX-005)

- REA `0x42a801..0x42a8a4` 已给出完整语义:mask 与目标层宽高必须相等,
  逐像素 `target.a = mask.R`(RGB 不动),并置 `+0x86=1`;
- 实现为 `mask_alpha` 合成 op(命令序、可折叠),soft/wgpu 同式;
- 缺 mask/尺寸不等时保留基图 + recoverable 诊断,不再整层跳过;
- **fixed(no-manual;图像层)**;组 mask 仍 open(evidence)。
  详见 `docs/divergences/DIV-GFX-005-mid-mask-alpha.md`。

### 6.3 图像层显式尺寸(DIV-GFX-022)

- 删除 `apply_layer_params` 末尾对“非黑图 + 显式尺寸”的
  `gfx.image.size.unsupported` 毒化;
- 层矩形 `+0x78/+0x7c` 直接保存;Asset/Snapshot/Movie 由两后端按矩形
  缩放(soft 已有 `asset_with_explicit_size_is_scaled_not_skipped` 像素用例);
- 单边尺寸:缺省轴按当前层矩形,新层按引擎构造默认 1,并留 recoverable
  诊断;不再 `Unimplemented`。

### 6.4 通用“未取证图像动作不毒化整层”

`cgact_image` 失败时不再把源/目标层 `texture_status` 置 Unimplemented:
只记录 `ImageOp`(status Unimplemented,后端跳过)并诊断,层按已有基图/
先前加工结果继续绘制。`emit_image_subtree` 的 `has_surface` 保证“无基图
且没有可执行生产者的层”仍不会凭空出现。`TRANSITION2` 等无执行体的动作
因此不再让层消失。

### 6.5 本波验证(原始结果)

```text
cargo test -p yu-gfx --all-targets
  group: 20 passed(新增 3 个 TEXCOPY/TEXSSCOPY 用例)
  parity_fixes: 28 passed(新增 MID / 未取证动作不毒化 / 资源图尺寸用例)
  其余套件原样通过
cargo test -p yu-backend-soft --test render
  19 passed(新增 asset_with_explicit_size_is_scaled_not_skipped)
```

### 6.6 本波未做(转下一波/取证)

- `RID` 资源图取图:缺 PSB/资源 provider,保留 `open(evidence)`;
- 多部件 `parts`:无写入点,保留 `open(evidence)`;
- 组/离屏 `ONMOUSE*`:需后端像素面 provider;
- INFO 多字段/缺失语义:`open(evidence)`;
- `041/042/060/009/010/063/072/075` 按原波次。

---

## 7. 第 2 波首项:072 §1-8(加工链 → COPY → CGEND)

- 现象:源层在同帧被 `CGEND` 删除时,COPY 回放只剩 `source_texture`(基图),
  官方命令时刻看到的“加工后像素”丢失。
- 实现:
  - `frame()` 在计算 `baked_flags` 之前,把“目标仍持有快照/仍被消费”的
    `Copy/Copy2` 的已删源层 id 播种进 `needed_sources`;
  - 已删目标的 `baked_flags` 若命中 `needed_sources`,强制重放该层加工链,
    使后端 `texture_overrides` 先得到加工后表面,COPY 再取用。
- 验收(soft 像素 + 状态双断言):
  `copy_after_processing_survives_source_cg_end`:
  `SRC(红图) → RECTPAINT(绿) → COPY2(SNAP) → CGEND(SRC) → CG(SNAP)`;
  断言 SRC 的加工 pass 在本帧被发射,且 SNAP 中心像素 = 绿
  (未修时 = 红)。`cargo test -p yu-backend-soft --test render` 20 passed。

---

## 8. 追加勘误:`CGINFO[TEX]` 是图层种类,不是 `CG[TEX=]` 写入值

- REA(CGINFO 处理体):
  - `0x43d962  movsx ecx, byte ptr [edx + 0xa4]` —— `+0xa4` 是图层种类
    (1=图像 / 2=组 / 0=已删),不是 `CG[TEX=]` 的字段;
  - CG 处理体 `0x42b072` 只在 `TEX == 1` 时进入 `0x42baa5`(把空层转成
    图像层,调用 `0x40ae99`),其它值无副作用。
- 内核修正:
  - `query_cginfo["TEX"]` 返回 `LayerKind` 码(图像 1 / 组 2),缺层仍回 0;
  - `CG[TEX=]` 原值只留在 `Layer.extra`/`Layer.tex` 供审计,不再参与
    `CGINFO` 回答;
  - 用例 `cginfo_tex_reads_layer_kind_and_absent_is_zero`:图像层=1、
    组=2、缺层=0。
- 台账:`DIV-GFX-011` 与 `DIV-GFX-046/061/063` 的旧“表面类型码”口径已改;
  `es_Sprite` 的 `IF[@texchk==2]` 仍成立(官方 E-mote 层在 `+0xa4` 上
  表现为组),E-mote 运行时本体仍 `deferred(master)`。

---

## 9. 本轮已闭环/已修正(2026-11-05)

| 项 | 结果 | 证据 |
|---|---|---|
| DIV-GFX-005 `MID` | **fixed(no-manual;图像层)**:`target.a = mask.R`,soft/wgpu 同式;组 mask 仍 open | REA `0x42a801..0x42a8a4`;DIV-GFX-005 档案 |
| DIV-GFX-007 `TEXCOPY/TEXSSCOPY` | layer-loss fixed:PASTESPRITE/GETSS 不换空快照、不毒化;像素 open | REA `0x42e67f/0x42e68a`;DIV-GFX-007 档案 |
| DIV-GFX-022 资源图 `SX/SY` | layer-loss fixed:层矩形缩放,两后端执行;像素对拍 open | `asset_with_explicit_size_is_scaled_not_skipped` |
| 未取证图像动作 | 只跳过该 op、不毒化整层(`TRANSITION2` 用例) | `unsupported_image_action_keeps_layer_drawable` |
| DIV-GFX-072 §1-8 | fixed:加工链→COPY→CGEND 重放 | `copy_after_processing_survives_source_cg_end` |
| DIV-GFX-025 | **fixed(no-manual;CGINFO)**:官方 if/else 优先级 | REA `0x43d758..0x43dddf` |
| `CGINFO[TEX]` | 修正为图层种类读回(1=图像/2=组) | REA `0x43d962` |
| DIV-GFX-030 `TIDNO` | **fixed(no-manual)**:`TID + "." + N` | REA `0x42a8e3..0x42a936` |
| DIV-GFX-032 `SWAP/SWAP2` | **fixed(no-manual)**:SWAP=mode1、SWAP2=mode2(手册相反) | REA `0x42e661/0x42e670`→`0x42f57d` |

仍未动:041(组 RT alpha,高风险,待主人拍板)、042/060/009/010/063、
056/057/075(文本线)、048(组 TID 微场景)、RID/多部件/组 ONMOUSE(provider)。

---

## 10. 本轮验证(2026-11-05,原始结果)

```text
cargo build --workspace --all-targets                  → Finished(exit 0)
cargo test -p yu-gfx --all-targets                     → 全绿
  (group 20 / parity_fixes 30 / transition 7 / info_query 12 / transform 9 …)
cargo test -p yu-backend-soft --test render            → 21 passed
cargo test -p yu-backend-wgpu --test wgpu              → 37 passed
  (含新增 mid_mask_red_channel_matches_soft,GPU/soft RGBA 逐字节一致)
cargo test --workspace --all-targets
  (YURIS_SAMPLE_ERIS_ROOT/ERIS_YSBIN/SDK0495/SDK0479 全部注入)
  → 187 个 test result;除既有 yu-sound 2 条
    `gated_ogg_load_reads_header_only_then_streams_on_play` /
    `gated_ogg_loop_wrap_back_to_zero_reopens_stream` 外全绿;
    两条为 PROGRESS 已记录的既有样本/流式失败,非本轮 gfx 改动路径。
  (未注入 SDK0495 时 `yu-save official_dno3...` 按设计显式 panic;
   注入后单跑通过。)
yu-cli case <project-eris> --case first-screen
  → 与改动前基线完全一致(ops 75 / skipped 0 / 同 sha);
    既有 FAIL 未新增,未改 golden。
```

未跑完的新增项:`case --all` 全量(基线已红,待官方证据后统一重生成)、
`coverage_census` 复跑(VM 未改,计数不受影响)。

---

## 11. 后续收口(2026-11-05,第 2/3 波)

| 项 | 结果 | 证据/用例 |
|---|---|---|
| DIV-GFX-041 子组 RT alpha | **fixed(no-manual)**:soft 离屏末 alpha=255;wgpu `OPAQUE_TEX` group 管线 | EVID-J-08;`subgroup_rt_alpha_is_opaque_*` |
| DIV-GFX-028 图像层子层 | **fixed(no-manual)**:后序扁平绘制 | `image_children_are_drawn_before_the_parent` |
| DIV-GFX-048 组 TID | **fixed(no-manual)**:`Op::Transition{from:Offscreen}` | `group_tid_emits_transition_from_offscreen` |
| DIV-GFX-016 CG 无 FILE | **fixed(no-manual)**:lookup 失败且无 FILE/RID ⇒ 返回 -1、不建节点 | REA `0x42a63d..0x42bbef` |
| DIV-GFX-031 ID3/ID4/Z2 | **fixed(no-manual)**:非 TRANSITION 官方不读取 ⇒ verbose 留痕 | REA `0x42c4ed..` |
| DIV-GFX-018 子窗口 | **fixed(no-manual;gfx 边界)**:输出状态/离屏,叠放归 host | 手册 window |
| DIV-GFX-037 WINDOW | **fixed(no-manual;gfx 读侧)**:字段读回;写侧 host 动作归 B7 | REA `0x4586ec` |
| DIV-GFX-009 颜色空间 | **fixed(no-manual)**:全树无 sRGB write | `render-parity §11` |
| DIV-GFX-010 清屏 | **fixed(no-manual)**:官方首屏黑 / D3D9 默认 | `fidelity-ledger` |
| DIV-GFX-064 浮点坐标 | **fixed(no-manual)**:f64 到顶点,行为对齐 | DIV-064 |
| DIV-GFX-063 无页滤镜 | **fixed(no-manual)**:MEIDO/RESIZE 已有用例;XOR/AND alpha 语料 0,保留 alpha 并登记 | DIV-063 |

### 仍未闭(明确 `open(evidence)` / 非 gfx 域)

| 项 | 需要的证据/归属 |
|---|---|
| DIV-GFX-042 半像素 | 官方 1px 边缘/四角截图;REA `x0=X-0.5` 已锚定,但无 oracle 前不做全局位移 |
| DIV-GFX-060 空 `FILE` 1×1 | 官方微场景中心像素(考虑 `+0x86`/纹理格式);当前 `[0,0,0,255]` 与手册/可见黑一致 |
| DIV-GFX-024 PSB/资源头 | PSB provider;E-mote 家族 `deferred(master)` |
| DIV-GFX-026 组/离屏 ONMOUSE | 后端已合成像素面 provider |
| DIV-GFX-038 INFO 缺失层非 EXIST | CGINFO 缺失分支 REA 返回语义 |
| DIV-GFX-072 §9 / 076 通用命令序 | 文本线会话 + 官方截图;本任务只做 §1-8 |
| DIV-GFX-056/057/075 | 文本线 H-P2-03 官方文本 oracle |

### 11.1 标题用例变化说明

`first-screen`(改动前已 FAIL):ops 仍 75 / skipped 0;`display_json_sha256`
从 `4b269c…` 变为 `1ed407…`。原因是本批两处**按官方修正**的可见状态:
- `DIV-GFX-016`:无 FILE/RID 且层不存在时不再建鬼层 ⇒ `layers` 表少了
  官方没有的节点;
- `DIV-GFX-041`:子组 RT alpha 置不透明 ⇒ 半透明组像素变化。
未改 golden;待 042/060 官方 oracle 到位后统一重生成并记录 SHA/像素统计。

---

## 12. 终态复核(2026-11-05 收尾)

### 已落入终态

`fixed` / `fixed(no-manual)` / `aligned-stub` / `unsupported-by-official`:
001/002/003/004(不可达,契约保留)/005/006/007(COPY/COPY2 + Tex* layer-loss)/
008/009/010/011/012/013/014/015/016/017/018/019/020/021/022(layer-loss)/
023/025/027/028/029/030/031/032/033/034/035/036/037/039/040/041/043/044/
045/046/047/048/049/050/051/052/053/054/055/058/059/061/062/063/064/065/
066/068/069/070/071/073/074/076(外字链)。
- `004` 多部件:运行期不可达,`Part` 契约保留;PSB provider 到位后再实装。
- `022`/`060`/`042`:见下。

### 明确保留为 `open(evidence)`(B8/其他会话)

| ID | 需要的证据 | 当前行为 |
|---|---|---|
| 042 半像素 | 官方 1px 边缘/四角截图 | REA 已锚定 `x0=X-0.5`;无 oracle 前不做全局位移 |
| 060 空 `FILE` 1×1 | 官方微场景中心像素(纹理格式/`+0x86`) | `SolidBlack1x1=[0,0,0,255]`,与手册“黑图”一致 |
| 024 PSB/资源头 | PSB provider;E-mote 家族 deferred | 未注入资产 `INFO_ASSET_SIZE` |
| 026 组/离屏 ONMOUSE | 后端像素面 provider | 图像层已实现;组返回 unsupported |
| 038 INFO 缺失层非 EXIST | CGINFO 缺失分支返回语义 | `INFO_MISSING`,不写 LET |
| 072 §9 / 076 通用命令序 | 文本线 + 官方截图 | §1-8 fixed;通用序列仍保守清文字 |
| 056/057/075 | 文本线 H-P2-03 官方文本 oracle | 行距/基线/页语义 Likely |

### 最终验证

```text
cargo build --workspace --all-targets        → exit 0
cargo test -p yu-gfx --all-targets           → 全绿
cargo test -p yu-backend-soft --test render  → 22 passed
cargo test -p yu-backend-wgpu --test wgpu    → 38 passed
probe_skips 600(soft)                    → 无 skip;结束帧无 Unimplemented
first-screen case                        → ops 75 / skipped 0;display sha 因
                                            016/041 的官方修正而变化,未改 golden
```

---

## 13. 推送前性能审阅(2026-11-05 追加)

- **<game-b> 轮询日志尖峰**:`CG[ID]` 无 FILE/RID 且层不存在时官方返回
  -1;脚本逐帧探测(`s66 pc245` 单帧 2 万次)。诊断改为按层生命周期
  一条 `Verbose`,层创建后复位。release 240 帧 `gfx.layer.missing`
  告警行 162 → 3,interval p50 16.66 → 16.64ms,max 149.9 → 116.7ms。
- **MID 后端一致性**:wgpu 在层声明尺寸未知时缺实际纹理宽高检查,会
  拉伸尺寸不等的 mask;补 `gfx.mask.size.mismatch`,与 soft 同跳过。
  新增 `mid_mask_size_mismatch_is_skipped_by_both_backends`。
- **debug 帧耗时**:`force_opaque` 展开为每 64 字节写 16 个 alpha,
  debug 240 帧 interval p50 33.23 → 16.86ms;release 不受影响。
- **夹具修正**:`yu-engine/tests/text_sync.rs` 的文本目标层改为显式
  `CG[FILE=""]`(官方 1×1 黑图建层),不再依赖已删除的 `CG` 鬼层行为。

验证:`cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
-p yu-engine` 全绿;`cargo test -p yu-rt` 带语料环境全 passed;
`cargo build --workspace --all-targets` exit 0。`case --all` 仍是既有
golden 红(0/9,ops/display sha 未重生成),未在无主人授权下改 golden。

**推送前红灯(需主人授权后处理)**:
- `cargo test --workspace --no-fail-fast`(带真实语料)仅 5 项失败:
  - `yu-gfx --test eris_first_screen` ×2:结构 golden 仍记 `layers=272`,
    实际 199(016 删除 73 个官方没有的无 FILE 鬼层;passes 82 / ops 85 /
    visible 8 均未变)。需授权后把 census 改为 199 并
    `UPDATE_GFX_GOLDEN=1` 重生成 display golden;
  - `yu-sound --test state` 2 项 `gated_ogg_*`:HEAD 既有,与 gfx 无关;
  - `yu-trace --test golden` 1 项 `synthetic_trace_matches_golden`:HEAD
    既有 synthetic ycd sha 差异。
- `yu-cli case --all`:0/9,全部为 display/ops golden 旧值(与审计 §11.1
  同源);未在无授权下改 golden。
