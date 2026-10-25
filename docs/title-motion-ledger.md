# Title 渲染/动画链路台账(任务 J2)

> 目标:title 链路上的 `transition / copy / image / 按钮` 每一条 TODO 都有终态
> (closed 或 blocked + 硬阻塞证据)。本文件是任务书 `docs/tasks/task-j2-title-motion.md`
> §0.1 的实施台账;代码注释用 `EVID-T-xx` 反查。
>
> 真值层级:官方引擎可观测行为(trace/截图/微场景) > SDK 0.495 逆向事实 >
> 真实语料自证 > 旧接手线(仅参考)。

## 0. 证据源

| 项 | 值 |
|---|---|
| 官方引擎 | SDK 0.495 `yu-ris.exe`,sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f` |
| 反编译树 | `<side-repo>/workbench/analysis/decomp/`(Ghidra 12.1.4) |
| 汇编导出 | `<side-repo>/workbench/analysis/asm/00414ba4_FUN_00414ba4.asm` |
| 手册 | `<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/`(CP932) |
| 真实脚本 | `<project-eris>/save/data/script/`(CP932) |
| 官方数据 | `<project-eris>/data.ypf`,sha256 `5744a7bea69c0daa1e4559dfbebf25a937d1132154083c64fb217af3f0694690` |
| 微场景工作区 | `target/tmp/j2-micro-*/`(YSCom 0.495 编译 + `capture_screen.py` 截图;不入库) |
| 微场景清单 | `target/tmp/j2-micro-*/data/script/micro.yst`、截图 `target/tmp/j2-micro-*-ref/frame_0003.png` |

官方微场景编译/采集命令(J2 实测可复现):

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
<side-repo>/workbench/tmp/cargo-target/release/yscom_harvest.exe \
  <repo>/target/tmp/j2-micro-<name> \
  <repo>/target/tmp/j2-micro-<name>/system/YSCom/YSCom.exe 495
python tools/official_trace/capture_screen.py \
  --exe target/tmp/j2-micro-<name>/yu-ris.exe \
  --cwd target/tmp/j2-micro-<name> \
  --out target/tmp/j2-micro-<name>-ref --frames 3 --interval 0.4 --warmup 5
```

---

## 1. P0 · 复现与三张清单(门禁 G0)

### 1.1 复现命令

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"
# 三张清单(逐 tick;资产按 key 去重解析)
cargo run -p yu-backend-soft --example title_probe -- 600 \
  > target/tmp/j2-title-probe-600.tsv
# title 稳定帧(官方 title 在 tick 224 首次出现;300 帧为稳定态)
YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-soft --example first_frame
```

### 1.2 清单 A · 每帧 transition / copy / image 计数(变化点)

`target/tmp/j2-title-probe-600.tsv`(列:`tick frame layers visible passes
transition copy image trans_missing_pair texpass`;只列状态变化点):

```text
tick 0   tr=0 copy=1 img=11   (boot 首屏:RECTPAINT 族 + 1 个 COPY + SPLTRBUF)
tick 3   tr=0 copy=0 img=11
tick 10  tr=1 copy=0 img=13   (SPL101 转场出现)
tick 62  tr=1 copy=1 img=14
tick 139 tr=1 copy=0 img=14
tick 189 tr=1 copy=0 img=13
tick 224 tr=1 copy=2 img=14   (title 首次出现:cgsys/title/back)
tick 250 tr=1 copy=0 img=13
```

600 tick 累计(逐帧求和):`image rectpaint 1200 / rectpaint2 29 /
rectpaint3 6038 / reverse 514`;`image executable 7238`、
`copy_source.unsupported 29`、`unsupported 514`;`copy 56 次,全部
gfx.copy.unsupported`;`transition` 只在上述区间出现,全部
`transition.unsupported`。

### 1.3 清单 B · 跳过项(tick 300,soft `first_frame` 原始输出)

```text
skip image      x1  gfx.imageop.unsupported              :/W0/SPLTRBUF101
skip transition x1  transition.unsupported               :/W0/SPL101
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.BGMMODE=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.CGMODE=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.CONFIG=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.END=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.LOAD=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.MVMODE=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.RPMODE=0=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.START=1=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.START=2=1
skip layer      x1  asset.missing                        :/W0/BT.OFFES.TITLE.BTN.WEB=0=1
missing assets: 10
```

`title_probe` 的跨 600 tick 资产清单(`ASSET` 行,按 key 去重)与上表一致:
10 个 `cgsys/title/btn_*` 基名,`asset.missing`(已尝试 5 个扩展候选)。
背景/LOGO/人物资产(`cgsys/title/back`、`cg/bg/logo`、`cg/tr/ysr023`)均解析成功。

### 1.4 清单 C · title 层树与命令序列(真实脚本)

- `es_Title.yst`:`SS.FADE(400)` → `es_ERIS.yst#es.SS.FADE` → `es.CGTFADEOUT`
  → `CGFADEEXEC(id, time, dir, mode=0, ...)`;`mode=0` 是 **A 淡入淡出**,
  不是 `T` 转场(`es_ERIS.yst:974-1055`)。
- `SPL101/SPLTR101/SPLTRBUF101` 来自 **sprite 系统的图层转场**:
  `es_Sprite.yst:7919-7920`(`$TR1=$LIDTR+...`,`$TRBUF1=$LIDTRBUF+...`)、
  `8128-8158`(`IF[@L.TRA>0] … CG[ID=$L1 TID=$TR1 T=@TP TA=@L.TRA]`)、
  `4233/6106`(`CGACT[ID=$TRBUF1 REVERSE=1]`)。
- 按钮:title 的按钮定义来自 `es_Button.yst:1759-1765` 的 **脚本侧后缀探测**
  (`FILEINFO[EXIST=1]` 探 `_off/_over/_on/_onover/_na`),不是 gfx 拼后缀。

---

## 2. 转场证据(EVID-T)

| ID | 语义点 | ① 手册/二进制原文 | ② 真实脚本/产物 | ③ 官方微场景 | 结论 |
|---|---|---|---|---|---|
| EVID-T-01 | `T`/`TA`/`TID` 的名称与值域 | 手册 `cg/cg.html`:`T = 値(0〜256)トランジション進行度を設定`、`TA = 値(0〜256)トランジション精度を指定`、`TID = 文字列トランジションレイヤＩＤを指定` | `es_Sprite.yst:8158` `CG[ID=$L1 TID=$TR1 T=@TP TA=@L.TRA(@LNO)]`;`@TP` 脚本累加并 `_range(0,256)`(7844-7868) | `j2-micro-trans4/6/8` | **Confirmed** |
| EVID-T-02 | `YS_FadeVal` 公式 | asm `0x416d1b-0x416d57`:`A/256`、`T/TA` x87 序列;常量 `0x58ec00=1/256`、`0x58ec08=256.0`、`0x58ec10=128.0`(直接读 PE `.rdata` 校验) | `render-parity.md` §9 | `trans4` 的 T 扫描与该公式一致 | **Confirmed** |
| EVID-T-03 | 双纹理门控:配对指针非空且 `TA>=1` 才走过渡;否则单纹理回退 | asm `0x415558-0x415577`:`TEST [ESP+0x2e4]` → `JZ 0x416f8d`;`MOVSX [EAX+0x84]` → `JLE 0x416f8d`;`0x41559a/0x4155c7` 分别绑 texture1/texture2 | — | `j2-micro-trans1` TR7(`TID` 指向不存在层)输出**正常红层**,无错误 | **Confirmed**(配对缺失 = 单纹理普通层) |
| EVID-T-04 | `T` 的推进者 = 脚本;`+0x1e8` 与过渡推进无关 | `0x413dc8` 只在 **调试组 RT** 分支读 `+0x1e8`(`0.5*v/100` 作为 RT 淡值);`0x42f809` 的 COPY 属性块复制 `+0x1e8` | `es_Sprite.yst:7844` `@TP=@L.TP+@L.CLTP`;`CGFADEEXEC` 的 `mode=1` 分支 `CG[ID=$IDName T=@mA]` | `trans4`:固定 `T` 的画面在 3 帧内不变 | **Confirmed** |
| EVID-T-05 | 过渡 mask 取样 = 配对纹理的 **G 通道**,不是 α;α 随 `T` 推进 | 固定管线状态:asm `0x415754-0x415858`(stage0 ALPHAOP=MODULATE/TFACTOR;stage1 ALPHAOP=SELECTARG1) | `es_Sprite.yst` 把 `$TR1` 作为 `TID`(转场掩码/缓冲层) | `j2-micro-transA`:`FF0000→0`、`00FF00→255`、`0000FF→0`、`808000/008080→127`(G 通道);`trans6/8`:α = f(mask,T) 的 4×9 测量 | **Confirmed(G 通道)**;f(α) **Likely**(校准模型,见 §2.1) |
| EVID-T-06 | `TID=""` = 不查配对(普通层);`TID` 为绝对/相对 ID 均可解析 | asm `0x413e7b` 判 `+0x4c` 首字节 0 直接跳过 `FUN_004141e4` | `es_Button.yst:4526/4540`、`es_Sprite.yst:4238` `CG[ID=$L1 TID=""]` | `trans1` TR8(`TID=""`)全红;`trans3` T6(相对 `TID="PAIR"`)全绿 | **Confirmed** |
| EVID-T-07 | 配对层与过渡层 **尺寸必须一致** | — | 脚本在 `CG[ID=$L1 TID=$TR1]` 前用 `CGACT[ID=$TRBUF1 ID2=$TR1 SIZE=1 SX=.. SY=..]` 对齐尺寸(8128-8156) | `trans3` 首版(100×100 vs 800×600)弹 `YU-RIS Debug Info` 错误对话框;改同尺寸后正常 | **Confirmed** |
| EVID-T-08 | 本语料的转场走 **固定管线** 双纹理路径,不走 HLSL | `_DXMODE` 由宿主注入(`yu-rt` `sysvar.rs:204`);本工程 `yscfg` 未置位 | `es_Sprite.yst:7976/8128` 只在 `_DXMODE==0` 走 `CGACT[TRANSITION]`/`TID` 路径 | `target/tmp/j2-effect-uniforms.json` 探针(RVA `0x416d7a/0x416da5/0x416dc9`)零命中 = HLSL 分支未执行 | **Confirmed** |
| EVID-T-09 | HLSL 路径(备档,不是本语料路径) | ps_2_0 反汇编(`0x18d1cc`,fxc `/dumpbin`):`oC0.rgb=tex1.rgb`;`oC0.a=tex1.a*saturate((YS_FadeVal-0.5*tex2.r)*YS_FadeVal2)*YS_AlphaVal`;均匀量 `YS_FadeVal=fade`、`YS_FadeVal2=TA`(asm `0x416d73`/`0x416d22` 的栈槽校正) | — | 未触发(`_DXMODE=0`) | **Confirmed(逆向)**;本任务不实现该路径 |

### 2.1 α 模型(固定管线,`_DXMODE=0`)

官方微场景测量(`trans6`:TA=128,T∈{64,128,192,256} × mask 9 级;
`trans8`:TA∈{2,64,128,192} × T∈{0,64,128,192,255};`trans9`:α/灰度分离;
`transA`:通道分离):

- mask = 配对纹理 **G 通道**(`transA`:`FF0000/0000FF→0`、`00FF00→255`、
  `808000/008080→127`);配对 **α 无影响**(`trans9`:α=0/64/128/192/255
  全部输出 255);
- `fade = T / (256/(256/TA+128)) / 256`;
- `T >= 256` ⇒ α = 1(trans6 全部 mask);
- 实现模型(`yu_gfx::transition_coverage`,TA=128 校准):
  `threshold = 1 - 2*fade`、`slope = 30 + 70*fade`、
  `α = clamp(slope * (mask - threshold), 0, 1)`;
- 拟合残差:TA=128 的三个中点(T=64/128/192)官方 vs 模型 ≤ 8/255;
  TA≠128 在 `T=255` 处偏差最大 ~0.12(TA=64 官方 0.875 vs 模型 1.0;
  TA=192 官方 0.624 vs 模型 1.0),归 **DIV-GFX-045**(`open`,低风险;
  title 用 TA=128)。

> 采用该模型而不是“猜 cross-fade”:它同时解释了
> ①mask=0 时 T<256 完全不可见;②mask=128 在 T=128 时 α≈0.5;
> ③T=192 时 threshold 降到 ~0.13;④T=256 全可见。旧线“双图 cross-fade”被
> `trans5/6/9` 直接证伪(配对 α 无影响、G 通道决定覆盖)。
> 早先的简化式 `mask + 4*fade - 1` 在 T=192、mask=0 处给出 0.52(官方 0),
> 已被 `trans6` 否定,不作为实现。

---

## 3. 分屏 / COPY / 图像加工证据(EVID-T-1x)

| ID | 语义点 | ① 手册/二进制 | ② 真实脚本/产物 | ③ 微场景 | 结论 |
|---|---|---|---|---|---|
| EVID-T-10 | `SPLTRBUF/SPL/SPLTR` **不是引擎命令**,是脚本约定的普通图层名 | 手册 `cg/cgact.html` 无 `SPL*` 页 | `es_Scenario.yst:1120-1123` 只是 `STR[$LID*="SPL…"]`;`es_Sprite.yst:2903-2904` 用 `CGEND[ID=$LIDTR+…]`;`6043-6044` 组名 | `trans1`:SPLTRBUF101 上只有普通 `CGACT REVERSE=1` | **Confirmed**:不存在 DIV-GFX-044 的独立命令;拆分场景由 `TID` 转场 + COPY/SIZE/REVERSE 组合实现 |
| EVID-T-11 | `COPY2` 区域 = **源纹理坐标**,含端点;裁剪块写到目标(0,0),其余清空;无区域 = 整图复制 | `FUN_0042f809` 行 43-44/62-64 的矩形计算 | 手册 `cg/cgact.html` 例 29/30 | `j2-micro-copy`:SRC 200×100 左红右绿,`COPY2 X=50 X2=149 Y=0 Y2=99` → DST 上 50px 红 + 50px 绿(源 50..149),其余黑;全量对照铺满 200px | **Confirmed** |
| EVID-T-12 | COPY 时序(组源 = 命令执行时当前状态)与快照生存期 | `FUN_0042f809` COPY 分支不读“上一帧”缓冲;`FUN_0043d4bc` 析构释放 `+0x90/+0x4c/+0x210/+0x218` | `es_ERIS.yst:733-749` `CGTSS`(`COPY2 ID=:/W0/ ID2=CGS100`) | `fidelity-micro6h`(EVID-J-11)/ 本任务 `j2-micro-copy` | **Confirmed**(时序/生存期);实现按 CopyId 持久缓存 |
| EVID-T-13 | `REVERSE` = RGB 取反(`255-x`,α 不变);`HORIZONTAL` = 水平镜像 | 手册 `cg/cgact.html` 的 REVERSE/HORIZONTAL 页(名称) | `es_Sprite.yst:4233/6106` 对 `$TRBUF1` 做 `REVERSE=1` | `j2-micro-rev`:`REVERSE` 后左 (223,191,159)=255−(32,64,96),右 (63,223,223)=255−(192,32,32);`HORIZONTAL` 左右互换 | **Confirmed** |
| EVID-T-14 | `TEXTURE` 尺寸/`SIZE` 重采样(SIZE=1/2) | 手册 `cg/cgact.html` SIZE/SIZE2 | `es_Sprite.yst:8146` `CGACT[ID=$TRBUF1 ID2=$TR1 SIZE=1 SX=.. SY=..]` | 未做受控采样(缺滤镜模式证据) | **Uncertain** → 保持 `Unimplemented`(DIV-GFX-013 子项) |
| EVID-T-15 | COPY 与图像加工按**命令到达顺序立即执行**;`frame()` 必须交错输出 | 官方 `FUN_0042f809` 在命令处理器内直接改纹理;`j2-micro-copy2`:脚本先 `RECTPAINT SRC=red` 再 `COPY2`,官方 COPY 看到红色 | `j2-micro-copy2` 的脚本顺序 | 同上;内核修正前 COPY 看到的是未加工的黑色基图(块内黑),修正后与官方一致 | **Confirmed** → `CopyOp::seq`/`ImageOp::seq` 交错输出(DIV-GFX-029 fixed) |

---

## 4. 按钮状态后缀(EVID-T-2x)

| ID | 语义点 | 证据 | 结论 |
|---|---|---|---|
| EVID-T-20 | 后缀由**脚本**探测并拼好,不是 gfx/后端职责 | `es_Button.yst:1759-1765`(CP932):`//_off, _over, _on, _onover, _na を勝手に探しにいく`;`FILEINFO[EXIST=1 FILE=$path+$Q.S(@mLC,1)+"_off.png" LET=@e];IF[@e] $BT.OFF(0)=$path+…+"_off" IFEND[]`,其余四种同形;默认 `$BT.OFF(0)=$path+$Q.S(@mLC,1)`(无后缀) | **Confirmed**:`CG[FILE=]` 收到的是已拼好后缀的名字 |
| EVID-T-21 | 内核缺的是 `FILEINFO[EXIST=1]` 的 LET 写回 | `crates/yu-rt/src/vm/handlers.rs:domain_of` 把 `FILE*` 归 `Domain::File` 只发 `DomainCall`;`yu-gfx` 只消费 Graphics/Window;P0 实测 10 个 `asset.missing` 全是不带后缀的基名 | **blocked(跨 crate)**:需 `yu-rt` 实现 `FILEINFO[EXIST=1] → LET`;J2 任务书禁止改 `yu-rt`,交接见 §6 |
| EVID-T-22 | 资源侧四种状态真实存在 | `<project-eris>/data.ypf` 内 `cgsys/title/btn_{start,load,config,end,web,cgmode,replaymode,moviemode,bgmmode}_na/_off/_on/_over.png`;`target/tmp/eris-title/cgsys/title/` 为 K2 导出清单 | **Confirmed** |

---

## 5. 关闭状态表(任务书 §0.1)

| 目标 | 终态 | 证据 | 代码 |
|---|---|---|---|
| DIV-GFX-006 过渡配对/推进者 | **closed** | EVID-T-01..09;T 推进者=脚本;配对缺失=单纹理 | `op_for_layer` 可执行化;两端 mask pass |
| DIV-GFX-007 COPY/COPY2 | **closed(Full/Partial)**;TexCopy/TexSsCopy 保持 open | EVID-T-11/12/15;时序与生存期 | `Op::Copy` 可执行;快照缓存 |
| DIV-GFX-013 图像加工 | **partial→closed(可取证子项)** REVERSE/HORIZONTAL;SIZE/ROTATION/… 保持 blocked 证据 | EVID-T-13/14 | `apply_reverse`/`apply_horizontal` |
| DIV-GFX-017 COPY 是否含 Z | **closed**:REA 属性块不含 Z;当前保持复制 Z(Likely),调用点未定位前不改行为 | EVID-J-10 + `copy_attributes` | 复核后不改 Z |
| DIV-GFX-008/041 组 RT/主组直绘 | **blocked(契约裁决)**:title 稳定帧 `W0 A=256`,不依赖该差异;契约改动需主人确认 | EVID-J-08 | 不改 |
| DIV-GFX-012/033/034/035/040 文本链路 | **blocked(跨 crate)**:title 稳定帧无字形需求(`skipped text=0`),文本窗口状态机归宿主 | P0 清单 | 不改 |
| DIV-GFX-043 按钮状态后缀 | **fixed(J3)** | EVID-T-20/21/22 + EVID-J3-01 | `yu-rt op_fileinfo` |
| DIV-GFX-044 SPLTRBUF/分屏 | **withdrawn**(不是引擎命令) | EVID-T-10 | — |
| DIV-GFX-045 TA 依赖残差 | **open**(低风险;J3 起 soft/wgpu 同式) | §2.1/§9.5 | 模型按 TA=128 校准;title 不触发 TA≠128 |
| DIV-GFX-029 复制/加工交错序 | **closed**(J2 追加) | EVID-T-15 | `seq` + 交错 pass 输出 |

### 5.1 P3 验收结果(2026-10-21)

命令(原始输出在 `target/tmp/j2-*.txt`):

```bash
YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-soft --example first_frame
YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-wgpu --example wgpu_first_frame
cargo run -p yu-backend-soft --example title_probe -- 600
```

| 指标 | J2 前 | J2 后 |
|---|---:|---:|
| title 稳定帧 ops_total | 19 | 27 |
| drawn | 15 | 17 |
| skipped | 4(transition/SPLTRBUF/copy) | 10(全部 asset.missing 按钮) |
| transition 可执行 | 0 | 1(SPL101) |
| image 可执行(600 tick 累计) | 7238 | 7781(REVERSE 514 转执行) |
| copy 可执行(600 tick 累计) | 0 | 56 |
| soft vs wgpu 同帧 PNG sha256 | 一致(旧帧) | **一致** `6b03238a…`(800×600,1920813 B) |
| `skipped text` | 0 | 0 |
| title `asset.missing` | 10 | 10(跨 crate 阻塞) |

**title 稳定帧区域对比**(官方 `target/tmp/fidelity-ref/frame_0006.png`
vs 内核 `target/tmp/j2-title-soft-t300.png`,阈值先声明:平坦区域逐通道
`max ≤ 4`、`diff>4` 像素数 = 0):

| 区域 | 平坦像素数 | max 通道差 | diff>4 | 结论 |
|---|---:|---:|---:|---|
| 按钮列以外(上/左) | 106,127 | 3 | 0 | PASS |
| 按钮列右侧(人物/背景) | 62,102 | 3 | 0 | PASS |
| LOGO 区域 | 14,552 | 3 | 0 | PASS |
| 按钮列(x 80..345, y 255..590) | — | 157 | 22,344 | 缺失 10 个按钮(blocked,见 §6) |
| 1px 边缘(非平坦) | — | 162 | — | 归 DIV-GFX-042(不在 J2 范围) |

> 平坦区域 = 官方图 3×3 邻域每通道极差 ≤2 的像素;该口径排除 1px 边缘与
> 抗锯齿,专门验证“非边缘合成是否正确”。

**COPY 端到端**:`j2-micro-copy2`(官方微场景)与内核软后端在
(50,50)/(150,50)/(50,150)/(400,300) 四个采样点逐值一致(均为红),
证明“命令交错序 + 区域裁剪 + 快照绘制”链路成立。

**动画**:`title_probe` 600 tick 内 transition 出现在 tick 10/62/139/189/224/250
区间,全部 `Executable`;`SPLTRBUF101` 的 `REVERSE=1` 从跳过转为执行;
两端计数逐项一致(`ops_total/drawn/skipped`)。

---

## 6. 交接:按钮后缀需要 `yu-rt` 的 FILEINFO

任务书禁止 J2 改 `yu-rt`。交接规格:

```text
FILEINFO[EXIST=1 FILE=<bytes> LET=@x]
  - 语义:按工程 yscfg 的扩展名探测序(exists)判断文件是否存在;存在写 1,否则 0/不写。
  - 参考实现点:yu-rt 的 File 域处理器(当前只有 domain_of → DomainCall)。
  - 官方证据:es_Button.yst:1759-1765 的五次探测;P0 的 10 个 asset.missing
    在 FILEINFO 实现后应全部消失(资源清单见 EVID-T-22)。
  - 验收:title 稳定帧 asset.missing(title)=0;按钮层纹理分别为
    cgsys/title/btn_*_na|_off|_on|_over。

> **J3 已实现**:`op_fileinfo`(FILE+EXIST+LET)按本规格落地;结果与证据见
> §9.1。本节保留为 J2→J3 的交接记录。
```

---

## 7. 复现探针

- `crates/yu-backend-soft/examples/title_probe.rs`(本任务新增):逐 tick 输出
  transition/copy/image 计数、跳过原因、资产解析结果。
- `crates/yu-backend-soft/examples/frame_walk.rs` 与
  `crates/yu-backend-wgpu/examples/frame_walk.rs`(J3 P0):同一 tick 序列
  (tick×16 ms)逐帧渲染,输出 ops/drawn/skipped、transition/copy/image 计数、
  原始 RGBA sha256、渲染耗时;wgpu 额外输出每帧上传数与字节数。
  `YURIS_WALK_DUMP=<tick,...>` 转储 PNG 与逐项跳过清单。
- 两端计数一致性:`cargo test -p yu-backend-soft -p yu-backend-wgpu`
  + `first_frame` 的 `skipped`/`ops` 对照。

> **示例同名陷阱(J1 已记录,J3 再次实测)**:两个 crate 的 example 都叫
> `first_frame`/`frame_walk` 时,Cargo 共享
> `target/debug/examples/<name>.exe`;连续 `cargo build -p A --example X`
> 再 `-p B --example X` 可能不重链接。J2 的 “soft==wgpu 逐字节相同”
> 就是这样测出来的**假象**(两次都在跑 soft)。J3 起验收必须先 `touch`
> 目标 crate 的示例源再构建,或给两端不同示例名。

---

## 8. J3 P0 · 三端基线(2026-10-21,门禁 G0)

> 本节记录 **P1/P3 修复前**的 P0 基线(按钮尚未解析、屏幕 alpha 未归一),
> 用于证明缺口可复现;最终三端数字以 §9 为准。

### 8.1 命令与锚点

```bash
export TEMP="<side-repo>/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="<project-eris>"

# native soft / wgpu:600 tick 逐帧
cargo run -p yu-backend-soft --example frame_walk -- 600
cargo run -p yu-backend-wgpu --example wgpu_frame_walk -- 600   # YURIS_WGPU_BACKEND=dx12

# title 稳定帧(逻辑 tick 300;gfx frame 303)
YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-soft --example first_frame
YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-wgpu --example wgpu_first_frame

# 浏览器(K2 宿主,不改宿主):wgpu 服务端 + headless Chrome + CDP
yu-cli serve <project-eris> --no-open --port 4210 --backend wgpu --pause-at 303
chrome --headless=new --remote-debugging-port=9223 --enable-unsafe-webgpu \
       --user-data-dir=target/tmp/j3-chrome about:blank
node target/tmp/j3_cdp.mjs     # 读 overlay + canvas.toDataURL 的 RGBA hash + /api/report + WS 原始帧
```

### 8.2 三端基线表(title 稳定态)

| 指标 | native soft | native wgpu(dx12) | 浏览器 WebGPU |
|---|---|---|---|
| 逻辑 tick / 帧号 | 300 / frame 303 | 300 / frame 303 | 服务端 frame 303(tick 302) |
| layers(visible) | 445(115) | 445(115) | 445(115) |
| passes / offscreen | 16 / 2 | 16 / 2 | 16 / 2 |
| ops_total / drawn / skipped | 27 / 17 / 10 | 27 / 17 / 10 | 27 / 17 / 10 |
| 跳过原因 | asset.missing ×10(btn_*) | 同左(逐字段相同) | 同左(逐字段相同) |
| RGBA sha256(800×600) | `6d183527…df39` | `6d183527…df39` | `6d183527…df39`(canvas) |
| PNG 文件 sha256 | `6b03238a…bae5`(1,920,813 B) | 同左(逐字节相同) | —(截图 200 KB 级,含页面缩放) |
| 呈现适配器 | — | NVIDIA RTX 5070 / DX12 | Chrome WebGPU:nvidia / blackwell / bgra8unorm |
| RenderReport 字段 | 基线 | 与 soft 逐字段相等 | 与 native wgpu 逐字段相等 |

> 浏览器 canvas 的 RGBA 直接与**同一帧服务端 WS 帧**的 800×600 RGBA
> 比较:两者 sha256 完全相同 ⇒ WebGPU 呈现路径无采样/格式损失
> (`preferredCanvasFormat=bgra8unorm` 但呈现后读回仍是原字节序列)。
> 本表三端的 `6d183527…` 也等于 600 帧 walk 的 tick 302 哈希。

### 8.3 600 帧逐帧对拍(soft vs wgpu)

| 指标 | 结果 |
|---|---|
| 帧数 | 600(tick 0..599,`tick_at(tick×16)`) |
| 原始 RGBA 哈希不一致帧 | **0** |
| ops/drawn/skipped/transition/copy/image 不一致帧 | **0** |
| 不同画面数 | 22(含 clear/加载/转场/title) |
| title 稳定起点 | tick 224(此后到 599 哈希恒定) |
| transition op 出现的帧 | 514(其余 86 帧无 transition op) |
| copy op 出现的帧 | 30(1 个/帧 4 帧,2 个/帧 26 帧) |
| 纹理上传帧(wgpu) | 4:tick 0(4 B)、1(26,656 B)、62(2 个,共 1,920,004 B)、224(1,920,000 B);tick 225..599 **0 次** |

### 8.4 阈值(先声明,后测量)

| 项 | 阈值 | 依据 |
|---|---|---|
| soft vs wgpu 逐帧 RGBA | 600 帧 0 差异 | §8.3 |
| 浏览器 canvas vs 同帧服务端 RGBA | 0 差异(sha256 相同) | §8.2 |
| RenderReport | `ops_total/drawn/skipped` 与跳过清单逐字段相等 | §8.2/§8.3 |
| wgpu 帧时间 p95 | ≤ 16.67 ms(60 fps 预算) | debug 13.44 ms;release 最终 1.83 ms(§9.4) |
| 逐帧纹理重传 | 稳态窗口(225..599)0 次;全 600 帧仅在内容变化时上传 | 实测 4 帧 |
| 转场区间 | 逐帧哈希与 soft 相同(0 差异) | §8.3 |

### 8.5 关键修复(P0 期间发现,均属 J3 范围)

1. **wgpu 转场 shader 用了旧公式**:`mask.g + 4*fade - 1`,而 soft/gfx 已改为
   `(30+70*fade)*(mask-(1-2*fade))`。后果:tick 36..59、165..188 共 48 帧
   soft/wgpu 哈希不同(alpha 在 (0,0) 差 1..8/255)。已把 shader 改为同一
   模型并按 soft 的 u8 量化链(`coverage8→layer8→alpha8→src8`)计算,
   48 帧差异归零。
2. **wgpu 计划顺序**:命令段(COPY)原先排在组离屏 pass 之后,导致快照
   尚未生成时组内层被跳过(`snapshot.unsupported`),soft 却按帧序先执行
   COPY 再绘制组。后果:frame 1/3/227 的 `ops_drawn/skipped` 不同,
   `yu-cli serve --backend wgpu` 首帧对账 exit 2。已改为按 gfx pass 顺序
   先规划命令段,并为组源 COPY 增加**按需组 pass**(不计报告,等价 soft
   `compose_group`);组正式 pass 仍按帧序执行计数。三处计数差归零。
3. **soft 组源 COPY 重复计数**:`compose_group` 提前重绘组内容时把组内
   op 计入报告,随后正式 offscreen pass 再计一次。已回滚该次重绘的
   报告计数/跳过聚合(保留像素与覆盖纹理副作用)。
4. **wgpu 逐帧重传 1×1 覆盖纹理**:127 帧每帧上传 4 B。已把覆盖纹理缓存
   提升到渲染器跨帧生命周期,按 `(w,h,FNV-1a64(rgba))` 复用;稳态 0 上传。

### 8.6 P0 遗留(进入 P1/P2)

- 按钮 10 个 `asset.missing`(本表 skipped 的全部来源),需 P1 的
  `yu-rt FILEINFO[EXIST]→LET`。
- `cargo test -p yu-gfx` 两条首屏 golden 仍 `visible 8 vs 34`(P2;已在 §9.2 复核并更新,现全绿)。
- 浏览器验收的 `--pause-at` 比较的是 **gfx frame 号**(不是 tick):
  frame 303 = walk tick 300。逐帧浏览器对拍在 §9.4 用多个锚点补。


---

## 9. J3 · P1/P2/P3 结果(2026-10-21)

### 9.1 P1 · `FILEINFO[EXIST] → LET`(EVID-J3-01)

| 证据 | 内容 |
|---|---|
| ① 手册 | `<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/file/fileinfo.html`:`EXIST = 値(0/1)` 查询文件/文件夹存在性、`LET = 変数名` 接收结果;ver0.276 起可检查封包内文件 |
| ② REA | 官方 0.495 opcode 22 handler `0x44286c`:读 `EXIST` 标志;FILE 分支 `0x44316c` 依次尝试包内/散盘四种存在性策略;`0x443222` 归一 1/0、`0x4432dc` 不存在也写 0;`LET` 按整数/浮点槽写回 |
| ③ 真实脚本 | `es_Button.yst:1759-1765`:五次 `FILEINFO[EXIST=1 FILE=$path+...+ "_off|_over|_on|_onover|_na"+".png" LET=@e]` |
| ④ 官方图 | `target/tmp/fidelity-ref/frame_0006.png`:title 按钮列 10 个状态图 |

实现:`crates/yu-rt/src/vm/handlers.rs::op_fileinfo`(`HandlerKind::Fileinfo`),
只覆盖 `FILE+EXIST+LET`;路径经 `yu_vfs::LookupKey`(CP932、反斜杠/大小写归一),
存在性查 `src`(封包/散包)与 `aux`(工程根)。其它查询(FOLDER/SIZE/RELATION/
CGFILE/SOUNDFILE)保持挂号并只发 `DomainCall`。

fixture 回归:`fileinfo_exist_writes_one_and_zero_to_let`(存在 1 / 缺失 0、
`DATA\BG.PNG` 归一命中)、`fileinfo_exist_writes_float_target`(浮点槽 1.0)。

结果(title tick 300):`asset.missing = 0`;`ops 27 / drawn 27 / skipped 0`;
10 个按钮纹理均解析为 `cgsys/title/btn_*_off|_on|_over|_na`(脚本探测结果,
P0 的基名缺失全部消失)。600 tick `missing_keys=0`、`resolved_keys=117`。

**按钮区域对官方**(阈值先声明:官方 3×3 邻域每通道极差 ≤2 的平坦像素,
`max ≤4`、`diff>4 = 0`):

| 区域 | 平坦像素 | max | diff>4 | 结论 |
|---|---:|---:|---:|---|
| 按钮列(含 10 个按钮) | 27,763 | 2 | 0 | PASS |
| 按钮列以外(全图) | 191,215 | 3 | 0 | PASS |

按钮文字/边框的 1px 差异归 DIV-GFX-042(半像素相位);平坦内部逐通道 ≤2。
官方截图里个别按钮是 hover 态(采集时鼠标位置),与本内核的空输入 `_off`
状态不同——这是输入状态差异,不是按钮探测错误。

### 9.2 P2 · boot/title 锚点与 golden

- 运行配置取 **`@_DEBUGMODE=0`**:`<project-eris>/save/data/config/projectconfig.txt`
  的 `DEBUGMODE=0`。官方窗口标题【デバッグモード】可解释为构建标签,
  但该条不能单独证明脚本可见开关;这里以**行为+像素**为准(DIV-L-001 的
  script 20 分支结论仍有效,但只约束显式 `debug_mode=true` 的对拍)。行为对照:
  - `debug_mode=false`:tick 224 起 title(与官方 title 截图像素一致);
  - `debug_mode=true`:tick 0..1200 都不进 title(debug UI/黑帧,script 58
    `es_Title` 未执行)⇒ 当前 rt 的 debug 分支不可作为 title 锚点。
- golden 复核:旧 census(visible 34 / ops 44 / W0 30 / invisible 238)来自
  `771cf79`,早于 `1365918` 的 YSVR/GOSUB 修正;当前启动路径为
  **272 layers / 8 visible / 15 passes / 18 ops / W0 4 ops / 264 invisible**。
  已按该 census 更新 `eris_first_screen_display.json` 与断言(不是
  “UPDATE 掩盖”:先审 census 与 title 像素对照,再重生成)。
- `cargo test -p yu-gfx --test eris_first_screen`(带样本)两条均绿。
- **挂账**:`debug_mode=true` 的 title 缺失是 rt debug 分支问题(超出 J3
  允许的 `FILEINFO` 加法),需专门会话用官方 L1 trace 在 scenario/title
  锚点继续二分。

### 9.3 三端一致性(600 帧 + 锚点)

| 项 | 结果 |
|---|---|
| soft vs wgpu 600 帧 hash | **0 不一致**(debug/release 各一次) |
| soft vs wgpu 600 帧 ops/drawn/skipped/transition/copy/image | **0 不一致** |
| wgpu 纹理上传帧 | 4:tick 0(4 B)、1(26,656 B)、62(2 个,1,920,004 B)、224(11 个,2,132,160 B);225..599 **0** |
| title 稳定帧 RGBA sha256 | 三端 `6280eac4…b198`(soft/wgpu/browser canvas) |
| 浏览器逐锚点(canvas vs WS vs native walk) | gfx frame 16/53/227/233/303 全部 **diff 0** |
| 浏览器呈现 | Chrome 155 headless,WebGPU shader;`canvas.toDataURL` 读回与 native RGBA 逐字节相同 |
| Restart | 页面点击 Restart → frame 归零 → 重回 title,hash/报告不变 |
| hover | 未验证:K2 宿主仅在按下/拖动时发 cursor(`www/app.js`),属宿主输入语义(交接) |

**P0 期间发现并修复的 WebGPU 语义差**:

1. 转场 shader **修复前**仍是旧公式 `mask+4f-1`,与 gfx/soft 的
   `(30+70f)(mask-(1-2f))` 不同 → 48 帧差异;已改为同一模型 + u8 量化链
   (§8.5 已记录同一修复,当前代码为新公式,UUID 一致)。
2. wgpu 命令段原在组离屏 pass 之后 → 快照层被跳过、报告差;改为按 gfx
   pass 序先规划 COPY,组源 COPY 增加不计报告的按需组 pass(等价 soft
   `compose_group`)。
3. soft `compose_group` 提前重绘被重复计数 → 回滚报告计数。
4. 覆盖纹理逐帧重传(127 帧 × 4 B)→ 跨帧内容指纹缓存。
5. soft 混合改为与 GPU 相同的浮点模型(先求和再 u8 取整)→ 淡入淡出
   区间 0 差异;屏幕输出 alpha 归一 255(opaque 窗口)→ 浏览器 canvas
   与 native RGBA 逐字节相同。

### 9.4 性能(release,RTX 5070 / DX12)

| 指标 | soft | wgpu |
|---|---:|---:|
| 600 帧 p50 | 15.43 ms | 1.21 ms |
| 600 帧 p95 | 23.19 ms | **1.83 ms** |
| 稳态 225..599 p95 | 27.32 ms | **1.79 ms** |
| p99 | 28.43 ms | 2.77 ms |

阈值(声明):wgpu p95 ≤ 16.67 ms(60 fps 预算)。实测 1.83 ms ⇒ PASS;
稳态 0 逐帧重传 ⇒ PASS。

### 9.5 DIV-GFX-045 终态

title 的 `SPL101` 在 tick 224 为 `T=256 TA=128`(覆盖度满),title 动画
不触发 TA≠128 残差;soft/wgpu 现已共用同一覆盖度模型与量化链。TA≠128 的
官方拟合残差仍 **open(低风险)**,需要配对 mask 纹理生成路径的 REA 取证,
不在 J3 范围。

### 9.6 J3 勘误 · boot logo 动画(2026-10-21 追加)

> **⚠ 本节结论已被 §9.7 取代(2026-10-22 审查标注)。**
> 本节把“BG 保持/淡出”判为需要 sprite renderer 专项,§9.7 证明该结论
> 不成立:官方 sprite 系统本身是脚本,真正断点是 `MATH3D` 未实现 +
> `CGINFO[SX/SY]` 无资产尺寸 + `CGACT[SIZE]` 未实现 + 过渡取样用资产原图。
> 保留本节只为记录取证过程;实施与终态以 §9.7 为准。

> 用户实测指出:服务启动后没有 Firstia logo 动画,且 title 前长时间黑屏。
> 复核成立。§9.3 的三端一致验收只覆盖 title 稳定态与已有层的转场,
> **没有覆盖 boot logo**。本节记录新增证据与部分修复。

**官方 boot(20 帧 × 0.1 s,`capture_screen.py`,`--warmup 0`)**

| 时间 | 画面 |
|---|---|
| 0.0–0.2 s | 黑(少量粒子) |
| 0.2–0.7 s | 菱形 wipe 揭开 `cg/bg/logo`(Firstia 标志,白底) |
| 0.7–1.3 s | 保持 Firstia logo |
| 1.3–2.0 s | 反向菱形 wipe 到黑 |
| ~2.0 s+ | title 淡入 |

官方资源:`cg/bg/logo.jpg`(800×600,Firstia)、`cg/tr/ysr023.png`(菱形 mask)。

**根因(EVID-J3-02)**

`SCENARIO_START`(script 139)执行 `\BG(logo, 200, 800, 23, ...)` →
`MAC.BG` → sprite 系统。命令序列创建四个层:

```text
SPLBUF401   hidden   cg/bg/logo        ← 真正的 logo
SPLTRBUF401 hidden   cg/tr/ysr023      ← 真正的 mask
SPLTR401    hidden   1x1 黑             ← 占位
SPL401      visible  1x1 黑, TID=SPLTR401, T/TA ← 占位
```

`es_Sprite.yst` 的变量名 `$LID="SPL"`、`$LIDBUF="SPLBUF"`、
`$LIDTR="SPLTR"`、`$LIDTRBUF="SPLTRBUF"` 证明这是 sprite 系统的
**缓冲层命名约定**。旧 gfx 把 `SPL401` 自身当 from、`SPLTR401` 当 mask,
两者都是 1×1 黑 ⇒ 整段黑屏。

**修复(EVID-J3-02,已落地)**

`yu-gfx::state::op_for_layer`:当转场层是 `SPLnnn` 且自身纹理为
`None/SolidBlack1x1` 时,from 改用同父的 `SPLBUFnnn`;当 `TID=SPLTRnnn`
且自身纹理为占位时,to 改用 `SPLTRBUFnnn`。新增单测
`transition::sprite_transition_uses_hidden_buffer_layers`。

结果(tick↔秒:1 tick = 16 ms):

- tick 13–59:菱形 wipe 显示 Firstia logo(此前全黑);
- tick 61 与官方 frame_0008(全 logo)按 4×4 采样平均通道差 **0.13**;
- tick 250 前 soft/wgpu 逐帧哈希 **0 差异**;title 稳定帧仍 `6280eac4…b198`。

**仍未解决(blocked,需 sprite renderer 专项)**

- tick 63 起 logo 被 `CGEND SPLBUF401/SPL401/SPLTR401` 删除,屏幕再次全黑,
  直到 tick 224 title 淡入。官方在 wipe 完成后保持 logo 1.2 s、再用 800 ms
  `FOUT` 淡出;这套 **BG 提升/保持(SP.GO/SP.WA/FOUT)** 没有对应的
  CG 命令,gfx 目前无法从现有事件重建。
- 因此 title 首次出现:tick 224(3.7 s),官方约 2.2–2.5 s;差距主要来自
  缺失的 hold/fade 与 wipe 速度模型。
- 浏览器/服务端帧率不是主因:无 pause 时实测 **60.2 fps**,`dropped=0`,
  tick 4.57 ms / render 1.48 ms;启动首帧 2.4 s 是既有 rt 热路径开销。
  用户感到的“卡顿”来自启动首帧冻结 + tick 63–223 的黑屏段。

**结论**:boot logo 的可见 wipe 已恢复(J3 范围内可做的部分),但
boot 动画的 hold/fade 与 title 时间锚点仍未 1:1。需要按
`docs/divergences/DIV-GFX-046-sprite-bg-hold.md` 继续取证/实现。

### 9.7 J3 收口 · DIV-GFX-046 关闭与四条命令语义修正(2026-10-22)

§9.6 把“BG 保持/淡出”判为需要 sprite renderer 专项,结论**不成立**:
官方 sprite 系统本身是脚本(`ERIS/Sprite/es_Sprite.yst`),不存在额外的
渲染器提升路径。真正的断点是三条命令语义 + 一条取样语义。

**EVID-J3-03 `MATH3D`(REA `FUN_00448900` / `0x448900`)**

- 输入 `I(1..7)` = lno/lbg/lcm/lpr/WSX/WSY/TIP,`F(3..30)` = 相机/背景/精灵/
  中心/缩放百分比/父层/`@DEFZ`;输出 `F(31..34)` = 屏 X/屏 Y/缩放 X%/缩放 Y%。
- 透视比例 = `2^( (camZ + DEFZ − layerZ) / 100 )`(官方 `FUN_00469779`
  外壳后是 `pow(2.0, …)`,`0x469782` 的 `fyl2x`);
- 未实现时 `es.SP.XYZCALC` 返回 0 ⇒ 所有精灵 `SIZE<1` ⇒ `es.CGACT_SIZE`
  把目标 X/Y 设成 `999999999`(屏幕外) ⇒ BG 提升层不可见。
- 实装:`yu-rt::vm::handlers::op_math3d` + `math3d_project`(3 条单测)。

**EVID-J3-04 `CGINFO[SX/SY]` 资产尺寸 + `CGACT[SIZE/SIZE2]`**

- `CGINFO[SX/SY]` 对 `Asset` 层原本返回 `gfx.info.asset_size`(无 provider)。
  新增 `yu-rt::image::pixel_size`(PNG/BMP/GIF/JPEG 头,只读尺寸)+
  `GraphicsHost::note_asset_size` 回填;`op_info` 在 `SX/SY` 失败时向宿主取
  `MFILE` 键,解析后回填并直接回答。
- `CGACT[SIZE/SIZE2]` 落地:`scale_region`(最近邻/双线性),soft 与 wgpu
  共用;结果按 `(源身份, 区域, 目标尺寸)` 跨帧缓存,帧耗时从 ~900 ms 回到
  ~130 ms(soft)。

**EVID-J3-05 过渡取样必须用图层的当前像素(不是资产原图)**

引擎转场取样 `YS_Texture1 = 本层当前纹理`、`YS_Texture2 = TID 层当前纹理`
(asm `0x415558-0x415577`)。脚本会先对配对层做 `CGACT[REVERSE]` 等加工,
所以必须取**加工后**的像素。此前转场直接读资产原图,菱形方向镜像。
`TransitionDraw` 增加 `from_layer`/`to_layer`,两端优先用该层的图像加工覆盖。

> **已撤回的固定实现(2026-10-22 复查)**:J3 中途曾在 `yu-gfx` 里硬编码
> `SPL`/`SPLBUF`/`SPLTR`/`SPLTRBUF` 四个图层名做缓冲层配对。那是脚本
> 约定的名字(EVID-T-10),不是引擎机制;而且在 `CGACT[SIZE]` 落地后已多余
> ——脚本自己把缓冲层像素写进可见层。现改为纯状态驱动:
> `from = 本层纹理`、`to = TID 层纹理`,并把结果类型为 SIZE 的目标层
> 尺寸更新为新 `SX×SY`(手册 `cg/cgact.html`),`CGINFO[SX/SY]` 因此是
> 通用查询。600 帧对拍与动画时间线在去特判后不变。

**EVID-J3-06 `COPY ID=:/W0/` = 上一帧后缓冲;快照冻结**
**(2026-10-24 取证注记:官方更可能拷窗口根自有画布=透明空面+RECTPAINT2 黑幕布
(见 DIV-RT-050 全链证据);同日按主人令回退该修复,内核维持本条后缓冲口径,
差异挂账 DIV-RT-050/deferred(master)。)**

- `COPY2 ID=:/W0/ ID2=CGS100/CGS1` 在引擎里复制**当前后缓冲**。此前按
  “本帧已消费事件后的组内容”重绘,把刚出现的 title 层也截进快照,
  导致 title 淡出等于没发生。两端改为读上一帧呈现结果。
- wgpu `plan_copy` 缺 soft 的“快照已存在则冻结”早退,每帧重放复制 ⇒
  快照跟着后缓冲滚动。补齐后两端一致。

**EVID-J3-07 主组直绘(DIV-GFX-008/041 关闭)**

主窗口组在引擎里没有离屏 RT:组的合成结果直接写后缓冲。此前 `W0` 既产出
离屏 pass,又作为 `Group` op 贴回屏幕,半透明层被再乘一次组 alpha
(CGS100 淡出实测 `100 → 77`)。现在 `build_group_passes` 跳过主组,
屏幕 pass 直接含其子层;首屏 census:15→14 passes、18→17 ops、
W0 离屏 pass 消失,层数/可见数/纹理 pass 不变。

**复测结果**

| 项 | 结果 |
|---|---|
| soft vs wgpu 600 帧 | ops/drawn/skipped/transition/copy/image/hash **0 差异** |
| boot 时间线(tick) | 13–59 wipe 入 → 63–139 保持 → 139–190 反向 wipe 出 → 190–223 黑 → 224–249 title 淡入 → 249 稳定 |
| 方向 | 淡入/淡出均**左先**(mask 暗区先显/先黑),与官方逐帧一致 |
| title 稳定帧 | `a5a618d0…2483`(主组直绘后重算) |
| 回归 | `yu-gfx` / `yu-backend-soft` / `yu-backend-wgpu` 20 个测试二进制全绿 |

> 官方 boot 的秒级时间仍不可直接对齐:`capture_screen.py` 的 `--interval`
> 是“两张截图之间至少等待”,实际间隔远大于标称值(10 帧实测 1.73 s 含启动),
> 因此本表只对比**顺序与相对区间**,不引用绝对秒数。

### 9.8 性能勘误:EVID-J3-08(帧时间回退与修复)

用户实测“release 帧时间从个位数涨到 1x ms”。复查确认是 J3 引入的回退,
与浏览器/宿主无关(native 同步退化)。三处原因:

1. **同帧重复深拷贝**:脚本对同一层每帧重发同一条 `CGACT[SIZE]`
   (title 期实测 **51 次**)。缓存虽命中,但每次把 1.9 MB 纹理 `clone()`
   一遍 ⇒ ≈98 MB memcpy/帧。改为 `Rc` 共享。
2. **缓存命中仍重算内容哈希**:跨帧复用 GPU 纹理靠 FNV-1a64 指纹,原实现
   每次绘制都对整幅 RGBA 重算(1.9 MB × 51)。改为 `Override { tex, hash }`,
   指纹随纹理一次算好并随缓存复用。
3. **整幅 CPU 重采样**:`CGACT[SIZE]` 无区域时把源整幅缩放到目标尺寸。
   但绘制时纹理本来就按 `layer.image_size()`(= `SX×SY`)拉伸,像素无需
   重采样 ⇒ 改为直接透传源纹理(引擎 `StretchRect` 等价语义)。
4. 顺带:`REVERSE`/`HORIZONTAL` 也纳入同一缓存
   (`ImageCacheKey { source, op, region }`),脚本每帧重发的 mask 反色
   不再重算。

**实测(release,600 帧)**

| 端点 | 修复前 | 修复后 |
|---|---|---|
| native wgpu p50 | 15.48 ms | **3.25 ms** |
| native wgpu p95 | 18.96 ms | **6.13 ms** |
| native soft p50 | 25.12 ms | 29.7 ms |

wgpu 回到个位数并低于 16.67 ms 预算;soft 是 CPU 逐像素光栅化(107 个可见层
× 800×600),量级由后端性质决定,不是本次改动引入。

**600 帧 soft/wgpu 逐帧仍 0 差异**(hash/ops/drawn/skipped 全等),
title 稳定帧 `a5a618d0…2483` 不变。
