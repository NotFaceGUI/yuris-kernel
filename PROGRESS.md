# PROGRESS · 进度真相

> 唯一进度真相。每项改动追加"成果 N"(日期、内容、证据、验证命令与原始结果)。
> 勘误必须留痕: 旧结论、更正内容、发现方式。

## 结论汇总(2026-10-20 按实际落地重写)

| 子系统 | 状态 | oracle | 备注 |
|---|---|---|---|
| yu-log | **已实现** | 结构化码/JSONL 往返 | 成果 A |
| yu-vfs | **已实现** | 等价性 + 真包索引闭合 | 成果 A;两种方言,一种待真样本 |
| yu-format | **已实现** | 真实 ERIS/SDK 语料 + 字节一致 | 成果 B1/B2、E3 |
| yu-rt | **已实现** | 事件流/任务/帧边界 + 真语料 | 成果 C/E2;脚本/VM/任务/`InputTimeline` 均在此 |
| yu-gfx | **已实现(语义层)** | `CompositorFrame` + ERIS 首屏 golden | M3-GFX/F1/F2;COPY/图像加工/部分 MODE 挂账 |
| yu-font | **已实现** | 字形/样式 golden + ERIS 配置直出 | 成果 G1 |
| yu-backend-soft | **已实现(确定性参考)** | 同帧 PNG 逐字节确定 + 首帧已产出 | 成果 M3-SOFT;可执行子集,skipped 可审计 |
| yu-trace | **已实现(schema v3)** | 内核自回放 + 官方屏幕级 producer | 成果 H1;官方事件级 L1 未达成 |
| yu-backend-wgpu | 未开始 | soft 同帧逐像素 diff | 语义稳定后开工 |
| 桌面 / wasm / Android 宿主 | 未开始 | 同帧呈现 + 输入时间线 | wasm 仍是 M3 最终呈现目标 |
| yu-sound / yu-save | 未开始 | 各域命令 / 字节一致 | 尚无 crate |
| scenario 解释器 / yu-cli | 未开始 | 同文本同锚点 | CLI 是否保留待定 |

> **勘误(2026-10-20,架构与实现对账)**:本表替换 2026-10-19 的旧汇总。
> 旧结论是 12/9-crate 设计,列有 `yu-script`/`yu-macro`/`yu-vm`/`yu-graphics`/
> `yu-input`/`yu-player` 等;实际落地把脚本/VM/任务/帧/输入时间线收敛进
> `yu-rt`,渲染语义独立为 `yu-gfx`,字体独立为 `yu-font`,软后端为
> `yu-backend-soft`,**没有 `yu-assets` 兄弟 crate**(纹理解析在后端/装配层)。
> 旧表述“M3 = wasm 呈现闭环”保留为最终目标,但 M3 已拆成
> M3a 语义层 → M3b 软后端首帧 → M3c wgpu 对拍 → M3d wasm 宿主闭环。
> 发现方式 = `cargo metadata` + `git log` + 实际首帧产物;历史成果 1–7 不回改。
> `docs/architecture.md` 仍是 v1.2 设计稿,与本节冲突时以本节与代码为准。

## 架构现状(2026-10-20)

```text
yu-vfs → yu-format → yu-rt(脚本/VM/任务/帧/InputTimeline)
                         │ VmEvent::DomainCall / Text
                         ▼
                  yu-gfx(语义显示列表 CompositorFrame + GfxHost)
                         │
             ┌───────────┴───────────┐
             ▼                       ▼
   yu-backend-soft             yu-backend-wgpu(未开始)
   确定性参考 / PNG / golden    主后端,同帧逐像素对拍
             │                       │
             └───────────┬───────────┘
                         ▼
              hosts: 桌面 / wasm / Android(未开始)
yu-font: 字体发现/栅格/样式,供 gfx 文本与后端使用
yu-trace: vfs/vm/frame 边界录制、回放、跨 producer diff
```

- 纹理解析不在独立 crate:第一个实现是 `yu-backend-soft::TextureStore`
  (`yu-vfs` 取字节 + `frame.image_probe_order` + image 解码 + 缓存)。
- 文字链路: `yu-font` 已独立完成,尚未桥接进 `yu-gfx`/后端(DIV-GFX-012)。

## 成果 SND-LAYER-TREE (2026-10-29) · 游戏设置音量链对齐:声音层路径树 + 祖先音量继承

### 目标与证据

设置画面的主音量/频道音量滑杆在脚本里写成 `SOUND[ID=":/S0/" …]`、
`SOUND[ID="BGM/" …]`,最终声音是 `SOUND[ID="BGM/A1" …]`。官方 0.495 的
声音层是**名字树**(不是扁平表),音量按祖先链继承:

- ID 规范化 `FUN_004141e4`:前缀 `"S0"`(`.data:0x80bf54` → `[0x801718]`,
  启动 `0x462592..0x4625a6`);字符表 `0x689b80` **运行时实测**只有
  `'.'`/`'/'`/`':'` 非零;`:/` 绝对写法、`//`/`./`/`../` 折叠、末尾 `/`、
  空格 ⇒ -1。调用点 `0x44e235`(SOUND)/`0x44f500`(SOUNDINFO)/
  `0x44f346`(SOUNDEND)。
- 层树 `FUN_00414559` + `FUN_0041478c`(MurmurHash2,只查不建);节点
  `+0x10` 父 / `+0x00` 首子 / `+0x0c` 兄弟 / `+0x30` 容器 / `+0x34` 声音对象。
- 建节点规则 `0x44ef31..0x44f245`:带 FILE 建叶子(`+0x30=0`,`0x44f1fd`)、
  末尾 `/` 建容器(`+0x30=1`,`0x44f08e`)、带 FILE+末尾 `/` ⇒ 错误
  `0x18bac`、其余整条丢弃;父链必须已存在(`0x18786`,不补建中间节点)。
- 有效音量 `FUN_00424dfc`:`v=(E==1)*VOLUME; v=v*parent_eff>>8`(每级截断);
  容器改音量 `FUN_004238e0` → `FUN_00424db8` 递归重写后代。
- `SOUNDINFO[VOLUME]` 读 `+0x148`(自身字段,`0x44fb87`)。
- `SOUNDEND 0x44f308` 不删节点:type 0xe(`0x4253b8`)释放声音对象,
  保留元数据/TOTALTIME/位置/PAN/PITCH/LOOP/LOOPCOUNT/E/VOLUME;末尾 `/`
  时递归停止叶子后代(`0x44f388`→`0x44f3a4`)。

运行时佐证:官方引擎跑 ERIS 后 `ReadProcessMemory` 层树 =
`root → S0(容器) → BGM(容器) → B1(叶子)` + 15 条频道容器;
`prefix="S0"`,字符表非零项 `{'.','/',':'}`;`BGM/` 与 `:/S0/` 均未产生
多余空名/`":"`节点。脚本侧链路见 `ERIS/Sound/es_Sound.yst`
(`es.SOUND.SETVOL`/`es.VOL.*.GET`)与 `ERIS/es_GSD.yst`(`es.GSD.REFLECT`)。

### 改动(未提交工作区)

1. **`crates/yu-sound/src/path.rs`(新)**:`normalize`(快速/慢路径、绝对
   ID、`./`/`../`、末尾 `/`、空格拒绝、`trailing_slash`)。
2. **`crates/yu-sound/src/state.rs`**:`Layer.container`;`SoundState::new`
   建根层 `S0`;`apply_sound` 走规范化键 + 官方建节点规则
   (`parent_exists`);`query`/`layer` 同样规范化;`effective_for_key`
   逐级 `>>8`;`apply_soundend` 实现 type 0xe 释放语义与带 `/` 递归;
   `snapshot` 增 `effective_volume`/`container`。
3. **`crates/yu-sound/src/mixer.rs`**:每块按 `effective_for_key` 取增益
   (`volume_gain` 仍是 `(v/256)^2`,REA `0x424e4c`)。
4. **`crates/yu-sound/src/host.rs`**:`SOUNDEND` → `apply_soundend`。
5. **`crates/yu-cli/examples/sound_census.rs`**:输出 `eff=`/`container=`。
6. 台账:`docs/game-settings-ledger.md`(新)、`docs/divergences/DIV-SND-005-
   sound-layer-tree.md`(新)、`docs/sound-ledger.md` N0-14 + §1.2。

### 验证命令与原始结果

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test -p yu-sound
# → test result: ok. 51 passed; 0 failed
cargo run -p yu-cli --example sound_census -- 300
# → frames=300 layers=17
#   id=S0/BGM ... container=true
#   id=S0/BGM/B1 file=bgm/bgm01.ogg state=Playing time=1200 total=90802 ... eff=256 container=false
#   id=S0/SE / S0/VO / S0/SSE / S0/SVO ... container=true
#   snd_diags=0
python tools/rea/sound_tree_probe.py 6
# → char_table@689b80: [(46,1),(47,1),(58,1)]  prefix: b'S0'
#   root → S0(flag=01) → BGM(flag=01) → B1(flag=00) + 15 条频道容器
```

`layers=17` = 根 `S0` + 15 条频道容器 + 正在播的 `BGM/B1`,与官方运行时
层树同形(官方另有引擎内部 `ROOT` 层,不经 SOUND 命令创建)。

### 用例基线:既有环境漂移(非本轮回归)

`yu-cli case --all` 9 条里 8 条 FAIL,全部是**改动前既有**的哈希漂移:

- 现象:每个用例前都有 `fontdb: Failed to load a font face 0 from
  'C:\Windows\Fonts\mstmc.ttf' cause malformed font`。
- A/B 取证:临时 `git stash` 掉 `crates/yu-sound` 与 census 示例后重跑
  `record-events-off` / `symptom-ingame-config`,actual 哈希与本轮
  **逐字节相同**(`f2d2fbbc…` / `b4bba9b3…`);`save-load-loop`、
  `symptom-enter-story` 的 actual 也与本轮相同(`e264bf3b…` /
  `83e0e765…`)。
- 结论:失败源于本机字体环境与 golden 记录环境不一致,与本轮声音改动
  无关;golden 未更新(避免把坏字体环境固化进基线)。
- 同类既有漂移:`cargo test --workspace` 仅 `yu-trace
  synthetic_trace_matches_golden` 失败(项目 `system/YSCom/YSCom.ycd`
  sha256 `997b5b63…` vs golden `39ba92ef…`);stash 掉本轮改动后同样失败,
  属工程数据漂移,非本轮回归。其余 63 个测试二进制全绿。

### 后续挂账

- `WINDOW[NO=0 Z=]` 的宿主落地(常に手前 ⇒ `SetWindowPos`);内核只存
  字段(`WindowState.z`),桌面宿主未应用。
- `SYSTEM[BACKGROUNDEXEC=]/[SCREENSAVER=]` 维持宿主策略 aligned-stub。
- `yscfg.dat` 其余 31 项未逐项映射(第 10 项 `_WINDOW_INIT_POS` 已映射)。
- 空格 ID 拒绝的调用方 flag 为栈残值,按 `Likely` 处理(语料无含空格 ID)。

## 成果 SYSVAR-WRITE (2026-10-29) · 可赋值输入系统变量写入落地(`_INP`/`_KEYLAST`/`_MOUSELAST`)

### 现象与判定

桌面点击/推进时每次打印
`UNIMP [vm] rt.sysvar.write ... 写入系统变量 _INP ... 本次写入被忽略(DIV-RT-007)`。
**这是误报**:`_INP` 的写入语义早已取证,只是写处理器漏了路由(原语
`InputState::set_counter_code` 已存在且带单测)。

### 证据(0.495 REA,写分派)

- `_INP`(case 0x90 → `0x446efc` → `0x447014`):下标 `n>0` 时按物理按下
  状态钳制写**同一张** VK 计数表(`DAT_00685800[n*8]`,与 `_KEY_*` 读同址);
  `n<=0` 忽略。
- `_KEYLAST`(0x91 → `0x446f52`)/ `_MOUSELAST`(0x92 → `0x446f3c`):
  RHS 直接写 `DAT_00801908`/`0x80190c`,无钳制。
- 语料:`es_Button.yst:3417` 每次按下写 `_INP(...)=2`(消费按下);
  `es_Sprite.yst:851..854` 写 `=11`(连发限速);`es_Text.yst:1287`、
  `es_TXExec.yst:9/119` 写 `_KEYLAST=0`/`_MOUSELAST=0`(取消 skip)。

### 改动

- `crates/yu-rt/src/input.rs`:`InputState::set_last(key, value)`。
- `crates/yu-rt/src/vm/handlers.rs` 系统变量写路径新增 `_INP`(按
  `lval.indices` 取码 → `set_counter_code`)与 `_KEYLAST`/`_MOUSELAST`
  (→ `set_last`),不再落 `rt.sysvar.write` 挂号。
- 文档:`crates/yu-rt/docs/divergences/README.md` DIV-RT-007 增补收口记录。

### 验证

```bash
cargo test -p yu-rt --test fixture inp_keylast_mouselast_writes_are_applied
# → 1 passed(修复前:counter 1≠2,写入被丢弃)

YURIS_SAMPLE_ERIS_ROOT=<project-eris>   cargo test -p yu-cli --test sysvar_write -- --ignored --nocapture
# → 1 passed;A/B:修复前同一用例报 7 条 _INP/_KEYLAST/_MOUSELAST 挂号,修复后 0
```

## 成果 SETTINGS-PERSIST (2026-10-29) · 设置读取/存储闭环与 GSD 全量目录

### 取证

- 设置存储 = 官方 `save/*.sd`(YSSD v0x1E0):`gsd.sd`(全局设置)、
  `kidoku.sd`(既読)、`extra.sd`(解锁)、`seld.sd`(直前选择)、
  `saveNNNNN.sd`(游戏存档)。
- 读:`es_GSD.yst:154..165` `FILEINFO[EXIST]` + `LOAD[FILE="gsd.sd"
  DNO=01 LET=@es.GSD()]` / `DNO=02 LET=$es.GSD()` / `DNO=03 G_INT2=1` /
  `DNO=04 G_STR2=1`;启动序 `es_First.yst:139 GSD.INIT → :201 SOUND.INIT
  (SETVOL) → :237 GSD.REFLECT`。
- 写:`es_GSD.yst:558..585` `\GSD.SAVE` → `SAVE[DNO=01 SET=@es.GSD()]` /
  `DNO=02 SET=$es.GSD()` / `DNO=03 G_INT2=1` / `DNO=04 G_STR2=1` +
  `SAVE[FILE="gsd.sd"]`;`ES.CONFIG.END` 与标题开始都会触发。
- 设置目录:`es_GSD.yst:1..130` 的逐项注释(音量矩阵 001..010、功能开关
  110/111/112、字体 113、确认框 114、文本/演出 115/116/120/121/125、
  E-mote 131、运行期 141/150/151、存档槽 092/093/151)。全量映射见
  `docs/game-settings-ledger.md` §2.1。

### 验证(官方 gsd.sd 端到端)

新增 `crates/yu-cli/tests/settings_persist.rs`(两个 `#[ignore]`,需语料):

```bash
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test -p yu-cli --test settings_persist -- --ignored --nocapture
# → gsd_master_volume_is_loaded_and_applied ... ok
#   gsd_settings_survive_resave ... ok
```

1. **读 + 应用**:把官方 `gsd.sd` 的 `GSD(001,01)` 改成 128 → 启动到标题 →
   声音层 `S0/BGM/B1` 有效音量 = **128**(自身字段仍 256)。
2. **写 + 保留**:同文件跑「标题→スタート」触发 `\GSD.SAVE` → 重写后的
   `gsd.sd` 仍是 4 块,DNO/类型/变量 id/维数逐块与官方一致;主音量 128
   原样保留;与官方文件的差异只落在运行期字段(141,01 启动回数、
   141,03 累计时间、101,01..03 窗口尺寸、113,01 字体号)。
- 块格式层:官方 7 个 `.sd` parse→rebuild 逐字节一致(`yu-format`
  `tests/yssd_roundtrip.rs`,既有)。

### 发现的残余差异(已记档,非本轮改动)

- `101,01..03`:headless 内核按工程视口 800×600 回写,官方按真实窗口
  (2560×1369 最大化)回写 ⇒ 宿主窗口尺寸注入,不是存储格式问题。
- `113,01` 字体号:内核 fontdb 枚举顺序/过滤与官方 GDI
  `EnumFontFamiliesExA` 不一致,默认字体下标 0 vs 1(本机伴随
  `mstmc.ttf malformed` 告警)⇒ 字体域枚举保真待查。

## 成果 SURFACE-RESIDENT (2026-10-28) · wgpu 帧契约改常驻图层表面(不再逐 op 重放)

### 目标与证据

官方 `CGACT` 在命令处理器内**就地**改目标表面(`FUN_0042f809` 尾部
`*(iVar4+0x90) = local_68`,`crates/yu-gfx/src/state.rs:3440`);`COPY` 直接
读写纹理(`DIV-GFX-051`);存档直接读层表面(`0x4477c0`,
`crates/yu-gfx/src/host.rs:37`)。旧 wgpu 每帧重放整条命令日志并在 CPU 上
重新合成覆盖纹理(实测 `<game-b>` 1200 帧:image p95 15–35ms、上传平均
8.8MB/帧/重帧 115MB、快照整帧拷贝 2×35MB)。

### 改动(全部在本轮未提交工作区)

1. **`yu-render/wgpu/src/surfaces.rs`(新)**:`LayerSurfaces` 常驻表面表
   (`Content{tex,uv,width,height,display,bilinear,hash}`)+ 纹理池 + 预算/剪枝
   + `SurfaceStats`。大表面(>4MB)不入池;池上限 64MB。
2. **`yu-render/wgpu/src/plan.rs`(新)**:按 `Op.seq` 命令序就地应用。
   新增着色器 `composite.wgsl`(MERGE2/XOR/AND)、`transform.wgsl`
   (REVERSE/镜像/MEIDO/SAIDO/AKARUSA/MONO/SEPIA/MOSAIC/ROTATION)、
   `resample.wgsl`(`scale_region` 的 GPU 等价)。`MERGE` 仍用已验证的
   `merge.wgsl`。
3. **操作前缀复用**:每层 `(seq, 签名)` 前缀;签名含各源**当前内容身份**,
   自引用用基面身份。日志被折叠/`CG[FILE]` 重建 ⇒ 回基面重放。
4. **快照三态**:整幅+未采样 ⇒ 别名源内容句柄(零拷贝);否则区域拷贝;
   CPU 像素按需读回(存档抓帧)。
5. **文本进层表面**:`CGACT[TEXT]`/`TextRun` 以「修订号+内容签名+基面身份」
   为键合成一次、上传一次;`raw`(文本前像素)单独保存,供冻结文本 COPY。
6. **`yu-engine/src/capture.rs`**:删每帧 CPU 镜像预捕获与整帧软回放
   (`SNAPSHOT_READBACK_CAP`、`GfxCapture.mirrors` 的预捕获语义);改为
   `observe` 只登记新 `CopyId`、宿主渲染后按需读回这一张。`SAVE/LOAD`
   仍走合成资产叠层。
7. **`RenderPhases`** 新字段:`surface_passes/blits/alias_hits/reused_layers/
   materialized/cpu_fallbacks/upload_asset_bytes`(帧 CSV 46 列;`RenderReport`
   语义未改)。spike 行同步补 `surfaces/blits/alias/reused/materialized/cpu_fb`。
8. 旧路径删除:`Override`(Cpu/Gpu/Uv)、`apply_image_to_override`、
   `capture_copy_mirror`、`TextureCache::rewrite`、cpu 覆盖上传、合成记忆化。
   wgpu crate 零新增告警(`param_names`/`coerce_param` 为 yu-rt 既有)。

### 实机回归(本轮修复,均有对拍用例)

- **按钮栏全部同一段文字**:基面缓存只按图层 id 做键 ⇒ 同一模板层一帧内
  15 次 `CG[FILE]+COPY2` 全部命中第一次的纹理。修:键加纹理引用身份。
- **`CG[FILE]` 后旧表面仍被读取**:基面身份只在写路径核对。修:帧首/源层/
  COPY 源都核对;源层已 `CGEND` 删除时按命令时刻纹理重放。
- **区域复制越界 / 过渡被跳过**:旧内容尺寸与新纹理不符导致
  `Copy of X 198..264 overrunning Source texture of X size 200`;跨帧常驻的
  过渡 mask 不在逐帧 views 表 ⇒ `transition.mask_missing`。修:`push_blit`
  规划期校验;`transition_source` 直接带出 `TextureView`。

### 数据(`<game-b>` v494,1200 帧,桌面 wgpu,`--no-sound`,本机同口径)

```text
口径                     改前        改后
render p95               24.5ms      2.10–3.30ms
plan p95                 22.2ms      0.30–1.91ms
work p95                 49.6ms      7.27ms(轻场景)/23.5ms(含美术首次解码帧)
fps                      38.5        50.4–57.6
上传(合成,不含解码)     平均8.6MB   平均 0.01–0.05MB / 单帧 ≤3.63MB
上传(含资产首次解码)     平均8.9MB   平均 0.02–0.32MB / 单帧 ≤54MB(解码帧)
copy_bytes               重帧 79MB   单帧 ≤8.00MB(p95 = 0)
GPU 目标字节             127MB       4–217MB(池上限 64MB;大表面不复用)
surface_passes/帧        每 op 重放  平均 1.5–1.6(前缀复用 17.5 层/帧)
```

`<project-eris>` 3000 帧(`YURIS_CASE_BENCH=1 case perf-eris`):

```text
          改前(本机)   改后
tick p50  5.79ms       4.22ms
tick p95  8.35ms       5.68ms
启动帧    438ms        278ms
```

资产首次解码尖峰(18–54MB / 5–30ms)两版都在(改前同场景 71MB/38ms);
`upload_asset_bytes` 把“加载新美术”与“每帧渲染上传”分开记账。

### 验证命令与原始结果

```text
$ cargo build --workspace --all-targets                        OK(仅 yu-rt 既有 2 警告)
$ cargo test -p yu-backend-wgpu
  25 passed(逐像素对拍)+ 10 passed(surfaces 契约)+ 9 passed(单测)
  + 1 passed(wgsl_check:naga 解析三支新着色器)
$ cargo test --workspace --no-fail-fast
  59 suite 全绿;唯一失败 yu-trace --test golden(YSCom.ycd sha,既有红灯)
$ cargo test -p yu-cli --test logging_guards                   OK
$ YURIS_CASE_BENCH=1 yu-cli case <project-eris> --case perf-eris
  [casebench] frames=3000 p50=4.22 p95=5.68 p99=6.74 max=278.20
$ yu-cli case <project-eris> --all
  1 passed / 8 failed —— 与 pristine HEAD(未改动导出构建)对比:HEAD 为
  0 passed / 9 failed,失败集合相同(display JSON golden 与本机字体族相关,
  既有红灯);本改动额外使 save-load-loop 的 PNG 与 save-title-flow 通过。
$ yu-cli run <game-b> --mode desktop --frames 2500 --no-sound --script perf-vm-bench.json
  exit=0 · 2265 帧 · avg 52.3fps · gpu targets 26MB 稳定 · uncaptured error 0
```

### 追加(同日):`MERGE` 写 alpha —— 眨眼/换差分闪屏的 REA 收口

先按实机症状加过一版「快照目标取冻结像素」的特例;主人指出这是打补丁,
遂回到官方二进制取证。REA 0.495(Ghidra 12.1.4,`FUN_004405a4`
= `0x4405a4`)的合并分支结构是:

```text
list[i].ptr = layer[upper]->+0x38[i]     // 上层的每部件纹理
list[i].val = layer[lower]->+0x34[i]     // 下层的每部件位置
for each: dev->Draw(ptr, val, ratio*0.06, 0)   // 画到当前渲染目标(下层表面)
```

⇒ 官方 **不读下层旧像素**,只把上层画进下层表面;不透明度由 D3D9 固定
管线混合给出,未启用独立 alpha 混合 ⇒ `a' = a² + la·(1−a)`,即 **MERGE
会写 alpha**。这同时解释了为什么 `COPY2 → RECTPAINT3 SET=0 → MERGE`
清空后立绘仍可见,以及旧实现(保留目标 alpha)为什么闪屏。

改动:删掉「冻结像素」特例(wgpu/soft 都恢复“下层 = 当前表面”),把 alpha
改成同因子混合写回(`merge.wgsl` / soft `apply_merge`)。门禁改为
`merge_after_rectpaint_clear_on_snapshot_writes_alpha`(跨后端一致 +
清空后 RGB≈纯红、不黑)+ soft 两条 alpha 单测。

回归:`cargo test -p yu-backend-wgpu -p yu-backend-soft` 全绿
(soft lib 41 条含新锚点);`<game-b>` 2500 帧双后端对拍计数/跳过差异 0。

### 未做 / 已知残留

- `SIZE*` 的 UV 采样与 soft `scale_region` 在硬边缘差 >1(标题 logo f20
  max 194;平场/文字帧 0)——既有采样模型分歧,详见 DIV-GFX-066 §3.2。
- `BLUR*/DIFFUSION/WHIRLPOOL/RASTER*` 无 GPU shader:显式跳过并计数
  (`gpu.imageop.unsupported.*`),两份语料未出现。
- `TRIM` 读回一次求 alpha 包围盒(`cpu_fallbacks` 计数)。
- `YURIS_LEGACY_FRAME_REPLAY` 回退开关未保留:旧实现与“每帧 CPU 镜像”
  在同一改动里删除(保留死代码会与 W8 冲突);旧文件留
  `target/tmp/passes.orig.rs`(仅本地)。

## 成果 1 (2026-10-19) · 仓库脚手架

- 内容: 12 个子系统 crate 骨架 + oracle 方法论(docs/oracle.md)+ 不一致档案制度
  (docs/divergences/)+ 工作须知(AGENTS.md,不入库)。**零代码移植**;
  LICENSE 自旧接手线拷贝(GPL 全文,许可文本非代码)。
- 设计立场: 净室新写;真值 = 官方引擎行为;旧线 <repo-old> = 只读参考实现。
- 验证: `cargo check --workspace` 与 `cargo test --workspace` 结果见当日本机记录(骨架应全绿,
  但无实质测试 —— 这是起点,不是成果)。

## 成果 2 (2026-10-19) · SDK 一手脚本系统分析 + 架构结论

- 内容: 以官方 SDK 0.495 一手资料(71 个 .yst 源脚本 / 官方手册 html / YSCom.ycd 二进制 /
  REA 对 YSCom.exe 取证)完成脚本/语法/宏/命令/结构/推进/输入输出的完整分析,
  落盘 `docs/sdk-eris-analysis.md`(§7 为架构结论:宏=必需编译前端、16 槽寄存器调用约定、
  任务让出=WAIT/文本、全员让出才渲染、输入=带符号帧计数器、按名分派)。
- 勘误留痕: ①旁线"无文档化 CLI"说法不确 —— REA 取证 YSCom.exe 要求 argv[1] 以 "ys" 开头
  且依赖启动器创建的共享内存 "YU-RIS Compiler FileMap Object",双击秒退即此因;
  ②本日曾尝试 GUI 自动化编译 ERIS,**经用户决策取消**: 分析任务无需编译产物,
  商业游戏封包已足够当 yu-format 语料;方法留档 workbench(yscom-launch.ps1/yscom-compile.cmd)。
- 验证: YSCom.ycd 头部 `YSCD`+版本 0x1EF 与 105 个 COM_ 命令名经 Python 直读二进制复核;
  project-eris 副本自官方源重建,1446 文件计数一致 + 5 项 sha256 抽查一致。
- 附带: crates/ 12-crate 骨架已按用户指示整体移除,仓库形态待按 §7 结论重定(未定稿前保持空置)。

## 成果 3 (2026-10-19) · 架构基线 v1(散盘→封包迁移轴)

- 内容: `docs/architecture.md`。核心决策 A = 数据源抽象(DataSource: DirSource/YpfSource/MemSource
  + 叠加挂载 + 路径归一只做一次),散盘项目与 .ypf 封包游戏收敛为同一程序,配永久"等价性测试"自证;
  核心决策 B = 9 crate 分层(vfs/format/rt/gfx/snd/inp/save/trace/cli),依赖严格向下,
  四条可序列化边界(①vfs 读 ②事件流 ③帧描述 ④平台命令)挂 trace。
- 依据: sdk-eris-analysis §7 + 官方挂载顺序 REA 证据(base 包 + update1..9 依次覆盖)。
- 状态: 设计定稿待用户确认;crate 实体尚未创建(crates/ 空置)。

## 成果 4 (2026-10-19) · 文本编码策略(并入架构 v1)

- 内容: architecture.md 新增 §6——内核内部一律 Unicode;"编码"收敛为四条字节边界的编解码器参数
  (vfs 查找键的多字节跳步表 / format 文本字段 / save 原编码写回 / 字体呈现只看 Unicode);
  CLI `--codec cp932|gbk|auto`;auto=固定顺序采样+双解码评分+确定性裁决(缺省 CP932),
  结论进 trace/golden 头部,运行中绝不切换。
- 关键事实: CP932 的 0xA1–0xDF 是半角片假名单字节,GBK 同区间是双字节前导 ⇒ 资源名扫描
  必须按编解码器选跳步表;存档字节级兼容要求按**原编解码器**写回,不是按 Unicode。

## 成果 5 (2026-10-19) · 架构 v1.1:零硬编码不变式 + 渲染后端层(wasm 先行)

- 内容(用户指出两处缺失后修订 architecture.md):
  ① 新增不变式 A5「通用运行时零硬编码」——运行核/域状态机禁止任何游戏或版本特定常量,
  命令表/变量表/设置/清单全部运行期从游戏工程装载,缺表显式拒绝启动,禁止内置兜底表;
  CI grep 守卫为执行手段。② 新增渲染层:RenderBackend trait 定义在 yu-gfx(与 FrameDescription
  同处),平台实现各自一 crate —— yu-backend-wasm(当前实现,含 wasm-bindgen 宿主入口/rAF/OPFS)、
  yu-backend-win、yu-backend-android(后续);wasm 先行理由=调试回路最短(canvas 截图差分、刷新即看)。
  ③ 里程碑更新:M3=wasm 呈现闭环。
- 状态: 设计 v1.1;crate 实体尚未创建。

## 成果 6 (2026-10-19) · 架构 v1.2:错误+日志统一包 yu-log

- 内容(用户要求"明确的错误系统,错误就是 log 包"):新增 §8 与 crate `yu-log` ——
  Diag 结构(稳定命名空间错误码 + Fatal/Recoverable/Unimplemented 严重级 + 边界归属 +
  位置上下文 + 因果链)、七类码域(vfs/fmt/table/rt/gfx/snd/inp/save/platform)、
  LogSink+JSONL(与 trace 边界流同格式,trace 消费 log 事件入流,回放时错误序列一并比对)、
  禁止散落 println(与 A5 同族 CI 守卫)。严重级语义对齐官方:官方有 ERROR[COM_XX=n]
  按命令族设错误等级(yst 一手),错误策略是脚本可见语义,须建模。
- crate 清单更新:yuris-kernel = log/vfs/format/rt/gfx/snd/inp/save/trace/cli + backend-wasm(先行)
  + backend-win/android(后续)。

## 成果 7 (2026-10-19) · 三路并行任务书

- 内容: `docs/tasks/task-a-foundation.md`(workspace+yu-log+yu-vfs,先行)、
  `task-b-format.md`(yu-format,真实语料三方互证)、`task-c-runtime.md`(yu-rt,三层真值纪律:
  手册页/真实脚本印证/官方 0.479 行为对照,缺一即 Unimplemented)。B/C 接口契约冻结于任务书 B §4。
- 分工纪律: 各会话独占目录(workspace glob 自动入组);A 先行建骨架;共享文件只 A 可改。

## 成果 A (2026-10-19) · 地基: workspace + yu-log + yu-vfs(M0)

- 内容:
  - 根 `Cargo.toml`:`members = ["crates/*"]`、`resolver = "2"`、`workspace.package`
    (edition 2021 / GPL-3.0-or-later)、`workspace.dependencies = { flate2 = "1" }`;
    `.cargo/config.toml` 把构建产物重定向到工作沙箱的 `cargo-target`。
  - `yu-log`:`Diag { code, severity, boundary, where_: Loc, detail, causes }`、
    `Loc::{Script,Data,Table,None}`、稳定码表(18 个,唯一性/命名空间有单测)、
    `LogSink` + `JsonlSink` / `MemorySink` / `NullSink`(显式注入,无全局)。
    JSONL 为确定性单行输出,字节串按 `\u00XX` 转义可逐字节往返;零第三方依赖。
  - `yu-vfs`:`DataSource { open/exists/list }`;`LookupKey` 归一只做一次
    (`\`=`/`、ASCII A–Z 折叠、CP932/GBK 多字节整对跳过、拒绝 `.`/`..`/NUL);
    `MemSource` / `DirSource` / `YpfSource` / `Mount`(后挂覆盖先挂);
    `YpfSource` 懒索引 + 偏移直读,按包判别两种索引方言,未知字段
    (`0x10..0x20`、`indexPrefix`、`reserved`、`tail`)原样保留。
- 实测确证(本机真实数据):
  - v494 商业包 `data.ypf`(3,315,484,359 字节):`version=494`、`entryCount=12212`、
    `indexSize=0x9542A`;索引 **12,212/12,212 条**全部走完并闭合
    (`consumed=611334 = index_bytes`),名字全部可打印、偏移全部落在数据区,
    `flag=1 ↔ compressedSize<uncompressedSize` 2,702/2,702 成立,
    `flag=0 ↔ 两长度相等`(末条 tail 被索引区截断 4 字节)成立。
  - `.ybn` 抽样 `ysbin\yst00034.ybn`:索引声明 5,956 字节,zlib 解压后 5,956 字节,
    与独立 inflate 逐字节一致。
  - 图像抽样 `cg\ev\8_4e.jpg`:516,377 字节,解压后 JPEG 魔数命中。
  - 官方 0.479 SDK 散盘目录:1,339 个 ASCII 文件入索引,85 个非 ASCII 名按
    `DIV-003` 显式跳过并计数;大小写折叠查找命中(`data/bgm/bgm01.ogg`)。
- 按笔记/参考实现待验:
  - NUL-XOR 索引方言(v500 的 `bn.ypf` 样本缺失):仅由合成包测试覆盖,
    真样本复核待办(`DIV-002`)。
  - 索引 MurmurHash2 在官方查找中的确切作用未复现;本实现按归一名精确匹配
    (`DIV-001`)。
  - >4 GiB 封包的偏移表达未验证,当前显式 `vfs.ypf.unsupported`(`DIV-002` 附注)。
- 验证命令与原始结果:

```text
$ cargo test --workspace -p yu-log -p yu-vfs
yu-log unittests:  13 passed; 0 failed
yu-vfs unittests:  27 passed; 0 failed
equivalence.rs:     7 passed; 0 failed
nul_xor.rs:         2 passed; 0 failed
real_sample.rs:     4 passed; 0 failed

$ cargo test -p yu-vfs --test real_sample -- --nocapture
REAL SAMPLE: <data.ypf>
header: version=494 entries=12212 indexSize=0x9542a fileLen=3315484359 dialect=lenenc-not warnings=0
index: parsed=12212 valid=12212 clean=12212 pathlike=12212 lengths=12212 closed=true consumed=611334/611334
ybn sample: ysbin\yst00034.ybn declared=5956 read=5956 compressed=true
image sample: cg\ev\8_4e.jpg declared=516377 magic=ok
0.479 DirSource: files=1339 skipped=85 (non-ASCII names are explicitly skipped)
loose sample: data/bgm/bgm01.ogg bytes=907689 skipped_total=85
test result: ok. 4 passed

$ YURIS_SAMPLE_PAC=<missing> cargo test -p yu-vfs --test real_sample -- --nocapture
SKIP[count=1] YURIS_SAMPLE_PAC/bn.ypf (...) does not exist   (x3, 累计计数)
SKIP[count=4] YURIS_SAMPLE_SDK0479 is not set
test result: ok. 4 passed        ← 跳过显式可见,不是静默通过

$ cargo clippy --workspace --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
```

- 永久等价性测试:`crates/yu-vfs/tests/equivalence.rs` —— 同一棵临时文件树经
  `DirSource` 与测试专用 ypf 写入器打包后的 `YpfSource` 分别挂载,
  `open/exists/list` 逐项一致;含 root/中间目录/文件/缺失/非目录五类键。
- 勘误与未决档案:`DIV-001`(ypf 索引字段布局与旁线笔记不符,以真实字节为准)、
  `DIV-002`(NUL-XOR 方言无真样本)、`DIV-003`(非 ASCII 资源名缺编解码表)。
- 纪律:零 git 写操作;实现内无 `println!/eprintln!/dbg!`;除 `flate2` 外零依赖;
  游戏路径与条目名未进入实现(仅测试从环境变量取样本)。

## 成果 8 (2026-10-19) · ERIS 官方编译打通(143 产物 + 引擎验收)

- 内容: 以引擎侧协议(命名映射握手 + CreateProcess)驱动官方 YSCom 0.495 完成 ERIS 全量编译,
  产物落 `workbench/project-eris/data/ysbin/`(137 个 YSTB 脚本 + ysc/yscfg/yse/ysl/yst/yst_list/ysv),
  **官方 yu-ris.exe 直接启动并显示标题画面** = 编译链验收通过。
- 一手事实(已并入 `docs/sdk-eris-analysis.md` §8): 映射名精确无空格;项目路径必须带尾部反斜杠;
  PowerShell Start-Process(ShellExecuteEx) 不给这个 VC6 程序传 argv(秒退主因);
  CP932 文件名在非日文系统会损坏(需转区或 ASCII 化);YSTB 磁盘形态=明文头+分段异或
  (密钥 CRC32("Yu-ris")=0x96AC6FD3,LE 字节序,V5 四段各自重启);
  七个元数据文件各有独立魔数(YSCM/YSCF/YSER/YSLB/YSTD/YSTL/YSVR)。
- 工具: `workbench/yscom_harvest_rs`(Rust,零依赖,LE 模式 + 逐秒状态诊断)。
- 给 A/B 任务的语料: 上述 ysbin 可直接作 yu-format/ybn 解码的一手对照(文件名已 ASCII 化,
  映射清单 workbench/rename-manifest.json)。
- 下一步: 打包成 .ypf(商业形态),再作 yu-vfs 的 YpfSource 真实语料。

## 成果 9 (2026-10-19) · ERIS 测试工程立档 + YPF 布局实证修正

- 新增 `docs/eris-corpus.md`(测试工程):官方核心语料 = 编译产物 `data/ysbin`
  (137 YSTB + 7 元数据表)+ 官方 YSPac 包 `data.ypf`;四步复现(编译/调试运行/打包/发布启动);
  **发布模式实测通过**:`DEBUGMODE=0` + 包放游戏根 → 引擎直接读包内 ysbin 出标题,无编译。
- 新增 `docs/formats/ypf.md`(官方包逐字节实证):头 32B(索引自 0x20 起,v494 族可能 0x24——需探测)、
  步长 nameLen+27、hash 抽验 4/4 与 MurmurHash2(name,seed0) 相等、
  flag 0=zlib/2=原样、dataOffset **直指负载**(无独立记录头)、末 8 字节未知字段不依赖。
- 勘误(会话 A 的 DIV-001 定位):旧接手线 `LenEncNot` 解析器按 0x24 起索引且字段切分为
  `code+NUL+usize+...`,整体偏 1 字节 → 大包读出垃圾;修正依据与症状已写入 ypf.md §6。
- 验证: 1407/1407 条目行走恰好抵达数据区起点;抽验 3 条负载(zlib 头/PNG 字节)与索引字段完全吻合。

## 成果 10 (2026-10-19) · 开源工具参考档案 + 会话 A 重启计划(仅文档)

- 新增 `docs/references/open-source-tools.md`: 收录 9 个可参考的开源 YU-RIS 工具
  (RxYuris / YU-RIS-Script-Editor / YuRISTools / YURIS_TOOLS / yuri / YPF_Tool /
  YPF-Manager / ypf-repacker / YurisKeyFinder),按子系统索引"出问题先查谁",
  并写死许可边界(无声明许可=仅事实参考,如 Script-Editor)与冲突裁决(官方优先)。
  关键新增: Script-Editor 的 **YSTB V2 支路**、密钥/编码自检测、文本三行对照导出,
  以及其宣称支持的换密钥商业作品(含 Whirlpool 系)场景。
- 新增 `docs/tasks/session-a-restart.md`: A 的重启计划(修 YpfSource 两处实证错误 +
  官方真包 data.ypf 回归清单 + DIV-001 结案 + 启动提示词)。
- 本成果只写文档,未改任何代码。

## 成果 11 (2026-10-19) · 参考档案补充:翻译实现方式与解密代际(仅文档)

- `docs/references/open-source-tools.md` 增补:
  ①Script-Editor 行注明"**翻译=数据层嵌入**(写回 YBN 文本段、纯脚本 YPF 自动回封),
  无 exe/DLL 注入",并记其 README 自述已测 Whirlpool 系及多部商业作品;
  ②新增 GARbro(Ypf 解密第三实现)与"解密代际对照"(YURIS_TOOLS=新版 / RxYuris=旧版 /
  GARbro=YPF,均为二手说法待核实);
  ③新增「翻译实现方式对照」小节:数据层嵌入 vs exe 注入,结论为本项目只需字节级
  YSTB/YPF 往返,exe 注入因违反 A2(数据只读)不作参考方向。
- 仅文档改动。

## 成果 12 (2026-10-19) · 勘误:会话 A 计划的方言范围与语料指向

- 发现: A 的重启提示词只写了"0x24/0x20 起点探测",**未写明索引方言范围**;
  `task-a-foundation.md` 还残留"0.479 SDK 跨版本包"的旧指令,与"语料已替换为用户编译的
  ERIS demo 包"相矛盾。
- 更正(留痕,旧结论作废):
  ①`docs/formats/ypf.md` 新增 §1.1「索引方言」:LenEncNot(本 demo 包,已实证)与
  NulXor(NUL 结尾+XOR key、步长 nameLen+26,未验证)分列;本次只做 LenEncNot,
  NulXor 遇包显式报"未验证方言"挂 DIV-002,禁止猜 key。
  ②`docs/tasks/session-a-restart.md` 顶部新增「语料与范围」:语料 = `project-eris/data.ypf`
  (官方 YSPac 0.495,1407 条);旧商业样本(bn.ypf/0.479)不在范围。
  ③启动提示词同步补上这两条;`task-a-foundation.md` 的"0.479 跨版本包"改为方言范围条目。
- 仅文档改动。

## 成果 A2 (2026-10-19) · yu-vfs 拆包修正(官方 YSPac 0.495 实测驱动)

- 触发: 会话 A 重启计划 `docs/tasks/session-a-restart.md`;依据 `docs/formats/ypf.md`。
  旧 `YpfSource` 有两处实证错误: 索引起点固定 `0x24`、字段切分整体偏 1 字节。
- **实测确证**(官方 `data.ypf`: 1,407 条 / 31,501,753 字节,版本 495):
  - 索引起点 `0x20`;行走 1,407 条后 `consumed = 79,647 = index_bytes`,恰好等于头部
    `+0x0C` 的 `dataStart = 0x1373F`。
  - 条目布局 `u32 nameHash | u8 lenEnc | 反名 | u8 fileType | u8 isCompressed |
    u32 usize | u32 csize | u32 dataOffset | u32 unknown0 | u32 payloadHash`,
    步长 `nameLen + 27`;`dataOffset` 直指负载,无独立记录头。
  - `nameHash` = MurmurHash2(名字, seed 0): **1,407/1,407** 命中。
  - `payloadHash` = MurmurHash2(存储字节, seed 0): **1,407/1,407** 命中
    (这条把"校验字段"从猜测变成实证)。
  - `isCompressed=1` 共 152 条,全部是 zlib 流且 `csize` 等于流实际消耗字节;
    `isCompressed=0` 共 1,255 条且 `csize == usize`。
  - 24 条 zlib 解压长度 == `usize` 且与独立 inflate 逐字节一致;
    64 条 raw 首字节与扩展名一致(png/jpg/ogg/wav/ybn 全中,image magic 64/64)。
  - 点名验收: `open("ysbin\ysc.ybn")` = 11,156 字节,`YSCM`,版本 495
    (大小写与 `/` 分隔符变体同样命中)。
  - 类型分布: `{0:146, 2:1245, 3:7, 6:8, 9:1}` —— 其中 `type=3` 且 zlib、
    `type=0` 且原样并存,**证明 `fileType` 与 `isCompressed` 是两个独立字节**,
    旧文档的单字节 `flag` 模型无法同时解释。
- 修复:
  - 索引起点改为**探测**: 候选 `{0x20, 0x24}`,取"恰好闭合 + 名字哈希命中最多"者,
    平局取更小的起点(确定性)。
  - 字段按上述布局重写;`isCompressed` 非 0/1 → `vfs.ypf.unsupported`(Unimplemented);
    名字哈希不符 → `vfs.ypf.hash_mismatch`(Recoverable,可读但记账);
    负载哈希不符 → `vfs.ypf.checksum`(Unimplemented,引擎多段续读未实现,不猜)。
  - 未验证的 NUL-XOR 方言收窄为回退路径(须条数相等+走到块尾+全可打印名+含路径分隔符),
    命中即挂 `vfs.ypf.unsupported` 警告,不再吞掉官方布局的字段错误。
- 新增 `yu-vfs/src/encoding.rs`(用户要求"编码也要条理清晰"):
  零依赖、无浮点的确定性检测器,候选 `UTF-8/CP932/GBK/Big5`,输出
  `Decision { encoding, only_candidate, decisive, bom, scores }` 与 `evidence_line()`
  (可直接进 trace/golden 头)。判据参考开源实现 `YU-RIS-Script-Editor/core/encoding.py`;
  该实现未声明许可证,仅借鉴判据、未复制代码。码表仍归 `yu-format`(DIV-003 保持 partial)。
- 验证命令与原始结果:

```text
$ YURIS_SAMPLE_ERIS_YPF=<official 0.495 data.ypf> \
  YURIS_SAMPLE_SDK0495=<0.495 SDK システム> \
  cargo test -p yu-vfs -- --nocapture
yu-vfs lib:        37 passed; 0 failed
equivalence.rs:    10 passed; 0 failed
nul_xor.rs:         2 passed; 0 failed
real_sample.rs:     4 passed; 0 failed

REAL SAMPLE: <official data.ypf>
header: version=495 entries=1407 dataStart=0x1373f fileLen=31501753
index: start=0x20 bytes=79647 consumed=79647 closed=true nameHash=1407/1407 compressed=152 types=[(0,146),(2,1245),(3,7),(6,8),(9,1)]
dialect: hashed-len
ysc via "ysbin\ysc.ybn": 11156 bytes, magic YSCM, version 495
ysc via "YSBIN\YSC.YBN": 11156 bytes, magic YSCM, version 495
ysc via "ysbin/ysc.ybn": 11156 bytes, magic YSCM, version 495
0.495 DirSource: files=1358 skipped=92
loose sample: data/bgm/bgm01.ogg bytes=907689
zlib entries verified: 24
raw entries verified: 64 (image magic ok: 64)

$ cargo clippy --workspace --all-targets
（零告警）
```

- 追加(同日,用户要求对照开源实现补全算法):
  - **名字 XOR key 探测**: 官方 0.495 实测 key=`0xFF`(逐字节取反);
    参考实现给出逐游戏 key 的推导 `enc[nameLen-4] ^ '.'`,本实现以
    nameHash 命中数在 `{0xFF, 推导值, 0..=255}` 中择优(确定性: 命中数优先,
    平局优先 0xFF、其次更小 key)。官方包 1,407/1,407 命中。
  - **按版本切换长度交换表**: `version < 0x100` 用 10 对表、
    `0x12C..0x196` 用 8 对表、其余 12 对表(前两张表只有开源参考实现,
    使用时挂 `vfs.ypf.unsupported` 警告 → DIV-004)。
  - **meta 长度按版本**: `0x12 + extra`(extra=4 for >=0x1D9 / 8 for ==0xDE / 0);
    495 实测 22 字节,读取第 22–26 字节作为 `payloadHash`。
  - 实测输出更新: `index: start=0x20 key=0xff meta=22 ... nameHash=1407/1407`。
- 档案: `DIV-001` → **fixed**(记录旧/新布局对照与回归输出);
  `DIV-002` 保持 open 并注明"本次未触达"(NUL-XOR 无真样本);
  `DIV-003` 改为 partial(检测器已落地,码表待 B);新增 `DIV-004`
  (名字 key / 版本交换表 / meta 长度: 算法已实装,非 495 分支缺真样本)。
- 纪律: 零 git 写操作;未使用 <game-b> 语料;实现内无 `println!/eprintln!/dbg!`;
  测试仅从环境变量取样本路径。

## 成果 13 (2026-10-19) · 算法采用策略 + B/C v2 启动提示词

- 策略变更(用户决定): **算法/常量/流程可直接采用**;采用处文件头注明
  `参考 <项目>/<文件>`,并在 `docs/references/open-source-tools.md` §4 致谢链登记
  (README 致谢由仓库主人汇总)。整段复用代码前确认许可状态。
- 致谢链(照开源生态的"代际+职责"结构): YURIS_TOOLS=新版引擎解密、RxYuris=旧版引擎解密/
  YSCOM Loader/YSCM·YSTB 解析、GARbro=YPF 解密、Script-Editor=YSTB V2/V5 与编码自检测、
  yuri=反编译参照,其余按实际采用列。
- `task-b-format.md` / `task-c-runtime.md` 均升 v2: 新增可采用的算法来源表(文件级),
  并重申真值层级不变(B: 官方产物; C: 手册/真实脚本/官方引擎三层,缺一 Unimplemented)。
- 仅文档改动;两个 v2 启动提示词已发给用户。

## 成果 14 (2026-10-19) · 勘误:v2 提示词未点名旧线仓库可读路径

- 发现: B/C v2 提示词与任务书只在表格里写占位符 `<repo-old>`,未告知"本机可直接读
  `<repo-old>`",导致会话可能找不到参考实现。
- 更正: `task-b-format.md`、`task-c-runtime.md` 的参考索引上方新增说明:
  旧线仓库根 = `<repo-old>`(真名见 AGENTS.md),
  **只读参考,禁止修改其工作区/禁止在其中执行 git 操作**;算法采用需注来源并登记致谢。
- v2 启动提示词同步补上该路径与各会话应读的文件清单。
- 仅文档改动。

## 成果 C2 (2026-10-19) · yu-rt 运行核(M2):表装载 / 表达式 / 任务调度 / 按命令频度实现

> 会话 C 独占 `crates/yu-rt`;未改根 `Cargo.toml` 与其它 crate;零 git 写操作。
> 语料 = 官方 YSCom 0.495 编译的 ERIS 产物(136 个 `ystNNNNN.ybn` + 7 张表);
> 另加**商业发行版**(<game-b> v494,`data.ypf`)作为第二套真实语料。

### 内容

- **表装载(A5 零硬编码)**:`YSCM`(命令名/参数表)、`YSLB`(1941 标签,含
  `步长=名长+13`、`hash=MurmurHash2(seed 0)`)、`YSVR`(2568 条变量镜像)、
  `YSTL`(141 条清单)、`YSCF`(分辨率/扩展名优先序)、`YSTB`(四段各自异或,
  密钥 `CRC32("Yu-ris")`)、`YSCD`(编译器定义:命令参数属性 + 548 项系统变量)。
  缺必需表 ⇒ `table.missing` **拒绝启动**。
- **表达式**:`[op][len u16][payload]` 记录流 + 27 项已取证操作码(算术/比较/
  位运算/字符串拼接/数组取值/两个显式转换);左值形态(标量 / 引用+下标+aload)。
- **变量域**:INT/FLT/STR;行主序数组;写时改类型标签、读时按口味转换;
  `LET` 复合赋值码 0..8(语料实测)。
- **任务调度**:单一扁平链、链长入口快照、每逻辑帧每任务一步、FRAME 递减、
  TIME 绝对期限、TASK 续帧、泵到让出无预算(算法参考旧线 `task.rs`,语义按
  手册 + 引擎 §3.2b/§3.13 逐条核对)。
- **命令处理器(按频度)**:GOSUB/RETURN/GO/IF/ELSE/IFBLEND/IFEND/IFBREAK/
  IFCONTINUE/LOOP/LOOPEND/LOOPBREAK/LOOPCONTINUE/WAIT/FPS/END/LET/INT/FLT/STR/
  S_INT/S_FLT/S_STR/VARINFO(含 SEARCH)/VARACT(COPY/CUT/TYPE/UPPER/LOWER/
  DIMSIZE/PUSH/POP)/MATH(除 RND)/TASK/TASKEND/TASKINFO/TEXT/LABELINFO。
- **输入**:带符号帧计数器(手册三个例子逐条实现)+ 输入时间线注入 + 系统变量绑定
  **全部按名字**从表解析(`_KEY_A`=167 等 id 不硬编码)。
- **事件流**:确定性 JSONL(`frame/sub/kind/payload`),时钟与输入全注入。
- **商业发行版支持(用户指出)**:`YSCom.ycd` 改为**可选**;无 ycd 时用
  `data/sysvar-abi-0495.tsv`(由 `scripts/gen_sysvar_abi.py` 从官方 ycd 生成,
  带来源 SHA-256),并用**工程自己的 `ysv.ybn` 逐项校验**(548/548 才算通过)。

### 实测确证(原始输出)

```text
$ YURIS_SAMPLE_ERIS_YSBIN=<project-eris/data/ysbin> \
  YURIS_SAMPLE_GAME_YPF=<<game-b>/data.ypf> cargo test -p yu-rt -- --nocapture
running 31 tests   (lib 单测)                      ... ok. 31 passed
running 5 tests    (corpus.rs: 真实语料)
LABELS: 1941 条全部闭合;无标签引用的脚本 = [28, 59, 63]
SCRIPTS: files=136 groups=36974 windows=80102 record_failures=0
BOOT: entry SYSTEM_START -> s73 pc0 (2 组), 源 data\script\system_start.yst
COMMERCIAL: ysc v494 (119 命令) labels=2132 vars=2568
  sysvars<-engine ABI profile sysvar-abi-0495.tsv
  (ycd sha256 a626e40a8b25290dcca14c13aa3314c1fd123514b19a0ce9badd8559c7488d1d)
COMMERCIAL: SYSTEM_START -> s73 pc0 (2 组)
COMMERCIAL: frame1 commands=2438 errors=[] unimplemented=5
TABLES: yscm=119 params=1122 labels=1941 vars=2568 scripts=141 ycd_sysvars=548
                                                    ... ok. 5 passed
running 4 tests    (corpus_golden.rs: 真实脚本事件流 golden)  ... ok. 4 passed
running 20 tests   (fixture.rs: 控制流/变量/输入/错误路径)     ... ok. 20 passed
running 3 tests    (golden.rs: 事件流/错误路径 golden)         ... ok. 3 passed

$ python crates/yu-rt/scripts/audit_hardcode.py
audit: files=15 violations=0
```

关键实测数字(全部由测试钉住,语料变动即测试失败):

- ERIS 0.495:`ysc=119 命令/1122 参数槽`、`ysl=1941 标签(hash 全命中)`、
  `ysv=2568 记录(前 548 条 = 系统变量镜像)`、`yst_list=141 条`、
  `136 个脚本/36,974 组/80,102 窗口`。
- 命令频度(前三):`GOSUB 10215`、`LET 8136`、`IF 3326`;`IFEND 3326`、
  `RETURN 1750`、`INT 1428`、`IFBLEND/ELSE 887`、`LOOP 522`、`WAIT 95`。
- 系统变量 id(官方 0.495 ycd,548 项):`_TIME=0`、`_PAI=10`、`_LC=48`、
  `_PINT=53`、`_PFLT=54`、`_PSTR=55`、`_PFLG=56`、`_RINT=60`、
  `_SCREEN_SIZE_X=115`、`_MOUSE_X=133`、`_INP=144`、`_KEY_A=167`、`_KEY_ANY=275`;
  语料频度与之吻合(`_LC 18521`、`_PINT 12380`、`_RINT 7302` 等)。
- 商业 v494(无 ycd):ysv 前 548 条 kind=1 与 0.495 ycd 的 (type, dims)
  **548/548 逐项一致,0 差异** ⇒ 系统变量 id 空间是引擎 ABI。

### 三层出处对照表

见 `crates/yu-rt/docs/command-provenance.md`(逐命令:手册页 / 语料频度 /
引擎地址级证据 / 实现状态)。

### 未决(全部挂 `crates/yu-rt/docs/divergences/README.md`)

DIV-RT-001..020;本次新增并已修的三条:

- **DIV-RT-016**(fixed):商业发行版不带 `YSCom.ycd` → ycd 可选 + 引擎 ABI 表 +
  `ysv.ybn` 自证校验。
- **DIV-RT-018**(fixed):`ELSE` 的 **0 窗口**形态(商业包 726 次)⇒ 无条件进
  else 体;ERIS 的 3 窗口形态不变。
- **DIV-RT-019**(fixed):`VARACT[DIMSIZE/PUSH/POP]` **不需要 LET**(手册例 9/10)。

仍 open 的重点:`DIV-RT-002`(类型提升/溢出/负下标/越界访问的精确行为)、
`DIV-RT-005`(官方脚本错误继续策略)、`DIV-RT-006`(输入键码表)、
`DIV-RT-007`(手册无页系统变量)、`DIV-RT-011`(手册有页但语义未取证:
`RETURNCODE`/`F_*`/`WORD`/`VARINFO SEARCH 部分`/`VARACT INIT·G_*`)、
`DIV-RT-017`(变量空间前缀 `0x60`/`0x23`)。

### 说明

- 本会话未改动 `docs/architecture.md` 与其它 crate;`crates/yu-format` 由会话 B
  并发创建,期间整仓 `cargo test` 一度因该 crate 缺 `Cargo.toml` 不可用;
  上述结果为**沙箱工作区**(仅 log/vfs/rt)运行所得,代码与仓库内 `crates/yu-rt`
  逐字节一致(每次跑测前同步复制)。

### 补记(整仓构建恢复后)

`crates/yu-format` 落地后整仓构建恢复,以上结果在**仓库内**复跑一致:

```text
$ YURIS_SAMPLE_ERIS_YSBIN=... YURIS_SAMPLE_GAME_YPF=... cargo test -p yu-rt
yu-rt lib:            31 passed
corpus.rs:             5 passed   (含商业包 2438 命令/0 Fatal)
corpus_golden.rs:      4 passed
fixture.rs:           20 passed
golden.rs:             3 passed
$ python crates/yu-rt/scripts/audit_hardcode.py
audit: files=15 violations=0
```

`cargo clippy -p yu-rt --all-targets` 已 `--fix` 一轮;剩余为提示级
(类型别名/切片复制风格),不影响语义。

## 成果 15 (2026-10-19) · 任务书 D:`yu-trace`(旁挂对拍基建)

- 新增 `docs/tasks/task-d-trace.md`(v1):范围锁定边界①(vfs 读)与②(事件流);
  ③帧描述、④平台命令只留接口等 M3;明确**不做**官方 WOW64 采集器(独立工具任务)。
- 并行结论: D 可立即与 A/B/C 并行。边界①随 A 已解锁;边界②只消费 `yu-rt` 公开 API
  (`VmEvent`/`Vm::take_events`/`table_provenance`/`BootConfig`),C 改 API 时 D 适配,
  D 不改其它 crate,也不被任何 crate 依赖;B 与 D 无依赖关系。
- 冻结契约: 记录封套 `{schema,seq,frame,sub,kind,payload}`,header 也是 `seq=0` 的记录;
  `VmEvent` 穷尽匹配(新增变体会让 D 编译失败 = 强制 schema 评审);
  diff 严格逐行、首分歧定位、无容差;trace 语料不入库,仅合成 fixture 入库。
- 验收: roundtrip 字节一致 / `TracingSource` 等价性 / 官方 `data.ypf` 冒烟(env gate)/
  事件流 diff 闭环 / 零 `println!` / 改动仅限 `crates/yu-trace`。
- 仅文档改动;未执行 git 操作。

## 成果 D1(2026-10-19)· yu-trace(旁挂对拍基建)

**范围**:边界①(vfs 读)与边界②(事件流);③帧描述、④平台命令只留枚举占位,
schema 1 显式拒绝其记录(M3 再定义)。未实现官方 WOW64 采集器(独立工具任务)。

**新增文件**(全部在 `crates/yu-trace/`):
- `Cargo.toml` — 依赖仅 `yu-log` / `yu-vfs` / `yu-rt`;无第三方依赖。
- `SCHEMA.md` — 记录协议唯一定义(schema 1):封套、全部 kind/payload、字节/浮点编码、
  输入摘要算法、diff 语义、错误码、与 `docs/oracle.md` 草案的差异、版本历史。
- `src/lib.rs` — crate 文档、公开导出、`trace_dir()` / `write_trace()`。
- `src/record.rs` — `Record` / `TraceLog` / `Payload` / `Kind` / `Sub`;稳定 JSONL
  读写、严格字段游标、`seq` 连续性与 header 位置校验、`prepend_header`。
- `src/json.rs` — 本 crate 自有确定性 JSON(键序固定、`\u00xx` 字节转义、最短往返浮点)。
- `src/hash.rs` — 本地 SHA-256(零新依赖;空/abc/双块测试向量)。
- `src/source.rs` — `TracingSource<D: DataSource>` + `BoxedSource` + 帧号句柄。
- `src/event.rs` — `EventRecorder`;`VmEvent` **穷尽 match**(`JumpKind`/`Domain`
  亦由本 crate 映射,无 `_ =>` 兜底);输入时间线摘要。
- `src/diff.rs` — 逐物理行字节比较 + 首分歧 JSON Pointer;独立错误码。
- `src/replay.rs` — `VmTrace` 在线回环(只用公开 API `boot/boot_with_aux/tick_at/take_events`)。
- `tests/{roundtrip,equivalence,event_trace,diff,golden,real_sample}.rs` +
  `tests/support/mod.rs`(只读复用 `yu-rt/tests/support`,不复制)。
- `tests/golden/synthetic_frame.jsonl` — 3,690 B 合成 fixture(<32KB)。

**关键结论/决策**:
- 记录封套为 `{schema,seq,frame,sub,kind,payload}`;header 是 `seq=0` 的
  `meta/header` 记录。启动链 vfs 读取先入流,收尾用 `prepend_header` 补头并重编号。
- `frame=null` 允许 `vfs`(帧外读取),`vm` 子系统必须带帧号(schema 强制)。
- diff **不做任何归一化**:先协议校验,再逐行比原始字节;空白/键序变化也算分歧,
  定位不到单键时 `path=""`(整条记录)。少行 → `trace.truncated`;非法 JSON → `trace.parse`;
  未知 schema/kind/字段/多余字段 → `trace.schema`。
- 在线回放**未阻塞**:`VmTrace::tick_at` 已可用;已知限制 = 一次 tick 可能含多个逻辑帧,
  其内的 vfs 读取标注为该 tick 首个逻辑帧(需 `yu-rt` 帧钩子才能更精确,D 不改 yu-rt)。
- 当前 `yu-rt` 只发射 `FrameStart`(无 `FrameEnd`)、`TEXT` 走 `SystemCall`、
  `_` 走 `Unimplemented` Diag;13 个 `VmEvent` 变体的映射覆盖由单元测试守护,
  事件流集成测试只断言运行核实际发射的事件。

**验收原始输出**:

```text
$ cargo test -p yu-trace          # YURIS_SAMPLE_ERIS_YPF=<official data.ypf>
yu-trace lib:          26 passed
diff.rs:               13 passed
equivalence.rs:         6 passed
event_trace.rs:         8 passed
golden.rs:              2 passed
real_sample.rs:         2 passed   (官方 data.ypf 冒烟:字节/SHA-256 一致、3 帧回放 diff 0)
roundtrip.rs:           7 passed
Doc-tests:              0 passed
```

```text
$ cargo test -p yu-trace          # 未设置 YURIS_SAMPLE_ERIS_YPF
real_sample.rs: 2 passed — official_data_ypf_smoke 走 skip 分支:
  产 trace.sample.missing Info 诊断、SKIPPED 计数 = 1、断言后返回(非静默通过)
```

```text
$ cargo clippy -p yu-trace --all-targets
    Finished `dev` profile ... (yu-trace 0 告警;yu-rt 的 4 条为既有告警,未改动)
```

```text
$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-trace   → 0 命中
$ grep -rn "SystemTime\|Instant" crates/yu-trace         → 0 命中
$ grep -rn "yu-trace" --include=Cargo.toml .(排除自身)   → 0 命中(未被任何 crate 依赖)
```

**未做/阻塞**:
- ③ `frame_description`、④ `platform_call`:仅枚举占位,解析/写出均拒绝(M3)。
- 官方采集器(WOW64/Python):不做;schema 已按同一格式落定。
- 在线逐帧 vfs 归属:受 `yu-rt` 无帧钩子限制,当前为 tick 级标注。

仅改动 `crates/yu-trace/**` 与本次 `PROGRESS.md` 追加;未执行任何 git 写操作。

## 成果 B2 (2026-10-19) · yu-format:ybn 家族解码 + 码表 + 契约(任务书 B v2)

**交付**:`crates/yu-format`(仅依赖 `yu-log`;零第三方依赖;实现内无
`println!/eprintln!/dbg!`)。

- **容器/密钥**:`key.rs`(CRC-32 反射 0xEDB88320 + 分段异或,对合运算);
  `container.rs`(16 字节头;YSLB/YSTL 头实测仅 12 字节,见勘误)。
- **七张元数据表**:YSCM(命令表,`name_of/index_of`)、YSLB(全局标签表)、
  YSVR(变量声明)、YSTD(槽位表头)、YSTL(脚本清单)、YSCF(引擎配置,
  含引擎侧宽高/开关/标题校验)、YSER(id↔名字)。
- **脚本体**:YSTB V5 四区段解密(`Segments::decrypt/encrypt`,异或对合)
  → 契约类型 `DecodedScript{script_id, groups, labels}`(`Group{command,
  windows}`、`Window{[u8;12]}` 冻结接口未动);V2 → `Unimplemented`(DIV-007)。
- **编解码器**:CP932(9,604 双字节条目 + 398 别名)与 GBK(23,940 条,
  零别名)静态表,由 `gen_tables.py` 从 **Windows kernel32 码页 932/936**
  生成(`MultiByteToWideChar/WideCharToMultiByte`,即引擎平台系统映射;
  Python 自带 cp932 的 PUA 扩表被弃用);严格 decode/encode + auto 评分器
  (架构 §7:严格双解码 + 构成统计,固定次序,平局缺省 CP932)。
- **fixtures**:三个最小官方产物原样切片(yst.ybn 16B / ystb_00073.ybn 72B /
  yscfg.ybn 90B)+ 钉死清单 `pinned_unresolved.rs`;所有解析器过"逐字节
  截断安全"测试。

**实测确证项**(全部跑在官方 ERIS 产物上,非笔记转抄):

1. **opcode = ysc 记录下标(字母序)**:system_start=`GO[#=es.ERIS]`+`END[]`
   ↔ op43/op14;es_First 首指令 `TEXT[OUTPUTMODE=0]`↔op96、
   `FILEACT[CREATE=1 FOLDER=$u]`↔op21、S_STR/S_INT 声明↔op82/81。
   注意:**命令只在源码写出参数时才发射参数记录**,ysc argc 不约束指令
   参数个数(控制流命令在表中 argc=0 是占位)。
2. **yst_list 尾字段 = 时间戳×2 + 变量/标签/文本计数**(RxYuris 解释),
   旁线"脚本文件号"被否定:编号 = 记录 index(0..140);141 条中 5 条
   纯声明文件(varCount/labelCount=0xFFFFFFFF 哨兵)恰对应磁盘缺号
   {26,27,69,74,108};labelCount 与 ysl 逐条相等(136/136)。→ DIV-005。
3. **标签闭环**:10410 个 GO/GOSUB 静态引用中 10174 个在 ysl 解析,
   137 个动态目标(`H`/`V` 载荷),99 处(76 组/69 标签)引用 ysl 缺失的
   标签 —— 全部为 ERIS 样板的 `ES.*.VoicePlay.N` 音声钩子与更新钩子;
   YSCom 不校验目标,引擎靠 ERROR[] 错误等级容忍(启动实测到标题)。
   → DIV-006,清单钉死于 fixtures。
4. **ysv 字节级闭合**:2568/2568 条;`ty=0`=仅声明无初值段(452 条);
   kind 1/2/3 = 全局 1167/局部 928/静态 473;槽位全部 < yst 的
   varCount+1(4601);G_* 恢复组号实测 0..=3(未见 4)。
5. **三方互证**:ysc 119 条 ⊇ 手册 71 条;ycd 的 105 个 `COM_*` 中
   `PRODUCT` 不在 ysc(编译器/引擎表版本差)→ DIV-008;ERROR 命令参数表
   = 105 个可设错误级命令(NO/SET + COM_*),不能当命令全集用。
6. **ysl 字段佐证**:field_b 对全部 1941 个标签都落在对应脚本指令数内
   ("脚本内指令位置"假设成立);fieldC=脚本号(旁线实证一致);
   0x400 索引块实测为递增 u32 序列(语义待证,原样保留)。
7. **ysc 尾部 256 字节表 = 键位表**(A–Z=65..89、0–9=48..57、F1–12=112..123、
   方向键/小键盘/修饰键全序列),不是 opcode 映射;旁线"顺序即 opcode"
   的结论以 ysc 记录序的形式被证实。

**勘误留痕**:

- `sdk-eris-analysis.md` §8 密钥值表述:`CRC32("Yu-ris")` 标准值 =
  **0xD36FAC96**;引擎把 CRC **按大端**写进内存,按小端读回的 u32 =
  0x96AC6FD3,小端字节序列 = `D3 6F AC 96`(即异或实际使用的 4 字节)。
  两种值描述同指一字节序列;`key.rs` 单测钉死三向关系。
- 旁线 ysl "+0x0C byte[0x400]"、yst_list "16 字节未知前缀"实为
  12 字节头(魔数+版本+条数),块/记录起点在 `+0x0C`(本档 DIV-005)。

**未决/待验**(进 divergences 或代码内留痕):

- YSTB V2 无语料不实现(DIV-007,`Unimplemented` 显式)。
- ysl 0x400 块、field_a/field_d/field_e 语义;YSTB 头 +0x1C 字段;
  指令 anc0/anc1(观测 0..9/0);行号区(=count×4,已留 `Segments`)。
- `data.ypf` 交叉验证未做:ypf 布局归 yu-vfs(任务书边界),且本 crate
  零依赖无 zlib;如需可在 yu-vfs 侧做"包内 yst == 磁盘 yst"比对。

**验证命令与原始输出**(2026-10-19):

```
$ cargo test -p yu-format
running 6 tests   (codec 全表往返/单字节往返 ×2 码表 + key)
test result: ok. 6 passed; 0 failed
running 1 test    (doctest)
test result: ok. 1 passed
running 16 tests  (合成字节 + fixtures:七表/YSTB/契约/锚点/auto)
test result: ok. 16 passed; 0 failed

$ export YURIS_SAMPLE_ERIS_YSBIN=…/project-eris/data/ysbin
$ export YURIS_SAMPLE_SDK0495=…/yu-ris_0495_032
$ cargo test -p yu-format --test corpus_eris -- --nocapture
ysc − COM_* (15 条):{");", "CDDA", "COMPILEMODE", "EMOTE", "EMOTEINFO", "F_FLT", "F_INT", "F_STR", "F_VAR", "MATH3D", "MIDIEND", "MIDIINFO", "PROJECTFOLDER", "RELEASEMODE", "SYSTEMMODE"}
手册文档化命令:71
YSTB: files=136 insts=36974 windows=80102 GO/GOSUB=10410 resolved=10174 dynamic=137 unresolved=99 (unique 76)
未解析标签 76 组全部为 ERIS 样板的音声/更新钩子(根因见 docs/divergences/DIV-006)
auto: cp932=4.0 gbk=4.0
ysl field_b < 指令数: 1941/1941(field_b = 脚本内位置,假设成立)
corpus_acceptance: 全部通过(ysc=119, ysl=1941, ysv=2568, yst_list=141, yst 文件=136)
test result: ok. 1 passed; 0 failed
(auto 平分 4.0 = 采样池为标签名等 ASCII 串,双解码全合法 → 按架构 §7 缺省 CP932)

$ cargo clippy -p yu-format --all-targets
    Finished `dev` profile …(零告警)

$ 环境变量缺失时(显式 skip 并计数,不许空跑通过)
SKIP corpus_acceptance: 缺少 YURIS_SAMPLE_* 环境变量
test result: ok. 1 passed

锚点抽测(全量在测试内):CP932 "日"=93FA/"ｱ"=B1/"ソ"=835C(0x5C 后随)/
"メモリ不足です。"(yse.ybn 首条逐字节);GBK "你"=C4E3/"国"=B9FA/"€"=80。
全表往返:GBK 零别名双向全等;CP932 的 398 条别名 decode∘encode 恒等、
encode 取 Windows 系统首选(生成器导出的 ALIAS_OVERRIDES 覆盖表)。
```

**与开源参考实现的交叉验证**(把 Script-Editor 原版 Python 代码直接跑在本语料上):

```
① core/ystb.YSTBFile.from_bytes(data, key=0x96AC6FD3) 逐文件解码 136 个官方 YSTB:
   insts=36974 windows=80102 —— 与 yu-format 钉死总数逐项一致;
   且其 to_bytes(KEY) == 原文件(异或对合 + 段布局的第三方佐证)。
② core/ystb.YSTBFile.guess_key_from_bytes(yst00000) = 0x96ac6fd3
   —— 他们的密钥猜测启发式独立恢复出同一密钥(第三方佐证)。
③ core/encoding.detect_text_encoding:
   官方日文文本池(76 条/2,919 字节)→ shift_jis(= yu-format auto → cp932);
   GBK 中文样本 → gbk(与 yu-format auto 同判)。
   注:评分公式未采用他们的(本仓库重造为确定性裁决),交叉验证是
   "结论级"而非"逐分值级"——这是 auto 部件当前的验证深度。
```

RxYuris 为 C++,未编译执行;其 YSTL/YSCM 布局解释已在本语料上逐字节
吻合(等于事实性交叉验证),密钥处理与 `YSTB_Coder::Xor` 分段语义与
Script-Editor、本实现三方一致。

- 纪律:零 git 写操作;未使用 <game-b> 语料;逐文件头注明参考来源,
  致谢链登记于 `docs/references/open-source-tools.md` §6。

## 成果 D1 补记(2026-10-19)· yu-trace 代码审查修订

审查对象:`crates/yu-trace` 全部源码与测试。发现并修复 7 项,协议升至 **schema 2**
(按任务书"字段增删必须递增"规则;v1 从未被外部消费,但版本历史如实留痕)。

**修复项**:

1. **输入摘要帧窗口错位(确定性缺陷)**:`yu-rt` 的 `tick/tick_at` 首帧先
   `advance(scheduler.frame()+1)`(新建 VM 首帧 = 1),原摘要覆盖 `[0,N)` ⇒ 漏掉
   最后消费帧、多算一帧。改为 `[1,1+N)`;新增 `input_summary_range` 供非新建 VM 回放。
   回归用例:`event_trace::input_summary_window_matches_consumed_frames`。
2. **回放无法闭合(设计缺陷)**:header 原先只记 entry/fps/start_ms/frame_ms,
   而 `BootConfig` 还含 script_key/debug_mode/argv(审查期间 C 又新增
   screen_color/paths/media_readonly/os_sound/dx_mode)。补齐 `boot.script_key`、
   `debug_mode`、`argv{len,sha256}`、`host{screen_color,media_readonly,os_sound,dx_mode,
   paths{len,sha256}}`;路径与命令行只记摘要(禁止绝对路径入库),回放时校验。
   新增 `VmTrace::from_header` + `BootSummary::to_config` + `verify_input(_range)`。
   回归用例:`header_replays_to_a_byte_identical_trace`(录制→from_header 重跑→逐字节相等)。
3. **行数语义错误码合并**:原"少行/多行"都报 `trace.truncated`。按任务书"分别给独立
   错误码"拆为 `trace.length`(两侧各自合法但条数不同)与 `trace.truncated`
   (末行不完整/缺 LF/空文件);`trace.schema` 仍负责 seq 重复等结构错误。
4. **`vfs/list` 失败态未规范化**:协议文档要求失败时 `entries` 为空,解析器原先未强制;
   现在带 `diag` 且 `entries` 非空 ⇒ `trace.schema`。
5. **`TraceLog::push` 可写入 header**:会产生"自己解析不回来"的文件;现在 panic 并
   指向 `push_header`/`prepend_header`(带 `#[should_panic]` 用例)。
6. **`write_trace` 路径校验漏洞**:`"."`/`".."` 可通过;已拒绝并加用例。
7. **jump/domain 字符串双份来源**:生成侧(`event.rs`)与解析校验侧(`record.rs`)各一份,
   存在漂移风险。统一为 `JUMP_KINDS`/`DOMAIN_NAMES` 单表 + 全变体/唯一性用例。

**并行适配**:审查期间 C 扩展了 `BootConfig`(screen_color/paths/media_readonly/
os_sound/dx_mode)。D 按约定适配(不改 yu-rt):`BootSummary::from_config/to_config`
使用**穷尽字段字面量**,C 再增字段会使 D 编译失败 ⇒ 强制一次 header schema 评审。

**复核原始输出**:

```text
$ cargo test -p yu-trace          # YURIS_SAMPLE_ERIS_YPF=<official data.ypf>
lib:            31 passed
diff.rs:        13 passed
equivalence.rs:  6 passed
event_trace.rs: 12 passed   (含 from_header 逐字节回放、输入摘要窗口回归)
golden.rs:       2 passed   (fixture 3,998 B < 32KB,schema 2)
real_sample.rs:  2 passed   (官方 data.ypf;本机 debug 构建约 144s,成本在 yu-rt 启动)
roundtrip.rs:    7 passed
Doc-tests:       0 passed
$ cargo clippy -p yu-trace --all-targets   → yu-trace 0 告警
$ grep println!/eprintln!/dbg!             → 0
$ grep SystemTime/Instant                  → 0
$ 其它 crate 依赖 yu-trace                 → 0
```

`SCHEMA.md` 已同步到 v2(header 新字段、argv/host paths/输入摘要算法、`trace.length`/
`trace.replay` 错误码、diff 行数语义、版本历史)。本次仍只改 `crates/yu-trace/**`
与本文件追加;未执行任何 git 写操作。

### 追加(同日)· 端到端交叉验证与 7 类真缺陷修复

按"每一条都实现、继续对齐到完美 runtime"的要求,用**官方入口链**
(`SYSTEM_START → es.ERIS`)做端到端交叉验证,而不是停在单命令。

**发现的真缺陷(全部有真实语料证据,已修)**:

| # | 缺陷 | 证据 | 后果 | 对应 DIV |
|---|---|---|---|---|
| 1 | `eval_lvalue` 跳过**全部** `0x29`,只有末条才是左值标记 | ERIS s2 pc16 `$arr[@cnt(0)-1]` 真实字节 | 441 次负下标;数组追加全错 | DIV-RT-020 |
| 2 | `VARINFO[LENGTH]` 对数组返回 0 | ERIS 用 `LENGTH` 对数组取元素数 | `DIMSIZE+=1` 后下标算成 -1 | DIV-RT-020 |
| 3 | `DIMSIZE` 忽略复合码(`+=` 当绝对值),且**重填整个数组** | 语料 104/104 都是 `+=`;`VAR.ADD` 宏 | 13 次追加只剩最后 1 个 | DIV-RT-019 |
| 4 | `IF` 先求值后压块帧(引擎是先压帧) | 旁线 §3.13 opcode 45 | 条件求值失败时 `IFEND` 失配 | DIV-RT-027 |
| 5 | `ELSE` 只接受 3 窗口 | 商业包 726 次 0 窗口 | 商业包此处 Fatal | DIV-RT-018 |
| 6 | `VARACT` 把系统变量源当非法;`INIT/G_*` 未实现;缺 SET 检查过早 | ERIS `ystdlib.yst` 29 处 `$_PSTR` 源;s12 `VARACT[INIT G_*]` | stdlib 全废;全局恢复缺失 | DIV-RT-007/024 |
| 7 | `SYSTEMINFO`/`TASKINFO` 归为"域命令"未实现 | ERIS 启动链硬依赖(缺 → 除零) | 帧 1 除零、主线终止 | DIV-RT-023/025 |

**附带**:`@_MOUSE_L=5` 一类输入系统变量写入(手册标 ○,ERIS 注释
"切场景时让输入失效")实现为设定帧计数器(DIV-RT-021);色深/路径/`_DXMODE`/
`_MEDIA_READONLY`/`_OS_SOUND` 收进 `BootConfig` 由宿主注入(DIV-RT-022);
`yst_list` 逐脚本文本数按实测字段解析(DIV-RT-026)。

**跨语料交叉验证(原始输出)**:

```text
$ cargo run -p yu-rt --example run_eris -- <project-eris> 12
frame 0: cmds=1290322 errors=0
frame 1: cmds=329227  errors=0
frame 2: cmds=26      errors=0
frame 3: cmds=249     errors=0
frame 4..11: cmds=8765 errors=0        ← 稳态

$ cargo run -p yu-rt --example run_game -- <<game-b>> 8
  ysc: v494 (119 命令)  labels=2132  vars=2568  yst_list=160  yscfg=1100x825
  ycd: 不存在(商业发行版;系统变量改用引擎 ABI 表 + ysv 校验)
frame 0: cmds=850609 errors=0
frame 1: cmds=307356 errors=0
frame 2: cmds=26     errors=0
frame 3: cmds=248    errors=0
frame 4..7: cmds=8681 errors=0        ← 稳态

$ cargo test -p yu-rt
lib 31 + corpus 8 + corpus_golden 4 + fixture 21 + golden 3 = 67 passed; 0 failed
$ python crates/yu-rt/scripts/audit_hardcode.py
audit: files=15 violations=0
```

**A1 执行层等价性**(新增永久回归测试):

```text
loose  = ([1290322, 329227, 26, 249, 8765, 8765], 0, "cd1790…4bd2e")
packed = ([1290322, 329227, 26, 249, 8765, 8765], 0, "cd1790…4bd2e")
```

散盘 `data/` 与官方 `data.ypf` 逐帧命令数、错误数、**变量域终值 SHA-256** 完全相同。
另核对:两处 ysbin 的 143 个 `.ybn` 逐文件 SHA-256 **143/143 一致**(diff=0)。

**剩余未实现(均为域命令或手册缺页,已在事件流逐条挂号)**:
`CG/CGACT/CGEND/CGINFO`(图形)、`SOUND/SOUNDINFO`、`WINDOW/WINDOWINFO`、
`FONT/FONTINFO`、`FILEINFO`、`EMOTE`、`RETURNCODE`(无手册页)、`WORD`(无页)、
`SYSTEM`/`INPUT`/`MOUSE`(域)。ERIS 稳态帧里 `CG` 出现 3 万+ 次 —— 下一步
接 `yu-gfx` 时按事件流逐条实现。

## 成果 16 (2026-10-19) · 四线代码审查(A/B/C/D)与 REA 复核

- 范围:`yu-log`/`yu-vfs`(A)、`yu-format`(B)、`yu-rt`(C)、`yu-trace`(D) 全量源码 + 测试;
  证据要求:官方手册页、官方产物字节、反编译/REA、代码行四类分级;纯代码自证不作为语义证据。
- REA 直接复核(官方 0.495 `yu-ris.exe`,sha256 `4f5eefa3…`):
  - `FUN_00462a10`:恒等表 + **恰好 12 对交换**,无版本分支;同函数生成 CRC32("Yu-ris") 四字节。
  - `FUN_0045e3b8`:每个包索引起点恒为 `0x20`,步长 `nameLen+0x1B`,
    `T[0xFF-lenEnc]`、名字逐字节取反,循环内**无版本分支**。
  - `FUN_00404398`:任务泵按等待计数器(+0x18)与 `timeGetTime` 期限(+0x1c)门控;
    处理器表返回非 0 = 让出;`FUN_004046dc` 是按索引取链表节点,不是优先级排序本身。
  - 勘误:旧笔记中的 TASK 处理器地址 `0x4506f4/0x4521dc/0x4567c0` 在 0.495 官方 exe 中
    不是函数入口(REA 落在图形/变量恢复函数),不能作为 0.495 的 TASK 行为证据。
- 结构性问题:`yu-format` 当前**零依赖者**;`yu-rt` 自己重实现了 YSCM/YSLB/YSVR/YSTL/YSCF/YSTB
  解析(`crates/yu-rt/src/tables.rs`),形成两份真相。B 的格式缺陷暂不影响运行核,但两边会漂移。
- 主要确认缺陷(详见审查报告):
  - rt 编排与手册冲突:主任务 ID 应为 `SYSTEM`(代码 `@main`);`TASK/TASKEND` 无 ID 应作用于自身
    (代码落到 `@main`);`TASKEND` 不停止当前泵/不移除任务;显示文本 `_` 不让出;Z 优先级未参与调度;
    ERIS 的 10 处 TASK 全部因 TINT/SCRIPTPOS/RESTART 未实现而未生成。
  - rt 可触发 panic:`IFBREAK[LV=0]`、`GOSUB` 参数名 `PINT0/PINT17+`、`VARACT[DIMSIZE]` 溢出、
    数组维长乘积溢出/巨型分配。
  - vfs:未验证的 NUL-XOR 回退会把结构不匹配的包读成 `Ok` + 垃圾名字;
    版本化 8/10 对表与 18/26 字节 meta 与 REA 的“单一 12 对表”冲突;36 字节输入可触发放大分配 208MB。
  - format:YSTB 头 count 被忽略、回封丢弃尾部;YSCF 宽高按 u16(引擎为 u32);
    TASK 非首位 `#` 标签漏收;32 位目标存在回绕 panic。
  - trace:header 含内核版本/prose 且被严格 diff;无按子系统投影;`tick_at` 时钟与 codec 未回放闭合;
    `hex4` 对坏 `\u` 会 panic;缺样本静默 skip。
- 日志:`Diag` 结构化程度好,但脚本错误无 task/script/pc 定位(`diag::at_script` 从未调用)、
  多处 `let _ =`/类型回退吞错、`ChainReport.errors` 不含可恢复 Unimplemented,导致 E2E 测试“零 Fatal”假绿。
- 本次仅审查与文档记录;未修改任何源码,未执行 git 操作。

## 成果 17 (2026-10-19) · 修复会话任务书 E

- 新增 `docs/tasks/task-e-fixes.md`:把成果 16 的审查结论转成可执行修复清单
  (F=yu-format / V=yu-vfs / R=yu-rt / T=yu-trace / P2=性能),每条带证据与验收。
- P0 重点:format 的 TASK 非首位 `#` 标签、YSTB 头 count/尾部、YSCF u32;
  vfs 的 NUL-XOR 回滚、版本表对齐 REA、64 位 offset、hash 校验、条目上限;
  rt 的 SYSTEM 主任务 ID、TASK/TASKEND 自身目标与立即停止、`_` 让出、
  Z 优先级、ERIS TASK 阻塞项、panic 收口、Diag 定位、解析器统一。
- 并发守则:启动先检查 10 分钟内被改文件;C 仍活跃时禁改 yu-rt/yu-vfs/yu-log,
  先做 format/trace,再复查;blocked 项只报告不抢写。
- 仅文档改动;未执行 git 操作。

## 成果 C3 (2026-10-19) · yu-rt 第三轮对齐:文本流 / 按钮码 REA / 手册字节级重审 / 热路径 / panic 收口

> 承接任务书 C v2 与用户指示「抓游戏事件、ERIS 有源码、宏要能正常执行、
> 手册没覆盖的细节都要考虑到、和官方保持一致」。本轮全部改动在
> `crates/yu-rt/` 与文档;零 git 写操作。

### 1. 源码 ↔ 编译产物 ↔ 手册/社区 三方对齐(新实现)

- **文本流三件套**(DIV-RT-028):一行剧本文本被 YSCom 编译为
  `WORD(段)→[_($_TEXT(239)+$_TEXT(240))]→WORD(段)→RETURNCODE(n)`。
  `RETURNCODE` 双表无参,唯一窗口 `kind` = **每脚本文本行计数**(137 文件
  396 处全量 1..N 递增,回归钉死);`_` = 手册 `basic/basic.html`
  ●テキスト表示的 `_[式]`(撤销 DIV-RT-008 旧挂账);`WORD` 载荷 =
  CP932 原始字节(RxYuris `YSTB_TextEditor_V5.cpp` 同法互证,
  YSTL `text_count` == WORD 数)。新增 `VmEvent::ReturnCode`。
- **按钮码 `_CODE_*`(DIV-RT-029,REA)**:0.495 引擎 `FUN_00427350`
  证得——`_INP(n)` 与 `_KEY_*`/`_MOUSE_*` 共用一张 VK 计数表
  (`0x685800`,guard `n<=0→0`);`_CODE_*` = 运行期 BSS 字节表(静态不可读),
  由手册不变量 `@_INP(@_CODE_Z)≡@_KEY_Z` ⇒ VK 值 + JP106 OEM 映射;
  手柄纯算术 `384+n*36(+4+m%32)`。落成数据文件
  `data/button-code-abi.tsv`(113 基名;L1..3 同基码);手册 ○ 写 =
  键位重映射(`InputState::set_code`)。
- **TASK[TINT..10/TFLT..10/TSTR..10]**(DIV-RT-015 部分):两版手册均无页
  (旧注误引已勘误),语义取自官方 ERIS 源码生产者/消费者对
  (`es_ERIS.yst:524 TASK[TINT=…] → :533 @_TINT(1)`;stdlib `es._task` 同型),
  寄存器随新生 ExecState 落位。
- **TASKINFO[SCRIPTPOS] ↔ TASK[SCRIPTPOS]**(手册 taskinfo/task.html 有页):
  不透明位置 blob(8 字节 script_id+pc)往返;TASK 无 `#` 形态 = 位置装载。
- **状态系统变量**(DIV-RT-031):`$_TEXT`(WORD/_ 追加,写替换)、
  `○@_MENUCODE`、`○@_WINDOW_EXEC_MAX/CLOSE`(内核状态 + 宿主 setter)。
- **域归类修正**:`FONT/FONTINFO` → Graphics 域(手册 `font/` 有页),
  不再报「命令无处理器」。

### 2. 手册字节级重审(DIV-RT-030)

SDK 手册 html 含非文本字节,普通 grep 按 binary 跳过 → 曾把
`_WINDOW_EXEC_CLOSE`/`_MENUCODE` 等误判「无手册页」。全 548 名按字节级
重审:族谱(`_KEY_〜`/`_CODE_〜`/`_PAD_〜`/`_SERIAL_〜` 等)确认有页;
**63 项非族谱名确认真无页**(`_OS_MEMUSE`/`_MEDIA_TYPE`/`_DROPFILE`/
`_VARTYPE`/`_CHR`/`_SYSTEM_FRAME` 族/`_CGSET` 等,继续挂账)。错误引用
全部勘误:`_TINT`(曾误引 svariable)、`_KEYLAST/_MOUSELAST`(曾误引手册,
现改挂账 DIV-RT-033,InputState 内部照常记录)、`$_PROJECT_PATH`
(手册无页,按路径族宿主注入处理)。

### 3. panic 收口(任务书 E R7,DIV-RT-034)

四条可硬崩路径全部转 Fatal 诊断:①`IFBREAK[LV=0]`(越界 panic → no-op);
②GOSUB/RETURN 参数名 `PINT0/PINT17+`(回绕/OOB → 范围检查);
③`VARACT[DIMSIZE=<巨型>]`(分配 abort → 2^26 防御上限);
④数组维长乘积(回绕 → checked_mul+上限)。三条「修复前 panic / 修复后
Err」回归入 fixture(复现:`cargo test -p yu-rt --test fixture panic`)。

### 4. 热路径与可观测性税

- 启动期按工程 yscm 名字解析 **opcode→处理器表**(`HandlerKind`,
  替代每命令字符串 match);`BootConfig::record_events`(默认开,关闭时
  `Diag`/挂号照常);`Cmd`/表达式栈/解码缓冲三重复用。
- 基准(ERIS 4 帧 292.9 万条):300k → **582k cmds/s**(events 关);
  events 开 316k(事件流为可观测性税,可关)。

### 5. 验证(原始输出)

```
$ cargo test -p yu-rt --release        # 73 项全绿
  lib 31 ok / corpus 9 ok / corpus_golden 4 ok / fixture 26 ok / golden 3 ok
$ python crates/yu-rt/scripts/audit_hardcode.py
  audit: files=15 violations=0
$ cargo run --release --example bench_eris -- <eris> 4 0
  boot=74ms frames=4 cmds=2928592 run=5.03s (582436 cmds/s)
$ cargo run --release --example diag_census -- <eris> 8   # 8 帧诊断普查
  62(上轮) → 33 条:余 = 域命令 17 种(宿主)+ 手册无页挂账 16 条
$ ERIS E2E: frames=[1290326, 332550, 3315, 1302401, 15331, 15340, 15320, 15331]
  零 Fatal;FADE 任务链(TINT 参数)首次真实激活,主循环持续运行
$ 商业包 <game-b> v494(无 ycd): frames=[850613, 310456, 3091, 814287,
  14511, 14521, 14511, 14511],零 Fatal
```

钉值更新(钉值=回归金丝雀,变化必须显式注明):ERIS 首帧
1290322→1290326、商业包首帧 850609→850613(新语义路径 +4);
「稳态帧恒等」断言作废——FADE 链激活后每帧随场景切换进度波动,
这比旧的 TINT=0 空转稳态**更接近官方引擎行为**。

### 6. 文档与合规

- `docs/divergences/`:DIV-RT-028..034 新增;006/008/011/015 状态更新;
  三处错误引用勘误留痕。
- `crates/yu-rt/docs/command-provenance.md`:新命令行(WORD/RETURNCODE/_/
  TASKINFO SCRIPTPOS)与新系统变量行(_TEXT/_CODE_*/_INP/_MENUCODE/
  _WINDOW_EXEC_*)入「已实现命令 × 三层出处」表;§4.5 E2E 数字更新。
- 真实绝对路径 → AGENTS §1 占位符(`<side-repo>` 等)。
- 新增公开 API:`Vm::tables_mut/input_state_mut/set_menu_code`、
  `ScriptFile::pool`、`HandlerKind/build_handler_table`(crates 内)。
- 零 git 写操作。

## 成果 E1 (2026-10-19) · 审查缺陷修复(F/T/V 完成;R 阶段 blocked)

按 `docs/tasks/task-e-fixes.md` 执行。并发守则实测:R 阶段开始时 `yu-rt` 仍有
10 分钟内改动(`src/value.rs`、`src/vm/handlers.rs`、`tests/corpus.rs`),按守则
**不对 yu-rt 写入**,R1–R12 标 blocked(细节见末尾);C 会话已自行推进其中一部分。

### 完成编号

- **F1–F6(yu-format)**:TASK 非首位 `#` 按**窗口 slot id** 索引;YSTB 以头 count
  迭代并保留尾部字节;YSCF 宽高改 u32 且按引擎无符号减法判定;YSTB/YSTL/YSTD/
  YSLB/YSVR/YSER 全部 checked 算术 + `fmt.limit`;错误密钥结构自检 → `fmt.ybn.key`;
  V2 版本带改 `fmt.ybn.unsupported` 并写 DIV-009。
- **V1–V9(yu-vfs)**:删除 NUL-XOR 的 `Ok` 回退;版本表统一为 0x20 + 12 对 +
  22 字节 meta;`dataOffset` 按 u64;payload hash 无条件校验;重复名 Fatal;
  `is_verified()` 要求 0x20/0xFF;entry_count 上限 99,999 + `index_bytes/27`;
  key 探测满分提前退出;索引错误写实际失败偏移到 `Loc::Data.offset`。
- **T1–T2(yu-trace)**:`hex4` 改字节级解析(跨 UTF-8 字符边界不再 panic);
  JSON 嵌套上限 128;F/V/T 三个 crate 的样本缺失在 `YURIS_REQUIRE_SAMPLES=1`
  时硬失败,否则打印 `SKIP[count=N]`(format/vfs 由 `println!/eprintln!`,
  trace 由 `trace.sample.missing` Info 诊断)。

### 证据(REA 直接复核 0.495 `yu-ris.exe`,sha256 `4f5eefa3…`)

- `FUN_004518e9`:YSTB 以 `+0x08` count 为循环上界、`argc × 0xC` 推进窗口;
  四段各自从密钥下标 0 异或。
- `FUN_0045dbb0`:YSCF 宽高按 **u32** 读;`(w!=0||h!=0)` 时按
  `0x1F80 < (u32)(w-0x80) || 0x1FF0 < (u32)(h-0x10)` 判定(等价 0x80..=0x2000,
  且宽高必须同时为 0 或同时有效)。
- `FUN_00462a10`:长度表 = 恒等表 + 恰好 12 对交换,**无版本分支**。
- `FUN_0045e3b8`:索引起点恒 `0x20`,步长 `nameLen + 0x1B`,名字逐字节取反。
- `FUN_0043fb9c`:从条目读 **8 字节**偏移(`dataOffset` = u64,旧 `unknown0` 是高半)。
- `FUN_0043f71c`:payload MurmurHash2 **无条件**比较,0 不是哨兵。
- 真实语料:`yst00008.ybn` 第 232 条 TASK 窗口 id = 0/34/3/4,slot 34 = `#`,
  载荷 `"es.SS.FADE.TASK.MAIN"`;源文件 `es_ERIS.yst:524` 为
  `TASK[ID=es.SS.FADE.TASK #=es.SS.FADE.TASK.MAIN ...]`(ID 是任务 ID,不是标签)。
  `ysl.ybn` 对应标签 `(script_no=8, field_b=232/234)`。

### 修复前失败 / 修复后通过(原始输出摘录)

```text
# F1(修复前)
panicked at crates\yu-format\tests\corpus_eris.rs:216:
  left: []  right: [("es.SS.FADE.TASK", 232), ("es.SS.FADE.TASK.MAIN", 234)]
# F2(修复前)
thread ... panicked at synthetic.rs:519: 必须按头 count=1 解码: left: 2 right: 1
# F3(修复前)
thread ... panicked: 宽 0x2000 在引擎的无符号范围内,必须接受
# F5(修复前)
called `Result::unwrap_err()` on an `Ok` value: Segments { inst: [233, 76, ...] }
# T1(修复前)
`&self.src[self.pos..self.pos + 4]` 在 `\uあ` 上跨字符边界 panic
# V1(修复前)
结构不匹配包 → Ok + 解码垃圾名;V300 wrong12 包可读
# 修复后
cargo test -p yu-format -p yu-vfs -p yu-trace --no-fail-fast
  6 / 1 / 23 / 33 / 13 / 6 / 12 / 2 / 2 / 7 / 40 / 12 / 2 / 4 passed; 0 failed
YURIS_REQUIRE_SAMPLES=1 <同一命令>:全部 ok(含官方 data.ypf / ERIS ysbin 真样本)
cargo clippy -p yu-format -p yu-vfs -p yu-trace --all-targets:三 crate 0 告警
grep -rn "println!\|eprintln!\|dbg!" crates/*/src → 0
```

### 全工作区验收(23:00 运行时)

```text
cargo test --workspace --no-fail-fast
yu-format 6+1+23 / yu-log 13 / yu-rt 31+9+4+23+3 / yu-trace 33+13+6+12+2+2+7 /
yu-vfs 40+12+2+4  → 全部 ok,0 failed
```

(注:本机 `TEMP` 目录上某些 ephemeral 子目录会 `PermissionDenied`;
测试运行时另设 `YURIS_TEST_TMP=<repo>/target/test-tmp` 规避,属环境问题,
不是代码缺陷。)

### 改动文件

- `crates/yu-format/src/{ystb,yscf,ystl,ystd,yslb,ysvr,yser,code}.rs`
- `crates/yu-format/tests/{synthetic,corpus_eris}.rs`
- `crates/yu-vfs/src/ypf.rs`
- `crates/yu-vfs/tests/{equivalence,nul_xor,real_sample,support/ypf_writer}.rs`
- `crates/yu-trace/src/{json,event,record}.rs`
- `crates/yu-trace/tests/real_sample.rs`
- `docs/divergences/DIV-002`(→fixed)、`DIV-004`(→fixed)、新增 `DIV-009`
- 仅本文件追加;未执行任何 git 写操作。

### blocked · R1–R12(yu-rt)

复查时 `yu-rt` 仍在活跃写入(`src/value.rs`、`src/vm/handlers.rs`、
`tests/corpus.rs` 于 23:0x 仍在变),按并发守则**未写入**。已读现状与任务书对照:

| 编号 | 现状(只读核对) |
|---|---|
| R1 | **未完成**:`MAIN_TASK_ID` 仍为 `@main`;`TASK/TASKEND` 无 ID 仍落到 `@main` |
| R2 | 部分:C 已改为发 `Effect::FinishTask`;但缺省目标仍是 `@main`,"立即停止当前泵"未见 |
| R3 | **未完成**:`op_text_out` 仍返回 `Continue`,未让出并发 `VmEvent::Text`(已有事件) |
| R4 | **未完成**:调度按链表序,`priority` 字段未被排序使用;SYSTEM 默认 100 未落地 |
| R5 | 部分:C 已实现 TINT/TFLT 寄存器与 SCRIPTPOS 读;RESTART/TEXTPOS 仍 Unimplemented |
| R6 | 部分:`op_end` 返回 `Finish` 并发 `End`;`RESTART=1` 仅记录,未实现重启 |
| R7 | 部分:`PINT` 已加范围诊断;`IFBREAK[LV=0]`、`VARACT[DIMSIZE]`/数组维长乘积的 checked 收口未逐条确认 |
| R8 | 未核对完成(需逐条回归) |
| R9 | 未确认 GOSUB 64 上限;`IFCONTINUE` 帧累积未改 |
| R10 | 部分:有 `TaskFinish`;`FrameEnd` 与事件容量上限未确认 |
| R11 | 部分:`diag::at_script` 是否接线未确认 |
| R12 | **未完成**:`crates/yu-rt/src/tables.rs` 仍存在,未改为依赖 `yu-format` |

交接建议:由 C 会话继续 R1/R3/R4/R12(这四项是行为对齐的关键);R2/R5/R6/R7/R10/R11
按上表逐条收尾;每项补回归后再更新 golden。

## 成果 18 (2026-10-20) · 优化后快速复审(E1 + C3)

- 验证:E1 的 F/T/V 修复与 C3 第三轮对齐已在工作区;全工作区
  `YURIS_REQUIRE_SAMPLES=1 cargo test --workspace --no-fail-fast`
  共 249 个用例 **全部通过**(含官方 data.ypf 94s、ERIS ysbin 117s 真样本套件)。
- 抽查确认:
  - format F1 已改按窗口 `slot` 索引 ysc 参数名;F2 头 count/尾部;F3 YSCF u32;
    F4 checked 算术;F5 `fmt.ybn.key`;F6 V2 挂 Unsupported。
  - vfs V1 NUL-XOR 拒绝;V2 仅 0x20/12 对/22 字节;V3 dataOffset u64;
    V4 payload hash 无条件;V6 `is_verified` 严格;V7 entry cap;V8 提前退出。
  - trace T1 `hex4` 字节级解析 + 深度上限;T2 样本强制。
  - rt R7 四条 panic 路径收口;R5 的 TINT/TFLT/TSTR 与 SCRIPTPOS 已落地。
- 仍未完成(E1 标记 blocked、C3 未覆盖):
  R1 主任务 ID/SYSTEM 与 TASK/TASKEND 无 ID 自身目标;
  R2 TASKEND 立即停止(当前只置 finished);
  R3 显示文本 `_` 仍返回 Continue,未作为让出点(手册 `basic/task.html`);
  R4 Z 优先级字段存在但调度未使用;
  R9 GOSUB 上限仍为 63(基帧占 1 层,`vm.rs:208` + `handlers.rs:278`);
  R10 `FrameEnd` 从未发射、事件 Vec 仍无上限;
  R11 `diag::at_script` 从未调用;
  R12 `yu-rt` 仍未依赖 `yu-format`,解析器双份。
- clippy:全工作区无 error,但 `yu-rt` lib 有 6 条 warning、examples/tests 有未用 import 等,
  未达到“零告警”。
- 仅复审与文档记录;未修改源码,未执行 git 操作。

## 成果 E2 (2026-10-20) · 任务书 E 剩余 rt 项对齐(R1/R2/R3/R4/R9/R10/R11)

> 会话 E2。范围仅 `crates/yu-rt`(+ 一个 `yu-trace` golden 重生成)与文档;
> **未做 R12**;零 git 写操作。全部行为改动以 0.495 `yu-ris.exe`
> (sha256 `4f5eefa3…`)的 REA 反编译为证据,证据表见下。

### 1. 证据表(硬门槛逐项)

| 编号 | 手册页 + 原文 | 真实脚本位置/形态 | 引擎证据(REA,observed) | 结论 |
|---|---|---|---|---|
| R3 | `basic/task.html`「「ウェイト命令」か「表示テキスト」が来ると、タスクはそれを実行したあと、一旦休憩します(約0.016秒)」 | C3 已证一行 = `WORD→[_]→WORD→RETURNCODE`;`ystdlib.yst` 文本行同形 | `RETURNCODE` 处理体 `0x44c390`:`0x44c451 MOV byte [ESI+9],1`、`0x44c455 MOV EAX,-1`;泵 `FUN_00404398` `0x40442e` 检查 `task+9` 非零即跳过;`WORD` `0x458ee0` 成功尾 `0x459054 XOR EAX,EAX`, `_` `0x4567c0` 成功尾 `0x4569cc XOR EAX,EAX`;恢复判据 `FUN_004046fc` + 按钮表 `0x5b48ac..0x5b48bc`(MOUSE_L/ENTER/PAD×2/CTRL) | 让出点是 **RETURNCODE**,不是 `WORD`/`_`;一行文本一 vsync,直到推进输入 |
| R1 | `basic/task.html`「…「SYSTEM」というIDが名づけられています」;`control/task.html`/`taskend.html`「タスクＩＤを指定しなかった場合…実行した自身のタスク」 | `es_ERIS.yst:524 TASK[ID=… Z=@es.63BITMAX TINT=…]`;`stdlib/ystdlib.yst:759/781/789/816/824/850/858`(`TASK[E=0]`/`TASKEND[]` 无 ID 形态) | 启动链 `FUN_00465384` `0x4652f0 MOV EDX,0x807b00`("SYSTEM")→`0x414559` 按名查任务;`TASK` `0x45074a MOV EBP,[0x801720]`(当前任务);`TASKEND` `0x4521dc` 同取当前任务 | `MAIN_TASK_ID="SYSTEM"`,系统任务 Z=100;无 ID 目标 = 执行者自身 |
| R2 | `control/taskend.html`「タスクを終了します」 | `ystdlib.yst:850 TASKEND[]` / `:858 TASKEND[ID=$_PSTR(1)]` | `0x4521dc`:自身 → `CALL 0x452364`(双向链表解链 + `DEC [pool+0x20]`)→ `MOV EAX,-1`(让出);他人 → 摘链 + 返回 0;`0x45237f..0x4523b9` 为解链实现 | 立即摘链;同命令之后的指令不执行;`EXIST` 立即 0;同 ID 重建从新标签开始 |
| R4 | `basic/task.html`「優先度の高い順から実行されます」「同じだった場合の実行順序は不確定」;系统任务默认 100 | ERIS `Z=@es.63BITMAX` / `Z=@_PINT(3)`(stdlib `_task`) | 插入 `FUN_004613c0`:`+0x28/+0x2c` 组 64 位键 `jl` 前插(降序),等键插在既有节点之后;重配置 `FUN_0042f7bc` = 摘链后重插;启动 `FUN_004621d0` `0x4623f4` Z=0x64 | 降序按 Z;同 Z 取引擎实际序(创建序),钉死以保证 A3 |
| R9 | `control/gosub.html`「GOSUBのネストは64回まで可能です」;`control/ifcontinue.html`「直前の IF 命令まで戻り」 | ERIS 10,215 次 GOSUB;`ystdlib.yst` 深调用链 | `FUN_004459c4`:`0x4459e6 MOVSX ESI,[task+0x140]`、`0x4459ed LEA EAX,[ESI+1]`、`0x4459f0 CMP EAX,0x40; JE 报错`;`IFCONTINUE 0x446490` 深度=`depth-LV`(目标 IF 重新压帧);`IFBREAK 0x446408` 深度=`depth-LV+1`(保留目标层,由其 IFEND 弹) | 允许 64 次嵌套;第 65 次 Fatal;IFCONTINUE 丢弃目标层、IFBREAK 保留目标层 |
| R10 | 内部契约:`docs/architecture.md` §3/A3、`docs/oracle.md`(非官方语义) | — | — | 每逻辑帧 `FrameStart`/`FrameEnd`;事件队列有界 + `drain_events` |
| R11 | `docs/architecture.md` §8「每个 Fatal 报告自带…哪个脚本位置」 | 夹具坏标签用例 | — | `Diag.where_` 补 `Loc::Script{file,label=任务ID,pc,cmd}` |

### 2. 实现要点(改动文件)

- `src/task.rs`
  - `MAIN_TASK_ID = "SYSTEM"`;`TaskSeed::main` Z=100。
  - `TaskNode` 增 `text_rest`(`task+9`)与 `seq`(创建序);新增 `TaskView` 投影。
  - 调度:按 `(priority desc, seq asc)` 有序链(`insert_sorted`/`sort_nodes`);
    `upsert` 摘链后按新 Z 重插;`finish_by_id` 立即摘链;`find_by_id` 过滤已结束。
  - `StepOutcome::TextYield`(文本行尾让出);`TaskExecutor` 增
    `end_logical_frame`/`task_removed`/`text_advance_requested`/`sync_task_view`。
  - 帧尾 `retain` 前读取主线结束标记(`main_finished` 覆盖正常 END 路径)。
- `src/vm.rs`
  - `FrameEnd` 发射;`events` 有界(`DEFAULT_EVENT_CAPACITY = 1<<20`),
    队满**批量**丢弃最旧 1/4 并计 `events_dropped()`(逐条 `remove(0)` 会 O(n²));
    新增 `drain_events`/`with_event_capacity`/`events_dropped`。
  - `cur_script/cur_pc/cur_cmd/cur_task` 定位簿记(复用 String,零分配);
    `report()` 与错误路径经 `locate()` 补脚本位置;处理器内部 `Unimplemented`
    计入 `unimplemented_cmds`。
  - `TaskExecutor::task_removed` 回收执行位并发 `TaskFinish`。
- `src/vm/handlers.rs`
  - `RETURNCODE` → `TextYield`;`TASK`/`TASKEND`/`TASKINFO` 无 ID = 自身;
    `TASKEND[self]` → `Finish`(泵立即停),他人 → `Effect::FinishTask`;
    `TASK` 建任务/重配置时 T* 寄存器先清零再落参(REA `0x450916..0x450943`);
    目标=自身时直接改写当前执行位。
  - GOSUB 上限 64 次(`frames.len() > 64` 才报错);
    `IFCONTINUE` 改为丢弃目标层、`IFBREAK` 保留目标层。
  - `TASKINFO` 读 `task_view`(泵期间调度器被 `mem::take` 借出,原实现恒读到空链)。
- `src/input.rs`:`text_advance_requested()`(MOUSE_L/ENTER/CTRL/手柄计数器 > 0)。

### 3. 新增回归(全部通过)

`cargo test -p yu-rt --lib --test fixture --test golden` 原始输出:
```
test result: ok. 35 passed; 0 failed; ...        # lib
test result: ok. 38 passed; 0 failed; ...        # fixture
test result: ok. 3 passed; 0 failed; ...         # golden
```

| 用例 | 覆盖 |
|---|---|
| `task::tests::main_task_defaults_to_priority_100_and_id_system` | R1 |
| `task::tests::higher_priority_runs_first_and_equal_priority_keeps_creation_order` | R4 |
| `task::tests::taskend_of_self_stops_the_current_pump` | R2 |
| `task::tests::taskend_effect_unlinks_by_id_immediately` | R2 |
| `task::tests::same_id_rebuild_starts_at_new_entry_after_taskend` | R2 |
| `fixture::main_task_id_is_system_and_script_visible` | R1 |
| `fixture::taskend_without_id_ends_only_self_and_main_continues` | R1/R2 |
| `fixture::taskend_stops_neighbor_commands_and_rebuild_uses_new_entry` | R2 |
| `fixture::higher_z_task_runs_before_lower_z` | R4 |
| `fixture::one_text_line_per_frame_until_advance_input` | R3 |
| `fixture::gosub_allows_64_nested_calls` / `gosub_65_nested_calls_is_fatal` | R9 |
| `fixture::ifcontinue_does_not_accumulate_if_frames` | R9 |
| `fixture::frame_events_are_bounded_by_start_and_end` / `event_queue_is_bounded_and_reports_drops` | R10 |
| `fixture::script_diag_carries_task_script_and_pc` | R11 |
| `fixture::main_end_sets_main_finished` | 结束路径 |
| `fixture::task_without_tint_does_not_inherit_caller_registers` | T* 清零(REA) |

### 4. golden 变更清单(逐条理由)

| 文件 | 变更 | 理由 |
|---|---|---|
| `crates/yu-rt/tests/golden/logic_slice.jsonl` | `@main`→`SYSTEM`;+`frame_start/frame_end` | R1(手册+REA 启动链)、R10(帧边界) |
| `crates/yu-rt/tests/golden/input_counter.jsonl` | 同上 | 同上 |
| `crates/yu-rt/tests/golden/error_paths.jsonl` | 同上 | 同上 |
| `crates/yu-rt/tests/golden/corpus_{ev,menu,se}_define.jsonl` | 同上 | 同上 |
| `crates/yu-trace/tests/golden/synthetic_frame.jsonl` | 同上(seq 顺延) | 同上 |

更新方法 = `UPDATE_GOLDEN=1`(已有机制);更新后逐文件核对:**去掉帧边界行后,
新旧 golden 只差 `@main`→`SYSTEM` 一处**。

### 5. 真样本与全工作区验收

```
$ cargo test -p yu-rt --release --test corpus --test corpus_golden   # YURIS_REQUIRE_SAMPLES=1
E2E frames: [1290326, 332550, 3315, 1302412, 15340, 15320, 15331, 15320]
test result: ok. 9 passed; 0 failed; ...          # corpus
test result: ok. 4 passed; 0 failed; ...          # corpus_golden
loose  = ([1290326, 332550, 3315, 1302412, 15340, 15320], 0, "2bc95198…")
packed = ([1290326, 332550, 3315, 1302412, 15340, 15320], 0, "2bc95198…")

$ cargo test --workspace --no-fail-fast               # YURIS_REQUIRE_SAMPLES=1
CARGO_EXIT=0
```
- ERIS 首帧钉值 **1,290,326 保持不变**;后续帧因 Z 排序生效有 ±11 条差异
  (`1302401→1302412`、`15331→15340`),无 Fatal、主线不结束。
- 散盘 vs 官方 `data.ypf` 逐帧命令数 + 变量域摘要哈希 **完全一致**(A1)。

### 6. 性能(修 R10 时踩到的两个坑,已修)

- 一度把任务链投影**每条命令**克隆一次 ⇒ ERIS 首帧 130 万命令直接卡死;
  改为「每条任务泵一次 + 链变化时刷新」。
- 有界队列用 `Vec::remove(0)` ⇒ 队满后 O(n²);改为批量 `drain(..cap/4)`。
- 现状:`bench_eris` 4 帧 events 关 **615,094 cmds/s**(基线 582k);
  events 开 **212,205 cmds/s**(基线 316k;差额 = 帧边界事件 + 有界队列摊还)。
- `audit_hardcode.py`: `files=15 violations=0`;`grep println!/eprintln!/dbg!`
  在 `crates/*/src` = **0**。
- clippy:`yu-rt` lib = 基线 6 条(无新增),workspace 无 error。
  (注:本轮收尾时并行会话正在改 `yu-format` 的 `CommandTable/Segments`,
  其间 `--workspace` clippy 报的 E0063 属该会话在途改动,与本轮无关。)

### 7. 未完成 / 挂账

- **R12 未做**(任务书明确排除)。
- `TASKINFO` 的 `TEXTPOS/RESTART/A`、`TEXT[WAIT=…]` 对文本等待的影响仍
  `Unimplemented`(无手册页/无引擎证据);见 DIV-RT-035/015。
- 新增 7 条 divergence 档案:DIV-RT-035..041(`crates/yu-rt/docs/divergences/README.md`)。
- 行为差异记录:手册称同 Z 次序「不確定」,内核取引擎实际插入序(创建序),
  已写入 DIV-RT-038。

## 成果 E3 (2026-10-20) · R12 统一解析器:yu-rt 依赖 yu-format,删除重复表解析

> 任务书 E 的 R12(重构,非行为改造;唯一允许的行为面 = YSCF 宽高 u16→u32)。
> 基线取自成果 E2 完成态(`target/baseline-E3.log`);改后全量日志
> `target/final-workspace.log`、`target/final-corpus.log`。

### 依赖图变化

- `yu-rt` 新增依赖 `yu-format`(架构 §2 的 `RT → FMT` 落地);
  `cargo tree -p yu-rt | grep yu-format` 命中。
- 解码唯一来源:YSCM/YSLB/YSVR/YSTL/YSCF/YSTB 全部经 `yu-format`;
  YSCD 不在 yu-format 范围,仍在 `yu-rt/src/ycd.rs`(未动)。

### 删除的重复实现(旧 `crates/yu-rt/src/tables.rs` 行号)

- 容器解析底座:`MAGIC_YSCM/YSLB/YSVR/YSTL/YSCF/YSTB/YSTD/YSER`(L37-44,
  其中 YSTD/YSER 本就只有常量无解析器)、`YSTB_HEADER_LEN`(L47)、
  `WINDOW_SIZE`(L49)、`need`(L54)/`eight`(L64)/`u16_at`(L76)/
  `u32_at`(L81)/`cstr`(L86)/`check_header`(L104)、`xor_cyclic`(L844)。
- 逐表 `parse` 实现:YscmTable(L194)/LabelTable(L287)/VarImage(L403)/
  ScriptList(L531)/ScriptFile(L706)/EngineConfig(L611,整个结构删除)。
- 元数据表的字节入口签名改为 `from_format(fmt)`(审计 grep
  `fn parse(data` → 0);`ScriptFile::parse(script_id,data,key)` 按任务
  保留清单保留签名,内部 = `Segments::decrypt` + 区段类型化投影。
- 保留:运行时结构(BootTables/TableSet/ScriptFile/GroupRec/WindowRec、
  by_name 索引、pool/part4、first_window)、`ParamKind`/`Label`/`VarRecord`/
  `VarInit`/`ScriptEntry` 投影、`globals/locals_of/text_count/path_of`
  访问器、错误码包装、`DEFAULT_SCRIPT_KEY`(改为引用
  `yu_format::key::DEFAULT_KEY_BYTES`)。

### yu-format 加法式 API(最小,不改既有字段)

- `version: u32` 字段补进 `yscm::CommandTable`/`yslb::YslLabels`/
  `ysvr::VarDecls`/`ystl::ScriptList`/`yscf::Yscf`/`ystb::Segments`:
  运行核 provenance/`engine_version` 需要各表版本,解码层此前未保留。
- 无其它 API 变化。

### YSTL 字段勘误(唯一实质行为变化,已按用户复核裁决)

- 旧 rt 把尾部第 3 个 u32 当 `text_count`、第 5 个当 `file_no`;按
  yu-format 定性(第 3=`var_count`、第 4=`label_count` 与 ysl 逐条互证、
  第 5=`text_count`,`meta`/`file_no` 命名错位)修正,`meta`/`file_no`
  删除(无消费者)。
- 唯一受影响消费者 = `SYSTEMINFO[TEXTNUM]`:改后返回**真实文本条数**。
- 可观测影响(ERIS E2E,命令数):

  | 帧 | 改前(E2 基线) | 改后 | 说明 |
  |---|---|---|---|
  | 0(钉值) | 1,290,326 | 1,290,326 | 不变 |
  | 1 | 332,550 | **274,466** | 启动期 `SYSTEMINFO[TEXTNUM,NO=1..n]` 逐脚本查询循环,值修正后下游工作量变化(−58,084) |
  | 2..7 | 3,315 / 1,302,412 / 15,340 / 15,320 / 15,331 / 15,320 | 同左 | 逐条不变 |

  loose/packed 等价保持(两侧同为新值);变量域哈希
  `2bc95198…` → `18a94a71…`。**钉值未改**(corpus 钉的是首帧与">0"
  断言,均不变);是否把新值钉进回归,留主人裁决。
- YSCF 宽高 u16→u32:ERIS 800x600 数值不变,`_SCREEN_SIZE_X/Y` 输出
  不变(读 `vm.screen()`,来源改为 `Yscf.width/height` u32)。
- 容错面差异(仅畸形输入;真实语料零差异):YSTB「精确闭合」改为
  format 语义(count×4≤s1 富余保留 + trailing 保留);YSLB/YSVR/YSTL
  旧闭合检查随 format 不再重复;YSVR kind/ty 越界、YSCF 引擎范围、
  YSTB 结构自检(`fmt.ybn.key`)由 format 带入(更严)。缺表仍
  `table.missing`、结构缺陷包装为 `table.bad_magic`/`table.incomplete`
  (fmt 原诊断进 `causes`)。

### 夹具修正

- `tests/support/mod.rs` `build_ystl`:0 条清单补齐 16 字节容器头
  (format `read_header` 下界;真实语料不存在 0 条清单)。其余夹具
  (37 串+256 尾表、0x400 桶表、20 字节尾部)本就与 format 兼容。

### 新增测试

- `crates/yu-rt/tests/consistency.rs`(3 项,真实 ERIS 语料):①元数据
  表投影逐字段同源(name/args/ParamKind×flag 字节交叉/检查器、标签
  字节键→script/pc/base、变量 kind/group/script/slot/type/dims/初值
  按位、清单 index/path/var_count/label_count/text_count/file_time、
  YSCF 800x600);②136 个 YSTB 的 ScriptFile 投影 ↔ `Segments` 逐字段
  (组/窗口/池/part4/源码位置,组数 36,974 / 窗口 80,102 钉住);
  ③TableSet 生产装载路径 ↔ 直解(入口标签、config、按需装载脚本、
  `vm.screen()==(800,600)`)。缺样本显式 skip,`YURIS_REQUIRE_SAMPLES=1`
  硬失败。
- `tables.rs` 单测 `script_file_projection_roundtrip`(合成 YSTB 投影)。

### 验证(原始输出)

```
$ cargo test --workspace --no-fail-fast        # YURIS_REQUIRE_SAMPLES=1 + ERIS/SDK 样本
  exit=0;268 项全 ok,0 failed(含 corpus 9 / corpus_golden 4 /
  fixture 38 / golden 3 / consistency 3 / 真样本 data.ypf+ysbin 套件)
$ cargo test -p yu-rt --test corpus -- --nocapture   # 最终态
  E2E frames: [1290326, 274466, 3315, 1302412, 15340, 15320, 15331, 15320]
  loose  = ([1290326, 274466, 3315, 1302412, 15340, 15320], 0, "18a94a71…")
  packed = (同上)                                    # 9 passed; 0 failed
$ cargo tree -p yu-rt | grep yu-format         # 命中
$ grep -rn "MAGIC_YSCM|…|fn check_header" crates/yu-rt/src   → 0
$ grep -rn "fn parse(data" crates/yu-rt/src/tables.rs        → 0
$ cargo clippy -p yu-rt -p yu-format --all-targets
  新增告警 0(存量 15 处不变,另修复 1 处存量 doc 缩进;
  本会话引入的 3 处已全部清零)
```

### 并发注记

- 基线期间成果 E2 会话仍在收尾(11:58 最后写 `handlers.rs`;12:05 向
  `fixture.rs` 追加 `task_without_tint_does_not_inherit_caller_registers`,
  该测试在本重构下通过)。12:05 后仅本会话写入。
- 未修改 `<repo-old>`;零 git 写操作;真实路径均以占位符表述。

### E3 续记(同日,主人裁决后)

- **钉入 TEXTNUM 修正后的新钉值**:`eris_entry_chain_runs_end_to_end`
  新增 `counts[1] == 274_466`(第 2 帧启动期 TEXTNUM 查询循环,证据链
  见上文「YSTL 字段勘误」);商业包 850_613 钉值加注警示(本机无样本,
  TEXTNUM 修正可能使其漂移,失败时按 E3 解释重新基线)。
- **兜底日志(主人要求)**:商业发行版无 `YSCom.ycd` 时,系统变量启用
  内嵌引擎 ABI 兜底**不再静默**——启动链发 `table.fallback`(Info,
  Boundary::Table,定位 `YSCom.ycd`):说明兜底已用工程 `ysv.ybn` 逐项
  校验(DIV-RT-016)、命令表/参数名仍来自工程 `ysbin/ysc.ybn`(数据
  驱动,无版本兜底)。日志只进诊断汇(`Vm::diags`/`take_diags`),
  **不进事件流**(事件流是对齐对象,golden 不因此变化;表来源另见
  `table_provenance`)。新增错误码 `table.fallback`(code.rs + ALL_CODES)。
- **新增测试** `fallback_abi_logs_notice_on_real_data`(真实 ERIS 数据,
  单根无 aux 启动):ycd=None、sysvars=548、yscm=119(命令来自工程表)、
  诊断汇含 `table.fallback`、事件流不含之。
- 真实参考:主人提供的 `ysbin/ysc.ybn` 实测 YSCM v495 / 119 命令 /
  37 条消息(全非空)+ 256 字节键表,与 yu-format 解析一致。
- 验证:`cargo test -p yu-rt --test corpus` 9 ok(新钉值生效);
  快套件(lib 35 / fixture 38 / golden 3 / consistency 4 / corpus_golden 4)
  全绿;clippy 存量 16 处不变、新增 0。

## 成果 19 (2026-10-20) · yu-trace 录制路径关闭事件丢弃阈值

- 问题:运行核事件队列默认 `1<<20`;ERIS 首帧约 129 万命令,`VmTrace` 在
  `take_events` 前就会丢最旧事件,导致 trace 静默缺段而 diff 仍可能通过。
- 修复(`crates/yu-trace/src/replay.rs`):
  - 新增 `pub const TRACE_EVENT_CAPACITY: usize = usize::MAX`;`VmTrace` 启动后
    调用 `Vm::with_event_capacity(TRACE_EVENT_CAPACITY)`。该值只是丢弃阈值、
    不预分配,实际内存 = 单次 tick 的事件量。
  - 模块文档补充:录制路径不丢事件;若单 tick 超内存,应缩小录制粒度。
- 回归(`crates/yu-trace/tests/real_sample.rs`):
  - 录制后断言 `vm.events_dropped() == 0`;
  - 断言 `event_records > 1<<20`,保证该用例真的覆盖默认丢弃阈值。
- 验证(原始输出):
  ```
  $ cargo test -p yu-trace --test real_sample -- --nocapture
  official_data_ypf_smoke ... ok
  test result: ok. 2 passed; 0 failed; finished in 104.39s
  $ cargo test -p yu-trace --lib --test diff --test equivalence --test event_trace \
      --test golden --test roundtrip
  33 / 13 / 6 / 12 / 2 / 7 passed; 0 failed
  $ cargo clippy -p yu-trace --all-targets
  yu-trace 自身 0 告警(输出中的 warning 均来自依赖 yu-rt)
  ```
- 改动仅 2 个文件;未执行 git 操作。
- **版本判别示例** `crates/yu-rt/examples/version_probe.rs`(主人要求):
  散盘目录 / `data.ypf` 封包两形态直读(不经落盘拆包),打印七张元数据表
  头(魔数/版本/计数)、yscfg 分辨率与标题、脚本数与一份解密自检、ycd
  探测;七表版本交叉一致即判别引擎版本。实测:ERIS `data.ypf` = 0.495
  (1407 条目,136 脚本,800x600),<game-b> `data.ypf` = 0.494(12212
  条目,160 脚本,1100x825,标题「<game-b> ver1.02」);两包 ycd 均缺席
  → 兜底日志为常态。Tyn 旁线同法(读工程 YSCM 头 version)。
  注:`project-eris/data` 散盘语料已于当日 13:02 被删除(12:59 的
  全工作区真数据跑在其之前完成,269 项全绿);此后
  `YURIS_SAMPLE_ERIS_YSBIN` 缺样本,corpus 真样本测试显式 SKIP,
  ERIS 相关验证以官方 `data.ypf` 为准。

### E3 附:多版本官方 SDK 的 YSCD 对照(2026-10-20,主人指示)

- 官方下载页(https://yu-ris.net/download)公开存档仅四个大版本:0.292/0.92(Beta3)、
  0.466/0.19(Beta4.5)、0.479/0.14(Beta4.7 安定版)、0.495/0.32(Beta4.8);
  小版本(0.494 等)不单独归档。四个包已解压并规范到
  `<side-repo>/target/sdk/0.{292,466,479,495}/yu-ris_*`(0.466 的 SJIS 乱码目录名
  已还原),原始包集中在 `target/sdk/dl/`,四份编译器定义集中在
  `target/sdk/ycd-all/YSCom_0.*.ycd`。
- 逐版本实测(内核自己的 `YcdTable::parse` 四份**全部可读**,段定位对布局平移不敏感):

  | | v292 | v466 | v479 | v495 |
  |---|---|---|---|---|
  | 命令段 | 91 | 100 | 104 | 119 |
  | 系统变量 | 482 | 513 | 526 | 548 |
  | 消息 | 122 | 121 | 121 | 121 |
  | 尾部常量区 | 3374B | 3374B | 3374B | 3374B |
  | `_LC` | 32 | 48 | 48 | 48 |
  | `_PINT` | 34 | 50 | 50 | 53 |
  | `_SCREEN_SIZE_X` | 54 | 80 | 93 | 115 |
  | `_KEY_A` | 101 | 132 | 145 | 167 |
  | `_KEY_ANY` | 209 | 240 | 253 | 275 |

- 结论(直接回答"4xx/2xx/5xx 识别"):**4xx 内部并不一致**——466→479→495 的
  系统变量 id 逐个漂移(表按插入历史增长);2xx(292)形状完全不同(482 项/91 命令)。
  因此 ABI 不能按版本号段判别;唯一正确机制 = **每局用其自带 `ysv.ybn` 逐项校验,
  按"哪个 ABI 表能通过校验"选择**(数据驱动,A5)。现状:内嵌 ABI=0.495(548),
  v494 商业包已实证 548/548 吻合(即 0.49x 线稳定在 548 布局,大跳变发生在
  479→494 之间);旧线游戏会**校验失败拒绝启动**(安全失败,不静默读错)。
- 恒定项:`_TIME=0`、`_PAI=10` 四版一致;消息 121/122;尾部 3374 字节四版恒定
  (版本无关的固定表)。四份 ycd 可立即用于生成逐版本 ABI 表(下一步建议:
  装载时按 ysv 校验选择 ABI 表,不做版本号分支)。
- 「能否直接从游戏拿系统变量名」的实证(2026-10-20):**不能**。逐字节检查:
  0.495 `yu-ris.exe`、<game-b> 发行版 exe/`MyDll.dll` 对
  `_KEY_ANY/_SCREEN_SIZE_X/_MOUSE_L/_LC/_PINT` **全部 0 命中**(exe 里唯一
  `_UPPER` 串是 D3D 常量);包内 ysv 只有 id/类型/维数、yse 只有错误文案、
  ysc 里的 `_MOUSE/_SYSTEM/...` 是**命令名**不是系统变量;SDK 的
  `YSCom.exe` 同样 0 命中——编译器也是读 `YSCom.ycd` 才拿到名字。
  即名字表的唯一宿主是开发侧 ycd,商业包把它与编译器一起剥掉。
  游戏数据能提供的是**指纹**:ysv 前 N 条 kind=1 记录的 (id,type,dims),
  足够校验/选择 ABI 表、识别未知构建并拒绝启动,但推不出名字。

## 成果 E3 复核补充 (2026-10-20) · R12 只读验收

> 本会话只做复核;未改生产代码、测试或数据(一次测试路径试改已撤回)。
> 既有「成果 E3」条目保留不动,此处只补复核结果与环境说明。

- 生产代码与既有 E3 完成态一致:
  `find crates/yu-rt/src crates/yu-format/src crates/*/Cargo.toml -newer target/final2-workspace.log`
  无输出,故 `target/final2-workspace.log` 的 269 项通过仍覆盖当前生产代码。
- 只读复核:
  - `cargo tree -p yu-rt` 命中 `yu-format`;
  - `MAGIC_*`/`fn check_header` 审计 0;`tables.rs` 的 `fn parse(data` 0;
  - 非样本套件 lib 35 / fixture 38 / golden 3 全过;
  - `yu-format` 真语料 1 过(`ysc=119 / ysl=1941 / ysv=2568 / yst_list=141 / yst=136`);
  - `yu-vfs` 官方 `data.ypf` 4 过。
- 环境说明:当前封包为 `<side-repo>/workbench/project-eris/data.ypf`;
  `project-eris/data` 已不存在,散盘在 `project-eris/save/data`(用户故意放置)。
  测试的工程根推导 `data_root.parent()` 因此找不到 `system/YSCom/YSCom.ycd`;
  按用户指示未搭临时根、未改 `save/data`,ycd 相关用例本次跳过。
- E2E 对照(既有 E3 日志):首帧 `1,290,326` 不变;第 2 帧
  `332,550 → 274,466`(YSTL `text_count` 勘误,已按 E3 续记裁决钉值);
  总命令 `2,989,914 → 2,931,830`;变量域哈希
  `2bc95198… → 18a94a71…`;其余帧与 0 Fatal 不变。
- YSCF 宽高 u32:ERIS 800x600 前后一致;引擎合法范围 `0x80..=0x2000`,
  合法值均落在 u16 内,故有效游戏无数量变化;商业包样本当前不可用,未测。
- 风险/挂账:YSTL 勘误是 YSCF 之外的唯一行为变化;`ScriptFile` 用
  `Segments` 投影而非 `ystb::decode`(运行核按 opcode 分派且需要 `param`,
  `decode` 只给命令名);`BootTables.config` 直接使用 `yu_format::yscf::Yscf`,
  `EngineConfig` 删除。

- 「能否集齐小版本」结论(2026-10-20):**不能,也不需要**。官方 `/history` 是
  纯变更日志(0.400→0.495 每版都有条目,`down.cgi` 链接 0 个);Wayback 对
  `down.cgi` 无任何存档捕获。且履历本身就是"小版本会动 ABI"的铁证:
  **ver0.480/0.01** 一次性追加 `_PINT/_PFLT/_PSTR/_PFLG 11..16`、
  `_RINT/_RFLT/_RSTR/_RFLG 11..16`、`$_M11..16`、`$_MB11..16` —— 正是
  479(526 项,PINT dims=11)→494/495(548 项,dims=17)的跳变点(与四份
  ycd 逐项数据双向印证);另有 0.470/0.472/0.400 等变量追加。故 ABI 只能
  按**族**覆盖(四份 SDK 恰是四个族:Beta3 / ≤0.469 / 0.470–0.479 /
  ≥0.480),游戏用自身 ysv 指纹选族;指纹不匹配 = 未知构建,拒绝启动后
  再针对该构建取 ABI(自带 ycd 或逆向其 exe),不做版本号对齐。

### E3 附:系统变量 ABI 指纹选族(2026-10-20,主人指示"加入指纹实现")

- 新增三份官方 ABI 数据表(由 `scripts/gen_sysvar_abi.py` 从官方 SDK 的
  `YSCom.ycd` 生成,带来源版本/SHA-256):
  `data/sysvar-abi-0292.tsv`(482)、`sysvar-abi-0466.tsv`(513)、
  `sysvar-abi-0479.tsv`(526);原 `sysvar-abi-0495.tsv`(548)经同一脚本
  复现校验(仅首行注释差异)。
- `sysvar_abi.rs`:`known_profiles()` 四族(选择顺序新→旧)+
  `select_for(&VarImage)`——**按游戏自带 `ysv.ybn` 指纹**(var_id<表长 的
  kind=1 镜像恰好覆盖 0..表长,且 type/dims 逐项相等)选族,不看版本号;
  全不匹配 ⇒ `table.version` Fatal 并逐族列出失配原因(未知构建拒绝启动)。
  `builtin_abi()` 保留为 0495 兼容入口。
- `tables.rs`:无 ycd 时的路径由"内置 0495"改为"指纹选族";ycd 路径不变。
  `vm.rs` 兜底日志改为报告所选族(如 `engine ABI profile sysvar-abi-0495.tsv`),
  仍为 sink-only(不进事件流)。
- 测试:`fingerprint_selects_each_known_family`(四族指纹各自选中,长度
  548/526/513/482)、`fingerprint_rejects_unknown_and_lists_reasons`
  (改一个维数 ⇒ 四族全拒、错误含族名)、`trailing_extra_records_keep_newest_family`
  (v555 形态:548 前缀 + 尾部追加 ⇒ 归 0495 族,Likely);
  真实数据:`fallback_abi_logs_notice_on_real_data` 断言 ERIS 选中
  `sysvar-abi-0495.tsv` 且日志含族名。
- 测试基建:散盘语料被移到 `project-eris/save/data` 后,三个测试文件的
  "工程根 = ysbin 上两级"假设失效 → 改为**向上查找**
  `system/YSCom/YSCom.ycd`(`find_project_root`,最多 4 层)。
- 验证:corpus E2E 与上一轮**逐字节一致**
  (`frames=[1290326, 274466, 3315, 1302412, 15340, 15320, 15331, 15320]`,
  total 2,931,830, loose/packed 哈希 `18a94a71…`);全工作区套件见
  `target/abi-workspace.log`;clippy 无新增。

## 成果 M3-GFX (2026-10-20) · yu-gfx 渲染语义层 + CompositorFrame + 证据表

> 会话 M3-GFX。范围 = `yu-gfx`(语义层/可序列化显示列表/证据表/回归);
> 不含 wgpu/wasm/窗口。零 git 写操作。

### 1. 交付物

- 新 crate `crates/yu-gfx/`:
  - `frame.rs`(`CompositorFrame`/`Viewport`/`Clear`/`ColorSpace`/`TextureFilter`)
  - `pass.rs`(`Pass`/`PassTarget`)、`op.rs`(`Op`/`CopyOp`/`ImageOp`)
  - `layer.rs`(Layer/Part/BlendMode 0..18 状态表/Transition/Filter)
  - `group.rs`、`transition.rs`、`text.rs`(文本模型)
  - `state.rs`(`GfxState`:消费 `VmEvent::DomainCall`,维护图层树/组/复制)
  - `table.rs`(槽号→参数名,唯一来源 = 工程 `ysc.ybn`)
  - `path.rs`(ID/IDNO 路径规范化:`W0`/`:`/`.`/`..`/尾 `/`)
  - `json.rs`(固定字段顺序;`to_json`/`to_display_json`/`blend_table_json`)
  - `error.rs`(`gfx.*` 诊断码)
- 文档:`docs/render-parity.md`(证据表 + 后端执行契约 + 旧线勘误);
  `crates/yu-gfx/docs/divergences/README.md`(DIV-GFX-001..018)。
- 取证工具:`crates/yu-rt/examples/gfx_census.rs`、`crates/yu-gfx/examples/eris_dump.rs`。
- 必要 plumbing(非 gfx 语义):`yu-rt` 的 `DomainCall` 以前 `args: Vec::new()`,
  宿主域拿不到参数;现按窗口求值并填充(`handlers.rs` 新增
  `eval_arg_windows`,域分支不再空参)。通用 `Command` 事件路径不变。
- 首屏 golden:
  - `crates/yu-gfx/tests/golden/eris_first_screen_display.json`(20,620 B)
  - `crates/yu-gfx/tests/golden/synthetic_group_layers_display.json`

### 2. 证据表(详见 `docs/render-parity.md` §1)

| 语义 | 手册 | 真实语料 | 引擎证据 | 结论 |
|---|---|---|---|---|
| 图层/ID/路径/Z/组 | `basic/layer.html`、`cg/cg.html` | ERIS 660 `CG`/307 `CGACT`/103 `CGEND` | `FUN_00413dc8` 34-49;`FUN_00414ba4` 115-156 | Confirmed;同 Z 取创建序(手册称不確定) |
| `A` 0..256 | `cg/cg.html` `A=0～256` | 首屏 A=0/256 | `FUN_00414ba4` 116/135-154/910-913 | Confirmed;固定 `min(A,255)/255`,着色器 `A/256` |
| `MODE=0..18` | `cg/cg.html` 全表 | 首屏 MODE=0/2 | `FUN_00414ba4` 391-874 | 0 Likely;1/2 Confirmed;3/5/6/7/9 Unverified;4/8/10..18 Unimplemented |
| 纹理过滤 MAG/MIN/MIP | 无手册页(引擎设定) | 全局 | `FUN_0043cd6c` 46-117 | Confirmed 全局;旧线固定 Linear ❌ |
| X/Y/FX/FR/FC/FEX | 手册只文档化 X/Y | ERIS `FX=@fx*100` 等 | `FUN_00414ba4` 117-125/220-336 | X/Y Confirmed;FX 家族槽位绑定 Unimplemented |
| 多部件层 | 无直接页 | 首屏未出现 | `FUN_00414ba4` 155-156/200/219/985 | 结构 Confirmed;几何 Unimplemented |
| 组/离屏/COPY | `cg/cg.html`/`basic/layer.html` | W0/W3、`COPY2=1 ID=:/W0/` | `FUN_00413dc8` 34-36/51-102;`FUN_00462e40` | 机制 Confirmed;COPY 时序 Unimplemented |
| T/TA/TID | `cg/cg.html` | 源有 `T/TID/TA` | `FUN_00414ba4` 905-909 | 公式 Confirmed;TID 配对 Likely |
| 文本 | `font/font.html` | FONT 3 次 | 内部绘制种类 98/100 | 模型 Confirmed;栅格化 Unimplemented |
| 颜色空间 | 无 | 未采集截图 | 全树无 `D3DRS_SRGBWRITEENABLE` | Likely:gamma 空间混合 |

### 3. ERIS 首屏 census(实测)

- frame 0(初始化):1,290,326 命令;`CG` 30,668、`CGACT` 280、
  `CGEND` 1,183、`CGINFO` 606、`FONT` 3、`FONTINFO` 1。
- `CG` 参数:`ID` 30,668、`LINT` 29,068、`E` 1,058、`FILE` 552、
  `Z` 553、`MODE` 1,016、`X/Y` 1,023、`A` 36、`SX/SY` 13。
- 取值:`A` 256×34/0×2;`E` 0×1025/1×33;`MODE` 0×1015/2×1。
- 首个有可见层的逻辑帧:272 图层、34 visible、15 pass
  (1 可执行复制 + 11 图像加工 + 2 组离屏 + screen;screen 只含主窗口 W0)。
- 68 个按钮快照的复制源是已删除临时层 → 帧内不可重放,图层标
  `Unimplemented`;完整复制历史在 `GfxState::copies()`。

### 4. 回归(原始结果)

```text
$ cargo test -q -p yu-gfx          # YURIS_REQUIRE_SAMPLES=1,样本=save/data/ysbin
8 passed (lib) / 5 alpha / 9 blend_table / 1 eris_first_screen (12.8s)
6 filter / 6 group / 11 layer_order / 2 screen / 6 transform / 4 transition
1 display_list_golden — 全部 ok,0 failed

$ cargo test -q -p yu-rt --lib --test fixture --test golden
38 + 38 + 3 passed,0 failed

$ cargo test -q -p yu-rt --release --test corpus --test corpus_golden
9 + 4 passed,0 failed(21.8s + 0.2s;DomainCall 带参不改变 VM 状态)

$ cargo test -q -p yu-trace        # 带 YURIS_SAMPLE_ERIS_YPF
2 + 7 passed,0 failed(real_sample 116.8s)

$ cargo clippy -q -p yu-gfx --all-targets
yu-gfx: 0 warning;yu-rt 仍是既有 5 条基线 warning(无新增)

$ grep -rn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src   # 0
$ grep -rn "println!|eprintln!|dbg!" crates/yu-gfx/src           # 0
$ grep -rn "HashMap" crates/yu-gfx/src                          # 0(全部 BTree)
```

- 确定性:同一输入连续两次 `to_json`/`to_display_json` 逐字节一致;
  `eris_dump` 连跑两次输出 diff 为空。
- 首屏 golden 使用 `UPDATE_GFX_GOLDEN=1` 生成,默认测试比对字节。

### 5. 未实现/挂账(全部有 DIV 编号)

- MODE 3/5/6/7/9(状态有、辅助纹理/运算未取证);MODE 4/8/10..18
  (着色器本体未定位)。
- `MID` 掩码;`FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/
  FQU/FID/F` 的槽位→偏移;多部件层几何。
- `CGACT[COPY/COPY2]` 的上一帧时序/区域;`RECTPAINT*`/`SIZE*`/`ROTATION`/
  `MONO`/`BLUR*`/... 图像加工。
- 文本字形栅格化/行距/描边/窗口定位。
- 子窗口(`WINDOW[NO=1..3] CGID`)的叠放/位置/可见性:Screen 只合成主窗口
  `:/W0`,子窗口只出 Offscreen pass(交 window/host 会话)。
- 颜色空间/默认清屏/组无 SX/SY 时的离屏尺寸:均为 Likely/Open。

### 6. 给后端的契约摘要

- **只执行 `CompositorFrame.passes`**;`status.executable=false` 的 op
  必须跳过,不得近似。pass 顺序 = 纹理复制/加工 → 组离屏(子先父后)
  → screen。
- **Alpha**:`Layer.alpha` 原值 0..256;固定管线 `min(A,255)/255`,
  着色器 `A/256`;不要在线性空间混合(`color_space=gamma_encoded_srgb8`)。
- **Blend**:`json::blend_table_json()` 给出 19 模式的 D3D9 状态;
  0/1/2 可执行,3/5/6/7/9 状态可下发但默认跳过,4/8/10..18 跳过。
- **Filter**:`frame.filter.mag/min/mip` 是全局采样器设置,不是逐层。
- **Group**:先 `Offscreen` pass,再把组纹理按 `layer.alpha/blend` 合成;
  组 `E=0` 只影响父层合成,不影响组内渲染/COPY。
- **窗口**:Screen pass 只合成主窗口 `:/W0`(手册 `WINDOW[NO=0] CGID`);
  子窗口 `:/W1..W3` 只产出 `Offscreen` pass,窗口叠放/位置归 host(后续)。
- **Transition**:`from/to` 双纹理 + 已算好的 `fade`;`to=None` 跳过。
- **Transform**:X/Y 是引擎 i64;FX 家族原值在 `transform.filter`,
  当前 Unimplemented;后端不得自行解释槽位。
- **Copy/Image**:帧内只含可执行项;`GfxState::copies()/images()` 是审计
  历史,不是执行清单;`Snapshot` 纹理在 COPY 语义关闭前无效。

### 7. 环境勘误

任务书给的 `YURIS_SAMPLE_ERIS_YSBIN=<side-repo>/workbench/project-eris/data/ysbin`
已不存在(E3 已记语料迁到 `project-eris/save/data`)。本次用
`<side-repo>/workbench/project-eris/save/data/ysbin`;`data.ypf` 仍在
`<side-repo>/workbench/project-eris/data.ypf`。测试缺样本时:
`YURIS_REQUIRE_SAMPLES=1` 显式失败,否则打印 `SKIP` 可见跳过。

## 成果 20 (2026-10-20) · gfx 复核修复任务书(验证优先)

- 新增 `docs/tasks/task-f-gfx-repair.md`。
- **文件开头明确警示**:审查报告中的问题可能误读手册/反编译/代码,全部默认“不一定正确”;
  必须先产出 `gfx-verification-ledger.md`(Confirmed/Refuted/Uncertain + 决策),禁止照单修改。
- 核心目标:完美渲染。判定顺序 = 语义(层/顺序/可见性/Alpha/混合/变换/窗口/文本)
  → 显示列表 → 软参考像素 → 官方截图有据容差;本会话只做前两层。
- 审查主张按 A(手册/真实脚本)、B(REA/引擎)、C(硬编码/A5)、D(缺失)、
  E(健壮性/性能)、F(测试真实性)分组,每条附先看哪里;特别标注:
  - 手册“SX/SY 仅组有效”与 ERIS 图像层用法冲突,必须以引擎为准;
  - Z 重排/E 偏移/组 RT/TFACTOR/DIFFUSE 等只有审查主张,需直接 REA 复核;
  - 旧地址 0x42c4a4/0x43d334/0x43d4a4 不可复核,必须重新锚定。
- 决策规则:Confirmed+可见影响才修;Refuted 只勘误;Uncertain 保持 Unimplemented+DIV;
  手册与引擎冲突以引擎为准并留手册勘误。
- 仅文档改动;未执行 git 操作。

## 成果 21 (2026-10-20) · 字体会话任务书 `yu-font`(可并行)

- 新增 `docs/tasks/task-g-font.md`:新建独立 crate `crates/yu-font`,与 gfx/后端会话并行。
- 边界:`yu-font` 只做字体来源解析、字形栅格化、合成粗体/斜体、描边/阴影字形、
  glyph cache/atlas;排版/换行/对齐/ruby/颜色/渐变仍归 `yu-gfx/text` 与后端。
- **Phase 0 取证优先**:先确认官方字体来源(自带 TTF/系统字体/位图字体)、
  官方栅格化路径(GDI/自绘/ClearType)、`FONT` 参数语义、描边/阴影/旋转算法;
  产出 `docs/font-provenance.md`,未取证不写字体选择/样式公式。
- 技术默认:`swash` + `fontdb` + 可选 `rustybuzz` + `tiny-skia` 描边/阴影;
  `fontdue` 作为简化备选;Windows `GdiRasterizer` 仅作参考/可选 feature。
- 验收:确定性 coverage、字形/样式 golden、缺失字体/字形诊断、
  wasm32/Android 编译、示例 PNG 与官方文本截图并排;视觉相似为底线,
  逐像素差异必须归因(字体/AA/描边/取整)并留 DIV。
- 仅文档改动;未执行 git 操作。

## 成果 22 (2026-10-20) · 字体会话改为一次性完整实现(参考游戏配置直出)

- 重写 `docs/tasks/task-g-font.md`:取消“第一版/最小版”式分阶段,
  一次性实现完整字体 + 文本样式管线,并用 ERIS 真实 FONT 配置直接渲染
  PNG contact sheet 供对比;桥接留给后续会话。
- 范围:字体来源解析、字形栅格化、粗体/斜体/AA/颜色/双色/渐变/描边/阴影/旋转/
  DRAWMODE,全部按取证结果实现;排版(换行/对齐/ruby/逐字节奏)只定义接口。
- 硬门禁:Phase 0 必须先完成字体来源/引擎栅格路径/FONT 参数/官方截图取证,
  产出 `docs/font-provenance.md`;未取证项显式 Unimplemented,禁止猜测。
- 参考渲染器 `examples/render_reference.rs`:读取参考游戏 FONT 配置(编译产物扫描,
  宏展开后才有),生成单字/整行/描边/阴影/渐变/旋转/粗体样例,输出 PNG + JSON,
  与官方截图并排并给出 diff 统计。
- 并行:`yu-font` 为新 crate,不改 yu-gfx/yu-rt/后端;REA 单会话。
- 仅文档改动;未执行 git 操作。

## 成果 F1 (2026-10-20) · yu-gfx 复核修复:验证台账 + 14 项 Confirmed 修复

> 会话 F。**先复核、后修复**:先产出 `docs/gfx-verification-ledger.md`,
> 每条审查主张重新取证(A/B/C 共 28 条:C=19 / R=7 / U=2),未进台账不改代码。
> 零 git 写操作;旧线工作区未动。会话 G(`yu-font`)并行,本会话未触碰其文件。

### 1. 台账结论(详见 `docs/gfx-verification-ledger.md`)

- **Confirmed 19**:修复 14(A1–A4/A6/A7/B11/B12/B13/C1–C5),
  证据成立但保持现状 5(A5 CGINFO LET、A9 MODE 3/5/6/7/9、B6 辅助纹理、
  B9 清屏、B10 颜色空间)。
- **Refuted 7**(审查读错):
  - B1“每帧按 Z 重排是错的”:引擎链表确实不排序,但可见顺序=Z 升序;
    同 Z 创建序是 DIV-GFX-014 的确定性决策。
  - B2“E 可能是 +0xa4”:`+0x89`=E、`+0xa4`=层种类(1 图像/2 组/0 死),
    由 `0x413e34/0x413e47`、`0x42c400`、`0x40a38c`、`0x40e02c`、`0x43d4bc` 确认。
  - B3“+0x61c/+0x90 可能不是 RT/纹理”:`0x413f6f-0x413fab` 明确
    `SetRenderTarget(+0x61c)`/`GetSurfaceLevel(+0x21c)`。
  - B4“TFACTOR 只在 0/3/7”:实际集合 `{0,1,2,3,5,6,7,9,100,98}`
    (`FUN_00414ba4` 行 144-154);3/7 只是分支内覆盖为白。
  - B7“A 被 `in_stack_00000010==0` 清零”:调用点 `0x413f29` 恒传 1,
    清零分支不属于图层遍历。
  - B8“X/Y 可能 /100/16.16”:ERIS 的 X/Y 是像素值;`/100` 只作用于
    `+0x1b0..+0x1d8` 滤镜字段。
  - C6“域分类用前缀”:属路由启发式,不是可见语义错误。
- **Uncertain 2**:A8(`CG` 无 FILE 的 get-or-create 未重新锚定,保持
  DIV-GFX-016)、B5(顶点 DIFFUSE RGB 未还原,新增 DIV-GFX-019)。
- **B12 地址失效**:`0x42c4a4/0x43d334/0x43d4a4` 在本地反编译树不存在;
  改用 `0x413dc8`(遍历/TID/MID)、`0x4141e4`(路径规范化)、
  `0x414559`(哈希查表)、`0x43d4bc`(析构)、`0x42f57d`(SWAP)、
  `0x42f809`(CGACT 动作分派)。

### 2. Confirmed 修复(证据 → 代码 → 回归)

| 项 | 证据 | 代码 | 回归 |
|---|---|---|---|
| A1/B13 `TID=""` | `es_Button.yst:4526/4540`;`FUN_00413dc8 0x413e7b`:空串跳过配对 | `state.rs` TID 分支:空串清 `tid=None`;`op_for_layer` 无 TID 输出普通层 | `parity_fixes::empty_tid_clears_pairing_instead_of_w0` |
| A2 图像 SX/SY | 手册称仅组有效,但 `es_ERIS.yst:579/600` 用 `FILE="" SX/SY` 画黑幕;`FUN_00465730/0043772c/00408724` | `LayerKind::Image{size}`;`SolidBlack1x1` 按尺寸可执行,资源图+尺寸标 `gfx.image.size.unsupported` | `parity_fixes::solid_black_with_sx_sy_draws_at_that_size`、`asset_with_sx_sy_stays_unimplemented`;ERIS 首屏断言 `:/W1/DBG` 260×600、`:/W0/FONTL2` 233×27 |
| A3/C2 WINDOW | `window/window.html`;`es_First.yst:97/98/140`、`es_ERIS.yst:22` | 消费 `Domain::Window`;`CompositorFrame.windows`;Screen 用 `WINDOW[NO=0].CGID/E/A`;`WINDOWEND NO=0` 拒绝;子窗口归 host | `parity_fixes::window_cgid_selects_main_layer_set_and_e_zero_hides_screen`、`screen.rs` |
| A4 TEXT | `text/text.html` ID/CLEAR;`es_Text.yst:154` | `yu-rt` 保存 `text_layer_id`;`WORD`/`_` 事件带 ID;`CLEAR=1` 清文本缓冲 | `yu-rt fixture::text_id_sets_following_run_id_and_clear_empties_buffer` |
| A5 CGINFO/FONTINFO/WINDOWINFO LET | 手册三页 + 125 处真实使用 | **不修**:需脚本执行期同步查询 ABI;保持 Unimplemented | 新增 DIV-GFX-020 |
| A6 CGACT | `cg/cgact.html` 例 3/4/21/22;真实 ysc 表 70 参数 | `ImageOp.sx/sy`;动作表补 `RESIZE/TRIM/ROTATION2/TRANSITION2/XOR/AND/BLUR2..6`;像素仍 Unimplemented | `parity_fixes::cgact_size_keeps_sx_sy_and_resize_trim_are_recognized` |
| A7 FONT NO/BNO | 手册 `font/font.html` NO;脚本 `FONT[BNO=1]` | `TextStyle.font_no/font_bno`;BNO 值保留并挂 DIV | `parity_fixes::font_no_and_bno_are_recorded` |
| C1 视口 | `yscfg` 800×600 | `from_tables` 在宽高非 0 时覆盖配置 | `eris_first_screen` 以 1×1 占位构造后断言 800×600 |
| C4 T 默认 | `FUN_00465730 +0x82=0x100` | `Transition::default().t=256` | `parity_fixes::transition_defaults_to_t_256` |
| C5 psb | `FUN_0040c9b4` 行 383 `.psb` | `ImageFormat::Psb` | `filter::probe_order_from_eris_yscfg`、真实 yscfg 测试 |
| D15/D16 主组/图像子层 | `FUN_00413dc8 0x413e1a` 任意节点先递归子层 | 主组缺失不再猜所有顶层;图像层子层按后序扁平输出 | `parity_fixes::image_children_are_drawn_before_the_parent` |
| E1/E3/E9 | 诊断无界/粘滞/非法 JSON | 每帧清空+`count`;`texture_status`/`param_status` 重算;`128.0`/`null`/`-0.0` | `diagnostics_are_per_frame_and_count_duplicates`、`mode_update_recovers_executable_status`、`persistent_unsupported_attributes_keep_layer_unimplemented`、json 单测 |
| E2/E6/E7/E8/E10/E5 | 日志无界/递归/丢层/缺表/O(n²)/比较器分配 | 日志上限 16,384+`dropped_ops()`;显式栈;图像子层;必需命令 fail-fast;动作集合一次扫描;排序去分配 | `deep_path_and_deep_group_nesting_do_not_recurse`、`missing_required_graphics_command_fails_validation`、全量测试 |

### 3. 修复前后原始证据

**A1 旧行为**(`git show HEAD:...state.rs:1531`):

```text
"TID" => match value.as_bytes() {
    Some(b) => {
        let p = path::normalize(b, None, false).path;
        layer.transition.get_or_insert_with(Transition::default).tid = Some(p);
```

`normalize(b"")` = `:/W0`,因此旧代码把 `TID=""` 变成整屏 W0 过渡;
新回归断言此时**不得**出现 `Op::Transition`,`tid=None`(测试通过)。

**E9 旧/新**:

```text
旧: fmt_f64(128.0) == "1.3e2"(非法精度);NaN → "NaN"(非法 JSON)
新: fmt_f64(128.0) == "128.0";NaN/±Inf → null;-0.0 → "-0.0"
```

**Golden 前后**(ERIS 首屏):

```text
旧:"name":"other","code":5           新:"name":"psb","code":5
旧:"kind":{"kind":"image"}           新:"kind":{"kind":"image","size":{"width":260,"height":600}}
旧:无 windows 字段                    新:"windows":[{"number":0,"layer_set":":/W0",...}]
旧:20620 B(跨帧累计诊断)             新:19822 B(本帧诊断;帧 3 diag=0)
```

### 4. 测试真实性补强(F 类)

- ERIS 首屏从 `>100/>10/>=2` 改为精确 census:272 层 / 34 visible /
  15 pass / 44 op / 12 texture pass / W0 30 op / W1 1 op / screen 1 op;
  断言真实 `MODE=2` 层 `:/W0/SPLDST2`(A=0、E=0、SolidBlack1x1)、
  E=0 共 238 层、主窗口 `:/W0`、帧内诊断为空。
- golden 新增 `tests/golden/README.md`:样本 `ysc.ybn` sha256
  `741d4caf…`、`yscfg.ybn` sha256 `f451d3a7…`、生成命令。
- blend JSON 与 `fixed_state()` 逐 stage/arg/op/blend 字段交叉验证。
- filter 测试读取真实 `yscfg.ybn`(500 样本 gate);`Other(5)` 改为 `Psb`。
- SWAP2 增加 X/Y 断言;COPY2/属性不复制继续执行断言。
- 深路径 600 级 / 深组嵌套不递归爆栈。

### 5. 未完成 / blocked

- A5 `CGINFO/FONTINFO/WINDOWINFO` 的 `LET`:保持 Unimplemented + DIV-GFX-020。
  原因:必须脚本执行期同步写 VM 变量,而 gfx 事件是 tick 后异步消费;
  需要 VM↔gfx 同步查询 ABI,不是 gfx 单层可修。
- A8 `CG` 无 FILE:未重新锚定 CG 处理体,保持 DIV-GFX-016。
- A9 MODE 3/5/6/7/9、4/8/10..18:DIV-GFX-001/002 不变。
- B5 顶点 DIFFUSE RGB:DIV-GFX-019;可执行模式 0/1 的 DIFFUSE 只进 alpha
  stage,不影响可见 RGB。
- E4 每命令分配优化:记录为后续项,本会话未改(不改变语义)。
- 像素/官方截图对拍:属后端会话;本会话只到显示列表层。

### 6. 给后端的契约变化(只列已确认)

1. `LayerKind::Image { size }`:`SolidBlack1x1` + `size` 必须铺满该矩形;
   资源图 + size 的图层 `status.executable=false`。
2. `Op::Image` 新增 `sx/sy`;新增动作名 `resize/trim/rotation2/transition2/
   xor/and/blur2..blur6`;所有 ImageOp 仍 Unimplemented。
3. `CompositorFrame.windows`(按 NO):`NO=0` 的 `layer_set/visible/
   alpha/x/y/size/z`;Screen ops 为空表示主窗口隐藏。
4. `FrameDiag.count`:同键诊断合并计数;诊断**每帧重置**。
5. `ImageFormat::Psb`(code 5,`.psb`)。
6. `TextStyle.font_no/font_bno`;`VmEvent::Text.id` 现在可能非空
   (`TEXT[ID]`)。
7. `LayerStatus` 由纹理级/属性级重算;MODE/FX/MID 持久字段不会因一次
   无参数更新被洗白。
8. `GfxState::dropped_ops()`:日志上限丢弃计数;`gfx.log.truncated`。

### 7. 验收原始输出

```text
$ export YURIS_REQUIRE_SAMPLES=1 YURIS_SAMPLE_ERIS_YSBIN=.../save/data/ysbin
$ cargo test -p yu-gfx
10 lib + 5 alpha + 10 blend_table + 1 display_list_golden + 1 eris_first_screen
+ 7 filter + 6 group + 11 layer_order + 15 parity_fixes + 2 screen
+ 6 transform + 4 transition + 1 doc = 79 passed; 0 failed

$ cargo test --workspace --no-fail-fast      # 同一套样本 env
352 passed; 0 failed(exit=0)

$ cargo clippy -p yu-gfx --all-targets
yu-gfx: 0 warning(yu-rt 既有 5 条基线 warning 不变)

$ grep -rEn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src   # 0
$ grep -rEn "println!|eprintln!|dbg!" crates/yu-gfx/src           # 0
$ grep -rEn "HashMap" crates/yu-gfx/src                          # 0
```

- `cargo fmt -p yu-gfx -- --check` 通过。
- 确定性:`json::to_display_json` 两次逐字节一致(既有测试 + golden)。

### 8. 并行纪律

- 会话 G 期间新增 `crates/yu-font`、`docs/tasks/task-g-font.md` 并改
  `PROGRESS.md`/`Cargo.lock`;本会话只追加本节,未改其文件。
- 本会话未执行任何 git 写操作。

## 成果 23 (2026-10-20) · 并行任务书:官方 trace 对拍(H)

- 新增 `docs/tasks/task-h-trace-oracle.md`。
- 目标:把 `yu-trace` 从“内核自证”推进到“官方 trace 对拍”。
- 范围:边界①vfs/②vm;③帧描述/④平台命令留给后端会话。
- 关键:producer-neutral schema(v3)、按 sub/kind/frame 投影对齐、官方采集器
  (WOW64/调试器/屏幕级 fallback,注明等级)、tick_at 时钟/codec/aux 回放闭合。
- 不改 yu-rt/yu-gfx;采集器放 tools/ 或仓库外;语料/截图不入库。

## 成果 24 (2026-10-20) · 并行任务书:渲染后端(I)

- 新增 `docs/tasks/task-i-backend.md`。
- 新建 `yu-assets` + `yu-backend-soft` + `yu-backend-wgpu`;不改 gfx/rt/font。
- 顺序:纹理管线(YPF/Dir、扩展名探测、解码、缓存)→ 软后端确定性 PNG →
  headless 首帧 → wgpu 同帧对比。
- 文本由 G-FONT 并行提供;本会话遇到 TextRun 跳过并诊断,不阻塞第一帧。
- 验收:first_frame.png、soft vs wgpu diff、缺失资产/跳过项清单、无硬编码屏幕尺寸。
- 未执行 git 操作。

## 成果 25 (2026-10-20) · 复审并修正 trace/backend 任务书范围

- 复核 `yu-trace` 实际实现:①vfs、②vm、roundtrip/diff/replay 已完成;
  ③`frame_description`、④`platform_call` 只是 schema 2 占位,写入/解析均显式拒绝;
  官方 producer 不存在。此前提示词把①/②也当成待做,范围不准,已修正。
- `task-h-trace-oracle.md`:明确“不重写①②;本会话补③帧描述 + 官方 producer +
  producer-neutral schema/投影 + 回放闭合;④保持拒绝”。
- 复核后端:仓库尚无 `yu-assets`/`yu-backend-*`,是从零新建;gfx(F)与字体(G-FONT)
  正在并行写。`task-i-backend.md`:改为只消费 gfx 公开类型/最小适配层,
  第一帧输出 `target/tmp/first_frame.display.json` 供 trace③ 文件协作。
- 仅文档改动;未执行 git 操作。

## 成果 F2 (2026-10-20) · INFO/LET 同步查询闭环 + 未实现模块 TODO

> 会话 F 续。目标:推进上一轮最大的 1:1 缺口
> `CGINFO/FONTINFO/WINDOWINFO` 的 `LET`(A5/D2),并给所有未实现模块
> 落可检索的 TODO;不猜未取证语义。

### 1. 架构:同步 GraphicsHost

- `yu-rt` 新增 `event::GraphicsHost`:
  - `apply_event(&VmEvent)`:FrameStart/FrameEnd/Text/Graphics|Window DomainCall
    在**产生时**同步送入宿主;
  - `query(command, args) -> Result<Value, Diag>`:脚本执行期查询。
- `Vm::set_graphics_host/take_graphics_host`。安装后:
  - `emit` 同步转发图形事件(Command 热路径不转发);
  - Graphics/Window 域命令不再重复挂 `rt.cmd.unimplemented`;
  - **调用方不得再把同一批事件 drain 后喂给同一 GfxState**(已写进文档)。
- `yu-rt` 新增 `HandlerKind::Cginfo/Fontinfo/Windowinfo` 与 `op_info`:
  - 参数名按工程 ysc 表(opcode→参数表)解析,不写死槽号;
  - `LET` 用 `eval_lvalue` 求值;查询结果用 `write_target` 写回;
  - 无宿主/字段未取证 ⇒ 只诊断,不写 LET(不猜 0)。

### 2. yu-gfx 查询实现

- 新增 `GfxHost`(`Rc<RefCell<GfxState>>`),与调用方共享状态:
  tick 后 `state.borrow().frame()`;tick 期间 VM 同步 `apply_event`。
- `GfxState::query_info`:
  - **CGINFO**:EXIST / E / A / X / Y / Z / MODE / T / TA / TID;
    SX/SY 对组(含 W0 视口)、显式尺寸层、`SolidBlack1x1` 可答。
  - **FONTINFO**:当前 TextStyle 的 COLOR / COLOR2(已设置)/
    SX / SY / BOLD / ANTIALIASING / DRAWMODE / NAME(无 NO 索引)。
  - **WINDOWINFO**:EXIST / E / X / Y / SX / SY(主窗口默认 yscfg 尺寸)。
  - 多字段同时置位:顺序未取证 ⇒ `gfx.info.multi`,不写 LET。
  - 资源图尺寸:需要资产元数据 provider ⇒ `gfx.info.asset_size`,
    不写 LET(DIV-GFX-024)。
  - COLOR/ONMOUSE*/CAPTION/ACTIVE/FULLSCREEN 等 ⇒
    `gfx.info.unsupported`(DIV-GFX-026)。
- `Transition` 新增 `tid_raw`:引擎 `+0x4c` 保存脚本原文,
  `CGINFO[TID]` 返回原文而不是规范化路径;JSON 同步输出 `tid_raw`。

### 3. 回归(原始结果)

```text
$ cargo test -p yu-gfx --test info_query
7 passed; 0 failed
  - cginfo_answers_documented_attributes
  - cginfo_group_and_solid_black_sizes_are_known
  - cginfo_missing_layer_only_answers_exist
  - cginfo_multiple_fields_are_rejected_not_guessed
  - fontinfo_answers_current_style_fields
  - fontinfo_unset_fields_are_not_fabricated
  - windowinfo_answers_main_geometry

$ cargo test -p yu-rt --test fixture info_query_writes_let_with_synchronously_applied_state
1 passed; 0 failed
  - 宿主先看到 CG DomainCall,再收到 CGINFO 查询;
  - 查询值 42 写回 @v;LET 自身不进入查询参数。

$ cargo test -p yu-gfx --test eris_first_screen
2 passed; 0 failed(约 25s,两次真实 ERIS boot)
  - 旧模式 census:272 层 / 34 visible / 15 pass / 44 op
  - 同步宿主模式 census:**完全一致**;查询日志出现 CGINFO + WINDOWINFO
```

### 4. 未实现模块 TODO

- 新增 `docs/gfx-unimplemented-todo.md`:DIV-GFX-001..040 的状态、
  当前行为、解除条件、代码标记位置、推进顺序。
- 代码内新增 `TODO(DIV-GFX-xxx)`:
  - `layer.rs`:MODE 3/5/6/7/9、4/8/10..18;
  - `state.rs`:MID、滤镜、COPY、图像加工、资源图尺寸、组 RT 清屏;
  - `op.rs`:CopyOp/ImageOp 恒 Unimplemented;
  - `frame.rs`:清屏/颜色空间;
  - `text.rs`:字形栅格化/排版;
  - `transition.rs`:配对缺失/推进者;
  - `path.rs`:IDNO 后缀;
  - `yu-rt handlers.rs`:TEXT 显示节奏属性。
- 新增 DIV-GFX-024(资源图 `CGINFO SX/SY` 需资产元数据)、
  DIV-GFX-025(多字段写回顺序)、DIV-GFX-026(像素/鼠标/窗口 host 字段)、
  DIV-GFX-027(IDNO)、DIV-GFX-028(图像子层目标 RT)、
  DIV-GFX-029(pass 交错序)、DIV-GFX-030(`TIDNO`)、
  DIV-GFX-031(`CGACT ID3/ID4/Z2/BNO`)、DIV-GFX-032(SWAP 模式对应)、
  DIV-GFX-033(TEXT 节奏/OUTPUTMODE)、DIV-GFX-034(文本跨帧持久/CLEAR)、
  DIV-GFX-035(FONT NO/BNO/字体解析)、DIV-GFX-036(FONTINFO 字体列表)、
  DIV-GFX-037(WINDOW host 属性/主窗口 A/SX-SY)、DIV-GFX-038(INFO 缺层语义)、
  DIV-GFX-039(CGACT RASTER1/FSX/...)、DIV-GFX-040(TEXTINFO)。
- 复查后确认:**上一版 TODO 不是全部**;补入上述 030..040 后仍不能保证
  “宇宙级完整”,但 gfx 已知缺口已全部有编号/状态/解除条件。

### 5. 验收

```text
$ cargo test -p yu-gfx
10 lib + 5 alpha + 10 blend_table + 1 display_list_golden + 2 eris_first_screen
+ 7 filter + 6 group + 7 info_query + 11 layer_order + 15 parity_fixes
+ 2 screen + 6 transform + 4 transition + 2 doc = 88 passed; 0 failed

$ cargo test --workspace --exclude yu-trace --exclude yu-backend-soft --no-fail-fast
328 passed; 0 failed

$ cargo test -p yu-rt
98 passed; 0 failed

$ cargo clippy -p yu-gfx --all-targets
yu-gfx: 0 warning(yu-rt 既有 5 条基线 warning 不变)

$ cargo fmt -p yu-gfx -- --check     # 通过
$ grep -rEn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src   # 0
$ grep -rEn "println!|eprintln!|dbg!" crates/yu-gfx/src           # 0
$ grep -rEn "HashMap" crates/yu-gfx/src                          # 0
```

- **全 workspace 当前被并行会话的中间态阻塞**(本会话未改这些 crate):
  - `yu-trace`:golden schema 2 vs 3(`synthetic_trace_matches_golden`);
  - `yu-backend-soft`:`render.rs`/`texture.rs` 编译错误(借用/移动)。
  按“活跃写者只读”纪律等待其完成。`--exclude yu-trace --exclude
  yu-backend-soft` 下 328 项全绿。

### 6. 契约变化

1. `yu_rt::event::GraphicsHost` + `Vm::set_graphics_host`;安装后事件同步消费,
   不得重复喂。
2. `yu_gfx::GfxHost`(Rc<RefCell<GfxState>>)供宿主/测试使用。
3. `Transition.tid_raw`(JSON `tid_raw`):原始 TID 字符串。
4. INFO 查询失败时的诊断码:`gfx.info.multi/missing/unsupported/asset_size`;
   失败时 VM 不写 LET。
5. 资源图 `CGINFO SX/SY` 仍不可用,需资产元数据 provider(DIV-GFX-024)。

### 7. 仍未 1:1(按优先级)

1. 资产元数据 provider → `CGINFO SX/SY`(ERIS 按钮布局最大缺口)。
2. 官方引擎受控脚本/截图 → 过渡无配对、COPY 时序、组 RT 清屏/尺寸、
   MODE 0/1/2 像素对拍。
3. MODE 3/5/6/7/9 辅助纹理;4/8/10..18 着色器。
4. 图像加工/多部件/MID/顶点 DIFFUSE。
5. 文本栅格化(与 `yu-font` 会话合流)。

## 成果 G1 (2026-10-20) · yu-font:字体 + 完整文本样式 + ERIS 配置直出

> 会话 G-FONT。范围:新建 `crates/yu-font`、`docs/font-*`、示例与测试;
> 未改 `yu-gfx` / `yu-rt` / 后端 / 根 `Cargo.toml`。

### 1. 取证(硬门禁)

- `docs/font-provenance.md`:字体来源/栅格路径/FONT 参数/参考图证据表。
- 参考工程 `data.ypf` 1,407 条,0 个字体扩展;唯一 `.psb` 是 E-mote 模型 →
  **无自带字体**。
- 引擎启动 `FUN_00403afc` @ `0x403afc` 用 `EnumFontFamiliesExA`
  (`lfCharSet=SHIFTJIS_CHARSET`) 枚举,回退表 `0x807008..0x807044`:
  `ＭＳ ゴシック` → `標準ゴシック` → `ＭＳ Ｐゴシック` → `ＭＳ 明朝` → `標準明朝`。
- 栅格路径:GDI `CreateFontIndirectA` + `TextOutA` 到 8-bit DIB
  (`FUN_00445421` @ `0x445421`、`FUN_004450c0` @ `0x4450c0`);无
  `GetGlyphOutline`/`ExtTextOut`/`DrawText`/`AddFontResource`/`ID3DXFont`,
  非 ClearType,非自绘 outline。
- FONT 参数存储/clamp 由 `0x444428..0x444b1c` 逐条钉死;默认值由
  `FUN_00464fac` @ `0x464fac` 钉死(SX/SY=20,AA=1,COLOR=COLOR2=0xffffff,
  GRADATION=1,ROTATION=0,DRAWMODE=0,BOLD=0)。
- 42 个编译后 FONT 组;参考配置:es_text g1517 `12×12/0x999999/AA=0`,
  g1698 `BOLD=1/AA=1`,g1606 `COLOR=#ffffff COLOR2=#ffff00`,
  es_system g132 `14×15/BOLD/DRAWMODE=1`。
- 官方参考图:用户提供 `official_title_user.png`(1200×900);`スタート`
  墨迹框 54×16;不启动引擎自动截图(用户指示)。

### 2. 实现

- `FontSource`:fontdb + 自带字节优先;引擎默认 family 回退;`NAME`/`NO`
  解析;缺失显式 `font.missing`,无静默 fallback;wasm 禁止系统枚举。
- 栅格:swash/skrifa outline → 8-bit coverage;SX/SY 非等比用 transform;
  bold 优先 bold face,否则固定 `embolden`;italic face 优先,否则 shear;
  AA 灰度/阈值 128;ClearType 显式 Unimplemented。
- 样式:COLOR/COLOR2/GRADATION、DRAWMODE、outline(round 膨胀)、shadow
  (整数偏移,无模糊)、ROTATION(octant×45°)、kerning(legacy kern)、
  `StylePolicy::Strict/Reference` 证据门禁。
- 固定合成顺序:base → bold/italic → outline → shadow → color/gradient →
  rotation → 调用方 DRAWMODE 合成。
- 有界确定性 LRU cache(条目+像素双上限)。
- 排版只定义 `TextLayout` 接口;`render_run` 是参考单行 helper,拒绝换行。

### 3. 验收

```text
$ cargo test -p yu-font
14 lib + 1 eris_config + 5 glyphs + 7 source_config + 1 style_golden
+ 12 styles + 1 doc = 41 passed; 0 failed

$ cargo check -p yu-font --target wasm32-unknown-unknown     # pass
$ cargo check -p yu-font --target aarch64-linux-android     # pass
$ cargo clippy -p yu-font --all-targets                     # 0 warning
$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-font/src   # 0
```

- 确定性:同 `(font,char,style)` 两次 coverage/RGBA hash 逐字节一致。
- 字形 golden:ASCII/假名/汉字/标点 coverage hash + metrics
  (`crates/yu-font/tests/golden/glyphs.txt`)。
- 样式 golden:粗体/斜体/AA off/描边 1,3/阴影/渐变/旋转/非等比
  (`crates/yu-font/tests/golden/styles.txt`)。
- 缺失字体/缺字形/坏字体 → `font.bad_face`/`font.glyph_missing`,不 panic。

### 4. 参考渲染器

```text
$ cargo run -p yu-font --example render_reference
== samples: 19 written to target/tmp/font-ref/samples.json
== contact sheet: target/tmp/font-ref/contact_sheet.png
== official diff: iou=0.537 max_channel=178 mean=73.88
```

- contact sheet 真读 ERIS 编译产物;运行时打印 42 个 FONT 组与 5 个
  参考槽位(见 `target/tmp/font-ref/render.log`)。
- 官方 diff:best size=15 bold MS Gothic;official ink 227,ours 211,
  intersection 153,fp 58,fn 74,max channel 178,mean 73.88;归因 GDI vs
  swash outline/AA + 未建模的浅色 rim。
- 性能(release):1000 个可渲染 CJK × 2 字号 × 1px outline
  - 12px:first 1799.87 ms (1.800 ms/glyph),cache hit 0.38 ms
    (0.0004 ms/glyph),848 KiB
  - 24px:first 2054.27 ms (2.054 ms/glyph),cache hit 0.42 ms
    (0.0004 ms/glyph),2393 KiB
  - 命中 1000/1000,miss 0;cache entries=1000。

### 5. 未解决/需 REA 复核

- `GRADATION` 方向映射未取证(默认 1 解释为竖直上→下;Strict 只允许纯色)。
- `ROTATION` 0..8 的渲染语义未观察到(octant×45°,Reference only)。
- `BNO` 字体列表分支 `0x444ce7` 未解析(仅保存值)。
- GDI `lfHeight` 与 SX/SY 的精确映射、合成粗体像素量未完全取证。
- `FONT[NAME]` 找不到时引擎的回退/报错行为未取证(本 crate 显式失败)。

## 成果 H1 (2026-10-20) · yu-trace schema v3 + 帧描述 + producer-neutral 对齐 + 官方屏幕级 oracle

> 任务书 `docs/tasks/task-h-trace-oracle.md`。只改 `crates/yu-trace` 与 `tools/`;
> 未改 `yu-rt`/`yu-gfx`/字体/后端;无 git 写操作。

### 1. schema v3(producer-neutral)

- `SCHEMA_VERSION = 3`;v2 文件不迁移、直接 `trace.schema`。
- header 新增:
  - `producer`: `kind`(kernel/official)、`name`、`version`(official 侧是采集器版本)、
    `sample_sha256`;
  - `clock`: `mode`(none/tick/tick_at/mixed) + 逐 tick `now_ms` 序列。
- 跨 producer 语义比较只投影 `codec` + `boot` + `input` + 双方都存在的
  `producer.sample_sha256`;`kernel`/`tables`/producer 名称版本/`clock` 不参与
  跨 producer 整行比较(见 `HeaderCompare`)。
- `seq` 仍为同一 producer 内唯一序;跨 producer 对齐改用
  `(sub, kind, frame, ordinal)`。

### 2. ③ frame_description(真实记录)

- `sub=frame` / `kind=frame_description` 从占位变为真实记录,payload:
  `frame, level(event|screen), display_sha256, pass_count, op_count,
  executable_ops, skipped_ops, display`。
- 调用方输入结构 `FrameDescriptionInput`;本 crate 不硬绑 `yu-gfx` 字段:
  - 传 `display` JSON → 解析、规范序列化、计算/校验内容寻址;
  - 只传摘要 → `display=null` 的摘要级记录;
  - 坏 hash 格式 = `trace.parse`,display 与摘要不一致 = `trace.schema`;
  - 同一输入两次写入逐字节一致。
- ④ `platform/platform_call` 仍显式拒绝。

### 3. 投影/对齐 diff

- 新增 `diff_aligned`/`diff_aligned_logs` 与 `diff_traces` example:
  - 逻辑键 `(sub, kind, frame, ordinal)`;
  - `Projection` 按 sub/kind/frame 过滤;
  - `HeaderCompare::{Ignore, CodecSample, Semantic, Strict}`;
  - `payload_fields` 显式选择可比 payload 顶层键(用于 official 没有 args 的场景);
  - 首分歧报告 `key / expected_line / actual_line / path / expected / actual / context`。
- 原有 `diff_text` 逐行字节严格 diff 保持不变。

### 4. 回放闭合

- `VmTrace` 记录逐 tick 时钟,`finish` 写入 header;`verify_clock` 回放校验。
- `from_header_checked` / `boot_checked` 校验数据源编码与 `header.codec` 一致。
- aux 源与主源共享同一个 `FrameHandle`(`from_recorder_with_frame`)。
- `consumed_input_summary` 只覆盖 VM 实际消费的逻辑帧;`finish_with_timeline`
  使用它。

### 5. 官方 producer

- `tools/official_trace/capture_screen.py`:官方 0.495 `yu-ris.exe` + ERIS
  `data.ypf`,PrintWindow 截取 800×600 客户区,写 schema v3
  `frame/frame_description`(`level=screen`),另写 input timeline 与 manifest。
  **已验证**:启动、截图、trace 解析、与内核屏幕 trace 对齐 diff 均可跑通。
- `tools/official_trace/probe_dispatch.py`:事件级 L1 探针(WOW64 只读调试,
  0.495 分派点 `0x4044db`,任务全局 `0x80171c`,脚本 ID `task+0x3c`)。
  **未打通**:标题画面/点击后 25 s 内 0 命中;延迟布防后
  `ReadProcessMemory(0x400000/0x401000/0x4044db)` 仍返回 error 299
  (ERROR_PARTIAL_COPY)。**未伪造事件级 trace,L1 明确未达成**。
- 屏幕级对齐尝试已按用户裁决撤回:官方标题画面与内核 `first_frame` 不是同一
  逻辑锚点,不能作为渲染分歧证据。见 `docs/divergences/DIV-H-001`
  (status=withdrawn)。

### 6. 验证(原始结果)

```text
$ cargo test -p yu-trace   # YURIS_REQUIRE_SAMPLES=1, 真样本 data.ypf
lib 43 passed; diff 13; equivalence 7; event_trace 14; golden 2;
real_sample 2 (146.85s); roundtrip 7; doc-tests 0; 0 failed
```

```text
$ cargo clippy -p yu-trace --all-targets
yu-trace 自身 0 告警(输出中的 warning 均来自依赖 yu-rt)
```

官方屏幕级采集(原始输出摘录):

```text
$ python tools/official_trace/capture_screen.py --exe <官方 exe> --cwd <工程根> \
    --out target/test-tmp/official/eris_screen --frames 6 --interval 0.4 --warmup 8
window pid=... hwnd=... client=800x600
wrote target/test-tmp/official/eris_screen/official_screen.jsonl (6 frames)
```

事件级探针(原始输出摘录,证明未达成而非静默):

```text
$ python tools/official_trace/probe_dispatch.py --exe <官方 exe> --cwd <工程根> \
    --out target/test-tmp/official/dispatch_hits.jsonl --seconds 15 --click-after 5
base 0x400000 reloc 0x0 hide True
defer breakpoint for 3s
probe 0x44db None err 299
probe 0x400000 4d err 299
probe 0x401000 53 err 299
arm still failing err 299
done timeout hits 0
```

### 7. 未完成/挂账

- ④ `platform_call`:等平台后端,继续显式拒绝。
- 官方事件级 L1:分派探针 read error 299 未解决;下一步先在任务泵入口
  `0x404398` 验证泵是否运行,再处理 WOW64 读内存权限/反调试。
- 官方首屏 vs 内核首帧:**暂停**,锚点不成立,不作一致性结论。

## 成果 M3-SOFT (2026-10-20) · 软后端与首帧 PNG

> 提交 771cf79(`feat(gfx): 修正渲染语义并新增软件后端`)。范围
> `crates/yu-backend-soft`;只消费 `yu-gfx` 公开契约,不改语义层。

### 1. 实现

- `TextureStore`:`yu-vfs::DataSource` 取字节,按 `frame.image_probe_order`
  探测扩展名,`image` crate 解码为 RGBA8;按资产 key 缓存;`SolidBlack1x1`
  内建;`Snapshot`/`Offscreen` 跳过并计数;缺失/坏图产生 `asset.*` 诊断。
- `SoftRenderer`:严格按 pass 顺序;Offscreen 用组 `SX/SY`(未给时视口)+
  `pass.clear`;Screen 用 `frame.viewport` + `frame.clear`;MODE 0/1/2;
  固定管线 alpha `min(A,255)/255`;Point/Linear 采样;gamma 存储值混合;
  `TextRun` 只计数,不栅格化。
- `first_frame` 示例:GfxHost 同步路径 boot ERIS;`take_events()` 只做计数/
  drain,不把同一批事件重复喂给 `GfxState`;输出
  `target/tmp/first_frame.png` + `first_frame.display.json`。

### 2. 首帧实测

- frame 3,800×600,272 层,15 pass,44 op(15 executable / 29 skipped),
  `text=[]`。
- 17 个 `snapshot` 层因 `Op::Copy` 未实现不可渲染;11 个 `Op::Image`
  (`RECTPAINT` 族)+ 1 个 `Op::Copy` 被跳过;`filter.source=placeholder`。
- PNG 已产出;同帧连续两次编码逐字节一致(测试覆盖)。

### 3. 验证

```text
$ cargo test -p yu-backend-soft
10 lib + 5 render + 1 doc = 16 passed; 0 failed
```

### 4. 未完成(全部是 gfx 挂账,不是后端近似)

- DIV-GFX-007 COPY/COPY2 时序与区域;
- DIV-GFX-013 `RECTPAINT*`/`SIZE*`/`RESIZE` 等图像加工像素;
- DIV-GFX-012 文本栅格化/排版(与 `yu-font` 合流);
- DIV-GFX-009 颜色空间、DIV-GFX-008 组 RT 尺寸/清屏。

## 成果 M3-WGPU (2026-10-20) · wgpu 主后端与首帧对账

> 范围 `crates/yu-backend-wgpu`(+ 根 `Cargo.lock`)。语义唯一来源 =
> `yu-gfx::CompositorFrame`;`yu-backend-soft` 是整数参考执行器。未改
> yu-gfx / yu-rt / yu-font / yu-backend-soft / yu-trace,零 git 写操作。

### 1. 实现

- `device.rs`:instance/adapter/device/queue;`DeviceOptions` 支持后端与
  适配器名称子串覆盖;新增 `platform.gpu.*` 诊断码(不改 yu-log)。
- `textures.rs`:复用 `yu_backend_soft::TextureStore` 的 RGBA8 字节;
  资产按 key 上传一次,1×1 黑图单独缓存;资产/离屏目标共用进程级纹理
  身份,避免 bind group 键碰撞。
- `pipelines.rs`:MODE 0/1/2 × `Rgba8Unorm` 管线缓存;bind group 按
  `(texture_id, sampler)` 缓存;无 mip;`Point`=nearest,
  `Linear/Anisotropic`=linear。
- `passes.rs`:严格按 `frame.passes` 规划;Texture 只记账,Offscreen 用
  组 `SX/SY`/`pass.clear`,Screen 用 `viewport`/`frame.clear`;跳过原因与
  软后端逐项一致。
- `readback.rs`:GPU 纹理 → 256 字节行对齐 buffer → 紧凑 RGBA8;缓冲按
  尺寸复用。
- `lib.rs`:`WgpuRenderer::new` / `new_with_options` / `render_frame`;
  返回 `Surface + RenderReport + GpuStats`;`#![forbid(unsafe_code)]`;
  库内不碰 `std::fs`/`std::env`/路径。
- `examples/first_frame.rs`:GfxHost 同步路径 boot ERIS → wgpu 渲染 →
  PNG + 报告 + 与 `first_frame.display.json` 计数对账。

### 2. 首帧实测(默认选项,Vulkan)

```text
frame: 3 viewport: 800x600
gpu: backend=vulkan adapter=NVIDIA GeForce RTX 5070 type=discrete_gpu
layers: 272 (visible 34), passes: 15, offscreen passes: 2
ops: 44 total, 15 drawn, 29 skipped
missing assets: 0; skipped text: 0
filter: mag=point min=point mip=none source=placeholder
textures: decoded=13 cache_entries=13
gpu counters: draws=15 uploads=14 upload_bytes=1161036
              pipelines=1 (created 1) bind_groups=15 (created 15)
frame bytes: vertices=1800 readback=1920000
elapsed=32.237 ms deterministic=true
display check: OK (passes 15 ops 44 executable 15 skipped 29)
png: target/tmp/wgpu_first_frame.png (794572 bytes)
report: target/tmp/wgpu_first_frame.report.json
```

- 29 个 skipped = 1 个 `copy` + 11 个 `image` + 16 个 `copy` 纹理层
  + 1 个 `imageop` 纹理层,与软后端/`first_frame.display.json` 完全一致。
- 同帧渲染两次像素逐字节一致。
- 与软后端首帧的 PNG 不再逐字节相同(794572 vs 785127 字节):差异全部
  落在 9,451 个半透明像素上,最大 RGB 差 61;wgpu 保留这些像素的 alpha,
  soft 输出 alpha 恒 255。归因见下一条 alpha 决策与 README 差异账本;
  计数/不透明像素一致,**不做逐像素门禁**。
- DX12 覆盖实测:`YURIS_WGPU_BACKEND=dx12 YURIS_WGPU_ADAPTER=5070`
  → `backend=dx12 adapter=NVIDIA GeForce RTX 5070`,计数相同,
  `elapsed=38.334 ms`。

### 3. 验证(原始结果)

```text
$ cargo test -p yu-backend-wgpu
6 lib + 7 integration + 1 doc = 14 passed; 0 failed
  - device_initializes_or_skips / missing_adapter_is_explicit_error
  - blend_state 表(仅 0/1/2,D3D9 同因子 alpha `a²`)
  - clear_only_frame / 3x2 readback 行对齐 / 纹理缓存跨帧
  - unsupported_blend 计数与软后端逐项一致(不做像素门禁)
  - group_alpha 只在组合成时作用一次(断言 As² 链)

$ cargo test -p yu-backend-soft
10 lib + 5 render + 1 doc = 16 passed; 0 failed

$ cargo clippy -p yu-backend-wgpu --all-targets
yu-backend-wgpu 自身 0 告警(输出中的 warning 均来自依赖 yu-rt)

$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-backend-wgpu/src
0

$ grep -rn "<game-b>\|<game-a>\|C:\\\\Users\|D:\\\\Art" crates/yu-backend-wgpu/src
0

$ cargo check -p yu-backend-wgpu --target wasm32-unknown-unknown --lib
Finished `dev` profile [unoptimized + debuginfo]
```

### 4. 决策与差异

- wgpu 30.0.1(2026-10-20 配置索引最新稳定版);默认 features,无
  experimental;目标格式固定 `Rgba8Unorm`,禁用 sRGB 格式。
- alpha 通道按 D3D9 未开 `SEPARATEALPHABLENDENABLE` 的同因子语义执行:
  MODE 0/1/2 的 alpha 源项是 `a²`(gfx §5.2/§12.3 的 SRCBLEND/DESTBLEND)。
  soft 当前写的是 `a`(独立审查的 P1);本后端不复制该偏离,差异账本留档,
  首帧只在半透明像素上分叉,计数不变。
- 差异账本在 `crates/yu-backend-wgpu/README.md`:
  u8/f32 舍入、sRGB、`filter.source=placeholder`、适配器差异、
  双线性实现、alpha 语义、组 RT alpha 存储未取证、零尺寸组
  `ops_total` 修正、`draw_calls` vs `ops_drawn`、i64→f32 坐标。
- 任务书 §3 的硬检查已自动化:示例在计数不一致时退出非零。

### 5. 未完成

- 全部是 gfx 挂账(COPY 时序、图像加工、文本、MODE 3+ 等),本后端按
  契约跳过并计数,未近似。
- surface 呈现/窗口/wasm 事件循环留给宿主会话;wasm 仅留编译证据。

## 勘误 (2026-10-20) · 数据入口:清除 save/data 回退,封包/散盘二选一

- 旧结论(成果 F1/F2 环境节、示例代码):语料"迁移"到 `<project-eris>/save/data`,
  示例/测试增加 save/data 回退分支。**该回退是错的**:`save/data` 是官方 exe
  的解包产物(本机为让 exe 读 data.ypf 的布局),不是工程数据入口。
- 更正:工程数据入口 = 封包 `<project>/data.ypf`(优先)或散盘 `<project>/data/`,
  哪个存在用哪个;两者都是合法形态,`Mount` 后挂覆盖语义不变。
- 清理范围:soft/wgpu `first_frame`、`eris_dump`、`gfx_census`、
  `eris_first_screen`(两处 boot)删除 save/data 分支,boot 主源改为
  `YpfSource(data.ypf)` 优先;`first_frame` 纹理挂载栈在 `data/` 不存在时
  只挂封包;发现方式 = 用户指出 save/data 的来历。
- 验证(`YURIS_SAMPLE_ERIS_ROOT=<project-eris>`,soft `first_frame` 从封包启动):
  `loose_files=0 / ypf=true`;frame 3 census 不变(272 层 / 15 pass / 44 op);
  13 张纹理全部从包内解码;PNG 785,127 字节,二次渲染 `deterministic=true`;
  `events drained=578426 dropped=0`。

## 成果 M1 (2026-10-21) · 覆盖普查(600 固定时钟帧)与台账矩阵

> 范围 `crates/yu-rt/**`。零 git 写操作;未改 yu-gfx / yu-backend-* /
> yu-font / yu-trace / yu-vfs / yu-format / tools。

### 1. 新增普查工具

- `examples/coverage_census.rs`:
  - 数据入口 `data.ypf` 优先、散盘 `data/` 为备;语料指纹 FNV-1a64;
  - `SYSTEM_START` boot 后 `tick_at(f*16)` 跑 600 次 = **602 逻辑帧**;
  - 逐命令触发数、处理器类别、域事件参数组合样本(≤16/命令)、
    诊断按 `(code, 命令, 脚本:pc)` 归因、事件种类、每帧命令数;
  - `record_events=false`(频度不依赖事件流,避免 130 万事件帧挤爆队列)。
- `Vm::command_hits()` / `Vm::handler_kind()`(加法式 API):逐 opcode 命中
  计数**不受 `record_events` 影响**;`HandlerKind` 从 `vm` 再导出。
- `examples/extract_ypf.rs`:把 `data.ypf` 的 `ysbin/` 导出成散盘,供只认
  `YURIS_SAMPLE_ERIS_YSBIN` 的真实语料测试使用(测试输入仍来自封包)。
- `dump_yscm.rs` 的数据入口改为 `data.ypf` 优先(与 AGENTS §2 对齐)。

### 2. 普查结果(可复现)

```text
corpus: data.ypf (31,501,753 bytes), FNV-1a64 = 64d15de36b4fff30
static: 136 个 yst / 36,974 组 / 80,102 窗口
run:    tick 0..600, 602 逻辑帧, 首个可见帧 tick 0, 命令 7,046,350 次, 事件丢弃 0
text:   0 条 / 0 字节(600 帧内未进入文本事件路径)
```

- 运行期 Top(命令/次数):`LET` 1,725,119、`IF` 1,394,044、`IFEND`
  1,388,986、`LOOPEND` 1,075,308、`INT` 533,337、`GOSUB`/`RETURN`
  171,624、`CG` 80,620、`VARACT` 63,105、`STR` 36,342、`VARINFO` 36,152。
- 运行期未实现 Top:`CG` 80,620、`SOUNDINFO` 7,200、`CGEND` 5,713、
  `CGINFO` 1,598、`FILEINFO` 1,136、`CGACT` 631、`SOUND` 531、
  `WINDOWINFO` 129、`SAVE` 16、`EMOTE` 12、`WINDOW` 9、
  `SYSTEM` 6、`FONT` 5、`FILEACT`/`SAVEEND`/`FONTINFO` 3、`LET` 2。
- 最大单点缺口:`es_scenario.yst:231` 每帧 `IF[@_WINDOW_EXEC_CAPTION]`
  = 600 次;手册无页(三层缺①)⇒ 保持挂号。
- 复现:数据段在四次运行(含最终冻结代码)中逐字节一致;加入“首个可见帧”
  指标后的最终版本再跑两次,输出逐字节一致,sha256
  `4b58dba7f1e56ee539ff03477aec4598f5780b186e13d505c1451aa306b27450`。

### 3. 台账

- 新建 `crates/yu-rt/docs/coverage-matrix.md`(220 行):119 条命令全量矩阵,
  固定列 `命令名 | 域 | 语料频度 | 手册页 | REA 锚点 | 状态 | 决策`;
  手册页由 0.495 HTML 索引机械提取;REA 锚点 = `DAT_007ace20[opcode]`
  (0.495 `FUN_0045cf90` 的逐槽写入,92 个覆盖 + 默认 `0x459e90`)。
- 与 `docs/command-provenance.md` 交叉核对并回写 TEXT / TEXTINFO / `_` /
  WORD / RETURNCODE / `_TEXT_FLAG` 六行;勘误与静态/运行频度口径写进矩阵 §4。

## 成果 M2 (2026-10-21) · 按证据实现文本语义 top 项

> 台账:`COV-M-01..04`(矩阵 §6)。所有语义先有三层出处再写代码;
> 未取证路径继续保持 `Unimplemented`。

### 1. COV-M-01 · `_TEXT_FLAG` 读取(控制字节流)

- 证据:REA 0.495 `0x446b93`(字符串系统变量取值):id `0x7a` `_TEXT`
  → `[state+0x38]`,id `0x7b` `_TEXT_FLAG` → `[state+0x3c]`。
- 实现:`src/text.rs::TextState`;`sysvar.rs` 的 `_TEXT_FLAG` 返回与
  `_TEXT` 并行的控制字节流。
- 字节表(已取证):`'1'` 半角、`'2'` 全角、`'R'` 改行、`'P'` 改页、
  `'r'` 点击+改行、`'p'` 点击+改页、`'C'` 点击、`'E'` 行尾。
- 效果:600 帧里 LET 的运行时未实现从 **122 次降到 2 次**(剩
  `_SAVE_PATH` 宿主注入 1 次 + `_TEXT_FLAG` 写入 1 次)。

### 2. COV-M-02 · `TEXT` 属性 / 输出模式 / 行尾节奏 + `TEXTINFO`

- `TEXT`(REA `0x453448`):`OUTPUTMODE`(0=手动/非 0=自动,默认 1)、
  `WAIT`、`CLICKBUTTON1..4` / `SKIPBUTTON1..4`(`0`→哨兵 `0x3ff`)、
  `RETURN/PAGE/CLICKRETURN/CLICKPAGE/CLICK` 的控制标志、`CLEAR=1` 清空、
  `AUTORETURN/AUTOPAGE/AUTOCLICK`;**处理次序照引擎**:`CLICK` 先处理,
  自动模式下追加 `'C'` 后返回 -1 = 本任务本 vsync 让出,手动模式继续。
- `RETURNCODE`(REA `0x44c390`):自动模式按 AUTOCLICK/AUTORETURN/AUTOPAGE
  追加 `C/R/P`,最后恒追加 `E`;手动模式置清屏请求后追加 `E`;仍为
  `TextYield`。
- `TEXTINFO`(REA `0x4538cc`):按**参数是否出现**查询
  OUTPUTMODE/CX/CY/CLICK/AUTORETURN/AUTOPAGE/AUTOCLICK,写 `LET` 目标;
  `ID`/`SET` 槽引擎不读。
- 契约(加法式):`Vm::text_flags()`、`Vm::text_state() -> TextSnapshot`、
  `Vm::set_text_cursor()`、`Vm::set_text_click()`(CX/CY/CLICK 在引擎里由
  显示例程更新,内核不绘制,故由宿主推入)。**未改任何 VmEvent 字段含义,
  未新增事件变体**。

### 3. COV-M-03 · `WORD`/`_` 的逐字符宽度标志

- 证据:REA `0x458d44`(`WORD`)与 `0x4567c0`(`_`):文本字节进 `+0x38`,
  每字符一个标志进 `+0x3c`;CP932 首字节写 `'2'`,否则 `'1'`。
- 实现:`TextState::append_text`;`WORD`/`_` 的 `VmEvent::Text` 字段不变。

### 4. COV-M-04 · 域事件参数名视图 + 接口文档

- `VmEvent::domain_args(&yscm)` + `DomainArg`:把 `DomainCall` 的槽号按工程
  `ysc.ybn` 映射成参数名(槽号不跨版本稳定)。SOUND/SAVE/FILE 的槽名、
  手册值域、语料实测样本写入 `docs/domain-event-contracts.md`。
- 仍挂:变量引用身份(`SAVE[SET]/LOAD[LET]/FILEINFO[LET]`)未进事件,
  `DomainCall` 无 script/pc;两者需跨 crate 契约变更(DIV-RT-042)。

### 5. 回归(原始结果)

```text
$ cargo test -p yu-rt --lib
44 passed; 0 failed

$ cargo test -p yu-rt --test fixture
47 passed; 0 failed
  (+ text_flags_track_cp932_width_and_returncode_auto_defaults
   + text_outputmode_manual_click_continues_and_auto_click_yields
   + textinfo_queries_text_state_by_parameter_presence
   + text_flag_sysvar_reads_the_control_stream
   + domain_args_map_slots_through_project_yscm)

$ YURIS_REQUIRE_SAMPLES=1 \
  YURIS_SAMPLE_ERIS_YSBIN=$TEMP/eris-loose/data/ysbin \
  cargo test -p yu-rt --test corpus --test corpus_golden --test consistency
consistency: 4 passed
corpus:      9 passed (87.28s; loose vs packed 等价仍通过)
corpus_golden: 4 passed

$ cargo clippy -p yu-rt --lib
5 warnings(与基线一致;M-2 新增文件 0 条)
$ cargo clippy -p yu-rt --all-targets
17 warnings(全部为既有 examples/tests 告警;M-2 文件 0 条)
$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-rt/src
0
```

- 语料测试的散盘输入由 `extract_ypf` 从同一 `data.ypf` 导出,不是第二份语料。
- 回归金丝雀 `corpus::eris_entry_chain_runs_end_to_end` 第 2 帧:
  `274,466 → 272,296`。原因写在该测试注释里:①`_TEXT_FLAG` 读取不再挂号;
  ②`TEXT[CLICK=1]` 自动模式按引擎返回 -1 让出,后续命令推迟到下一帧。
  两次修正都有 REA 地址级证据,不是漂移。

### 6. 未完成 / 挂账

- `_WINDOW_EXEC_CAPTION` 600/600:手册无页,继续保持 `Unimplemented`
  (DIV-RT-007 已补频度与 REA 读点)。
- `_TEXT_FLAG` **写入**(语料 1 次):写路径未定位,保持挂号(DIV-RT-032)。
- `_TEXT_NUM`:手册无页;引擎读装载期全局 `0x80174c`,不是文本计数。
- `DomainCall` 的变量引用身份 + script/pc(DIV-RT-042)。
- `VmEvent::Text.id` 非 UTF-8 CP932(DIV-GFX-021):需改事件字段类型,
  跨 crate 契约变更,本轮不动。

---

## 成果 J1 · 首帧保真 Phase 0/A/B + COPY 取证(任务 J)

> 2026-10-21。范围:**取证 + soft alpha + RECTPAINT 族**。COPY/COPY2 只做到
> 证据与设计提案,**未改契约、未实现**。台账 = `docs/fidelity-ledger-j.md`
> (EVID-J-01..14);每个语义点四件套见台账原文。

### 1. 交付物

| 项 | 文件/产物 |
|---|---|
| 台账 | `docs/fidelity-ledger-j.md`(四件套 + 阈值 + 统计) |
| Phase 0 官方 title | `target/tmp/fidelity-ref/frame_0001..0006.png`、`official_screen.jsonl`、`manifest.json`(6 帧逐像素相同,800×600) |
| Phase 0 微场景 | `target/tmp/fidelity-micro4/`(RECTPAINT/2/3/区域/alpha)、`micro5`(组 RT)、`micro6h`(COPY2 时序);源 `data/script/micro.yst` + YSCom 0.495 编译 |
| Phase A | `crates/yu-backend-soft/src/render.rs` alpha 源项 `a²`;单测;soft README;wgpu README 差异账本 |
| Phase B | `crates/yu-gfx/src/state.rs` RECTPAINT 前置检查;`crates/yu-backend-soft/src/imageop.rs`;soft/wgpu 执行路径;gfx/soft/wgpu 测试 |
| Phase C | 证据:`fidelity-micro6h`;设计提案:`docs/render-parity.md` §8 + 台账 §3.1;**未动 `Op::Copy`/契约** |
| 文档 | `docs/render-parity.md`、`docs/gfx-unimplemented-todo.md`、`crates/yu-gfx/docs/divergences/README.md`(DIV-GFX-008/013/041/042)、两份后端 README |

### 2. 台账统计

| 范围 | Confirmed | Refuted | Uncertain |
|---|---:|---:|---:|
| A(alpha) | 2 | 0 | 0 |
| B(RECTPAINT 族) | 6 | 0 | 1(EVID-J-06 `+0x86`) |
| C(COPY/COPY2) | 2 | 1(EVID-J-11 旧线时序) | 2(EVID-J-12/13 机制) |

关键结论:
- **EVID-J-01**:D3D9 未开 `SEPARATEALPHABLENDENABLE` 时 alpha 源项 = `a²`;
  `FUN_00414ba4` 0x13/0x14/0xab 状态序列 + `d3d9types.h` + 微场景。
- **EVID-J-02..05**:RECTPAINT 填 RGB 并把 alpha 置 0xFF;RECTPAINT2 只填
  alpha;RECTPAINT3 填 `0xAARRGGBB`;区域含端点。处理体
  `FUN_0043185c` 模式 1/2/3。
- **EVID-J-08**(新增):官方主组 `W0` 子层直接画到屏幕;子组离屏 alpha
  不参与父级合成(K1 `(16,160,48)` vs G/K2 `(0,128,0)`)。挂
  DIV-GFX-008/041,**未改契约**。
- **EVID-J-11**(勘误):旧线 `last_rendered_frame` 被受控微场景 **Refuted**
  (group 源 `COPY2` 取命令执行时当前状态);`Op::Copy` 暂不动。

### 3. skipped 前后对照(ERIS 首个可绘制显示状态)

当前 HEAD 的该帧(tick 1)是 18 op;历史 golden(`771cf79`)是 44 op。两者
都列出,不把历史值当现状。

| 计数 | 历史 44-op 锚点 | 当前 18-op 锚点 |
|---|---:|---:|
| ops_total | 44 | 18 |
| executable/drawn 前 | 15 | 3 |
| executable/drawn 后 | **25** | **13** |
| skipped 前 | 29 | 15 |
| skipped 后 | **19** | **5** |

当前帧的 5 个跳过:`copy :/W0/CGS100` ×1、`image :/W0/CGS100` ×1、
`layer copy` ×2、`layer image(CGS100)` ×1。10 个 RECTPAINT 从
`gfx.imageop.unsupported` 转为 `Executable`;`CGS100` 因目标是 `Snapshot(271)`
保持 `gfx.imageop.copy_source.unsupported`(等 COPY)。

### 4. 量化验收

**Phase A(soft vs wgpu,同帧半透明):**

```text
semi-transparent parity: diff_pixels=0/16 max_channel_diff=0
```

**Phase B(官方 micro4 vs 内核 micro4,按台账 §2.4 区域):**

| 区域 | 最大通道差 | >4 像素 | 结论 |
|---|---:|---:|---|
| R1 内部 | 0 | 0 | PASS |
| R3 内部 | 0 | 0 | PASS |
| R5 内部 | 0 | 0 | PASS |
| R6 填充内部 | 0 | 0 | PASS |
| R2/R4(排除) | 48 | 10000/10000 | 归 EVID-J-08 |
| 1px 顶边(排除) | 74/53/74/85 | 每矩形 100 | 归 DIV-GFX-042 |

**Phase B 双后端(同一 micro4 CompositorFrame):**

```text
rectpaint parity: diff_pixels=0/32 max_channel_diff=0
soft vs wgpu(micro4 full frame): diff_pixels=0/480000 max_channel_diff=0
micro4 counts(两后端): 11 passes, 17 ops, 17 drawn, 0 skipped
RenderReport: 逐字段相等(wgpu 测试 assert_eq!(report, report))
```

**首屏 image op:** executable 0 → 10(SPLDST/SPLDST2/FONTL2..9);
`CGS100` 仍挂账。

### 5. 验证命令与原始输出

```text
$ cargo test -p yu-backend-soft
14 passed; 7 passed; 1 passed; 0 failed

$ cargo test -p yu-backend-wgpu
6 passed; 9 passed; 1 passed; 0 failed
rectpaint parity: diff_pixels=0/32 max_channel_diff=0
semi-transparent parity: diff_pixels=0/16 max_channel_diff=0

$ cargo test -p yu-gfx
10 passed; 5 passed; 10 passed; 1 passed
eris_first_screen_display_list FAILED: visible left=8 right=34
eris_first_screen_with_synchronous_info_queries FAILED: visible left=8 right=34
```

```text
$ YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo run -p yu-backend-soft --example first_frame
frame: 3 viewport: 800x600
layers: 272 (visible 8), passes: 15, offscreen passes: 2
ops: 18 total, 13 drawn, 5 skipped
png: target/tmp/first_frame.png (540267 bytes) deterministic=true

$ YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo run -p yu-backend-wgpu --example first_frame
ops: 18 total, 13 drawn, 5 skipped
display check: OK (passes 15 ops 18 executable 13 skipped 5)
frame bytes: ... deterministic=true all_black=true
warning: 首帧全黑(当前 boot 锚点未到 title/仅清屏);这不是 1:1 证据
```

```text
$ YURIS_SAMPLE_ERIS_ROOT=target/tmp/fidelity-micro4 \
  YURIS_FIRST_FRAME_TICK=5 cargo run -p yu-backend-soft --example first_frame
frame: 6, layers 8 (visible 8), passes 11, ops 17, 17 drawn, 0 skipped
png: target/tmp/fidelity-micro4-kernel-soft.png (1920813 bytes)

同参数 wgpu: display check OK (passes 11 ops 17 executable 17 skipped 0),
all_black=false, png 1920813 bytes;
soft vs wgpu: diff_pixels=0/480000 max_channel_diff=0。
```

```text
$ cargo clippy -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
本三项 0 新增告警(yu-rt 5 条既有 warning)

$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-gfx/src crates/yu-backend-soft/src crates/yu-backend-wgpu/src
0
```

> **命令陷阱**:soft 与 wgpu 的示例都叫 `first_frame`,Cargo 的
> `target/debug/examples/first_frame.exe` 会互相覆盖;按 `-p` 运行前
> 需 `touch` 目标 crate 的示例源(或给两个包用不同 `CARGO_TARGET_DIR`)。
> 本次验证用 `touch` 强制重建各自二进制。

### 6. 遗留挂账 / 阻塞

1. **ERIS boot→title 未达成(跨 crate 阻塞,未修)**:当前 HEAD 在 tick 1
   只有 8 visible / 18 op,屏幕全黑;历史 golden 的 34 visible / 44 op 来自
   `771cf79`,最新 `yu-rt` 的 YSVR/GOSUB 修复改变了启动路径。
   `cargo test -p yu-gfx` 的两条 golden 因 `visible 8 vs 34` 失败。
   任务 J 禁止修改 `yu-rt`;未用 `UPDATE_GFX_GOLDEN=1` 掩盖,保留失败并
   在此报告。**在 yu-rt boot 恢复到 title 前,官方 title 与内核的逐像素
   对拍无法进行**。
2. **COPY/COPY2**:EVID-J-10/11/14 已取证(含旧线时序 Refuted),
   EVID-J-12/13 仍 Uncertain;`Op::Copy`/`TextureRef::Snapshot`/后端表面
   缓存未改。设计提案见 `docs/render-parity.md` §8;契约改动需主人确认。
3. **组 RT**:EVID-J-08 行为已确认(主组直绘、子组 RT alpha 不参与合成),
   修正会改变所有半透明层;挂 DIV-GFX-008/041,需主人确认契约。
4. **1px 半像素边界**:DIV-GFX-042;RECTPAINT 区域内部已一致。
5. `+0x86`(RECTPAINT2/3 的 alpha 标志)未建模(EVID-J-06)。
6. `SIZE/SIZE2/ROTATION/RESIZE/TRIM/MONO/BLUR/...` 仍 Unimplemented;
   `CGS100` 的 RECTPAINT2 等 COPY。
7. 示例二进制同名覆盖问题(见上),未改 Cargo 结构。

## 成果 L1 (2026-10-21) · 官方事件级 L1：error 299 根因、泵验证、事件 trace 与对拍

> 任务书 `L`。只改 `tools/official_trace/**` 与 `crates/yu-trace` 的加法式扩展
> （新增 example + 顺序投影 API/单测）；未改 `yu-rt`/`yu-gfx`/`yu-backend-*`/字体；
> 无 git 写操作。本节取代 H1 中“官方事件级 L1 未达成”的挂账；顶部总表未改，
> 以本节为准。

### 1. error 299 根因（Phase L-1，GL-1）

先复核任务书给定事实：官方 exe sha256 `4f5eefa3…`、PE32、ImageBase `0x400000`、
无重定位目录、`DllCharacteristics=0`；REA 复核 `0x4044db` 确为
`ff 14 b9`（`call dword ptr [ecx+edi*4]`，位于 `0x404398`），`0x404398` 为任务
步进函数入口。

假设表与最小实验结果（原始 JSON 在 `target/tmp/l1-probe/l1_experiments_*.json`）：

| 假设 | 实验 | 终态 |
|---|---|---|
| H1 句柄权限 | 调试句柄 / `VM_READ` / `VM_READ|VM_WRITE|VM_OPERATION` / `ALL_ACCESS` 各读 `0x4044db` | **证伪**：四种句柄都成功读到 `ff14b9`；只有写需要 `VM_WRITE` |
| H2 时机 | 创建瞬间 / +1s / +3s / +6s 各读 | **证伪**：正确地址全阶段可读；`0x0/0x1000` 的 299 只是未映射 |
| H3 附加方式 | `DEBUG_ONLY_THIS_PROCESS` / 普通启动外部读 / `DebugActiveProcess` | **证伪**：三种方式都读成功 |
| H4 反调试 | REA 查 `IsDebuggerPresent`/`CheckRemoteDebuggerPresent`/`NtQueryInformationProcess`；运行时读 32 位 PEB / DebugPort / DebugObjectHandle | **无证据**：无相关导入；debugged 时 `BeingDebugged=1` 但引擎正常运行；未做任何绕过 |
| H5 地址/重定位 | PE 头 + 实际模块基址 | **成立**：base=`0x400000`，无重定位/ASLR |

**根因**：旧探针把 `--bp-rva 0x44db` 直接当绝对地址，实际落在 `0x44db`（未映射）；
正确地址是 `base + RVA = 0x4044db`。`ReadProcessMemory(0x44db)` → error 299，
`WriteProcessMemory(0x44db)` → 998，断点从未落下。旧日志里成功读之后的
`err 299` 是**未清零的陈旧 GetLastError**。修复：统一 `base + RVA`，每次 API
调用前 `SetLastError(0)`、`use_last_error=True`；并按任务书删除了旧版本的
PEB 反调试清零（不做绕过）。

### 2. 泵验证（Phase L-2，GL-2）

在 `0x404398` 下断点：标题启动后 **t≈0.68 s 首次命中**，`EAX` 为任务对象；
命中覆盖 script 8/10/12/20（随后 0/5/20/52/60/66/70/71/72）。泵在跑。

分派实测走 `0x404547`（`0x8015b0 != 0` 的分支），`0x4044db` 是另一条分支；
探针同时挂两点。命中记录：`EDI`=pc、`[0x80171c]`=当前任务、
`task+0x3c`=script、`[ECX+EDI*4]`=handler。

### 3. 事件级采集（Phase L-3）

工具 `tools/official_trace/probe_dispatch.py` 现支持：

- 两个分派点（`0x44db`/`0x4547`）与全局 handler 表 `0x7ace20`（119 项，
  运行时 87 unique；共享组 `0x459e90`=27 槽、`0x4634d8`=7 槽）；
- handler→opcode→`ysc.ybn` 命令名；共享 handler 的 `cmd/name` **显式留空**；
- 引擎 tick `[0x801268]` 写入 trace 的 `frame`；
- 自动输入（`sendinput`，前台校验/按住到 tick 前进再松开）与**手动触发**
  `--trigger-rva 0x5ab63`（有效 UI 点击后才挂分派断点，标题不卡）；
- schema v3 `sub=vm/kind=command`，`args=[]`（官方参数不可观测，不伪造）。

采集样本（`target/tmp/l1-probe/`，不入库）：

| 项 | 值 |
|---|---|
| 语料 | `<project-eris>/data.ypf`，sha256 `5744a7be…` |
| `ysc.ybn` | 11156 B，sha256 `741d4caf…`，119 命令；与官方解包副本逐字节一致 |
| 点击后官方事件 trace | 101,580 条命中，tick 299–308；`run_click2/official_events_debug.jsonl`（header 的 `debug_mode=true` 由窗口标题证据重生成） |
| 内核同语料 trace | `kernel_events_debug12.jsonl`：876,096 条记录 / 632,805 条命令，frame 4–11（`--debug-mode true`） |

**debug_mode 事实**：官方窗口标题 `GetWindowTextA` 原始字节
`81 79 83 66 83 6f 83 62 83 4f …`，CP932 解码 = **【デバッグモード】**；
`save/data/config/projectconfig.txt` 的 `DEBUGMODE=0` 不是运行期实际值。
探针默认 `--debug-mode true` 并记录证据。

### 4. 对拍（`diff_aligned` 顺序投影）

`crates/yu-trace` 加法扩展：

- `TraceLog::sequence_projection` / `TraceLog::command_sequence_projection`
  （header 保留，指定 script 的 `vm/command` 保留，`frame=0`、`seq` 重编号；
  显式比较投影，不改原日志/schema）；
- `diff_traces --order-only --script 60`：默认只比较 `script/pc/name`；
- 单测 `sequence_projection_collapses_frame_and_filters_script`。

对拍命令与原始结果：

```text
$ cargo run -q -p yu-trace --example diff_traces -- \
    --expected target/tmp/l1-probe/run_click2/official_events_debug.jsonl \
    --actual target/tmp/l1-probe/kernel_events_debug12.jsonl \
    --mode cross --header codec-sample --order-only --script 60 --context 5

order-only projection: expected 4681 commands, actual 1932 commands (script=Some(60))
DIVERGENCE code=trace.diff
key=vm/command frame=0 ordinal=966
path=/payload/pc
expected=1414
actual=820
```

- **script 60 前 966 条 `(script,pc,name)` 与官方逐条一致**；
- 首个分歧在 script 60 `RETURN`(pc975) 之后的**调用点**：官方为 script 58 的
  场景调用（入口 pc1414），内核为 script 20 的菜单调用（入口 pc820）；
  两边处于不同任务相位，不是 script 60 体内命令差异 → `DIV-L-002`（open）。
- `debug_mode=false` 时首分歧更早（ordinal 643），根因是内核启动配置：
  script 20 pc237 `IF[@11 >= @2240]` 官方为真（`GOSUB es.DebugTask`），
  内核默认 release 为假；`debug_mode=true` 后内核同样为真并走 pc238
  → `DIV-L-001`（fixed，运行期注入配置，不改内核代码）。

### 5. 验证

```text
$ cargo test -p yu-trace   # YURIS_REQUIRE_SAMPLES=1 + YURIS_SAMPLE_ERIS_YPF
lib 测试(含新增顺序投影单测)全绿; diff 13; equivalence 7; event_trace 14;
golden 2; real_sample 2 (136.44s); roundtrip 7; 0 failed
```

```text
$ python tools/official_trace/probe_dispatch.py --exe <官方 exe> --cwd <工程根> \
    --ysc <ysc.ybn> --out-dir target/tmp/official/events \
    --seconds 20 --arm-after 6.5 --trace-out .../official_events.jsonl
armed 0x4044db ff / armed 0x404547 ff
dispatch table 0x7ace20 unique 87
hit 0 tick 286 s 5 pc 33 cmd 49 IFEND
...
wrote .../official_events.jsonl
```

### 6. 挂账/限制

1. 官方 `args` 不可观测，trace 里为 `[]`；共享 handler 名称留空。
2. 官方任务/`WAIT`/跳转事件尚未采集；`DIV-L-002` 需要任务级对齐才能继续。
3. 官方 tick 与内核逻辑帧未证明同钟；当前只做命令顺序对拍。
4. 点击转场起点依赖手动触发或前台可控的自动输入；`run_click2` 已错过转场开头
   约 15 tick（`DIV-L-002`）。
5. 断点会话内不能安全截图（PrintWindow 等待被挂起 UI 线程而死锁）；屏幕级仍用
   `capture_screen.py`。
6. `DIV-H-001` 的“官方首屏 vs 内核首帧”仍按撤回处理，不作结论。

## 成果 P0 (2026-10-21) · 真实文本路径验证与输入计数器修正

> 范围 `crates/yu-rt/**`。零 git 写操作;未改其他 crate。

### 1. 工具

- 新增 `examples/text_scenario.rs`:输入时间线(周期点击/释放)+ 文本事件统计;
  支持 `YURIS_ENTRY_LABEL` 直接以真实语料标签 boot,可 dump 指定帧的
  脚本/命令/参数/任务/诊断。
- 全量 `SYSTEM_START` 600 tick + 每 20 tick 点击:**0 条 Text**;
  `es.ScenarioScript` 在 tick 126 被 `TASK[E=0]` 暂停,标题/UI 依赖
  `GraphicsHost`/`WINDOW`/`MOUSE`,未进入文本。这是宿主缺口(P2),不是
  文本语义缺口。
- 直接以真实语料标签 boot(绕过标题/宿主 UI)验证文本:
  - `SCENARIO_EPILOGUE`(s129,30 tick):`Text` 1 条 / 12 字节,
    `ReturnCode` 1 条,`_TEXT_FLAG` = `222222CRPE`(6 个全角 + 自动行尾);
  - `RP_101`(s133,120 tick,每 10 tick 点击):`Text` 10 条 / 132 字节,
    `ReturnCode` 10 条,flags 106 字节;10 行逐行推进,第 99 tick 主线正常结束;
    每行标志 = `'2'*n + C/R/P/E`;输出模式 1,AUTORETURN/AUTOPAGE/AUTOCLICK=1。
  - 文本内容(CP932)逐行核对:`回想シーン１スタート`、`あいうえお`、
    `かきくけこ`、`さしすせそ`、`回想シーン１終了`、`回想シーン２スタート`、
    `ＡＢＣＤＥ`、`ＦＧＨＩＪ`、`ＫＬＭＮＯ`、`回想シーン２終了`。

### 2. 发现并修复的输入 bug(COV-P0-01)

- 松开后计数器逐帧递减;重新按下时旧实现 `+= 1`,长期未按后一次短按
  从 `-N` 加到 `-N+1`,永远到不了正值 ⇒ 第二次点击无法推进文本。
  手册「押された瞬間から 1 → 2 → 3」⇒ 按下沿重置为 1。
- `text_advance_requested` 原检查 `ENTER`/`CTRL`,计数器键名却是
  `KEY_ENTER`/`KEY_CTRL` ⇒ ENTER/CTRL 从未生效(只有 MOUSE_L 生效)。
- 回归:`input::tests::repress_after_release_restarts_at_one`、
  `text_advance_buttons_use_binding_names`;fixture 的文本推进测试
  增加到第三行,验证长期松开后的第二次点击。

### 3. P0 验收

```text
cargo test -p yu-rt --lib --test fixture
lib:     47 passed
fixture: 47 passed
cargo test -p yu-rt --test corpus   10 passed(新增真实文本回归)
  - eris_real_scenario_text_path_emits_expected_flags:
    RP_101 → 10 Text / 10 ReturnCode / flags 106;首行 CP932 原样;
    首行 10 个 '2' + CRPE,末行 CRPE;120 帧内结束
cargo clippy -p yu-rt --lib   5 warnings(基线不变)
```

## 成果 P1 (2026-10-21) · `_WINDOW_EXEC_CAPTION` 有界取证(仍挂账)

- **手册**:0.479/0.495 全部 HTML 按字节搜索无 `_WINDOW_EXEC_CAPTION`;
  older SDK 0.292/0.466 也无。只有 `_WINDOW_EXEC_MAX/CLOSE` 有页 ⇒ 三层缺①。
- **语料语义**(`es_Scenario.yst`):与 `@_MENUCODE==30001` 并列的
  `IF[@_WINDOW_EXEC_CAPTION==1]` → 写回 0 → `WINDOW[NO=0 WSX=… WSY=…]`;
  注释掉的旧路径是 `WINDOW[NO=0 MAXIMIZE=1]`。即「窗口尺寸再初始化」的
  一次性宿主事件标志,不是 VM 可计算状态。
- **REA**(0.495,sha256 `4f5eefa3…`):
  - 读 `0x427704`(int getter case 0x78 → `[0x801924]`);
  - 脚本写 `0x446f70`(`MOV byte [0x801924], DL`);
  - 引擎写 `WM_SYSCOMMAND(0xA3)` 路径 `FUN_0045a9d0`(`0x45a9d0`);
    `0x45a9e7` 写 `[0x801924] = (char)*ecx + 1`,条件
    `[0x801338]==1` 且 `[0x8015e0]==0`;否则置 `[0x801504]=1` 返回 0。
  - `*ecx` 来源与两个条件全局仍未取证,不足以定义完整行为。
- **决策**:保持 `Unimplemented`(DIV-RT-007 已补);若放宽门禁,正确形态是
  宿主 setter(同 `_WINDOW_EXEC_MAX/CLOSE`),不在 VM 猜 0/1 状态机。
- 矩阵 §3/§7 与 DIV-RT-007 已同步。



---

## 成果 K1 (2026-10-21) · hosts 层:原生引擎服务 + wasm 呈现端(架构 B)

> 任务书 `docs/tasks/task-k2-hosts.md`。**2026-10-21 主人决定把架构从
> 「wasm 跑内核」改为 B:Rust 读本机数据 + 跑内核,wasm/JS 只做呈现**;
> 浏览器不下载整包,服务端按需读文件。本成果按 B 交付,任务书里被替代的
> 条目在 §7 逐条挂账。零 git 写操作;`yu-rt` / `yu-gfx` / `yu-backend-*` /
> `yu-font` / `yu-trace` **未改**;`yu-vfs` 最终 **零改动**(B 下不需要
> `from_bytes`,切架构后已撤销)。

### 1. 交付物

| 路径 | 内容 |
|---|---|
| `crates/yu-cli` | 原生宿主 + 本地服务 + HMR 监督 + 用例执行器。lib + 两个 bin:`yu-cli`(产品)、`yu-engine`(HMR 子进程) |
| `crates/yu-host-wasm` | 浏览器**呈现端**(wasm-bindgen):WebGPU shim 优先,Canvas2D 回退。不依赖 rt/gfx/后端/vfs |
| `www/` | `index.html` + `app.js` + `styles.css`:canvas、状态浮层、Restart/Pause、鼠标/键盘输入、WebSocket |
| `cases/` | `first-screen` / `until-visible` / `record-events-off` |
| `crates/yu-cli/tests/hmr.rs` | HMR 冒烟(10 次重建,默认 `#[ignore]`,显式跑) |
| `.gitignore` | `www/pkg/`、`www/node_modules/`(产物不入库) |

命令:`run`(headless)/`serve`(产品入口)/`dev`(HMR)/`case`/`bench`。

### 2. 架构与协议

```text
浏览器 www/index.html + app.js + yu-host-wasm(只呈现)
  │ WebSocket /ws:YUF1 二进制帧 + 输入/控制 JSON
  ▼
yu-cli serve(或 yu-engine serve)
  │ 同一份 native 读取:data.ypf 优先、data/ 为备
  ▼
yu-rt → yu-gfx(CompositorFrame) → yu-backend-soft / yu-backend-wgpu
```

- 服务端主线程按 `--fps` 推进引擎;帧只编码一次(`Arc<Vec<u8>>`)广播。
  慢客户端丢旧帧并计数;无客户端时不推进(不烧 CPU)。
- 帧头 56 字节:`YUF1` + w/h/frame/ops/skipped/executed/passes/layers/
  visible/ms + backend + flags(drawable/rendered/paused)+ rgba_len + RGBA8。
- 新连接先收 `/api/status` 文本 hello,再补发**最近一帧**(暂停/重连不空白)。
- `--pause-at N` 用于截图/验收:到达帧即停;Restart 会清暂停并重新跑。
- 重启单元 = `Vm` + `GfxState`;数据源母本、纹理缓存、渲染器实例保留。
- 浏览器 WebGPU:**呈现**走页面 shim(web-sys WebGPU 绑定需要
  `web_sys_unstable_apis`,会污染构建前提);合成器始终是 native 后端。
  两条呈现路径(2D/WebGPU)与 native 像素逐字节一致(见 §5)。

### 3. P0 阈值与实测

阈值在 `crates/yu-cli/README.md` **先声明后测量**;正式测量前把
「boot + 首个可绘制帧 <1s」这条自加口径改回任务书原文「re-boot 后可 tick
<1s」,理由与时间点写在 README。机器:RTX 5070 / Vulkan / Rust 1.96;
语料 `data.ypf` 31,501,753 B。

```text
$ ./target/release/yu-cli.exe bench <project-eris> --frames 300
boot(off): 104.43 ms; first tick 2439.77 ms; first drawable at frame 3 after 2841.95 ms
  frame 1: tick 2439.77 ms render 4.94 ms
  frame 3: tick 284.62 ms render 5.13 ms
boot(on): 54.45 ms; first tick 4570.21 ms; first drawable at frame 3 after 5441.08 ms
on/off 守恒: display_sha256 一致 (off 1729689580a5 / on 1729689580a5)
reboot: boot 74.94 ms; first tick 3220.34 ms; ready in 3300.67 ms
steady: 300 frames in 7245 ms -> 41.5 fps (avg 24.09 ms, engine avg 23.79 ms, p95 16.46 ms)
steady slowest 5: frame 6: 2816.2 ms, frame 3: 309.9 ms, frame 227: 153.1 ms, frame 142: 22.9 ms, frame 13: 21.7 ms
wgpu: 444.10 ms adapter=NVIDIA GeForce RTX 5070 backend=vulkan
wasm: no-op 171 ms; incremental 575 ms; bindgen 96 ms
server: first frame 2589 ms; 60.4 fps; input p95 32.3 ms; dropped 0
gate boot(off) < 500 ms: 104.43 <= 500.00 -> PASS
gate re-boot 后可 tick < 1000 ms: 3300.67 <= 1000.00 -> FAIL
gate 稳态 >= 30 fps: 41.51 >= 30.00 -> PASS
gates: FAIL(记录实况,不改阈值)
```

**两条 FAIL 的同一个根因**:ERIS 启动链第一个大帧 129 万命令,release
2.4–3.2 s(debug ~14 s)。boot 本身 44–104 ms;这个帧是 `yu-rt` 派发/表达式
热路径,本任务只读消费、未改。按纪律保留 FAIL,未调阈值、未换口径。

### 4. HMR 冒烟(原始输出)

```text
$ YURIS_SAMPLE_ERIS_ROOT=<project-eris>     cargo test -p yu-cli --test hmr -- --ignored --nocapture
baseline: marker=k2-engine-0 pid=... (supervisor pid ...)
change 8/10: marker=k2-engine-hmr-8 pid=48444 e2e=3548 ms
change 9/10: marker=k2-engine-hmr-9 pid=11092 e2e=4066 ms
change 10/10: marker=k2-engine-hmr-10 pid=41172 e2e=3855 ms
leak check: supervisor rss 11329536 -> 11329536 (+0 bytes), handles 150 -> 150 (+0)
restore: marker=k2-engine-0
HMR smoke: 10 changes, avg 3640 ms, max 4242 ms (limit 15000 ms)
test hmr_rebuilds_child_and_keeps_service_alive ... ok
```

监督进程 `yu-cli` 不重建(Windows 运行中 exe 不可覆盖),反复重建的是
`yu-engine` 子进程;浏览器 WebSocket 自动重连并重新出首帧。改 `www/` 走
`reload` 控制行 + SSE。**系统 TEMP 不可写**会让链接器 `LNK1104`;监督进程
给 cargo 显式设 `TEMP/TMP=<workspace>/target/tmp/cargo-temp`。

### 5. 浏览器验收(自动化,headless Chrome 149 + CDP)

```text
服务: yu-cli serve <project-eris> --no-open --port 4205 --pause-at 302
native: yu-cli run <project-eris> --until-frame 302
  frame: 302 viewport: 800x600
  layers: 272 (visible 8), passes: 15
  ops: 27 total, 15 drawn, 12 skipped
  rgba sha256: 6d183527625c64f0eb12831a59f2ccd37a86cc39c20d62466d38bf133ed5df39

浏览器 ?present=2d:
  overlay: frame=302 ops=27 skipped=12 executed=15 present=2d
  canvas sha256: 6d183527...ed5df39       ← 与 native 逐字节一致
浏览器默认(WebGPU):
  overlay: frame=302 ops=27 skipped=12 present=webgpu
  canvas sha256: 6d183527...ed5df39       ← WebGPU 呈现同样逐字节一致
Restart: 状态回到 frame 1 → 再次跑到 302 并停住,浮层 302/27/12/15 不变
```

截图 `target/tmp/browser-webgpu-paused.png` 是 title 画面(frame 302,
「Ku-ris B4 for Windows 98/Me/2000/XP/2003/Vista/7/8/10/11」)。

### 6. 途中发现并修正(都有证据)

1. **宿主注入字段会改启动分支**:`@_OS_SOUND=0` + `@_MEDIA_READONLY=1` 时
   ERIS 首帧只剩 2 层/4 op;改回参考环境(`os_sound=true`、
   `media_readonly=false`)后与 `first_frame` 完全一致(frame 3 / 272 层 /
   18 op / 13 drawn / 5 skipped / PNG 540267 B)。README 与 `--help` 已写明
   「宿主不播放声音 ≠ 引擎认为声音不可用」。
2. **WebSocket 读写锁死锁**:读线程持 writer 锁阻塞读,发送循环永远拿不到
   锁(hello 之后无帧)。改为读不持锁,ping 以消息返回、由调用方回 pong。
3. **Windows 系统 TEMP 不可写** → cargo 链接 `LNK1104`;监督进程/bench 的
   cargo 子进程显式 `TEMP/TMP`。
4. **Restart 后仍保持暂停**:`--pause-at` 的暂停标志没有随 Restart 清零。
5. **wasm 导出用法**:`module.default()` 返回 wasm exports,不是 JS 命名空间;
   `new wasm.Presenter` → `new module.Presenter`。
6. **暂停/重连新连接无帧**:补「最近一帧」缓存,连接时立即补发。

### 7. 与任务书的差异 / 挂账

| 任务书 | 实际(B) | 理由 |
|---|---|---|
| P2 wasm HMR(改 Rust→重建 wasm→浏览器) | native HMR(重建 `yu-engine`→WS 重连) | 内核在 native;wasm 只是呈现端。presenter 构建实测 575 ms |
| P3 `Host::new(ypf_bytes)` 跑内核 | `Presenter`(呈现端) | 读取/执行在服务端,浏览器不持有包 |
| `--vite` 可选 | **取消** | 页面由引擎同源提供,HMR 用 WS 重连,不需要 Node |
| P4 用例带输入 | 输入时间线已实现并单测;3 条必做用例无输入 | `CLICK` = `MOUSE_L`,按下 frame、frame+1 松开 |
| `yu-vfs` 加 `from_bytes` | **无改动** | B 下不需要;已撤销,保持零跨 crate 改动 |
| 浏览器首帧 ≤5 s(含 31.5 MB fetch) | 不再下载整包;首帧 2.59 s | 初始化帧占 2.4 s;与上表同一根因 |

仍挂账:

- 两条性能门禁(首帧/重启后可 tick)受 `yu-rt` 初始化帧限制;若要做,属于
  M 线的派发热路径优化(本任务禁止改 `yu-rt`)。
- `clients` 计数在页面反复导航时可能短暂包含正在关闭的 socket(写失败即
  注销,不影响帧);未做主动心跳清理。
- 浏览器生产装载的 file picker/OPFS 不需要(B:服务端读本机);
  多 GB 包由 `YpfSource::open` 惰性随机读,浏览器只收帧。

### 8. 验证命令与原始结果

```text
$ cargo test -p yu-cli --all-targets
10 passed (lib);0 failed;hmr ignored(显式跑时 1 passed)

$ YURIS_SAMPLE_ERIS_ROOT=<project-eris>   cargo test -p yu-cli --test hmr -- --ignored --nocapture
1 passed(10 次重建,见 §4)

$ cargo clippy -p yu-cli -p yu-host-wasm --all-targets
yu-cli / yu-host-wasm 0 告警(输出中的 5 条 warning 均来自 yu-rt)

$ cargo clippy -p yu-host-wasm --target wasm32-unknown-unknown
Finished(0 告警)

$ cargo check -p yu-host-wasm --target wasm32-unknown-unknown
Finished

$ cargo test -p yu-cli --lib
10 passed; 0 failed

$ cargo test -p yu-vfs
(未改 yu-vfs;全量 workspace 跑法见下)
```

全量 `cargo test --workspace --no-fail-fast`(带 AGENTS §2 样本):

```text
yu-format corpus_eris 1 passed;yu-rt consistency 4 passed;yu-rt corpus 10 passed
(以上三项在首次跑时因 SDK 路径/散盘根布局失败,修正
 YURIS_SAMPLE_SDK0495=…/yu-ris_0495_032(不带 システム)并把
 system/YSCom/YSCom.ycd 放进散盘根后全绿)
yu-gfx tests/eris_first_screen:2 failed(visible left=8 right=34)
  ← 既有问题,成果 J1 §6 已报告(golden 34 visible 来自 771cf79);不在本任务
    允许改动的 crate 范围,未用 UPDATE_GFX_GOLDEN 掩盖。
其余包全绿。
```

### 9. 改动文件

新增:`crates/yu-cli/**`、`crates/yu-host-wasm/**`、`www/{index.html,app.js,styles.css}`、
`cases/*.json`;修改:`.gitignore`、`Cargo.lock`(新 crate 的依赖)。
未改:`yu-rt` / `yu-gfx` / `yu-backend-*` / `yu-font` / `yu-trace` / `yu-vfs`。


### K1 补充(2026-10-21)· 页面日志模块与 boot 链诊断

- 新增 `GET /api/report`(最近一帧的完整后端报告:skipped 原因/缺失资产/
  文本计数)与页面日志面板(`Log` 按钮,`?log=1` 自动打开;环形 300 行):
  连接/hello/每 0.5 s 帧摘要/输入/错误 + 每 2 s 的 skipped/missing 摘要。
- **boot 链没有动画的原因**(日志与 `/api/report` 实证,frame 20/60/180):
  `transition.unsupported`(`:/W0/SPL101`、`SPL401`)与
  `gfx.imageop.unsupported`(`:/W0/SPLTRBUF101/401`)被 gfx/后端跳过,
  boot 动画的可见内容就在这些 op 上;302 帧后的 title 是静态的。
- **title 按钮不显示的原因**:gfx 发出的资产名是
  `cgsys/title/btn_bgmmode` 等基名,包里实际是状态后缀文件
  (`_na/_off/_on/_over`,共 69 个 `cgsys/title/*`);状态后缀选择尚未实现,
  所以 `asset.missing` ×10。
- 以上两项都属 gfx/资产保真线(不在 K2 可改 crate 范围),本任务只提供
  可观测性与证据,不改语义。

## 成果 J2 · Title 链路 TODO 清理:转场双纹理、COPY、图像加工与按钮取证

> 2026-10-21。范围:`crates/yu-gfx`、`crates/yu-backend-soft`、
> `crates/yu-backend-wgpu` 及其文档。未改 `yu-rt` / `yu-vfs` / `yu-cli` /
> `yu-host-wasm`;零 git 写操作。台账 = `docs/title-motion-ledger.md`
> (EVID-T-01..15;P0 三张清单 + P3 区域对拍)。

### 1. 交付物

| 项 | 文件/产物 |
|---|---|
| P0 探针 | `crates/yu-backend-soft/examples/title_probe.rs`(逐 tick transition/copy/image 计数、跳过原因、资产解析) |
| P0 原始数据 | `target/tmp/j2-title-probe-600.tsv`(J2 前) / `target/tmp/j2-title-probe-600-after.tsv`(J2 后) |
| 台账 | `docs/title-motion-ledger.md`(EVID-T-01..15 + 关闭表 + P3 结果) |
| 官方微场景 | `target/tmp/j2-micro-{trans1..transA, rev, copy, copy2}/`(YSCom 0.495 编译 + `capture_screen.py`) |
| 官方编译 shader | `target/tmp/j2-shaders/`(`fxc /dumpbin` 反汇编;ps_2_0 转场公式备档) |
| 转场实现 | `yu-gfx::transition_coverage`(G 通道 mask + fade 模型);soft `draw_transition`;wgpu transition pipeline(`pipelines.rs` 双纹理 WGSL) |
| COPY 实现 | `Op::Copy` 可执行;soft 持久快照 + 区域 blit;wgpu `copy_texture_to_texture` + 快照目标 + 快照 RECTPAINT pipeline |
| 图像加工 | `apply_reverse`(RGB 取反)/`apply_horizontal`(水平镜像) |
| 交错序 | `CopyOp::seq` / `ImageOp::seq`;`frame()` 按命令序交错输出纹理 pass(DIV-GFX-029) |

### 2. 关闭/挂账(DIV-GFX)

| 条目 | 终态 | 证据 |
|---|---|---|
| 006 过渡配对/推进者 | **fixed** | EVID-T-01..09;配对缺失/TA<1 = 普通层;T 由脚本推进;mask=配对 G |
| 007 COPY/COPY2 | **fixed**(TexCopy/TexSsCopy 仍 open) | EVID-T-11/12/15 |
| 013 图像加工 | **partial**(新增 REVERSE/HORIZONTAL;SIZE/MONO/... 仍 open) | EVID-T-13/14 |
| 017 COPY 的 Z | **fixed(决策保持)** | EVID-J-10;调用点未定位前不改行为 |
| 029 copy/image 交错序 | **fixed** | EVID-T-15;`j2-micro-copy2` 三方向量一致 |
| 044 SPLTRBUF/分屏 | **withdrawn** | EVID-T-10(脚本图层名,不是命令) |
| 043 按钮后缀 | **blocked(跨 crate)** | EVID-T-20/21/22;需 `yu-rt FILEINFO[EXIST]→LET` |
| 045 TA 残差 | **open(低风险)** | TA=128 校准,残差 ≤8/255;TA≠128 在 T≈255 偏差 ~0.12 |
| 008/041 组 RT | **blocked(契约裁决)** | title 稳定帧 W0 A=256 不依赖;EVID-J-08 |

### 3. P0 → P3 数量变化(title)

```text
J2 前(tick 300):ops 19, drawn 15, skipped 4
  skip image SPLTRBUF101 / transition SPL101 / layer copy ×2
J2 后(tick 300):ops 27, drawn 17, skipped 10
  skip layer ×10 = asset.missing(cgsys/title/btn_* 基名;跨 crate 阻塞)
600 tick 累计:image executable 7238 → 7781;copy executable 0 → 56;
              transition 全部 Executable(SPL101/SPL401)
```

### 4. 验收命令与原始结果

```text
$ YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-soft --example first_frame
layers: 445 (visible 115), passes: 16, offscreen passes: 2
ops: 27 total, 17 drawn, 10 skipped
missing assets: 10   (全部 cgsys/title/btn_* 基名)
png: target/tmp/j2-title-soft-t300.png (1920813 bytes)

$ YURIS_FIRST_FRAME_TICK=300 cargo run -p yu-backend-wgpu --example first_frame
layers: 445 (visible 115), passes: 16, offscreen passes: 2
ops: 27 total, 17 drawn, 10 skipped
display check: OK (passes 16 ops 27 executable 27 skipped 0)
all_black=false deterministic=true
png: target/tmp/j2-title-wgpu-t300.png (1920813 bytes)

$ sha256sum j2-title-soft-t300.png j2-title-wgpu-t300.png
6b03238a93bd4fa90e699864ad15b7ba793daab082ae87814decbd6898dcbae5  (两文件相同)

$ cargo run -p yu-backend-soft --example title_probe -- 600
IMAGE_ACTION rectpaint 1200 / rectpaint2 29 / rectpaint3 6038 / reverse 514
IMAGE_STATUS executable 7781 (无 unsupported)
COPY_STATUS executable 56 (无 gfx.copy.unsupported)
ASSET: 10 个 cgsys/title/btn_* asset.missing

$ cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
yu-backend-soft: 16 + 7 passed; yu-backend-wgpu: 6 + 9 passed;
yu-gfx: 10 + 5 + 10 + 1 + 15 + 4 + ... passed;
eris_first_screen_display_list / ..._with_synchronous_info_queries FAILED
  (既有跨 crate boot 漂移 visible 8 vs 34,见成果 J1 §6;本任务未改 golden)

$ cargo clippy -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
三个 crate 0 新增告警(yu-rt 既有 5 条)
```

### 5. 区域对拍(阈值先声明:平坦区 max ≤4、diff>4=0)

```text
官方 target/tmp/fidelity-ref/frame_0006.png
内核 target/tmp/j2-title-soft-t300.png
平坦区域(官方 3×3 每通道极差 ≤2):
  按钮列以外(上/左) 106,127 px  max 3  diff>4 0  PASS
  按钮列右侧          62,102 px  max 3  diff>4 0  PASS
  LOGO 区域           14,552 px  max 3  diff>4 0  PASS
按钮列:缺失 10 个按钮(blocked:yu-rt FILEINFO)
1px 边缘:归 DIV-GFX-042(不在 J2 范围)
```

### 6. 证据要点

- **EVID-T-03/06**:asm `0x415558-0x415577` 双纹理门控;配对缺失/
  `TA<1`→`0x416f8d` 单纹理;`TID=""` = 不查配对;`j2-micro-trans1` TR7。
- **EVID-T-04**:`T` 由脚本 `@TP` 推进;`+0x1e8` 只在调试组 RT 分支。
- **EVID-T-05**:mask = 配对纹理 **G 通道**(`FF0000/0000FF→0`,
  `00FF00→255`);覆盖度 `clamp((30+70*fade)*(mask-(1-2*fade)),0,1)`,
  `T>=256` 为满;TA=128 测量残差 ≤8/255。
- **EVID-T-08/09**:本语料 `_DXMODE=0` 走固定管线;HLSL `ps_2_0`
  (`oC0.rgb=tex1; oC0.a=tex1.a*sat((FadeVal-0.5*tex2.r)*FadeVal2)*AlphaVal`,
  `FadeVal2=TA`)从 exe 提取并 `fxc /dumpbin` 反汇编,备档不实现。
- **EVID-T-10**:`SPLTRBUF/SPL/SPLTR` 是脚本图层名 → DIV-GFX-044 withdrawn。
- **EVID-T-11/12/15**:COPY 区域/时序/生存期/命令交错序;`j2-micro-copy`
  + `j2-micro-copy2`;软/硬后端与官方采样点逐值一致。
- **EVID-T-13**:REVERSE=RGB 取反、HORIZONTAL=水平镜像(`j2-micro-rev`)。
- **EVID-T-20/21/22**:按钮后缀由脚本 `FILEINFO[EXIST=1]` 探测;内核缺的是
  `yu-rt` 的 LET 写回,交接规格见台账 §6。

### 7. 遗留 / 阻塞

1. **按钮 10 个 asset.missing = 跨 crate**:需 `yu-rt` 实现
   `FILEINFO[EXIST=1]→LET`;J2 任务书禁止改 `yu-rt`。交接台账 §6。
2. **DIV-GFX-045**:转场覆盖度的 TA 依赖未完全收敛(TA=128 校准);
   需固定管线 mask 纹理生成路径的 REA 取证。
3. **DIV-GFX-042**:1px 边缘与官方仍不同(既有)。
4. **DIV-GFX-008/041**:组 RT/主组直绘契约未裁决(title 不依赖)。
5. **快照捕获时机**:后端在“首次渲染该 `CopyId` 的帧”捕获;若只渲染
   复制之后的某一帧(不逐帧播放),快照内容是渲染帧的源状态而非复制命令
   所在帧的状态。title 稳定帧无 COPY,不影响验收;逐帧播放时与官方一致。
6. `yu-gfx` 的两条首屏 golden 仍因既有 boot 漂移失败(`visible 8 vs 34`),
   未更新 golden(沿用成果 J1 的处理)。

## 成果 J3 (2026-10-21) · Title 跑通:FILEINFO、boot 锚点与 WebGPU 一等公民

> 任务书 `docs/tasks/task-j3-title-webgpu.md`。范围:`crates/yu-gfx`、
> `crates/yu-backend-soft`、`crates/yu-backend-wgpu`,以及对 `yu-rt` 的
> **唯一加法** `FILEINFO[EXIST]→LET`。未改 `yu-cli`/`yu-host-wasm`/`www`;
> 零 git 写操作。台账 = `docs/title-motion-ledger.md` §8/§9(EVID-J3-01)。

### 1. 交付物

| 项 | 文件/产物 |
|---|---|
| FILEINFO | `crates/yu-rt/src/vm/handlers.rs::op_fileinfo`;fixture 两条 |
| 逐帧基线 | `yu-backend-soft/examples/frame_walk.rs`、`yu-backend-wgpu/examples/wgpu_frame_walk.rs`(同列 TSV,含 RGBA sha256/耗时/上传计数) |
| 示例重命名 | wgpu 端改为 `wgpu_first_frame`/`wgpu_frame_walk`,消除 Cargo 输出文件名冲突 |
| WebGPU 修复 | 转场 shader 公式+量化、命令段/按需组 pass、覆盖纹理跨帧缓存、屏幕 alpha 归一 |
| soft 修复 | `compose_group` 报告回滚、D3D9 浮点混合模型 |
| golden | `crates/yu-gfx/tests/golden/eris_first_screen_display.json` 按复核后的 census 重生成 |
| 文档 | `docs/title-motion-ledger.md` §8/§9、`docs/gfx-unimplemented-todo.md`、`crates/yu-gfx/docs/divergences/README.md`、`docs/render-parity.md`、两份后端 README |

### 2. P1 · 按钮列(FILEINFO)

证据四件套:手册 `file/fileinfo.html`(EXIST/LET、0.276 起可查包内文件)、
REA opcode 22 `0x44286c`(EXIST 分支、四种存在性策略、`0x443222/0x4432dc`
写 1/0)、真实脚本 `es_Button.yst:1759-1765`、官方 title 截图。

实现只覆盖 `FILE+EXIST+LET`(其它 FILEINFO 查询仍挂号);路径经
`yu_vfs::LookupKey` 归一,存在性查封包/散盘与工程根。

结果(tick 300):`asset.missing 0`,`ops 27 / drawn 27 / skipped 0`;
10 个按钮纹理为 `cgsys/title/btn_*_off|_on|_over|_na`。
对官方 flat-region 阈值(先声明 `max≤4`、`diff>4=0`):
按钮列 27,763 平坦像素 max 2 / diff>4 0;其余 191,215 像素 max 3 / 0。

### 3. P2 · boot/title 锚点

- 运行配置取 `@_DEBUGMODE=0`(工程 `projectconfig.txt` DEBUGMODE=0);
  窗口标题【デバッグモード】只作旁证(可能是构建标签),以 title 像素与
  debug=true 的行为对照为准。
- A/B:`debug_mode=false` 在 tick 224 进入 title;`debug_mode=true` 在
  1200 tick 内不进 title(debug UI/黑帧,script 58 `es_Title` 未执行)。
- 旧 golden(visible 34/ops 44)来自 `771cf79`,早于 `1365918`;当前
  census 为 272 layers / 8 visible / 15 passes / 18 ops / W0 4 ops /
  264 invisible。先审 census 与 title 像素对照,再重生成 golden:
  `cargo test -p yu-gfx --test eris_first_screen`(带样本)两条绿。
- **挂账**:`debug_mode=true` 的 title 缺失属 rt debug 分支,超出 J3 允许的
  `FILEINFO` 加法;需后续用官方 L1 trace 继续二分(台账 §9.2)。

### 4. P3 · 三端一致性与性能

原始结果:

```text
$ cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
  全部 test result: ok(20 个测试二进制;含 eris_first_screen 两条,12.8s)
$ cargo test -p yu-rt
  47 + 4 + 10 + 4 + 49 + 3 passed; 0 failed

$ ./target/release/examples/frame_walk 600 / wgpu_frame_walk 600
  600 帧: hash 不一致 0;ops/drawn/skipped/transition/copy/image 不一致 0
  wgpu 上传帧: tick 0(4 B)、1(26,656 B)、62(2 个)、224(11 个);225..599 = 0

$ yu-cli run <project-eris> --backend soft|wgpu --until-frame 303
  soft: 27/27/0 rgba sha256 6280eac4…b198
  wgpu: 27/27/0 同 hash;reconcile OK(逐项一致)
  png 文件 sha256 两端相同 6271f89d…

浏览器(Chrome 155 headless,WebGPU 呈现):
  gfx frame 303:canvas == 服务端 RGBA == native wgpu,`diff 0`,hash 6280eac4…b198
  锚点 frame 16/53/227/233:canvas == WS == native walk,全部 diff 0
  Restart:frame 归零后重回 title,hash 仍 6280eac4…b198,/api/report missing=0
  hover:未验证。K2 宿主只在 pointerdown/按住拖动时发送 cursor
    (`www/app.js` 的 pointermove 需 `buttons&1`);纯悬停不改状态,
    属 K2 输入语义,写交接不抢改。按钮四种后缀的探测已由 FILEINFO 验证。
```

性能(release,RTX 5070 / DX12,600 帧):

| 指标 | soft | wgpu |
|---|---:|---:|
| p50 | 15.43 ms | 1.21 ms |
| p95 | 23.19 ms | **1.83 ms** |
| 稳态 225..599 p95 | 27.32 ms | **1.79 ms** |

阈值(声明):wgpu p95 ≤ 16.67 ms ⇒ PASS;稳态 0 逐帧重传 ⇒ PASS。

### 5. 途中发现并修复(均有复现)

1. **J2 的 soft==wgpu 是假象**:两个 crate 的 example 同名,
   `target/debug/examples/first_frame.exe` 互相覆盖;J2 两次都在跑 soft。
   已重命名 wgpu 示例并写入 README/台账。
2. **wgpu 转场 shader 旧公式**:`mask+4f-1` vs gfx 的
   `(30+70f)(mask-(1-2f))`,48 帧差;改为同公式+u8 量化链。
3. **wgpu 命令段顺序**:COPY 原在组离屏 pass 之后 → 快照层被跳过,
   `serve --backend wgpu` 首帧对账 exit 2;改为按 gfx pass 序 + 组源按需
   pass(不计报告)。
4. **soft 组源 COPY 重复计数**;报告回滚。
5. **覆盖纹理逐帧重传**(127 帧 × 4 B);跨帧 FNV 缓存。
6. **淡入淡出 ±1 LSB**:soft 分项整数取整 vs GPU 先求和;soft 改浮点
   模型后 0 差异。
7. **屏幕 alpha**:组 RT alpha 留在读回里,浏览器 opaque canvas 归一 255;
   `render_frame` 两端都归一,三端哈希一致。

### 6. 仍然 blocked / 未做

- `debug_mode=true` 的 title 缺失(P2 挂账;需 rt debug 分支专门取证)。
- DIV-GFX-045(TA≠128 覆盖度残差):title 用 T=256/TA=128,不触发;
  模型/量化链已在 soft+wgpu 统一,残差仍需 mask 生成路径 REA 取证。
- DIV-GFX-042(1px 半像素):按钮/文字边缘仍与官方有 ≤ 通道差,平坦区
  已 PASS。
- DIV-GFX-008/041(主组直绘/组 RT alpha):契约未裁决;title 平坦区
  不依赖,按钮阴影边缘仍归此/042。
- K2 宿主未暴露 `debug_mode` 开关;当前默认 false 与 J3 的 title 锚点
  一致,无需改动;如需 A/B,由 K2 会话加宿主参数(交接,不抢文件)。

### 成果 J3 勘误(2026-10-21 追加)· boot logo 动画与启动时间锚点

用户实测指出“启动没有 logo 动画、卡顿严重”。复核成立:上面的 J3
验收只覆盖 **title 稳定态与已有层的转场**,没有覆盖 boot 的
Firstia logo 动画。追加记录:

1. **官方 boot 基准**:`capture_screen.py --warmup 0 --frames 20 --interval 0.1`
   显示 0.2–0.7s 菱形 wipe 揭开 `cg/bg/logo`(Firstia),保持到 ~1.3s,
   1.3–2.0s 反向 wipe 到黑,~2.0s 后 title 淡入。
2. **根因(EVID-J3-02)**:`SCENARIO_START` → `MAC.BG(logo)` 创建
   `SPLBUF401=cg/bg/logo`(隐藏)、`SPLTRBUF401=cg/tr/ysr023`(隐藏)、
   `SPL401/SPLTR401`(1×1 黑占位)。旧 gfx 用占位层自身做 from/mask,
   整段黑屏。`es_Sprite.yst` 的 `$LID/$LIDBUF/$LIDTR/$LIDTRBUF` 是
   sprite 系统的缓冲命名约定。
3. **已修**:`op_for_layer` 在 `SPLnnn` 自身为空/1×1 黑时改用
   `SPLBUFnnn`;`TID=SPLTRnnn` 占位时改用 `SPLTRBUFnnn`。新增单测
   `transition::sprite_transition_uses_hidden_buffer_layers`。
   结果:tick 13–59 显示 logo wipe;tick 61 与官方 frame_0008 的
   4×4 采样平均通道差 0.13;title 稳定帧仍 `6280eac4…b198`;
   soft/wgpu 250 帧 0 差异;全量测试仍绿。
4. **仍未解决(新挂账 DIV-GFX-046)**:tick 63 起 `CGEND` 删除
   SPLBUF401/SPL401/SPLTR401,屏幕黑到 tick 224;官方在 wipe 后
   保持 1.2s + 800ms FOUT。该 BG 提升/保持不在现有 CG/CGACT 命令里,
   需要 sprite renderer 的 REA 取证与契约裁决。title 首次出现 3.7s
   vs 官方 ~2.2–2.5s,差距主要来自此。
5. **卡顿归因**:无 pause 时浏览器 + 服务端实测 **60.2 fps**、
   `dropped=0`、tick 4.57ms / render 1.48ms;启动首帧 ~2.4s 是既有
   rt 热路径。用户感到的“卡顿”来自首帧冻结 + tick 63–223 黑屏段,
   不是 WebGPU 呈现链路。

结论:**boot logo 的可见 wipe 已恢复;hold/fade 与 title 时间锚点仍未
1:1**。在 DIV-GFX-046 关闭前,不得把这一段黑屏当作验收通过。

## 成果 J4 (2026-10-21) · 引擎着色器 1:1:4 个真实 blob、dx_mode 双路径与固定管线补全

> 任务书 `docs/tasks/task-j4-shader-parity.md`。范围:`crates/yu-gfx`、
> `crates/yu-backend-soft`、`crates/yu-backend-wgpu`;`yu-rt` 未改。
> 零 git 写操作。台账 = `docs/shader-parity.md`(唯一 shader 映射);
> 证据产物在 `target/tmp/j4-shaders/` 与 `target/tmp/j4-walk/`(不入库)。

### 1. 交付物

| 项 | 文件/产物 |
|---|---|
| shader 台账 | `docs/shader-parity.md`(全 exe 扫描、4 blob、FXO/pass 映射、调用链、阈值、15 项结论) |
| 扫描器/反汇编 | `target/tmp/j4-shaders/scan_shaders.py`、`psA..psD.{cso,asm}`、`effect.fxo` |
| 契约 | `CompositorFrame.dx_mode`(默认 0)、`GfxConfig::with_dx_mode`、`GfxState::set_dx_mode` |
| 独立 WGSL | `crates/yu-backend-wgpu/src/shaders/{mod.rs,quad.wgsl,transition_fixed.wgsl,transition_psc.wgsl,transition_psd.wgsl,paint.wgsl}` |
| 管线 | `PipelineCache` 新增 psC/psD 变体、`SRC_MODE` 常量、MAX/MIN blend、paint bind group 缓存 |
| soft | MODE 3/5/6/7/9 stage 归约 + blend;psC/psD 转场;MODE 2 RGB 修正 |
| 测试 | soft blend 向量 4 条;gfx 组 TID 1 条;wgpu dx_mode psC 端到端 1 条;filter gate 改为 ROOT 优先 |
| 文档 | `render-parity` §5.2/5.3/5.4/12.4/12.6、`title-motion-ledger` §9.3 勘误、DIV-GFX-001/019/047/048/049/050、TODO 总账 |

### 2. P0 · 全 exe 扫描与真实 blob(推翻“8 个/13 个候选”)

对官方 0.495 exe(sha256 `4f5eefa3…`)按 D3D9 version token 全量扫描,
按注释/指令长度走到 `END=0x0000FFFF` 过滤:

```text
version-token occurrences: 76
VALID stream + exact size prefix: 4
  ps_2_0  off=0x18c9a8 size=1432 end=0x18cf40
  ps_2_0  off=0x18cf58 size= 604 end=0x18d1b4
  ps_2_0  off=0x18d1cc size= 492 end=0x18d3b8
  ps_2_0  off=0x18d3d0 size= 792 end=0x18d6e8
```

- 其余 72 处是代码 `cmp reg, 0xFFFFxxxx` 立即数、四个 blob 的 CTAB
  `Version` 字段、D3DX9 版本字符串/ps_1_x opcode 表、以及 `0x1cb...`
  元数据表;均无法走完 token 流。
- **没有 `ps_1_1/ps_1_4/ps_3_0` 字节码,也没有 `vs_1_1` 顶点着色器**。
  审查报告 §1.4 的 “13 个真实候选” 全部是上述误报(逐条见台账 §1)。
- J2 的 `ps1/ps3/ps5/ps7` 是 CTAB 内嵌字段;`ps6` 只截了 656 B
  (真实 792 B),所以反汇编为空。按正确长度重提后 4/4 完整反汇编。

四个 blob 在同一个 D3DX9 编译 effect 容器(VA `0x58d420`,`0x1540` B,
magic `0xFEFF0901`)里;按 Wine `d3dx9_36/effect.c` 字段顺序解析出:

```text
technique "YS_ShaderTest1" passes=4
pass0 -> object 7  (792 B)  psD 交叉淡化: mix(tex1,tex2,FadeVal)*AlphaVal
pass1 -> object 8  (492 B)  psC mask 转场: a=tex1.a*sat((FadeVal-0.5*tex2.r)*FadeVal2)*AlphaVal
pass2 -> object 9  (604 B)  psB 4-tap 锐利缩放
pass3 -> object 10 (1432 B) psA 16-tap 双三次缩放
```

调用链(REA):`FUN_00462d28` → `memcpy(0x58d420,0x1540)` →
`D3DXCreateEffect`(`0x4cd434`,检查 magic)→ `SetTechnique`;
`FUN_00414ba4` 按层结构 `BeginPass(0/1)`(0=配对/交叉淡化,1=`TA>=1`
mask),`FUN_004195ec` 按 `DAT_00801334` 选 `BeginPass(2/3)` 做显示缩放。
`CreatePixelShader` 由静态链接的 D3DX9 在 `D3DXCreateEffect` 内调用,
exe 只导入 `Direct3DCreate9`,所以字符串里没有它。

### 3. 15 项必查结论

| # | 结论 | 证据 |
|---|---|---|
| 1 | 全 exe 只有 4 个真实 ps_2_0;无 ps_1_x/ps_3_0/vs_1_1 | 台账 §1;扫描器 |
| 2 | ps1/3/5/7 是 CTAB、ps6 截断;按 792 B 重提后全部可反汇编 | `fxc /dumpbin` 4/4 |
| 3 | 创建=`D3DXCreateEffect`+FXO;`SetPixelShader` 只在 `0x414dfa/0x4198cd` 复位 0 | 台账 §3;REA |
| 4 | 不存在 ps_1_4↔MONO/BLUR 映射;`CGACT` 是 CPU 像素路径,不引用 effect | `0x801e7c` 的 34 个 xref 无 `0x42f809` |
| 5 | 无顶点着色器;顶点 CPU 生成 + 固定 FVF(`XYZRHW|DIFFUSE|TEX1/4/8`) | `0x41a308`、`0x462e40` |
| 6 | MODE 3/5/6/7/9 状态可达:soft 与 wgpu 全部执行(颜色因子映射) | §4/§5;DIV-GFX-047 fixed |
| 7 | 顶点 DIFFUSE=`A` 灰度(asm `0x414d09..0x414d21`),不补顶点 RGB | DIV-GFX-019 fixed |
| 8 | 引擎硬编码 MAG/MIN=LINEAR、MIP=POINT、aniso=0,无 mip 链;后端 LOD0/aniso=1 等价 | `FUN_00460924` 尾部 |
| 9 | paint bind group 按 `(alpha_only,color)` 跨帧缓存;transition 仍按 draw(fade/目标逐帧变) | `PipelineCache::paint_groups` |
| 10 | J3 基线复跑:soft/wgpu 600 帧 **0 差异**;浏览器 227/233/303 与 native 同哈希;16/53 是服务端连接时机导致的瞬态帧(canvas==WS) | §5 |
| 11 | 样本 gate 统一:ROOT(data.ypf/data)优先、YSBIN 备选;只设 ROOT 时 `REQUIRE_SAMPLES=1` 通过 | `tests/filter.rs` |
| 12 | `render-parity` §5.3 重写;ledger §9.3 “仍是旧公式”改为“修复前”;无矛盾 | 文档 diff |
| 13 | `image_ext_order` 为空:保留宿主/默认顺序 + 每帧 `gfx.config.probe_order_fallback` 诊断(A5 显式记录) | `state.rs::frame()`;DIV-GFX-049 |
| 14 | `_DXMODE` 是**脚本变量**,不是引擎渲染开关;内核以加法契约 `dx_mode` 映射,默认 0;宿主交接 | 台账 §6;`CompositorFrame.dx_mode` |
| 15 | 组 TID:gfx 输出 `Op::Transition{from:Offscreen(组)}`,配对可为图像/组;官方微场景待编译器恢复 | `group_transition_op`;DIV-GFX-048 |

### 4. P1 · dx_mode 双路径(**内核 A/B 契约,非引擎开关**)

> 2026-10-22 审查勘误:本节“双路径”指**内核实现的两条转场路径**
> (固定管线 G-mask 拟合 / psC/psD 移植),不是引擎的两条路径。官方引擎
> effect 始终创建;psC 触发 = 有配对且 `TA>=1`,psD = 有配对且 `TA<1`;
> `@_DXMODE` 只决定脚本走 `CGACT[TRANSITION]`/`TID` 还是 `DXBUF`
> (`docs/shader-parity.md` §3、`docs/audit-2026-10-22.md` §B1)。
> 第 14 项的结论(脚本变量)仍正确。

```text
$ YURIS_DX_MODE=1 cargo run --release -p yu-backend-soft --example frame_walk -- 600
$ YURIS_DX_MODE=1 cargo run --release -p yu-backend-wgpu --example wgpu_frame_walk -- 600
soft vs wgpu: hash 差异 1 帧(tick 35),ops/drawn/skipped/transition 0 差异
  tick 35 逐像素:max 3/255,0 像素 >4(量化残差;声明阈值 ≤4)
稳态帧:soft=wgpu=6280eac4…b198;dx0 与 dx1 稳态同哈希
dx0 vs dx1:47 帧不同(过渡区间),符合两条引擎路径
```

wgpu 端新增 `shaders/transition_psc.wgsl` / `transition_psd.wgsl` 与
`DrawKind::TransitionPsc/Psd`;soft 端 `TransitionMask` 增加
`hlsl_mask/crossfade/fade2`。psC 的 mask 取 **R**(固定管线取 G),
`FadeVal2=TA`、`AlphaVal=A/256`。

### 5. 固定管线补全(MODE 2/3/5/6/7/9、DIFFUSE、filter、bind group)

- **MODE 2 修正**:stage0 `MOD(D,T)` 使 RGB 先乘 `A`;旧实现 `dst−T·a`
  改为 `dst−T·a²`(soft 单测向量 `[50,50,50,191]`)。
- **MODE 3/5/6/7/9**:`BCA`(BLENDCURRENTALPHA,值 16)+ 白/黑 aux
  把源色归约为 `T·A` / `T·A+(1−C.a)`,再由 D3D9 因子混合:
  - soft:全部执行(乘/屏/max/min/除外);
  - wgpu:同样全部执行:`BlendFactor::Src/OneMinusDst/OneMinusSrc` 就是
    D3D9 的 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR`;6/7 用 MAX/MIN。
    单元/端到端测试 `fixed_pipeline_modes_3_5_6_7_9_match_soft` 逐像素一致。
- **DIFFUSE**:顶点色=`A` 灰度(shader 模式 `0xFF000000`),不新增顶点属性。
- **filter**:引擎硬编码 `1/1/0`;后端固定 LOD0/aniso=1 与引擎等价,
  `render-parity` §12.4 记录(不再称“静默忽略”)。
- **bind group**:paint 缓存实测 title 首帧 `bind_groups=13(created 14)`;
  600 帧稳态上传 0;wgpu p95 2.01ms(J3 1.83ms,阈值 16.67ms)。

### 6. 复跑与验收

```text
$ cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
  全部测试二进制 139 passed / 0 failed(ROOT-only gate)

$ cargo clippy -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
  三个 crate 0 新增告警(yu-rt 既有 5 条)

$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-{gfx,backend-soft,backend-wgpu}/src
  0

$ cargo run --release -p yu-backend-soft  --example frame_walk -- 600
$ cargo run --release -p yu-backend-wgpu  --example wgpu_frame_walk -- 600
  hash 差异 0;ops/drawn/skipped/transition/copy/image 差异 0
  steady 6280eac4…b198;tick 249..599 恒定;稳态上传 0
  wgpu p50 1.36 / p95 2.01 / p99 2.60 ms

浏览器(Chrome headless + CDP,服务端 `yu-cli serve --backend wgpu --pause-at N`):
  anchor 303:canvas==WS==native 6280eac4…b198
  anchor 227/233:canvas==WS==native(walk 同帧)
  anchor 16/53:canvas==WS(0 差异);与 walk 同号帧不同 = 服务端连接时机导致
    的逻辑时间差(非渲染差异);J3 的“16/53 与 native 同帧”表述据此修正
```

### 7. 遗留 / 阻塞

1. **MODE 4/8/10..18**:4 个 shader 都不实现 Photoshop 公式;没有对应
   blob/调用点,保持 `Unimplemented`(不猜公式)。
2. **WebGPU 3/5/9 已复核可实现**:初版误以为 WebGPU 无颜色因子;
   `BlendFactor::Src/OneMinusDst/OneMinusSrc` 就是 D3D9 的
   `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR`,已修正并测试(DIV-GFX-047 fixed)。
3. **官方微场景编译器阻塞**:`yscom_harvest` 启动的 YSCom 在
   `state0 != 1` 不 attach(120s 超时;LE/非 LE 均复现,
   `target/tmp/j4-micro-modes2/harvest.log`)。MODE 3/5/6/7/9 与组 TID 的
   官方像素对拍待工具恢复;当前证据 = D3D9 状态表 + soft 单元向量 +
   两端端到端。
4. **DIV-GFX-045**:TA≠128 的固定管线覆盖度残差仍 open(title 用 TA=128,
   不触发)。
5. **组 TID**:标 Likely;等官方微场景确认后转正。
6. **宿主交接**:CLI/wasm 需把 `BootConfig.dx_mode` 传给
   `GfxConfig.dx_mode`(一行);当前示例用 `YURIS_DX_MODE` 做 A/B。
7. **psA/psB**:仅反汇编/触发条件记录;当前 1:1 视口不触发显示缩放,
   未接入 `CompositorFrame`。

## 成果 J5 (2026-10-21) · wasm 前端 UX:log 滚动/拖动与 restart 加载遮罩

> 范围:仅 `www/`(`index.html`、`styles.css`、`app.js`);Rust/wasm 零改动。
> 动机:用户反馈 log 每条新日志都强制刷到底部、面板不可移动;Restart 后
> 画面停留在旧帧无任何加载反馈。

### 1. 交付物

| 项 | 实现 |
|---|---|
| log 贴底才跟随 | `app.js` `log()`:追加前计算"距底 < 24px"才 `scrollTop=scrollHeight`;上翻阅读不再被打断 |
| log 面板可拖动 | header 按住拖动(pointer capture)移位,右下角 CSS `resize` 缩放;位置/尺寸存 `localStorage(yu-log-layout)`,重开/刷新恢复并夹回视口 |
| restart/启动 loading | 新增 `#loading` 遮罩(黑底盖住 canvas + 转圈动画):点 Restart 立即黑屏显示 "restarting…",启动期显示 "loading…";首帧到达(`handleBinary`)自动撤掉;窗口缩放重定位 |

### 2. 验证(Chrome + IAB,`yu-cli dev` @4173,soft 后端,ERIS 语料)

```
$ node --check www/app.js                      → 语法 OK
贴底跟随:滚到顶后等 1.6s,新增 8 行(56→64),scrollTop 稳定 0(不拉底);
         滚回底部后等 1.2s,scrollTop+clientHeight==scrollHeight(仍跟随)✓
拖动:cua.drag header (700,481)→(250,150),面板 rect (653,464)→(204,133),
     localStorage 写入 {"left":204,"top":133,"w":614,"h":280}✓
刷新:面板恢复 (204,133,614×280) ✓
restart:点击后帧号 1125→复位(重启后 205→233 连续推进),遮罩 hidden=false
        (text=restarting…)→ 首帧到达后 hidden=true 自动撤掉 ✓
```

注:黑屏+转圈瞬间的截图取证两次因截图表面 3s 超时未捕获;流程以上述
DOM 状态断言为准(遮罩 DOM/动画为纯 CSS 声明式)。

### 3. 途中发现(未修,已报告用户)

**ws 每 ~11s 断开根因 = HTTP 读超时泄漏进 WebSocket**:
`crates/yu-cli/src/server.rs` `read_request()` 对请求 socket
`set_read_timeout(10s)` 后从未清除,升级 ws 后同一内核 socket 带
SO_RVTIMEO;页面空闲(无鼠标/键盘 ws 消息)时读线程恰 10s 超时 → 注销
客户端 → 写循环 channel Disconnected → 连接关闭。实测三次连接存活
10.012/10.015/10.015s;活跃输入(拖动)期间连接存活 >70s 佐证"有消息就续命"。
次因:dev 监督者以 mtime+size 指纹监视 `crates/**/*.rs`,本机并发改动
(含 0 重编译的纯 touch)即杀引擎重建 → 帧号复位级断开(真实编辑的
HMR 预期行为,纯 touch 为误报)。

## 成果 J6 (2026-10-21) · 修复 ws 读超时泄漏 + yu-host-wasm README(最小示例)

### 1. 修复:ws 每 ~11s 断开(承接 J5 §3 的发现)

`crates/yu-cli/src/ws.rs` `accept()`:升级 ws 前清除 `read_request()` 为解析
HTTP 头设的 10s 读超时(SO_RCVTIMEO 留在内核 socket 上,try_clone 共享)。
修复前:页面空闲(无输入消息)时读线程恰 10s 超时 → 注销客户端 → 写循环
channel Disconnected → 连接关闭,实测连接存活 10.012/10.015/10.015s;
活跃输入期间存活 >70s(有消息就续命)佐证。

### 2. 验证

```
$ cargo build -p yu-cli --bin yu-engine --release          → 6.9s ✓
$ cargo test -p yu-cli                                      → 10 passed / 0 failed
  (含 ws::tests::handshake_and_echo_text,覆盖修改后的 accept())
$ cargo clippy -p yu-cli --all-targets                      → 0 新增(yu-rt 既有 5 条)
$ yu-engine serve project-eris --port 4174 --backend soft --no-open
浏览器(无 dev 监督者,直连):布防 WebSocket 构造器记录器 + log 行基线,
零交互空闲 54s:
  __wsEvents = [](零重连);ws closed/connecting/error 日志行 = 基线值 1(首连);
  frame 持续推进至 1148,ms≈39-49。
  对照修复前:同环境连接每 10.0s 死一次,54s 窗口内会断 ~5 次。
```

### 3. 文档:crates/yu-host-wasm/README.md(新增)

架构 B 呈现端职责与边界(不依赖 yu-rt/gfx/backend-*,语义留在 native)、
确切构建命令(`cargo build --target wasm32-unknown-unknown` +
`wasm-bindgen --target web`,与 `yu-cli bench` 第 6 步一致)、**最小示例**
(`www/mini.html` 形态:shim 契约 + init + `Presenter.present()` 呈现一帧
合成渐变,零服务依赖)、契约表、.wasm 是什么(魔数/沙箱/路线图 wasm 宿主
接入缝)。验证:

```
$ cargo check -p yu-host-wasm --target wasm32-unknown-unknown → ✓
$ cargo clippy -p yu-host-wasm --all-targets                  → 0 新告警
```

### 4. 追加(同日):wasm 定位说明(用户要求)

`crates/yu-host-wasm` 的 `lib.rs` 头注释与 `README.md` 开头均补"当前定位"
段:现在的 wasm 只是 dev 热重启调试期的呈现胶水(native 引擎推帧、页面画
帧),**后期的 wasm 包才是真正的编译产物**(rt+gfx+soft 整体编入 wasm32
的 wasm 宿主),届时 `present()` 即帧生产端替换的接入缝。
`cargo check -p yu-host-wasm --target wasm32-unknown-unknown` ✓。

## 2026-10-22 — 成果 J3 收口:`MATH3D` / `CGINFO[SX/SY]` / `CGACT[SIZE]` / 主组直绘(boot logo 与 title 淡入淡出修复)

用户实测指出两个仍未对的点:①boot logo 只有淡入没有淡出(旧线同 bug);
②title 出现时“突然显示→变暗→再亮”。两者都已定位到具体命令语义并修复。

### 1. 新增/修改的语义(全部带官方证据)

| 项 | 证据 | 实现 |
|---|---|---|
| `MATH3D` | REA `FUN_00448900`(`0x448900`,sha256 `4f5eefa3…`) | `yu-rt::vm::handlers::op_math3d` + `math3d_project`;透视比 `2^((camZ+DEFZ−layerZ)/100)` |
| `CGINFO[SX/SY]` | 引擎语义 = 已装载纹理尺寸 | `yu-rt::image::pixel_size`(PNG/BMP/GIF/JPEG 头)+ `GraphicsHost::note_asset_size` 回填 |
| `CGACT[SIZE/SIZE2]` | 手册 `cg/cgact.html` 例 3/4 + `es.CGACT_SIZE` | `yu-backend-soft::imageop::scale_region`,两端共用,跨帧缓存 |
| 过渡取样 | `es_Sprite.LOADTR` 对 `SPLTRBUF` 做 `CGACT[REVERSE=1]` | `TransitionDraw.from_layer/to_layer`,取样优先用加工后纹理 |
| `COPY ID=:/W0/` | 引擎读当前后缓冲 | soft/wgpu 均改读上一帧呈现结果 |
| wgpu 快照冻结 | EVID-T-12 | `plan_copy` 补 `snapshot.is_some() → Ok(())` 早退 |
| 主组直绘 | DIV-GFX-008/041(EVID-J-08) | `build_group_passes` 跳过主组;屏幕 pass 直接含子层 |

### 2. 复测(原始输出)

```text
# soft vs wgpu 600 帧逐帧对拍(target/tmp/soft600d.txt / wgpu600d.txt)
rows 600 600
mismatches 0 []

tick  hash(soft)==hash(wgpu)          ops/drawn/skipped/tr/copy/img
 20   7f60f61f9fa5a491…             33/29/4/2/0/0
 30   b25087b66b8e17c0…             43/39/4/2/0/0
 63   9cb4dbcb62a6a7c9…             23/19/4/0/0/0
150   771f37f95712f1bc…             39/35/4/2/0/0
224   a7aff0f8e3f44663…             96/96/0/2/2/0
249   a5a618d067bb20ea…             96/96/0/2/2/0
303   a5a618d067bb20ea…             89/89/0/2/0/0
599   a5a618d067bb20ea…             89/89/0/2/0/0
```

boot 时间线(soft,左/右半屏亮度;方向 = mask 暗区先动):

```text
tick  L      R
  13    0.4    0.0     wipe 开始(黑)
  30  197.1   63.3     左先显
  63  253.5  253.8     logo 保持
 150  189.1  249.5     反向 wipe:左先黑
 170    1.7   65.3
 190–223  0/0          黑场
 230   45.7   39.4     title 从黑平滑淡入
 249  193.3  167.0     title 稳定
```

### 3. 回归

```text
cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu   → 20 个测试二进制全 ok
cargo test -p yu-rt(含 corpus 金丝雀)                       → 全 ok
```

首帧命令数金丝雀由 1,290,326 → **1,292,091**(MATH3D 实装改变首帧分支);
corpus 测试内已注明原因。

### 4. 台账

- `docs/divergences/DIV-GFX-046-sprite-bg-hold.md`:**fixed**,补根因与四条语义;
- `docs/title-motion-ledger.md` §9.7:EVID-J3-03..07 + 复测表;
- `docs/gfx-unimplemented-todo.md`:008 fixed / 046 fixed / 041 partial;
- `crates/yu-gfx/docs/divergences/README.md`:同步。

### 6. 性能勘误(EVID-J3-08):release 帧时间回退已修

用户实测 release 帧时间从个位数涨到 1x ms;复查确认是 J3 引入的回归,
与浏览器宿主无关。三处原因与修法:

| 原因 | 量级 | 修法 |
|---|---|---|
| 同帧重复深拷贝覆盖纹理(脚本每帧重发 51 条 `CGACT[SIZE]`) | ≈98 MB memcpy/帧 | `Rc<Texture>` 共享 |
| 缓存命中仍重算 FNV 指纹(整幅 RGBA × 51) | ≈100 MB 哈希/帧 | `Override { tex, hash }`,指纹随纹理缓存 |
| 无区域 `SIZE` 做整幅 CPU 重采样 | ~14 ms/条 | 直接透传源纹理(绘制期按 `SX×SY` 拉伸) |
| `REVERSE` 每帧重算 | ~2 ms/帧 | 并入 `ImageCacheKey { source, op, region }` |

release 600 帧实测:

```text
native wgpu  p50 15.48 → 3.25 ms   p95 18.96 → 6.13 ms
native soft  p50 25.12 → 29.7 ms   (CPU 光栅化固有量级)
soft/wgpu 600 帧逐帧 hash/ops/drawn/skipped 仍 0 差异
title 稳定帧 a5a618d0…2483 不变
```

### 7. 复查:撤回固定实现(通用实现对齐引擎)

1. **删除 `SPL/SPLBUF/SPLTR/SPLTRBUF` 名字特判**(EVID-T-10:那是脚本命名,
   不是引擎语义)。转场只按 `YS_Texture1 = 本层纹理` / `YS_Texture2 = TID 层
   纹理` 取像素;缓冲层内容由脚本自己的 `CGACT[SIZE]` 写入。
   `yu-gfx/tests/transition.rs` 的 SPL 专项测试换成
   `transition_uses_layer_state_not_layer_names`(换名后行为相同)。
2. **`CGACT[SIZE/SIZE2]` 成功后更新目标层绘制尺寸为 `SX×SY`**
   (手册 `cg/cgact.html`),`CGINFO[SX/SY]` 因此完全由层状态回答,
   不再需要任何命名约定。
3. **`CGINFO` 资产尺寸回退按工程 ysc 参数名取槽号**(原硬编码
   `unwrap_or(8)` / `slot <= 1`)。
4. **wgpu `layer_image` 补“组 → 离屏 RT”分支**(与 soft `layer_surface` 同义)。

---

## 成果 AU1 (2026-10-22) · 全量改动审查:4 项确定错误、4 项文档失真、3 项工程回归

> 范围:工作区未提交改动(`git diff --stat` ≈ 6000 行 / 49 文件),覆盖
> `yu-gfx`、`yu-backend-soft`、`yu-backend-wgpu`、`yu-rt`、`yu-cli`、
> `yu-host-wasm`、`www/`、`cases/`。方法:逐文件读实现 + 对照 REA 反编译/
> 手册/真实脚本/官方截图;所有“实测”都在 `<project-eris>` 上复跑。
> 完整报告:`docs/audit-2026-10-22.md`(含复现命令与证据缺口)。
> 零 git 写操作。

### 1. 结论

| 等级 | 数量 | 条目 |
|---|---:|---|
| 确定错误 | 4 | 主组首帧 COPY 无源、`image_probe_order` A5 兜底、重启不清渲染器、REVERSE 组合分支未对拍 |
| 文档失真 | 4 | `dx_mode` 被写成引擎开关、README 能力落后、`render-parity §12.6` 过期、过期审查/台账段落 |
| 工程回归 | 3 | `cases/*.json` 全过期、`src/` 有 `eprintln!`、示例未注入 HostPaths |

### 2. 实测原始输出

```text
$ cargo test --tests -p yu-gfx -p yu-backend-soft
  全部测试二进制通过(0 failed)

$ cargo run -p yu-backend-soft --example first_frame
frame: 3  layers: 272 (visible 8)  passes: 14  offscreen passes: 1
ops: 17 total, 12 drawn, 5 skipped
  skip copy  x1  gfx.copy.source.offscreen_missing      :/W0/CGS100
  skip image x1  gfx.imageop.copy_source.unsupported    :/W0/CGS100
missing assets: 0

$ cargo run -p yu-cli --bin yu-cli -- case <project-eris> --case first-screen
case first-screen: frame=3 ticks=2 ops=17 skipped=2
  display_json_sha256: expected=1729689580a5… actual=48af8a1f6fde…
  ops_total: expected=18 actual=17
  skipped_ops_total: expected=5 actual=2
case first-screen: FAIL
```

> 注:`cargo test`(含 examples 链接)在本机会因
> `target/debug/examples/frame_walk.exe` 被残留进程占用而 `LNK1104`;
> 杀进程即可,不是代码错误。

### 3. 关键证据(逐条)

1. **A1 主组 COPY**:J3 让 `build_group_passes` 跳过主组
   (`state.rs` 的 `if id == main { continue; }`),但 soft `compose_group` /
   wgpu `plan_group_compose` 仍要求组有 Offscreen pass;首帧无 `last_screen`
   /`backbuffer` ⇒ `offscreen_missing`。归档 `DIV-GFX-051`。
2. **A3 重启**:`engine.rs::restart` 只重置 `Vm`/`GfxState`,渲染器的
   `last_screen`/`backbuffer`/`snapshots` 保留;`copy_seq` 归零后新 `CopyId`
   与旧快照同 id ⇒ 冻结早退命中旧像素。归档 `DIV-HOST-001`。
3. **A2 A5**:`state.rs::from_tables` 在 `image_ext_order` 为空时保留默认
   `[png,bmp,jpg,gif,psb]` + 一条 Info;A5 原文要求缺表 = 拒绝启动或
   `Unimplemented`。DIV-GFX-049 从 `fixed(J4)` 回退为 `open`。
4. **B1 `dx_mode`**:代码注释已写明“官方引擎没有该渲染开关”,但
   README/PROGRESS/shader-parity 仍按“引擎双路径”叙述。已改三处措辞 +
   `architecture.md` §5.1 新增范围说明。
5. **D3 MATH3D 复核**:逐段对照
   `<side-repo>/workbench/analysis/decomp/00448900_FUN_00448900.c`;
   `lVar21==0` 主分支、`lVar18!=lVar19`/`lVar20==0` 子分支、
   `pow(2,(cam+defz-z)/100)`、尾部 `(100-BX)*z/100` 均一致;
   常量 `0.0/100.0/2.0/0.5` 从 PE `.rdata`(`0x58ee00..0x58ee18`)直接读出。
   **结论:公式正确**,不是错误项。
6. **D2 状态表**:`d3d9types.h` 确认 `D3DTOP_BLENDCURRENTALPHA=16`、
   `D3DTA_DIFFUSE=0/TEXTURE=2/TFACTOR=3`;`render-parity §5.2` 的
   MAA=18 勘误正确。

### 4. 本次文档改动

| 文件 | 改动 |
|---|---|
| `docs/audit-2026-10-22.md` | **新增**:完整审查报告(§0..§7) |
| `docs/divergences/DIV-GFX-051-main-group-copy-first-frame.md` | **新增**:A1 档案(现象/根因/二分/两种修法/回归) |
| `docs/divergences/DIV-HOST-001-restart-renderer-state.md` | **新增**:A3 档案 |
| `docs/todo-inventory.md` | 新增 §2.0(P0)、更新 §5 顺序与 §6 文档失真清单 |
| `docs/gfx-unimplemented-todo.md` | DIV-GFX-049 回退为 open;新增 051/052;修复 047..050 脱离表格的格式错误;更新推进顺序 |
| `crates/yu-gfx/docs/divergences/README.md` | 同步 049/050/051/052 |
| `docs/architecture.md` | v1.3:crate 数与依赖图按实现重写;删除不存在的 `RenderBackend` trait,改为 `CompositorFrame` 数据契约;新增 §5.1 `dx_mode` 范围、§5.2 已知未对齐、§10 审查入口 |
| `README.md` | 能力声明与实现对齐(crate 表补 `yu-cli`/`yu-host-wasm`;wgpu 不再写“待 J4”;新增 P0 待办) |

### 5. 未完成/待裁决

- A1 的修法二选一(回退自合成 / 显式 `no_backbuffer`)需主人裁决;
- A3 的修法二选一(`reset()` / `CopyId` 代际);
- `cases/*.json` 的重生成依赖 A1 结论(先定契约再 update);
- `image_probe_order` 的 A5 处置需裁决(拒绝启动 / 全层 Unimplemented)。

## 成果 J4 续 (2026-10-22) · 补齐 effect Pass2/Pass3:psB 4-tap 锐利缩放 + psA 16-tap 双三次

> J4 遗留:4 个真实 ps_2_0 blob 中 pass0/1(交叉淡化/mask 转场)已有 WGSL,
> pass2/3(显示缩放滤镜)只有反汇编,`docs/shader-parity.md` §5 标
> “未接入”。本次补齐两个 shader、接线、像素对拍,并用 REA 复核了
> `FUN_004195ec` / `FUN_0041a308` / `FUN_0043cd6c`。

### 1. 新增证据(官方 exe `4f5eefa3…`)

| 地址 | 事实 |
|---|---|
| `0x41a308` | UV 生成:mode2 → t0=(u,v) t1=(u+1/W,v) t2=(u,v+1/H) t3=(u+1/W,v+1/H);mode3 → t0..t3=(u-1/W, v-1/H..v+2/H),t4..t7=(u, v-1/H..v+2/H) |
| `0x419f72` | `YS_TexDiffU = (2/W, 0)`(仅 psA) |
| `0x419dbe..0x419e13` | `YS_Texture3` + `YS_TexSize=(W,H)`;`SharpN` 全函数无 SetValue |
| FXO 参数初值 | `SharpN = 0x3FA00000 = 1.25` |
| `0x419832..0x41988d` | 缩放 pass 硬编码 `SetSamplerState(0, 5/6/7, 1)` = D3DTEXF_POINT |
| `0x43cd6c` | 全局帧采样把配置枚举 0/1/2 映射为 D3DTEXF 1/2/3;配置 1/1/0 ⇒ LINEAR/LINEAR/POINT |

### 2. 实现

- `shaders/scale_psb.wgsl`:2×2 tap、`w=sat((frac-0.5)*1.25+0.5)`、`lrp_sat`;
- `shaders/scale_psa.wgsl`:4×4 tap、三次权重 `-f³+2f²-f / f³-2f²+1 /
  -f³+f²+f / f³-f²`,累加序与 ps_2_0 一致,无最终 saturate;
- 两者用 `floor(u*W)` + `textureLoad` + clamp 复刻 D3D9 POINT 的 texel
  选择(避免 WebGPU nearest 在整 texel 边界的 tie-break 差异);
- `pipelines.rs`:独立 bind layout(纹理/point 采样器/uniform)、
  按 `(源纹理 id, 变体, uniform)` 缓存 bind group;
- `lib.rs`:`ScaleFilter{Sharp,Bicubic}` + 加法 API
  `render_frame_scaled(frame, output, filter)`;`render_frame` 行为不变;
  缩放前屏幕画面仍作为 `:/W0/` 后缓冲;
- 文档:`shader-parity.md` §3.3/§5/§6、`render-parity.md` §5.3、
  `gfx-unimplemented-todo.md` 050、crate README。

### 3. 验证(原始输出)

```text
$ cargo test -p yu-backend-wgpu --test wgpu display_scale -- --nocapture
running 3 tests
test display_scale_1to1_reproduces_composited_screen ... ok
test display_scale_psa_matches_reference_upscale_and_downscale ... ok
test display_scale_psb_matches_reference_upscale_and_downscale ... ok
test result: ok. 3 passed; 0 failed

$ cargo test -p yu-backend-wgpu
unittests: 9 passed
wgpu.rs:   14 passed
doc-tests: 1 passed
```

对拍阈值:GPU 与同式 CPU 参考(8×6 → 16×12 与 8×6 → 5×4)逐通道
`abs_diff ≤ 1`;1:1 与 `render_frame` 逐通道 `≤ 1`。

### 4. 仍 Unimplemented(不猜)

- `MODE 4/8/10..18` 的 Photoshop 公式:引擎只有 4 个 effect pass,
  没有任何除算/差分/Overlay 像素体;`fixed_state()` 仍返回 `None`,
  后端继续显式跳过并计数(DIV-GFX-002)。

## 成果 J4 续 2 (2026-10-22) · 补查 SDK/语料:E-mote 是独立插件运行时

> 回应“不能只看 exe”:查了 SDK 全树、手冊、游戏 `yscfg.dat`/语料与
> E-mote DLL。结论:主引擎 shader 集仍是 4 个(psA/psB 已实现);
> E-mote 不在 exe 内,是可选插件,自带 58 个 shader。

### 1. 主引擎没有外部/自定义 shader 入口

- `yu-ris.exe` 无 `D3DXCreateEffectFromFile`/`CompileShader`/
  `CreatePixelShader`/`.fxo`/`.fx`/`.hlsl`/`.cso` 字符串;
  所有 shader 相关串都是静态链接 D3DX9 的错误文本 + 引擎
  `HLSL BeginPass0..3/SetTexture/SetValue` 诊断。
- 语料 `project-eris/save/data` 与 `unpack/` 无 `.fx/.fxo/.cso/.hlsl`;
  扩展名只有 png/jpg/ybn/yst/txt/ogg/sd/ymv/psb/mpg。
- 手冊(CP932 搜索)`YU-RIS` 部分无“シェーダ/フィルタ”页;
  `エンジン設定.exe` 只有“拡大(外接)/(内接)”,没有 shader 画质选项。
- 脚本命令表无 shader/effect 命令;`EMOTE` 是唯一的图形扩展入口。

### 2. 显示缩放滤镜真的会被语料使用

`project-eris/yscfg.dat`(128 B i32 数组)index 8 = **1**;
`FUN_00460924` 映射 `1 → DAT_00801334=2 → effect Pass2(psB)`。
所以窗口缩放时本语料走 4-tap 锐利缩放,`render_frame_scaled(Sharp)` 对应。

### 3. E-mote(新发现,独立子系统)

- `FUN_004661ec`:`LoadLibraryA("emotedriver.dll")` 探测 →
  `LoadLibraryA("YSEmote.dll")` → `GetProcAddress` ordinal `0x1..0x3`、
  `0x65..0x100`;`DAT_00801750=1`。DLL 缺失时引擎照常启动。
- `YSEmote.dll`:256 个无名导出,导入 `emotedriver.dll` 的
  `EmoteCreate`/`EmoteFilterTexture`;`emotedriver.dll` 导入 `d3dx9_43`.
- `emotedriver.dll` 自带 **58 个 unique shader**:27 ps_2_0 + 27 ps_3_0
  (13 plain / 13 alpha-mask / 1 pixelate)+ 3 vs_2_0 + 1 vs_3_0。
- 数据:`cg/stand/emote_test.psb`(`PSB\0` v2,8.8 MB);
  手冊 `E-moteERIS命令マニュアル.txt` 定义
  `\SP.ST/\SP.EM.VAR/PLAY/DPLAY/STOP/DSTOP/VO`.
- 语料 120 帧 census:`EMOTE` 8 次(`BUST/HAIR/PARTS=128`, `SMOOTH=1`)、
  `EMOTEINFO` 0 次;首 120 帧未加载 `emote_test.psb`。
  当前内核只发 `Domain::Emote`,不渲染角色。

### 4. 交付

- `docs/emote-parity.md`(证据/架构/58 shader 清单/现状/里程碑);
- `docs/tasks/task-emote-runtime.md`(M0 契约 → M1 PSB → M2 动画 →
  M3 渲染 → M4 对拍);
- `docs/shader-parity.md` 范围声明:引擎 4 个 ≠ E-mote 58 个;
- `docs/todo-inventory.md`:EMOTE 渲染语义为独立子系统例外;
- `docs/emote-parity.md` §5:wasm 与开源生态对照
  (`emote-psb-rs` MIT 解析器、`eluna` Rust+wgpu 预览、`amadeus-emote-loader`
  显式 wasm32、FreeMote/WebGL 非商业且非 1:1);结论:官方 DLL 不能进 wasm,
  干净 Rust 实现可 wasm;当前 presenter 架构不需要内核 wasm。

---

## 成果 AU2 (2026-10-22) · 修复审查项:P0/P1 全部落地(含 2 个新发现)

> 承接 `docs/audit-2026-10-22.md`。范围:`yu-gfx`、`yu-backend-soft`、
> `yu-backend-wgpu`、`yu-rt`(仅 `TableSet: Clone`)、`yu-cli`;
> `cases/*.json` 重生成。零 git 写操作。
> 纪律:每一项都先定证据来源再写代码;缺子系统的补齐(不是跳过)。

### 1. 修复清单

| 项 | 状态 | 做法与证据 |
|---|---|---|
| A1 首帧主组 COPY 无源(DIV-GFX-051) | **fixed** | 首帧无 `last_screen`/`backbuffer` 时用 `frame.clear` 合成视口大小的后缓冲初值。依据:D3D9 设备创建后后缓冲即存在,内容为初始化清屏色(DIV-GFX-010);主组已直绘无离屏 RT(EVID-J3-07) |
| A3 重启不清渲染器(DIV-HOST-001) | **fixed** | soft/wgpu 各加 `reset()`(清 `last_screen`/`backbuffer`/`snapshots`/离屏目标/`image_cache`/`uploaded_overrides`);`Engine::restart` 调用。纹理缓存与管线保留(只依赖游戏数据) |
| A2 探测序内置兜底(DIV-GFX-049) | **fixed** | `GfxConfig::new` 不再填默认序;`from_tables` 在 yscfg 空且宿主未显式注入时返回 `gfx.config.probe_order_missing` |
| A4 REVERSE/HORIZONTAL 组合(DIV-GFX-052) | **fixed** | wgpu 补上与 soft 逐字对齐的“源已加工则现算 / 否则按 `(源身份,操作)` 缓存”分支 |
| C1 cases 过期 | **fixed** | `case --all --update`:18→85 ops、5→0 skipped;三条用例 PASS |
| C2 `src/` 的 `eprintln!` | **已清** | 复核时已无命中(并发会话已移除);`grep` 守卫仍为 0 |
| C3 HostPaths 未注入 | **fixed** | 7 个示例 + `yu-cli::boot()` 注入 4 个手册有页的路径;`$_PROJECT_PATH` 手册无页保持 `None` |
| **DIV-GFX-053(新)** 复制源同帧被 CGEND 删除 | **fixed** | `CopyOp.source_texture` 在命令时刻固定源基础纹理(EVID-T-15:官方命令处理器立即执行) |
| **DIV-GFX-054(新)** COPY 区域缺省坐标 | **fixed** | `CopyRegion::Partial` 坐标改 `Option`,`resolve()` 缺省 = 整幅。证据:ERIS `COPY2=1 X=0 X2=@sx-1` 无 `Y/Y2`,官方是整高切片 |

### 2. 新发现(DIV-GFX-053 的实证过程)

`gfx_dump` 的 copy log 有 69 条复制,但 `frame()` 只输出 1 条:

```text
copy log (69):
  copy 271 -> :/W0/CGS100
  copy 272 -> :/W0/BT.OFFES.GAMEMAIN.TIP.CLICK=0=1
  ...
  copy 339 -> :/W0/BT.OFFES.GAMEMAIN.BTN.MENU=0=1

frame 1: passes=14 ops=17     ← 68 条被丢弃
```

根因:`frame()` 的 `source_live` 检查用 `nodes.contains_key(源)`;ERIS 按钮流程是
`CG[ID="BT.OFF.TMP" FILE=…]` → 逐按钮 `COPY2` 切片 → **全部按钮建完后
`CGEND[BT.OFF.TMP]`**,纹理 pass 在帧末回放时源节点已删。

同时暴露 `DIV-GFX-054`:dump 里的区域是 `{"x":0,"y":0,"x2":-1,"y2":0}`——
旧实现把未指定的 `Y/Y2` 填 0,只复制首行。真实脚本是
`CGACT[ID="BT.OFF.TMP" ID2=$off COPY2=1 X=0 X2=@sx-1]`(整高切片)。

修复后首屏 census:

```text
passes 14 → 82(texture pass 12 → 80 = 69 copy + 11 image)
ops    17 → 85(drawn 85, skipped 0)
```

### 3. 验收原始输出

```text
$ cargo test --workspace --no-fail-fast        (带 AGENTS §2 样本)
61 个测试目标 test result: ok;0 个 FAILED

$ cargo test -p yu-gfx --test eris_first_screen
3 passed(含新增 A5 拒绝用例 empty_image_ext_order_is_rejected_not_defaulted)

$ cargo test -p yu-backend-soft --test render
10 passed(含 A1/A3/区域 3 条新用例)

$ cargo test -p yu-backend-wgpu --test wgpu
15 passed(含 reverse_after_rectpaint_matches_soft)

$ cargo test -p yu-cli --test restart -- --ignored
1 passed(先跑到 title 再 restart,同帧 RGBA SHA 一致)

$ frame_walk 600 / wgpu_frame_walk 600
hash 差异 0;ops/drawn/skipped/transition/copy/image 差异 0
title 稳定帧 a5a618d0…2483(两端相同)

$ case --all
3 passed; 0 failed

$ cargo clippy --workspace --all-targets
0 error;yu-rt 5 条既有 warning(与本线无关);yu-gfx / yu-backend-soft /
yu-backend-wgpu / yu-cli 全部 0 告警

$ grep -rn "println!|eprintln!|dbg!" crates/*/src
唯一命中是 yu-cli/output.rs 的纪律注释本身;src/ 无 stdout 宏
```

### 4. 回归验证(证明测试是真门禁)

| 手法 | 结果 |
|---|---|
| 临时注释掉 `SoftRenderer::reset()` 的 `snapshots.clear()` | `reset_drops_snapshots_so_new_copy_ids_do_not_alias` **FAILED**(旧像素 `[255,0,0]` 冻结) |
| 临时注释掉 `Engine::restart` 的 `renderer.reset()` | 单元级测试仍通过(3 帧黑屏前后缓冲相同)⇒ 已把 restart 用例改成**先跑到 title 再重启**;这是该用例第一版不敏感的原因,记录在测试注释里 |

### 5. 验证首帧像素

`first_frame`(tick 1,含 COPY)现在为纯黑 `(0,0,0,255)`,与
“官方首帧 = 设备初始化黑屏”一致;title(tick 300)与官方 `frame_0006.png`
平坦区域逐通道 max 2 / diff>4 = 0。

### 6. 未闭合

- 官方首帧(启动瞬间)截图缺失 ⇒ DIV-GFX-051 的锚点是“设备初始化黑屏”推断,
  非逐像素取证;
- `bt` 快照按钮的实际像素未与官方 title 按钮列逐像素对拍(需要 hover 态输入
  与官方同状态采集);
- 重启语义仍为“重启 = 新设备”的决策(A3),没有引擎 trace 证据。

## 成果 E1-M1 (2026-10-22) · yu-emote 独立仓库:PSB 解析/body 解密/key 搜索 + YU-RIS 适配骨架

> 用户要求:不能只看 exe;把可复用的开源实现拉下来,建立独立仓库并在当前
> 工程回链。仓库(已公开):<https://github.com/NotFaceGUI/yu-emote>,
> 以 **git submodule** 挂在 `<repo>/yu-emote`(gitlink `2cb7261`)。

### 1. 参考项目(本地 `<projects>/emote-refs/`)

| 项目 | 许可 | 用途 |
|---|---|---|
| storycraft/emote-psb-rs | MIT | `yu-emote` 的 PSB/MDF 二进制层(直接依赖 `emote-psb = 0.8`) |
| Mieluoxxx/amadeus-emote-loader | Apache-2.0 | 规范化/部件/显式 wasm32 的行为参考(未复制) |
| xmoezzz/eluna | 未声明 | PSB/动画结构行为参考;**不复制代码** |
| UlyssesWu/FreeMote | CC BY-NC-SA 4.0 | PSB body cipher/key 事实与 oracle;不复制 |

### 2. 实现

- PSB/MDF 解析;body-only 解密(6 词流,每游戏 key);
- `psb-key-search`:单 DWORD key 搜索(数组头 + root 0x21 过滤);
- `psb-info`:确定性摘要(version/资源/top-level/节点计数/深度);
- `yuris`:`EMOTE` 参数按名记录、`EMOTEINFO` 未取证字段显式拒绝、
  capability(`SYSTEMINFO[EMOTE]` 保持 0 直到渲染可用)、`suspend/resume`、
  dirty 帧标记;
- 许可:MIT OR Apache-2.0;`NOTICE` 记录 emote-psb/amadeus/eluna/FreeMote 边界。

### 3. 验证(原始输出)

```text
$ cargo test                      # 无样本:全部通过(样本测试显式 SKIP)
$ YURIS_EMOTE_SAMPLE=<emote_test.psb> YURIS_EMOTE_KEY=0x175058c9 cargo test
test emote_test_sample_parses ... ok
{
  "version": 2, "resources": 2, "root_kind": "object",
  "top_level": { "easing":"list", "id":"string", "label":"string",
                 "metadata":"object", "object":"object", "screenSize":"object",
                 "source":"object", "spec":"string",
                 "stereovisionProfile":"object", "version":"float" },
  "counts": { "objects": 5863, ... }, "max_depth": 36
}
$ cargo run --release --bin psb-key-search -- emote_test.psb
found key: 0x175058c9
```

`eluna psb_extract` 对同一解密文件报 `names=405 strings=388 resources=2`,
与 `yu-emote` 一致。key 是**每游戏**数据,只从 `YURIS_EMOTE_KEY` 读取,
不入库、不硬编码。

### 4. 回链

- `docs/emote-parity.md` §0/§3.1:实现仓库与当前状态;
- `docs/tasks/task-emote-runtime.md` §0.1:M0 partial / M1 partial;
- `docs/references/open-source-tools.md`:五个 E-mote 项目的许可与用途;
- 当前内核:公开仓库以 **git submodule** 挂在 `yu-emote/`
  (`.gitmodules` + gitlink `2cb7261`);已 staged,**未提交**(按 AGENTS
  零 git 写操作,待主人 commit)。未加 Cargo 依赖;M1 稳定后由适配 crate
  用子模块内 path 依赖。

### 5. 未完成

M1 剩余(规范化/部件表)、M2(时间线)、M3(wgpu 渲染 58 变体)、
M4(官方 DLL 对拍)。

## 成果 AU3 (2026-10-22) · AU2 产出独立复验 + P2-8 blend 状态表交叉检查 + 台账/README 收口

> 与 AU2 分会话并行推进后接手(本会话最初按 todo-inventory §5 顺序做 P0,
> 发现 AU2 同批落地后转做独立复验与剩余 P1)。零 git 写操作。

### 1. AU2 产出独立复验(不看结论,重跑命令)

```text
$ export TEMP=<side-repo>/workbench/tmp  YURIS_SAMPLE_ERIS_YSBIN / ERIS_ROOT=…
$ cargo test --tests -p yu-gfx -p yu-backend-soft -p yu-cli
  yu-gfx 10 目标 + soft 22+10 + yu-cli 10 全部 ok;0 FAILED
$ cargo test -p yu-cli --test restart -- --ignored
  restart_is_deterministic_at_same_frame … ok(120.7s,真语料跑 title 后重启)
$ cargo test --tests -p yu-backend-wgpu
  9 + 15 passed
$ cargo run -q -p yu-cli --bin yu-cli -- case <project-eris> --all
  first-screen / record-events-off / until-visible:3 passed; 0 failed
```

关键路径代码审读(逐 diff):A1 首帧 `frame.clear` 后缓冲合成(soft/wgpu 同因)、
A3 `reset()` 清单(soft 三项 + wgpu 六项,`TargetSet` 清理连带
`pipelines.forget_texture`,屏幕目标按尺寸复用故保留——正确)、A2 拒绝构造
(允许宿主显式注入,优于只拒 yscfg)、053 `source_texture` 命令时刻固定、
054 `CopyRegion::resolve` 缺省整幅。另核实:游离在 `mod tests` 外的
`shader_transition_bypasses_fixed_stage_reduction` 用例仍在文件尾
(编译/收集不受影响,纯卫生问题,AU3 已归位)。

### 2. P2-8:blend 公式与状态表交叉检查(`yu-backend-soft`)

- `blend_pixel_with_stage` 的 stage 归约提取为 `stage_reduce()`
  (纯抽取,零行为变化);psC/psD 直通分支保留在函数内。
- 新增测试 `blend_pixel_matches_generic_d3d9_equation_from_state_table`:
  - 通用 D3D9 方程求值器只读 `fixed_state()` 的 `src_blend`/`dst_blend`/
    `blend_op`(因子语义按 D3D9 头:**绝对**——`SRC*` 恒读 oC0、`DEST*`
    恒读目标像素;首个版本把因子按“所在操作数”解释,网格立即抓出
    MODE 0 alpha 错,修正后全过——这正是该测试存在的意义);
  - stage 归约复用 `stage_reduce`,对 8 个可执行模式 × 12 点网格
    (全 0/全满/半透明/奇数低位/白混敏感点)逐通道对拍;
  - 同断言“可执行集 == `fixed_state()` 非空集合”,执行门也由状态表驱动。
- 效果:状态表(`yu-gfx`)与公式(`yu-backend-soft`)任何一侧单独改动
  都在测试失败,不再漂移到像素。

```text
$ cargo test -p yu-backend-soft --lib
  22 passed(21 → 22);clippy -p yu-backend-soft --all-targets 本体 0 告警
  (仅 yu-rt 5 条既有 warning,AU1 已注明)
```

### 3. 复核/收口(无代码)

- **P2-2**:soft 零尺寸分支与 wgpu 规划期都逐 op 计入 `ops_total` +
  `group.size.zero`,不变量两端成立 → 关闭。
- **P2-9**:A2 后探测序只在 `from_tables`/宿主注入确定,`GfxState` 无
  setter,“帧内变化不刷新”失去前提 → 作废。
- **example 重名**:wgpu 侧已是 `wgpu_first_frame`/`wgpu_frame_walk`
  (目录实测无重名)→ 关闭。
- **README**:TODO 总览 4 行过期(A1/A3/A2 标 open、case 标全过期)
  → 按现状校正。
- **todo-inventory**:§2.1 第 2/4/5/6/10 项、§5 前 6 行、§6 第 5 条按
  AU1/AU2/本线结果更新;未动各台账真值。

### 4. 验证原始输出(本成果全部命令)

见 §1/§2;另 `case --all` 于 AU3 复跑通过(3 passed; 0 failed)。

## 成果 N (2026-10-22) · 声音域宿主 `yu-sound`:N0 取证 + 状态机 + yu-rt/CLI 接线

> 任务书 `docs/tasks/task-n-sound.md`。净室实现(未复制旧线代码);零 git 写操作。
> 逐条证据/地址/公式见 `docs/sound-ledger.md`;分歧见 `DIV-SND-001..004`。

### 1. N0 取证(静态 REA,0.495 `4f5eefa3…`)

| 处理体 | 事实 |
|---|---|
| SOUND `0x44e1e8` | 层表=名字哈希表(MurmurHash2);新层 0x38 字节;`+0x34`=声音对象;参数块 E/FILE/PLAY/PAUSE/VOLUME/PAN/PITCH/LOOP/END/START/RESTART |
| SOUNDINFO `0x44f4bc` | 22 字段按固定顺序逐个写 LET(最后者胜出);缺层清 0;TIME=`pos*1000/bytes_per_sec`;TOTALTIME=解码器 `+0x120` |
| SOUNDEND `0x44f308` | 按 ID 查表→删层 |
| 位置/循环 `0x464960` | `TIME>=END` ⇒ LOOPCOUNT(+0x110)+1;`LOOPCOUNT==LOOP(+0x10c)` ⇒ 清零/停止;否则回 RESTART |
| 淡变 `0x423718` | 线性、ms、固定点 1/1024;暂停顺延淡变时钟;淡出完成重置音量 256 |
| 映射 | DSBVOLUME=`clamp(round(4000*log10(v/256)),-10000,0)`;DSBPAN 同式(0..512);PITCH=`base*(pitch+256)/512` |
| 空桩 | MIDI/MIDIEND/MIDIINFO `0x44aa40/60/80`、CDDA `0x429fb0` 只守卫后 return 0 |

导入表:DirectSoundCreate / winmm(mci,mmio,waveOut…) / MSACM32(acmStream*) /
静态 `vorbis` 字符串 @`0x5b8e5c`;`fmod*` 仅为 CRT DOMAIN error 文案。
工程 yscm 实测:SOUND 14 槽(多 SPEED)、SOUNDINFO 24 槽(多 FILETYPE/
SAMPLING/DATALEFT/DATARIGHT)——与手册 19 字段不同,已立 DIV-SND-001,
宿主按名消费。

动态曲线:YSCom 0.495 探针编译成功,但探针工程在 LE 下未进脚本;运行中的
ERIS 工程断点 0x44e1e8 命中 1 次、0x44f4bc 命中 0 次。故 LOOPCOUNT/TIME
逐帧曲线**未取得动态样本**,台账标 `Confirmed(静态 REA)` 并立 DIV-SND-003。

### 2. 实现

- 新 crate `crates/yu-sound`:`state.rs` 层状态机、`decode.rs`(lewton OGG +
  hound WAVE)、`mixer.rs` headless 确定性混音、`query.rs`(并入 state)、
  `error.rs`(`snd.*` 码表+唯一性单测)、`loader.rs`(`SoundLoader` 注入)、
  `host.rs`(`SoundHost`);`#![forbid(unsafe_code)]`;src 零 println/eprintln/dbg。
- `yu-rt` 加法式:`event::SoundHost` trait;`Vm.sound_host` + `set/take`;
  `emit` 同步路由 `Domain::Sound`;`tick/tick_at` 注入 `advance(now_ms)`;
  `SOUNDINFO` 仅在宿主已安装时走 `op_soundinfo`(LET 左值→query→write_target),
  未装宿主保持 M-2 路径逐位不变。
- `yu-cli` 加法式:`boot()` 在 `os_sound=true` 时用工程 `DataSource` 构造
  `VfsSoundLoader` 并安装 `SoundHost`;不碰文件系统布局。
- 手册示例 1..12 的值断言单测;WAV 合成夹具 + 环境门控 OGG(bgm03)。

### 3. 验证原始输出

```text
$ cargo test --workspace
（全部 target ok;yu-sound: 1+17 passed;yu-rt: 54 passed;yu-vfs/format/cli 全绿）

$ cargo test -p yu-sound
running 1 test  ... ok
running 17 tests ... ok (17 passed)

$ cargo clippy -p yu-sound --all-targets
（yu-sound 0 warning;yu-rt 仅既有 5 条 warning）

$ YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
  cargo run -q -p yu-rt --example coverage_census -- 600 8
- 合计: 命令 7735670 次,事件丢弃 0
| `SOUND` | 87 | 34 | 531 | 域事件:sound |
| `SOUNDINFO` | 89 | 25 | 7200 | 域事件:sound |
| `SOUNDEND` | 88 | 2 | 1 | 域事件:sound |

$ cargo run -q -p yu-cli --bin yu-cli -- case <project-eris> --case first-screen
case first-screen: PASS
```

### 4. Unimplemented(挂账,不猜)

- 负 START/RESTART 延迟(DIV-SND-002)、ACM WAVE(DIV-SND-004)、
  MIDI/MIDIEND/MIDIINFO/CDDA、DATALEFT/DATARIGHT、SOUND[SPEED=]。
- 设备输出(cpal)未接入;headless 混音与 golden 不依赖设备。

### 5. 文档

`docs/sound-ledger.md`、`DIV-SND-001..004`、`crates/yu-sound/README.md`、
`coverage-matrix.md` §8、`todo-inventory.md` §4(域事件 34→31)、
`architecture.md`(12 crate + 依赖图)、`README.md` 模块表。

## 成果 N 续 (2026-10-22) · 按 REA 二次校正 + 诊断出口 + golden

> 复核 `0x40c9b4` / `0x45f348` / `0x4251d8` / `0x425238` / `0x425298` /
> `0x424e4c` 的汇编后修正。仍为零 git 写操作。

### 1. 修正（均为引擎语义）

1. **文件名**：`0x45f348` 整条 ASCII 小写；`0x45a064` 检查尾部
   `.wav/.ogg`，已带后缀不再追加，否则追加小写 `.ogg`/`.wav`；
   `SOUNDINFO[FILE]` 返回最终候选名（含扩展名），不是原始参数。
2. **装载失败**：`+0x000` 保留最后一个候选名，旧 PCM 卸载，层保留但
   `EXIST=0/PLAY=0/TOTALTIME=0`；解码失败同样清空。
3. **START/RESTART/END**：`>= total` 的 START/RESTART 写 0；END 只
   `> total` 才钳到 total；三者都只写字段，单独 START 不移动 TIME。
4. **PAUSE/PLAY**：处理体顺序 PLAY→PAUSE；PLAY 不清 `+0x109`，
   `PLAY=1 PAUSE=1` 结果为“播放后暂停”。
5. **音量**：`DSBVOLUME=4000*log10(v/256)` ⇒ 振幅 `(v/256)^2`；
   mixer 由线性改为平方。
6. **END 边界**：每帧只检查一次（对应引擎每个音频回调一次），去掉
   1024 次 while 保护；跨边界余量不携带。
7. **空 ID**：不再拒绝；引擎直接以空键建层。
8. **WAV**：`fmt ` chunk 走 RIFF 链查找（JUNK/LIST 前置也成立），
   hound `Unsupported` 映射到 `snd.codec.unsupported`。
9. **诊断出口**：`Engine` 保留 `SoundState` 的 `Rc`，每帧 drain
   `snd.*` 到 `Engine::take_sound_diags()`（上限 8192）。

### 2. 新增测试

- `SOUNDINFO` VM→宿主→LET 端到端（mock `SoundHost`，不依赖 yu-sound）。
- 文件名解析/小写/失败清空、单独 START 不改 TIME、PLAY+PAUSE 同命令、
  START/RESTART/END 边界、音量平方曲线。
- `tests/golden/loop_timeline.txt`：循环时间线 + render FNV-1a 哈希。

### 3. 验证原始输出

```text
$ cargo test -p yu-sound
unittests: 1 passed
state.rs:  24 passed
doc-tests: 0 passed

$ cargo test --workspace --no-fail-fast
（无 FAILED；含 fixture 新增 soundinfo_query_writes_let_through_host）

$ cargo clippy -p yu-sound --all-targets
（yu-sound 0 warning）

$ coverage_census 600 8
SOUND 531 / SOUNDINFO 7200 / SOUNDEND 1
（未装宿主，诊断文案仍为“域命令未接线”）
```

### 4. 仍未完成

- 负 START/RESTART 延迟（DIV-SND-002）、ACM（DIV-SND-004）、
  MIDI/CDDA、DATALEFT/RIGHT、SPEED。
- 动态逐帧曲线（DIV-SND-003）。
- `www` 音频通道：仍无。原生设备输出已补 `yu-cli --features device`
  （cpal 0.15，主线程 render → 环形缓冲 → 回调；不进 golden）。
- 新增依赖许可：`hound` Apache-2.0（WAV）、`lewton` MIT/Apache-2.0（OGG）、
  `cpal` 0.15 MIT/Apache-2.0（可选设备输出，仅 `device` feature）。

## 成果 N 续 2 (2026-10-22) · 装载/VOLUME/LOOPCOUNT/SOUNDINFO 多字段按原始汇编校正

> 复核 `0x44e1e8`（SOUND 处理体参数分派）、`0x424614`（type 1 load）、
> `0x42511c`（type 5 volume）、`0x425164`（type 6 volume target）、
> `0x424950`（type 2 play）、`0x42519c`（type 7 loop）、`0x464960`
> （循环边界）、`0x44f4bc` 原始汇编（多字段短路）。仍为零 git 写操作。

### 1. 修正（均为引擎语义）

1. **VOLUME 无 PLAY**：type 5 立即写 `+0x148/+0x154/+0x150` 并 SetVolume；
   旧实现只在“未播放”时改当前音量，播放中 `SOUND[VOLUME=x]` 不生效。
2. **VOLUME 有 PLAY**：type 6 只更新目标 `+0x150`，由 type 2 的 FADE
   从当前音量淡到新目标（手册例 12）。补 `fade_to_new_target` 单测。
3. **装载重置**：type 1 在重装时把 PAN/PITCH 回 256、START/RESTART/END/位置
   清零；E/VOLUME/LOOP/LOOPCOUNT 保留。旧实现保留旧 PAN/PITCH/START。
4. **PLAY 起始**：type 2 state 0 分支 `if (END <= START) START = 0` 后 seek。
5. **LOOPCOUNT 生命周期**：`+0x110` 只由 `0x464960` 的循环边界写；type 1/2/14
   都不清零。旧实现在装载/PLAY 时清零；改为保留（LOOP=N 正常结束时仍清零）。
6. **LOOP 停止条件**：引擎是 `count+1 == LOOP`，不是 `>=`；改为相等。
7. **淡出中 PLAY**：type 2 的 state 4 分支交换淡变当前/目标后立即停止；
   旧实现会重新淡入。
8. **SOUNDINFO 多字段**：`EXIST..PITCH`（槽 1..7）各带独立 `ret`，按固定
   顺序首个命中即返回；不含这 7 个时其余字段连续写 LET、最后者胜出。
   旧实现一律“最后者胜出”。
9. **DATALEFT/DATARIGHT**：`0x44fe3c/0x44ff58` 已逐行读完（播放光标处读
   一帧、16-bit 取 abs、8-bit `|byte-0x80|`、单声道右值 -1）。手册无这
   两个字段 ⇒ 三层缺手册层，按任务书 §0.1 **保持 Unimplemented**；
   只更新诊断文案与台账 N0-12。
10. **文件缺失**：`FUN_0040c9b4()==1` 时 SOUND 处理体直接返回，后续
    PLAY/VOLUME/START 等参数不应用；若此前 state != 0，先执行 type 0xe
    （清 `+0x000`、Stop、释放 PCM、清 START/RESTART、state=0），但保留
    `+0x105/+0x107/+0x120/+0x118/PAN/PITCH/LOOP/LOOPCOUNT`。旧实现清空
    元数据、把最后候选名写进 FILE，均与汇编不符。
11. **元数据存活**：新增 `SoundMeta`（file_type/codec/channels/bits/rate），
    卸载后 `EXIST/CODEC/CHANNEL/BIT/SAMPLING` 仍返回旧值；`TOTAL/TIME`
    也保留（`0x423940` 只清 START/RESTART）。
12. **LOOP=N 结束**：`0x464960` 停止分支调用 `0x423940`，额外清
    START/RESTART；已同步。

### 2. 新增测试（9 条）

- `volume_without_play_applies_immediately`
- `volume_with_play_fades_to_new_target`
- `load_resets_pan_pitch_start_restart_but_keeps_volume_and_loop`
- `play_resets_start_when_end_is_not_after_start`
- `loop_count_survives_stop_and_play`
- `play_during_fade_out_stops_like_engine_state4_branch`
- `multi_field_query_short_circuits_early_fields`
- `missing_file_unloads_clears_name_but_keeps_metadata`
- `missing_file_on_stopped_layer_changes_nothing`

### 3. 验证原始输出

```text
$ cargo test -p yu-sound
unittests: 1 passed
state.rs:  33 passed; 0 failed
doc-tests: 0 passed

$ cargo test --workspace --no-fail-fast
（全部 test result: ok，无 FAILED；YURIS_SAMPLE_ERIS_YSBIN 指向
 project-eris/save/data/ysbin，YURIS_TEST_TMP 单独 export）

$ cargo clippy -p yu-sound --all-targets
（yu-sound 0 warning；yu-rt 的 5 条为既有告警）

$ sound_census 600
frames=600 layers=167
id=BGM/B1 file=bgm/bgm01.ogg state=Playing time=6000 total=90802
loop=0 count=0 vol=256 pan=256 pitch=256 e=true pause=false
snd_diags=0

$ sound_loop_census
t=90144ms time=88176 count=0
t=95152ms time=2368 count=1
FINAL t=96000ms time=3200 count=1
snd_diags=0
```

### 4. 仍未完成

- 负 START/RESTART 延迟（DIV-SND-002）、ACM（DIV-SND-004）、
  MIDI/CDDA、DATALEFT/RIGHT（缺手册层）、SPEED。
- 动态逐帧曲线（DIV-SND-003）。
- `www` 声音：按用户 2026-10-22 决定**不实现**（不是真 wasm，只是胶水）；
  设备输出终点为 `yu-cli`（默认 `device` feature）。

## 成果 N 续 3 (2026-10-22) · CLI 默认设备输出 + i16/u16 声卡格式

> 用户反馈“启动游戏没有声音”。原因是 `yu-cli` 的 `device` feature 默认关闭：
> 状态机/查询都在跑，但 PCM 没有推给声卡。改为默认开启，并补非 f32 输出格式。

### 1. 改动

1. `crates/yu-cli/Cargo.toml`：`default = ["device"]`；纯 headless 用
   `--no-default-features`（golden/CI 不依赖声卡）。
2. `sound_device.rs`：除 f32 外支持 i16/u16 默认输出格式（WASAPI 设备
   不一定是 f32）；不支持的格式仍显式报错并继续无声运行，不 panic。
3. `main.rs` 帮助：删除“宿主只是不播放”的旧说明，写明默认设备输出与
   `www` 不实现音频的边界。
4. `docs/architecture.md`、`docs/sound-ledger.md`、`yu-sound/README.md`
   同步“默认 device、可 `--no-default-features`”的口径。

### 2. 验证原始输出

```text
$ cargo check -p yu-cli --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.37s

$ YURIS_SAMPLE_ERIS_ROOT=… cargo run -q -p yu-cli --example sound_census -- 30
frames=30 layers=166
snd_diags=0
（stderr 无 “[sound] 设备输出未启用”，说明默认设备流已打开）
```

### 3. 说明

- `yu-cli run` 默认只 tick 1 帧（`--frames/--until-*` 控制），不是实时游戏循环；
  实时出声入口是 `yu-cli serve`（按 `--fps` 节拍持续 tick 并 pump）。
- `www` 浏览器端仍无音频（用户决定，见台账 §7）。

## 成果 E1-M2 (2026-10-22) · E-mote 数据/动画/渲染 core + 官方 ABI/纹理/shader 取证

> 接续 E1-M1。目标:把 `yu-emote` 从“能解析”推进到“schema/atlas 确定、
> 时间线可求值、58 个 shader 有 WGSL 映射”,并把官方 DLL 的事实写进台账。
> 零 git 写操作;证据产物只在 `<side-repo>/workbench/tmp/`。

### 1. 官方二进制取证(REA + 离线 capstone)

- `EmoteCreate` → `PEmoteDevice`(vftable `0x100716CC`,22 槽);
  `+0x48 CreatePlayer` → `PEmotePlayer`(vftable `0x1007172C`,56 槽);
  `psb-key` = `strtol("391141577")` @ `0x10071350`,即 `0x175058C9`;
- `YSEmote.dll` 80 个跳板:序号 1/2/3 + 101..121(device)+ 201..256(player);
  引擎加载器把指针存到 `yu-ris.exe 0x801384..0x8014C0`;
- `EMOTE`/`EMOTEINFO` 处理体:按名应用字符串/标志/数值变量,数值 fade
  `×0.06`;`EMOTEINFO` 调 player 槽 45;
- 纹理:`RGBA4444` → `D3DFMT_A4R4G4B4`;转换器 `0x10054BA0`,nibble 顺序
  `A(15..12) R(11..8) G(7..4) B(3..0)`,×17;
- 58 个 shader 从指针表 `0x100866C0..0x100867AC` 重新提取
  (27 ps2 + 27 ps3 + 3 vs2 + 1 vs3),逐个 `fxc /dumpbin` 归类为
  13 plain / 13 masked / 1 pixelate(+4 顶点)。

### 2. `yu-emote` 实现

- `schema`:screen/metadata/sources(objects/motions/layers/frames)/
  variables/timelines/controllers;全部 `Serialize`,对象键排序;
- `texture`:RGBA4444 解码 + atlas 页读取;
- `anim`:按名变量、fade、PLAY/DPLAY/STOP、循环、固定 tick、线性采样
  (`easing==0`),未支持 easing 记录并保持;
- `render`:`PIXEL_PROGRAMS` 27 项特征表;58 个官方变体落成
  `shaders/ps_2_0/*.wgsl`、`shaders/ps_3_0/*.wgsl`、`shaders/vs_2_0/*.wgsl`、
  `shaders/vs_3_0/00.wgsl`(`gen-shaders` 生成,测试防漂移);
  `MeshPatch`(4×4 cubic Bezier,identity `(col/3,row/3)`)+ `MeshGrid`
  按 `meshDivision` 细分;
- `psb-dump`:确定性 JSON、`--raw`、`--resources`、`--decode`;
- `PsbDocument.resource_data`:保留资源字节。

### 3. 验证(原始输出)

```text
$ export YURIS_EMOTE_SAMPLE=…/emote_test.psb YURIS_EMOTE_KEY=0x175058c9 YURIS_REQUIRE_SAMPLES=1
$ cargo test
   schema 5 passed; sample 1 passed; roundtrip 2; yuris 5; doc 1; 0 failed
$ cargo clippy --all-targets
  Finished (0 warnings)
$ cargo run --bin psb-dump -- <psb> --key 0x175058c9
  container: v2, resources=2, key_used=true
  model: id=motion label=template spec=win version=3.03
  sources: tex#0 2048x2048 RGBA4444 icons=97 resource=1
           tex#1  512x256  RGBA4444 icons=13 resource=0
  stats: objects=6 motions=21 layers=234 frames=888 variables=24 timelines=11
  resources: 0=262144 B sha256 44819f5b…; 1=8388608 B sha256 b73e697e…
$ cargo run --bin psb-dump -- <psb> --resources out --decode
  page-tex_0.rgba8 sha256 55b0d647…; page-tex_1.rgba8 sha256 89e9dd77…
```

两次 `Model` JSON 序列化逐字节一致(测试 `dump_is_deterministic`)。

### 4. 未完成 / 挂账

- 物理控制器(bust/hair/parts)的数值积分、差分淡入曲线、官方帧率;
- wgpu 管线与运行时 shader 变体选择(当前只有 WGSL 文本与几何生成);
- 官方 DLL 像素对拍(M4),Bezier 网格的官方确认;
- 内核适配层(把 `EMOTE` 事件接到 `yu-emote`)尚未接线。

## 成果 E1-M2 续 (2026-10-22) · pose/plan/soft 渲染链 + priority 顺序

> 接续 E1-M2。目标:把数据层推进到“能按 motion 求值并画出角色”,同时把
> 官方 player 的 ABI/渲染入口继续写进台账。

### 1. 新增实现(`yu-emote`)

- `pose`:frameList 求值(type 0 hold / type 3 插值 / `ti` 量化)、
  `inheritMask` 0x4..0x400、`transformOrder` [0,3,2,1]、coordinate XY/XZ、
  type-3 嵌套 motion、motion `priority` 绘制顺序;
- `plan`:atlas 矩形/枢轴/UV、corner color(0xAARRGGBB)、4×4 Bezier 网格、
  blank 白色填充页、view fit 变换;
- `soft`:确定性 CPU 光栅(bilinear/clamp、source-over)、PNG 输出;
- CLI:`psb-pose`、`psb-plan`、`psb-render`。

### 2. 验证(原始输出)

```text
$ export YURIS_EMOTE_SAMPLE=…/emote_test.psb YURIS_EMOTE_KEY=0x175058c9
$ cargo test
  lib 16 passed; pose 6 passed; sample/schema/roundtrip/yuris/doc 全绿
$ cargo run --bin psb-render -- <psb> --key 0x175058c9 --time 0 --out out.png
  covered_pixels=335774 bbox=(126,55,703,1045)
  stats: items=145(textured=30, blank/mask=115) triangles=76718
         masked_items=121 unsupported_blend=0
  输出:完整角色(头发/眼/眉/身体/裙/腿),见临时目录 out.png
```

`psb-pose` time 0:229 个节点、145 个可见 drawable;model 空间 bbox
`x -643..429, y -971..767`。当前视觉缺口 = stencil/alpha-mask 合成:
脸部内层(白色皮肤)被头发覆盖;这不是 z 顺序问题(priority 已排序),
而是官方 type-12/mask 合成未实现。

### 3. 官方 player 继续取证

- `InitParam = { IDirect3DDevice9*, malloc, free }`;
- `CreatePlayer(device, psb_bytes_ptr, psb_size, out**)`(实测参数顺序;
  传反会崩在 `0x100014e0` 的 crypt filter);
- player wrapper:`+0xc` AEmotePlayer、`+0x10` impl、`impl+8` MMotionPlayer
  (vtable `0x10072248`);
- 渲染候选入口 `emotedriver+0x3c50`(MMotionRenderer::Render,仅 this 参数);
- oracle 宿主已能创建官方 player、枚举 23 个变量、推进/渲染,但仍为空白帧:
  缺 motion 激活/屏幕参数与 stencil;持续挂账,不把空白帧当对齐结果。

### 4. 未完成

- stencil/alpha-mask/pixelate 合成;wgpu 管线与 shader 变体选择;
- 物理控制器、差分淡入曲线、官方帧率;
- M4 官方像素对拍;kernel 适配层接线。

## 成果 E1-M2 续 2 (2026-10-22) · 绘制顺序/参数表/composite mask + 实机 demo

> 接续 E1-M2 续。修复“脸被头发盖住”的根因,打通变量表情,并用真实游戏
> `emote_test.psb` 生成可交互演示页。

### 1. 根因与修复

- **绘制顺序不是 priority**:native 先按 `priority` 发射 DrawFrameInfo,再按
  解析 Z(`DFI+36 = LayerInfo+620`)升序排序。样本 Z:後髪 -10、胴体 0、
  輪郭 3、前髪 4、瞳 5..6;只按 priority 会把後髪放在脸前。`priority` 现仅
  作同 Z tie-break。
- **motion 级 parameter 表**:`parameterize` 索引所属 motion 的 `parameter`
  数组,不是根表;`Motion` 现保存 `parameters` + motion 级 `parameterize`。
  眼睛/嘴/眉/泪/頬 motion 由 `face_eye_open`/`face_talk`/`face_eyebrow`/
  `face_tears`/`face_cheek` 驱动;默认变量值 0 再经 range→division 映射。
- **type-0 保持**回退到同 frameList 最近一个有 content 的关键帧(嵌套眼部
  motion 在 type-0 间隙后仍可见)。
- **corner color = 0xRRGGBBAA**(低字节 alpha);眼皮 `0x000000FF` 是
  不透明黑。
- **composite alpha mask**:type-12 + `stencilType & 4` +
  `stencilCompositeMaskLayerList` 的 owner,其子孙按同 scope source 图层
  的 alpha 并集裁剪;opacity 0 的 `add_mask` 不参与。CPU coverage buffer 已
  实现。
- **blank proxy 不再画白色**:`src=="blank"` 的 115 个 deformer/mask 节点
  只参与结构,不产生彩色 draw item。

### 2. 验证

```text
$ cargo test
  lib 16 + pose 6 + sample 1 + schema 5 + roundtrip 2 + yuris 5 + doc 1 全绿
$ cargo clippy --all-targets   # 0 warnings
$ cargo fmt --check            # clean
$ psb-render … --time 0
  items=30 triangles=3960 mask_sources=2 bbox=(95,20,704,1059)
  输出完整角色,脸/眼/眉/嘴/发层级正确
$ psb-render … --var face_mouth=30 --var face_eyebrow=40 --var face_eye_open=20
  笑顔;--var face_eye_open=10 半目;--var face_tears=2 泪;均逐像素变化
```

### 3. 实机 demo

- `yu-emote/demo/make_demo.py` 用 CPU 渲染 11 个姿态/表情,裁剪并内嵌为
  单文件 `target/emote-demo/index.html`(约 1.9 MB,含真实 PSB 渲染帧);
- 页面含表情按钮、←/→ 键切换、draw item/三角形/atlas/mask 统计与来源说明;
- 已在本地 IAB 打开验证:页面加载、按钮切换、图像与统计均正常。

### 4. 仍挂账

- 参数化 deformer mesh 对子层的继承(`head_UD/head_LR` 的网格变形);
- pixelate、wgpu 管线、shader 变体选择;
- 物理控制器、差分淡入曲线、官方帧率;
- M4 官方像素对拍(oracle 宿主已能 CreatePlayer/Update/Render,但 draw
  list 仍为空;YSEmote 导出是 stdcall,已修正 oracle 的调用约定);
- 内核适配层接线。

## 成果 E1-M2 续 3 (2026-10-22) · 真实 Nekopara 模型、时间线、sprite mesh 与 mask 底层修复

> 对照公开实现(eluna 所引 native `sub_10353CF0`/`sub_1038B340`/
> `sub_10337560`,只取事实、未复制代码)修正“眼睛溢出/歪脖子变小”的底层
> 几何语义,并把 PSB v4 `ems` 的真实游戏模型纳入验证。

### 1. 新增能力(`yu-emote`)

- **PSB v4 `ems`**:Nekopara `*.pure.psb` 为 `spec: ems`、`source.tex` 单页
  4096² `RGBA8`、四位字符串 icon 键;`texture::decode_page` 按格式分派,
  `schema` 接受 `source: "tex"`。已渲染 chocola/vanilla/azuki/coconut
  四个 maid 模型。
- **时间线播放**:`psb-render --timeline NAME --seconds T [--loop] [--fps N]`
  用 `Animator` 推进 timeline,再交给 pose;`--view SCALE,X,Y` 固定相机。
  demo 用 `待機ループ00` 生成 6 帧循环,页面支持实时循环播放。
- **sprite mesh 继承**(本次核心修复):`mesh.bp` 是归一化到 sprite icon box
  的 4×4 Bezier patch,不是世界空间 warp。仅
  `meshTransform == 1 && (meshSyncChildMask & 8)` 的祖先把 patch 折叠给
  子层;折叠是控制点位移逐点相加(`dst = next + (self - identity)`),绘制
  时在 sprite 自身 box 内采样。眼部 `UD→LR→hitomi_sp→eye_R`、头/身/口/
  眉形变链现在都生效;中性帧不再对单位 patch 做无意义细分。
- **composite mask source 展开**:owner 的 `stencilCompositeMaskLayerList`
  在 owner 的 motion scope 内解析;type-0 source 贡献自身几何,type-3
  source 展开为嵌套 player 内所有实际 sprite。修复嵌套眼/泪 source 漏配。

### 2. 验证

```text
$ cargo test        # lib 21 + pose 7 + sample 1 + schema 5 + roundtrip 2 + yuris 5 + doc 1 全绿
$ cargo clippy --all-targets   # 0 warnings
$ cargo fmt --check            # clean
$ psb-render emote_test.dec.psb --time 0
  items=30 triangles=60 mask_sources=2 bbox=(95,20,704,1059)
$ psb-render chocola-maid.pure.psb --time 0
  items=34 triangles=68 mask_sources=2 bbox=(34,121,779,938)
$ psb-render … --var face_eye_open=25 --var face_mouth=30 --var face_eyebrow=40
  triangles=3260;笑颜眼部由 mesh 压缩,composite mask 裁剪,不再溢出
```

- 固定相机下 neutral/smile/angry 的 bbox 完全一致(34,121,779,938);
  `head_slant=30` 仅 ±8° 旋转(模型自身幅度),bbox (23,111,796,938),
  不再出现“歪脖子变小”。

### 3. demo

- `demo/make_demo.py` 支持多模型 `--model LABEL=PATH`,每个模型从 neutral
  推导固定相机,渲染 4 表情 + 6 帧 `待機ループ00`;单文件
  `target/emote-demo/index.html`(5 模型,约 6.9 MB,WebP 内嵌)。
- 页面含角色/表情切换、空格实时循环、帧计数;真实游戏资产只在本地测试,
  不入库、不随 demo 分发(NOTICE 已声明)。

### 4. 仍挂账

- 参数化 mesh combinator(`rawMeshList` + variable + neutralIndex)与
  coord/angle/zoom sync(`meshSyncChildMask` bit 0/1/2);
- type-7 clip rect、非默认 blend(Coconut 32 项)、pixelate、wgpu 管线;
- 物理控制器、差分淡入曲线、官方帧率;
- M4 官方像素对拍(oracle 可 CreatePlayer/Update/Render,draw list 仍空);
- 内核适配层接线(submodule gitlink 待主人提交)。

## 成果 E1-M2 续 4 (2026-10-22) · 继承源 / mesh sync / mesh combinator / blend 对齐

> 继续按 eluna(库 crate 元数据 **MPL-2.0**,player 为 MIT OR Apache-2.0;
> 仅作行为参考,未复制源码)所引 native 语义对齐。`NOTICE` 已把 eluna 从
> “无许可”更正为 MPL-2.0。

### 1. 本轮实现(`yu-emote`)

- **native 继承源**:`inheritMask & 0x400000` 的层对继承透明,后代沿用其
  上方 source;`inherit_source` 单独保存 linear/linear_state/location/
  coordinate/opacity/mesh_sync。partial inherit 先按 mask 减去 motion root
  通道,构建相对矩阵,再在外侧乘 root 矩阵;嵌套 type-3 motion 以闭合层
  复合状态作为新 player 的 synthetic root。
- **meshSyncChildMask 0x1/0x2/0x4**:子层坐标按 `patch.domain` 映射;
  角度/缩放用 ±0.0001 差分的轴平均角与三角面积开方(`sub_10335500`)。
- **meshCombinator**:解析 `combinatorList` + `rawMeshList` 资源,按变量
  在 `meshCount` 个 patch 间插值,`neutralIndex` 为单位 patch;首条目按
  `parameterize` key 决定 self patch,其余作为 child patch(identity 位移)。
- **blend/MODULATE2X**:CPU 光栅实现 `sub_1041A190` 的 0..5 blend 表;
  MODULATE2X 帧默认 vertex color 改为 `0x808080FF`(×2=白),bm=0 帧仍为白。
  `unsupported_blend` 统计改为 `non_default_blend`(仅 low nibble 1..5)。
- **mesh division/domain** 随 patch 传播;unit patch 不再无意义细分。

### 2. 对照证据

- eluna 窗口/dump 对 ERIS 的 **draw order** 可直接读取:32 sprites、30
  visible,标签序列与本实现 `psb-plan` 输出逐项一致(後髪/下半身/腕/裙/
  胴/耳/胸/輪郭/横髪/口/頬/目影/眉/前髪/shirome/鼻/eye_L/hi_L/mabuta/
  eye_R/hi_R/mabuta)。eluna 的预览投影本身有偏移(窗口画面被拉伸),因此
  只把其 draw list 当顺序证据,不当像素 oracle。
- Chocola neutral 在继承源修复后仅有轻微像素变化(bbox 不变),smile 帧
  逐像素不变;说明修复命中的是结构性继承链而非主要可见层。
- `cargo test`:lib 25 + 其余全绿;clippy/fmt clean。

### 3. 仍挂账

- type-7 clip rect、inner/outer stencil phase、type-9 anchor、type-10
  feedback、particle、pixelate、wgpu 管线;
- 物理控制器(bust/hair/parts)、差分淡入曲线、官方帧率;
- M4 官方像素对拍(oracle 的官方 draw list 仍为空;D3D9 回读全黑);
- 内核适配层接线(submodule gitlink 待主人提交)。

## 成果 AU4 (2026-10-22) · 输入闭环:INPUT/MOUSE、按钮命中、关闭与平台映射收敛

> 目标:让真实语料的按钮/弹窗“点得动、关得掉”,并把平台差异从内核里
> 彻底隔离。证据全部来自 0.495 官方 `yu-ris.exe`(sha256 `4f5eefa3…`)
> 的新一轮 REA 反编译 + 手册 + ERIS 真实脚本。

### 1. 取证(新增,均为 0.495 一手)

| 对象 | 地址/页 | 事实 |
|---|---|---|
| `INPUT` 处理体 | `0x446564` | KEY→`0x5b4888`、PAD→`0x8015d4`、MOUSE→`0x5b488c`(带重启用锁存) |
| `MOUSE` 处理体 | `0x44aaa0` | MODE/TIME/MOVE/CLICK/WHEEL/X/Y/CURSOR/HX/HY/ID;X/Y 写 `app+0x5b8/0x5bc` 并 `SetCursorPos` |
| 输入轮询 | `FUN_0041a4b8` | 三个源开关分别**整段跳过**鼠标/键盘/手柄计数器更新;MOVE/CLICK/WHEEL 只参与“自动隐藏后唤醒光标”判定 |
| 四态转移 | `FUN_00461530` + `0x801e84` | `0/0→-1`、`0/1→1`、`1/0→-1`、`1/1→+1` |
| 脚本写回钳制 | `FUN_00447014` | 未按住:正数/零→0,负数保留;按住:正数保留,零/负→0 |
| `_KEYLAST/_MOUSELAST` | `FUN_00427350` case 0x91/0x92 | 读 `DAT_00801908` / `DAT_0080190c`;写入点 `FUN_0045a3b4`(L/R/M=1/2/4) |
| 文本推进 | `FUN_004046fc` | 读 `TEXT[CLICKBUTTON1..4]`/`SKIPBUTTON1..4]` 的**按钮码**计数器,不是硬编码键名 |
| `CGINFO[ONMOUSE*]` | `0x43d664` → `0x43de01/0x43dec8/0x43df7f` | ONMOUSE=`r|g<<8|b<<16`;ONMOUSE2=`a`;ONMOUSE3=alpha+RGB;不在部件内=-1 |
| 手册 | `system/input.html` / `system/mouse.html` / `cg/cginfo.html` | 源开关、光标模式/时间/反应开关、ONMOUSE 语义 |

### 2. 实现

- `yu-rt::input`:计数器改为**按引擎按钮码**存储(0=`_KEY_ANY`,1..7=鼠标,
  8..=键盘,384+=手柄);新增 `InputSettings`(源开关 + MOUSE 字段)、
  四态 `Counter`、写回钳制、`_KEYLAST/_MOUSELAST`、单帧脉冲。
- `yu-rt::vm`:新增 `op_input`/`op_mouse`(按 ysc 参数名消费,不写死槽号);
  `text_advance_requested` 改用 `TextState.click_buttons/skip_buttons`;
  `GraphicsHost::set_pointer` 每逻辑帧把光标推给 gfx。
- `yu-gfx`:`GfxState.pointer` + `TextureSampler`;实现
  `CGINFO[ONMOUSE/ONMOUSE2/ONMOUSE3]`。快照层按 `snapshot_files` 的源资产
  采样(ERIS 按钮 `BT.OFF...` 正是 `COPY2` 快照),组/离屏显式 Unimplemented。
- `yu-cli::Engine`:注入 `StoreSampler`(独立 `TextureStore`);极短点击的
  `pressed_for/deferred_release`(按下至少被一帧看到);`pulse_input`(滚轮);
  `request_window_close` → `@_WINDOW_EXEC_CLOSE=1`;`input_settings/main_finished`。
- `yu-platform-input`(新 crate):平台事件 → 引擎按钮名的**唯一**映射;
  键表以 `button_code` ABI 为准;光标形状/可见性纯函数化。桌面把 winit
  `KeyCode` 直接交给它;Web 页面只发 `{t:'key',code}` / `{t:'pointer'}` /
  `{t:'wheel',dy}`,由 native 侧映射。`yu-desktop` 的鼠标右/中键、滚轮、
  CURSOR/MODE/TIME 全部接线。
- `yu-cli serve`:`pulse` 控制 + `key/pointer/wheel` 原始事件;Web 不再复制键表。

### 3. 验证(原始输出)

```text
$ cargo test -p yu-rt --lib input
running 10 tests ... test result: ok. 10 passed; 0 failed

$ cargo test -p yu-platform-input
running 3 tests ... test result: ok. 3 passed

$ cargo test -p yu-cli --test input_settings -- --ignored --nocapture
test startup_input_settings_match_es_first ... ok

$ yu-cli case <project-eris> --all
case first-screen: PASS
case record-events-off: PASS
case until-visible: PASS
case title-config-back: PASS
4 passed; 0 failed
```

`cases/title-config-back.json`:第 300 帧点 `設定(217,368)`,第 390 帧点
`戻る(750,565)`,第 470 tick 回到标题(147 ops / 0 skipped)。

真实桌面宿主(winit + wgpu,release)人工验收:标题 → 設定 → 戻る → 标题;
第一次窗口关闭只推 `@_WINDOW_EXEC_CLOSE=1`(标题脚本不处理,窗口保留),
第二次关闭强制退出。

### 4. 性能口径(回答“为什么卡”)

- `target/release/yu-cli run … --frames 300 --backend wgpu`:
  `avg tick 5.03ms / avg render 1.44ms`;soft 为 `avg tick 5.59ms /
  avg render 24.89ms`。
- `yu-desktop` **debug** 构建:title 首帧 `engine_tick=6778ms`,稳态
  `tick≈32ms + render≈8ms`;这是未优化构建,不是正常游玩口径。
  `yu-desktop` 现在在 debug 构建启动时显式提示用 `--release`。
- 配置画面**首次**打开有一帧约 1.1s(release):该帧执行 ERIS 配置场景的
  大量脚本(`LET` 百万级),属一次性场景装载;稳态为 4–13ms。
  `TextureSampler` 实测 10 次调用 / 2 次解码 / 0.2ms,不是热点。

### 5. 仍挂账

- `DIALOG`(原生模态框,手册 `other/dialog.html`;处理体 `0x43fc74`):
  需要宿主 UI + 任务阻塞/LET 写回;当前保持 Unimplemented。窗口关闭在
  场景/文本中会走 `DIALOG` 确认,未实现时表现为“第一次关闭被脚本消费但
  不退出,第二次关闭由宿主强制退出”。
- `CGINFO[COLOR]`(显式坐标取色)、`ONMOUSE*` 对组/离屏快照的真实像素面。


### 2026-10-23 · E1-M2 续 5(时间轴/物理/动画)

- `yu-emote::anim` 改为 native 命令流时间轴(60 Hz、过渡时长、easing
  指数、type-0 游标、loop seek、差分 DPLAY delta×ratio)。
- `Player` 串联 timeline → clamp/eye → pose → physics;新增
  bust/hair/parts 弹簧、clamp、确定性眨眼;`psb-render --no-*` 可诊断。
- 修复空 `mesh.bp` 被当作“无 mesh”的底层错误(Nekopara hair 参数在
  中点跳变);逐帧像素差 ~7.6→~0.1。
- `pose` 支持顶层 easing 表、`cp` 贝塞尔、native 颜色 lane 插值、
  type-7 clip、type-9 anchor。
- `psb-anim` 整段渲染;demo 改为 30fps alpha WebM + 表情交叉淡入。
- 许可口径改为明确参考 eluna(MPL-2.0),README/NOTICE 已更新;
  `LICENSE-MPL-2.0` 入库。
- 仍挂账:粒子/feedback/内外 stencil/图控制器/selector/wind/pixelate/
  wgpu/M4。


### 2026-10-23 · E1-M2 续 6(眼睛高光/物理锚点)

- 嵌套 motion 深度 32→256:恢复 `hi_L/hi_R` 眼部高光(34→36 draw items)。
- type-1/5 物理锚点改走 native mesh-chain 位置;hair/bust 输出恢复,
  待机有真实二次运动。
- `psb-anim --no-loop`;demo 增加 1.5s 挨拶一次性片段。
- 仍挂账:粒子/feedback/内外 stencil/图控制器/selector/wind/pixelate/
  wgpu/M4。


## 成果 P5 · yu-trace 排障闭环(帧描述投影 / 故障快照 / 官方任务相位)

**日期**: 2026-10-23 · **任务书**: `docs/tasks/task-p5-trace-triage.md` ·
**交接件**: `docs/tasks/handoff-p5.md`(H-P5-01 panic hook 接线、H-P5-02 `case --trace`)

### 1. M-A · 帧描述轨道(record/replay/diff 闭环)

- 任务书写的 `record.rs:579/1121` 占位拒绝在 schema v3(成果 H)已解除;
  v4 这里补的是**投影规则**:新增 `FrameFieldPolicy::{Full, Screen}`
  (`crates/yu-trace/src/diff.rs`),`AlignedDiffOptions.frame_fields`。
  - `Full`:全部字段精确比较(同 producer / 事件级);
  - `Screen`:只比较 `frame/level/display`;`display` 非 null 时省略派生
    `display_sha256`;`display = null` 时摘要就是唯一内容,仍比较;
    `pass_count/op_count/executable_ops/skipped_ops` 在引擎外部不可观测,
    屏幕级跨 producer 明确省略(不声称相等)。
  - `display` 内部的 volatile 键由调用方录制前剔除;diff 不做自动过滤。
- `diff_traces --frame-fields full|screen`;`SCHEMA.md` §6.3 记录字段分类。
- 单测:`screen_frame_policy_compares_pixels_not_placeholder_counts`
  (占位计数不同 = 0 分歧;像素不同必须报 `/payload/display/pixels_sha256`;
  纯摘要记录必须比较 `display_sha256`)。

### 2. M-B · 故障快照(ring buffer + dump_on_fault + 离线重演)

新增 `crates/yu-trace/src/fault.rs`(约 1.5k 行,含测试):

- **ring buffer**:`RingConfig` 默认 300 tick / 单 tick 8 MiB / 总 64 MiB;
  每 tick 保留 {命令流摘要(增量 SHA-256 + 事件种类计数)、Diag(含
  `unimplemented` 标记)、显式输入帧、frame_description、失败的 vfs 读取};
  命令本体不入 ring。超预算先丢 display(退化为摘要),再截断 Diag detail;
  单 tick 超总预算保留**占位摘要** `retained=false`;淘汰/丢弃全部有计数
  (`evicted_ticks` / `diags_dropped` / `inputs_dropped` /
  `displays_dropped` / `timeline_truncated` / `clock_truncated`),不静默丢帧。
- **增量 SHA-256**:`hash::Sha256`(`update/finish`),与 `sha256_hex`
  同一测试向量;避免为百万级事件拼大 buffer。
- **dump**:`EventRecorder::dump_on_fault(path, FaultInfo)` 原子写两份文件:
  - `<path>`:schema v4 trace(header + 启动链 vfs + 保留 tick 的
    Diag/frame_description/vfs 失败记录);header 的 `input` 摘要与 `clock`
    按故障帧重算,`producer.sample_sha256` 由宿主写入;
  - `<path>.timeline.json`:工具级伴生文件,含故障类别/原因、每 tick 摘要、
    原始输入时间线、clock。schema 4 没有原始输入字段,时间线不塞进记录协议。
  - 先写伴生文件、最后 rename JSONL(提交标记);`FaultDump::read` 校验
    `trace_sha256` + producer + header input 摘要,不符 = `trace.schema`。
- **回放**:`FaultDump::replay(source, aux, argv, paths)` 用 header 重建
  `VmTrace`,按 clock 序列推进到故障 tick,逐 tick 精确比较
  `first_frame/frames/now_ms`、命令数/摘要、事件摘要/种类计数、Diag、输入;
  任何差异 = `trace.replay`。`frame_description` 是呈现层观测,离线 VM
  不重建画面,不参与逐 tick 比较(仍保留在 dump),不是容差。
  mid-run 挂 ring / tick 被淘汰 / 时间线或时钟截断 ⇒ `replayable=false`,
  replay 显式拒绝。
- **接线**:`TracingSource` 的 vfs 记录改走 `EventRecorder::record_payload`
  (ring 模式成功只计数、失败留记录);`VmTrace::tick_with` 自动
  `begin_tick` / `record_timeline_entry`(含 frame 0)/ `record_tick_input` /
  `note_tick_frames`;`VmTrace::enable_fault_ring` 必须在第一 tick 前调用。
- **示例**:`crates/yu-cli/examples/fault_dump.rs`(真实 `data.ypf` + 合成
  panic + `FaultDump::read` + 离线 replay)。`yu-cli` 仅新增 **dev-dependency**
  `yu-trace`,不改变 “yu-trace 旁挂、非运行期依赖”。
- **测试** `crates/yu-trace/tests/fault.rs`(8 个):
  ring 捕获 Diag/输入/帧描述并 replay 零差异;`catch_unwind` 捕获后 dump+replay;篡改 tick 摘要 = `trace.replay`;
  篡改 trace 文本 = `trace.schema`;容量淘汰计数;display 超预算退化;
  单 tick 超总预算占位;屏幕帧投影。

### 3. M-C · 官方任务相位(DIV-L-002 续)

- `tools/official_trace/probe_dispatch.py` 新增 `--mode task-phase`:
  只在 tick 入口 `0x4040f8` 下 int3,每 tick 只读遍历任务链表
  (`0x801704` 列表对象;节点 `+0x0c/+0x30/+0x34`;
  上下文 `+0x18/+0x1c/+0x20/+0x3c`、`+0x14 & +0x15`、`+0x09`);
  不挂命令分派断点。布防前先非阻塞找窗口并置前台(挂断点时
  `SetForegroundWindow` 会等 UI 线程而死锁)。输出 `task_phase.jsonl`。
- `crates/yu-trace/examples/emit_kernel_phase.rs`:同 600 tick 的内核
  `Scheduler::nodes()` 相位快照(script/pc/wait/deadline/enabled/text_rest)。
- `tools/task_phase_diff.py`:按 `click` / `index` / `anchor` 对齐,
  比较每 tick 启用任务的 `(script,pc)` 多重集合(另有 script-only),输出
  首分歧与焦点脚本 presence 区间;不做排序/去重/容差。
- **600 tick 采集(无点击)结果**:
  - 官方 600 样本(tick 4..603;布防晚 4 tick),产物
    `target/tmp/p5-phase-click5/task_phase.jsonl`;
  - 内核 600 tick(frame 1..602),产物
    `target/tmp/p5-kernel/kernel_phase.jsonl`;
  - `--align anchor --anchor-script 66`:官方 tick 12 / 内核 frame 12
    **锚点一致**;script 20 三个实例的 `(script,pc)` 在相对 0..590 逐 tick 相同;
  - 首分歧:官方常驻 `script 0`(容器/主任务)与 `script 8`;内核 `SYSTEM`
    主任务(script 8)在 frame 16 左右结束并从链表消失;
  - 显式忽略 0/8 后(`--ignore-scripts 0,8 --from-relative 0`):首个
    `(script,pc)` 分歧在相对 49——官方 `66:5168`、内核 `66:5100`(官方 66
    在 5100/5168 间推进,内核 600 tick 一直 5100);
  - 首个 script-only 分歧在相对 206:官方用 `139` 取代 `66`,内核仍保留 `66`;
    script 66 存活窗口官方 0..205、内核 0..590。
  - 这是任务生命周期/调度差异,不是 script 60 体内命令差异;`DIV-L-002`
    保持 open,新增证据已写入 `docs/divergences/DIV-L-002-script60-call-phase.md`。
- **点击未生效(挂账)**:本会话 `SendInput` 在窗口前台时已发送
  (`input_sent` 有记录),但官方 `0x80157c` 有效 UI 点击标志始终 0、
  `trigger=null`;对已知的 設定 按钮 (217,368) 同样无 trigger。
  script 60 返回后的转场相位仍缺官方样本;下一步需真实用户点击/可激活桌面。
- `tools/official_trace/README.md` §7 记录任务链表布局证据、用法与限制。

### 4. schema v4(回放闭包,外部 API 变更的必要适配)

- 并行工作中的 `yu-rt::BootConfig` 新增 `window_init_pos`
  (`@_WINDOW_INIT_POS`)与 `rng_seed`(MT19937 种子)。两者都是宿主注入且
  影响脚本行为,**不进 header 就无法重放**。
- `SCHEMA_VERSION` 3 → 4;`boot.host` 增加 `window_init_pos`(i64)/
  `rng_seed`(u32);`BootSummary::from_config/to_config` 同步;v3 文件直接拒绝。
- `tools/official_trace/{capture_screen,probe_dispatch}.py` 改写 schema 4,
  并在 `boot.host` 写 `window_init_pos=0` / `rng_seed=5489`
  (官方外部不可观测,按缺省占位;与内核缺省一致)。
- `SCHEMA.md` 更新 §2/§4.1/§6.3/§6.4/§9 与字段表。
- `crates/yu-trace/tests/golden/synthetic_frame.jsonl` 重新生成:外部
  `yu-format`/`yu-rt` 夹具字节变化(ysc 3434→3793、YSCom.ycd 5132→5587)
  只改了 vfs 身份与 table hash,**事件流逐字节不变**;schema 4 的
  `window_init_pos/rng_seed` 同时进入 header。
- `VmEvent::DomainCall` 新增可选 `script/pc`(P4):本 crate 的穷尽 match
  用 `..` 兼容,payload schema 不变;ring 全事件摘要同样忽略这两个可选字段。

### 5. 验证(原始输出)

```text
$ cargo test -p yu-trace -- --skip fault_dump_survives_600_ticks_and_replays
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ YURIS_SAMPLE_ERIS_YPF=...project-eris/data.ypf \
  cargo test -p yu-trace --test real_sample fault_dump_survives_600_ticks_and_replays
test fault_dump_survives_600_ticks_and_replays ... ok
test result: ok. 1 passed; 0 failed; ... finished in 236.50s
```

- 真语料 600 tick:ring 记录 → `dump_on_fault` → 读回 → 离线 replay
  **零差异**;`report.ticks_replayed=600`。
- `cargo clippy -p yu-trace --all-targets --no-deps -- -D warnings`:clean。
- `cargo fmt -p yu-trace`:clean。
- 任务相位 diff 原始输出(`target/tmp/p5-kernel/phase_diff_title_anchor_ignore08.txt`):
  `anchor script 66: official_tick=12 kernel_frame=12`;
  `script 20 presence official=(0,590) kernel=(0,590)`;
  `script 66 presence official=(0,205) kernel=(0,590)`;
  `--ignore-scripts 0,8 --from-relative 0`:
  `first divergence: relative_tick=49 expected=[20:190,20:241,20:248,66:5168] actual=[...,66:5100]`;
  `first script-only divergence: relative_tick=206 expected=[20,20,20,139] actual=[20,20,20,66]`。
- 官方 600 tick 采集 manifest:`task_samples=600`, `stop=max-ticks`,
  `trigger=null`, `effective_click=0`(点击限制见 §3)。

### 6. 仍挂账 / 交接

- **H-P5-01**:桌面宿主/serve 的 panic hook → `dump_on_fault` 接线(P1/P4,
  一行式见 `docs/tasks/handoff-p5.md`);`Fatal Diag` 同样入口。
- **H-P5-02**:`case --trace` 开关(case.rs,P4)。
- `yu-cli/examples/fault_dump.rs` 已写;本会话快照中 `cargo check -p yu-cli
  --example fault_dump` 被并行 `yu-backend-wgpu` 的 `RenderReport` 字段改动
  阻塞(错误在 `yu-render/wgpu/src/passes.rs`,不在本示例);wgpu 稳定后重跑。
- `DIV-L-002` 仍 open:任务生命周期两处新分歧(script 0/8 常驻、script 66
  结束/替换)已取证;script 60 返回后的转场仍需可用的官方点击样本。
- 官方 `args` 级不可观测的结论不变;任务相位是**快照**,不是 spawn/finish 事件。


---

## 成果 P3 · yu-rt 命令与系统变量取证收口(默认槽 / DIALOG / P1–P4)

> 任务书 `docs/tasks/task-p3-rt-forensics.md`。零 git 写操作;只改
> `crates/yu-rt`(src/tests/examples/data/docs)。逆向锚点 = 0.495
> `yu-ris.exe` sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`
> (REA/Ghidra 12.1.4 + 本仓库 `target/tmp/disasm.py` 区间反汇编)。

### 1. P0-A · 默认处理体 `0x459e90` 定案(DIV-RT-043)

- **分发表**:`FUN_0045cf90` `0x45d1ae..0x45d1c0` 先用
  `MOV [EAX+0x7ace20], 0x459e90` 填满 119 槽(`0x1dc/4`),再逐槽覆盖。
- **默认体**原始字节(REA `read_bytes 0x459e90`):
  `6a 01 68 d2 bb 0d 00 e8 6c be 00 00 83 c4 08 b8 ff ff ff ff c3`
  = `push 1; push 0xdbbd2; call 0x465d08; add esp,8; mov eax,-1; ret`。
- `0xdbbd2` 经 `0x465dec` → `0x465e30` 查 `ysbin/yse.ybn`;工程 yse 解出
  `「[内部エラー]\n\nスクリプト実行エラーが発生しました。」`。
  `FUN_004612b0` 用全局 `[0x5e4f84]` 抑制重复错误框。
- 结论:**不是 no-op**:官方报一次脚本执行错误 + 返回 -1(本帧让出),
  pc 已前进、任务不终止。
- **no-op 槽**:`0x4634d8` = `xor eax,eax; ret`,覆盖写地址
  `0x7ace44/0x7acef0/0x7acf4c/0x7acf58/0x7acf60/0x7acf64/0x7acf68`
  ⇒ DEBUGLIST/LABEL/PREP/RETURNBREAK/S_FLT/S_INT/S_STR。
- **静态 vs 运行**:F_INT/F_FLT/F_STR 静态 337/72/64,全部在
  `global_f.yst`(Sprite/Sound/Backlog)声明段;入口标签指向声明段之后,
  600 帧运行命中 0。扫描器不改,两口径并列记录。
- **内核**:新增 `HandlerKind::Noop` / `ScriptError`;`F_*`/`G_*`/`F_VAR`/
  `S_VAR`/`G_VAR`/`MACRO`/`_IFDEF`/`_IFNDEF`/`_IFEND`/`);`/
  `SYSTEMMODE`/`COMPILEMODE`/`RELEASEMODE`/`PROJECTFOLDER` 走
  `rt.script.error`(仅首次进事件流)+ `Yield`;`S_INT/S_FLT/S_STR` 改
  no-op;`script_slot_hits()` 计数全部命中。

### 2. P0-B · DIALOG 完整契约与状态机(DIV-RT-044)

- 三层:手册 `other/dialog.html`;语料 17 处(实际只用
  STR/CAPTION/YESNO/LET/DEFBUTTON/INPUT/DEFSTR/LETSTR/OKCANCEL/
  YESNOCANCEL);REA `0x43fc74` + 输入模板 `0x8083c4` + 过程 `0x4403d0`。
- 关键 REA 事实:参数存在性判定不看值;非 INPUT = `MessageBoxA`;
  按钮组按存在性 YESNOCANCEL > YESNO > OKCANCEL > 缺省 OK;`OK` 槽
  从不读取;DEFBUTTON 1..4 → MB_DEFBUTTON1..4;ICON 1=Info/2=Question/
  3=Warning/4=Error;LET = 1 基按钮序号(Yes/OK=1,No/Cancel=2,
  YesNoCancel-Cancel=3);INPUT 走自绘对话框,INPUTLIMIT>0 时
  EM_SETLIMITTEXT,OK 后文本写 LETSTR、LET=1。
- **内核**:`src/dialog.rs` + `Vm::pending_dialogs()` /
  `Vm::answer_dialog(id, button, text)`;DIALOG 使命中任务挂起
  (`TaskExecutor::dialog_pending`),回答后下一逻辑帧继续;事件仍为
  `DomainCall{System,"DIALOG"}`(通知,权威状态在 pending 队列)。
- 未取证参数(STR2..8/A*/E*/CHECK*/FILEOPEN/FILEEXT/FOLDER/CHECKBOX/
  SLIDER/NOWAIT)⇒ `Unimplemented`,不弹窗、不写回。
- 与 `_WINDOW_EXEC_CLOSE` 的交互:对话框挂起期间宿主把关闭请求当取消,
  不再推 `_WINDOW_EXEC_CLOSE`(写进 `domain-event-contracts.md` §8.4)。
- 宿主呈现面列交接件(P1/P4)。

### 3. P1 / P2 · 其余命令

| 命令 | 结论 |
|---|---|
| `TRAY` | `Domain::System` 事件(宿主托盘图标;REA `0x453a4c`) |
| `CLIPACT` | 内核镜像 + 事件;TEXT+SET 都存在才生效(REA `0x43e2d0`) |
| `CLIPINFO` | 读镜像写 LET;过滤 `<0x20`(REA `0x43e410`) |
| `REGINFO` | `Unimplemented`(需宿主注册表) |
| `YMV`/`YMVEND` | `Domain::Movie` 事件;`YMVINFO` 需宿主媒体元数据 |
| `ERROR` | 写逐命令启用表 `[0x689b00+opcode]`(默认全 1);消费者未定位(DIV-RT-046) |
| `CSV`/`PLUG`/`ERRORINFO` | 官方 = 参数求值 + 返回 0 ⇒ no-op |
| `MATH3D` | 代码已实现;本表旧「未取证」作废 |

### 4. P3 · 系统变量

- `_TEXT_FLAG` 写:REA `0x446b98..0x446bb5`(id 0x7b)= RHS 逐字节拷进
  `[state+0x3c]`,不重建文本/不重算标志 ⇒ `set_text_flags`。
- `_TEXT_NUM`:`0x4276e3` ← `[0x80174c]`;装载 `yst.ybn` 时由头部
  text_total 写入(`0x45d73f`)。ERIS `yst.ybn` text_total=396 ==
  `yst_list` 141 条 text_count 求和。`Vm::text_total()`。
- `_WINDOW_INIT_POS`:`0x4276f3` ← `[0x80133c]`;写点 `0x460b74/0x460b87`
  来自 `yscfg.dat` 第 10 个 int ⇒ `BootConfig::window_init_pos`(ERIS=0)。
- `_WINDOW_EXEC_CAPTION`:维持宿主 setter 结论;新增
  `Vm::request_window_caption()`(置 1)+ 脚本写回 0 的 LET 路径。
- `HANTOZEN`/`ZENTOHAN`:从 0.495 exe 取三张表(A `0x5b5170` 95 项、
  K `0x808840` 63 项、S `0x5b5230` 126 项),落
  `data/case-tables-0495.tsv`;算法逐条对照 `0x4546aa..` / `0x4548cf..` /
  `0x46b327` / `0x46b395`。单测覆盖 ASCII/半角片假名/反向。
- `MATH[RND]`:REA `0x448571..0x4485b7` = `min + rand32 % (max-min+1)`,
  反向交换;MT19937(`0x465f64` 种子常数),新增 `BootConfig::rng_seed`
  (缺省 5489)与 `src/rng.rs`。

### 5. P4 · 事件契约

- `DomainCall` 新增可选 `script`/`pc`(内核发出总是 `Some`);JSON 仅在
  `Some` 时输出。yu-trace 的映射已有 `..`,无需改动。
- `ArgValue` 变量引用身份仍按 `DIV-RT-042` 只写提案,不实现(跨 crate)。

### 6. 验证(原始输出)

> 环境注记:本机 `project-eris/data` 已打包成 `data.ypf`,散盘语料用
> `extract_ypf` 解到 `target/tmp/p3proj/data`,并以
> `YURIS_SAMPLE_ERIS_YSBIN=target/tmp/p3proj/data/ysbin` 跑真实语料测试;
> 否则 `tests/consistency.rs` 的缺样本路径会 panic(既有 skip 助手缺陷,
> 不在 P3 范围)。

```text
$ cargo test -p yu-rt
test result: ok. 64 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 63 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
$ cargo run -p yu-rt --example coverage_census -- 600 8
- 运行: `SYSTEM_START`,tick 0..600 共 600 次,逻辑帧 602;…
- 合计: 命令 7735679 次(报告累计 0),事件丢弃 0,文本 0 条 / 0 字节
运行时未实现:LET=1(仅 _SAVE_PATH);ERROR=0;_WINDOW_EXEC_CAPTION 诊断=0
```

首帧金丝雀:`1_292_091 → 1_292_100`(+9;`_WINDOW_INIT_POS=0` 使
`es_GSD.yst:490` 初始化分支首次成立;第 2 帧 272,296 不变)。
逐命令计数**无回退**:`LET` +2、`IF` +2、`IFEND` +2、`ELSE` +1、
`IFBLEND` +1、`WINDOW` +1(同一分支);其余命令与基线一致或只降
「未实现」计数。
`cargo fmt -p yu-rt -- --check` clean;
`cargo clippy -p yu-rt --all-targets --no-deps -- -D warnings` clean。

证据产物(仅 target/tmp):`default-handler.asm`、`dialog-handler.asm`、
`p1-handlers.asm`、`p2-handlers.asm`、`sysvar-write2.asm`、
`census-baseline.txt`、`census-final.txt`、`disasm.py`。

### 7. 交接 / 仍挂账

- **H-P3-01**:DIALOG 宿主呈现(原生/Web 选型)与窗口关闭路由(P1/P4)。
- **H-P3-02**:`REGINFO`/`YMVINFO`/`CSVINFO` 需宿主能力。
- **H-P3-03**:`ArgValue` 变量引用身份(DIV-RT-042)。
- `ERROR` 启用表的消费者未定位(DIV-RT-046 open)。
- `_WINDOW_EXEC_CAPTION` 触发条件(`*ecx` 来源)未取证;VM 只接受宿主
  setter(DIV-RT-007/047)。
- `_TEXT_NUM` 以 `yst_list` 求和代替 `yst.ybn` 头部;两者在其它工程若
  分歧,需 `yu-format` 提供 YSTD 解码后再切(DIV-RT-048)。
- `DIALOG` 扩展参数(STR2..8/A/E/CHECK/FILEOPEN/FOLDER/CHECKBOX/
  SLIDER/NOWAIT)未取证。

## 成果 P4 (2026-10-23) · 存档与文件域宿主 `yu-save` + YSSD 容器逐字节写路径 + log 门面桥

> 任务书 `docs/tasks/task-p4-save-domain.md`。净室实现;零 git 写操作。
> 与并行会话同树工作:期间 P3 契约(`DomainCall.script/pc`)与文本渲染
> 重构先后落地,本成果在其上验证。

### 1. M-E · log 门面消费(排障刚需)

- `yu-log` 新增 `facade.rs`(加法,`log = "0.4"` 唯一新依赖):
  `log::Record` → JSONL 一行(`{"sub":"log","level",…,"target","file","line","message"}`,
  复用本包 JSON 转义)+ 控制台一行(回调注入;`yu-log` 不碰终端层)。
  级别由 `YU_LOG` 控制(`error|warn|info|debug|trace`,缺省 `warn`;
  不解析 `RUST_LOG`);`YU_LOG_JSONL` 给 JSONL 路径(进程内创建/截断)。
- `yu-cli`/`yu-engine` 两个 main 入口初始化(先于引擎 boot);wasm 宿主不初始化。
- **实测**:强制 `YURIS_WGPU_ADAPTER=__NO_SUCH_ADAPTER__ --backend wgpu`,
  `wgpu_hal::vulkan::instance` 的 2 条 warning 同时出现在控制台与
  `target/tmp/log-adapter-warn.jsonl`(带 file/line);`YU_LOG=error` 时
  双通道均为 0 行;`YU_LOG=info` 时可见 `wgpu_hal::vulkan::adapter` 枚举信息。

### 2. M-A · YSSD 容器:官方存档自证(真值层级 3)

- 布局(证据:官方 0.495 产物 + 旧线 `<repo-old>` v494 反汇编事实,只引事实):
  头 `YSSD + 0x1E0 + 块数 + 0x400` + 偏移表 `0x400×u32`(表项[i]=块 DNO=i+1
  绝对偏移,0=空);块 = `dno u32 + 类型 u8 + 严格维数 u8 + 变量 id u16 +
  压缩长 u32 + 未压长 u32 + 载荷`。载荷:`ty u32(1=INT 2=FLT 3=STR) +
  维数 + 边界[] + 数据长 + 数据`(INT/FLT 8B/元素;STR 逐元素 {u32 长 + 字节})。
  type1=配置块(段A/段B,结构往返);type2=多变量块(**外层裸**,子块各自
  zlib,`pad` 恒 `0x0100`;外层 `ulen = Σ(子块 ulen + 12)` —— ERIS 实测,
  旧线「clen=ulen=内层长」在 ERIS 不成立)。
- **压缩层**:0.495 全部产物为 zlib level 1(`78 01`)。**libz-sys 钉版
  `=1.1.22`(vendored zlib 1.3.1)**:1.1.30 的 zlib 1.3.2 deflate level-1
  输出与官方产物不同(实测);flate2 `zlib` 后端的 deflateInit2 参数
  (memLevel 8/默认策略/15 位窗)与 `compress2(level=1)` 同参。
- `yu-format/tests/yssd_roundtrip.rs`:官方产物**快照**
  (`target/tmp/yssd-corpus/`,5 文件 269 块)parse → rebuild **逐字节一致**
  (extra 6 / gsd 4 / kidoku 1 / save50001 257 / seld 1;含 264 个值载荷与
  全部 type1/type2 块)。快照原因:`<project>/save/` 是官方 exe 活动写盘位
  (本轮实测 extra/gsd 在 12:24 被运行中的官方 exe 重写、save30001/30002
  消失),移动靶不能当 golden。
- 勘误留痕:AGENTS.md 的 `YURIS_SAMPLE_ERIS_YSBIN=project-eris/data/ysbin`
  已失效(该目录不存在);实际在 `project-eris/save/data/ysbin`(官方 exe
  解包产物,成果 N 续 2 同口径),本 rounds 测试均按此设置。

### 3. M-B · 新 crate `yu-save`

- `error.rs`:`sv.*` 码表 14 条(唯一性单测);`state.rs`:`SaveState`
  (槽表=盘上事实 + SAVE/LOAD/SAVEEND/SAVEINFO/FILEACT/FILEINFO 事件
  事实消费,参数名经 `domain_args`);`host.rs`:`SaveHost`(**不加
  `yu-rt` trait** —— yu-rt 本轮冻结;由 Engine 每帧 drain 后喂入,事实
  记录容忍一帧延迟,契约文档 §6 前提)。
- **写侧门禁(不猜)**:`SAVE[FILE]` 刷盘显式拒绝(`sv.write.refused`):
  `SET/LET` 变量引用身份在事件层丢失(DIV-RT-042),`var_id` 与整组数组
  内容拿不到,写不出与官方一致的块;容器写路径本身已逐字节就绪(M-A)。
  `FILEACT[EXEC]` 拒绝(`sv.exec.refused`);FILEACT 落盘仅在**显式可写
  save 根**下允许(默认只读观察,保护语料目录);路径逃逸检查
  (`sv.path.escape`)。SAVEINFO 挂账(REA 0x44e1c8 未取证,语料 0 次);
  FILEINFO[EXIST] 不双写(内核 J3 已实现)。

### 4. M-C · Engine 接线

- `EngineOptions.save_root: Option<PathBuf>`:`None`=工程根 `save/`
  (只读观察);`Some(dir)`=可写根(cases 注入 `target/tmp/save-cases/`)。
  `$_SAVE_PATH` 跟随该根注入。事件 drain 改为取内容后喂 `SaveHost`。
- `cases/save-title-flow.json`(600 帧,标题流):**PASS**;expect 新增
  `save_state_sha256`(`SaveSnapshot` 确定性文本:槽表+预约+域事件计数;
  case 启动时用 `target/tmp/yssd-corpus` 快照播种 save 根)。case 期间
  语料工程目录零写入。
- census 不回退:`SAVE 16 / LOAD 0 / FILEINFO 1136(已实现) / FILEACT 3 /
  SAVEEND 3`,与 `coverage-matrix.md` 完全一致。

### 5. M-D · P1 交接件

- **H-P1-01**:engine.rs 三处 `sound.lock().unwrap()` 改
  `unwrap_or_else(into_inner)` 中毒降级(SaveHost 同口径)。
- **H-P1-02**:`Engine::take_diag_report()` → `DiagReport {frame,
  waiting_tasks(等待任务面), unimplemented_cmds(挂号表), script_errors,
  diags(内核+sv+snd 三路 drain), sound_layers, save_* 计数}`。
- **H-P1-03**:`serve --diag-ws` 开 `/diag` WebSocket(500ms 推快照,
  关闭即 404)。**实测**:握手 101;快照含 `waiting_tasks`/
  `unimplemented_cmds`/域计数,frame 52 时 `save_calls=16, saveend=3,
  fileact=3` 与 census 一致(实时闭环:VM 事件 → Engine drain →
  SaveState → WS)。P3 的 `DomainCall.script/pc` 已在 `/diag` 的
  `waiting_tasks`(script/pc 字段)透出。

### 6. 验证原始输出

```text
$ cargo test -p yu-save -p yu-format -p yu-log -p yu-cli --lib
（全部 ok:yu-save 6、yu-format lib 23、roundtrip 1、yu-log 17、yu-cli 10）

$ cargo test -p yu-format --test yssd_roundtrip -- --nocapture
extra.sd: 6 块,roundtrip OK … seld.sd: 1 块,roundtrip OK
合计: 269 块 / 264 值载荷,全部逐字节一致

$ yu-cli case <project-eris> --all
5 passed; 2 failed
（2 failed = 并行会话的 symptom-enter-story / symptom-ingame-config
  「没有 expect;先跑 --update」,其 WIP;本任务 5 条含 save-title-flow 全 PASS)

$ coverage_census 600 8
SAVE 16 / LOAD 0 / FILEINFO 1136 / FILEACT 3 / SAVEEND 3(与矩阵一致)

$ serve --no-open --diag-ws + WS 探针
frame 52: save_calls=16 saveend=3 fileact=3 slots=5 waiting=5

$ cargo clippy -p yu-save -p yu-log --all-targets
0 warning(yu-cli 余 5 条警告均在 yu-sound/soft/gfx 既有或并行改动文件)
```

### 7. 挂账(不猜)

- SAVE/LOAD 的**忠实写盘与 LET 写回** ⇒ DIV-RT-042(引用身份)关闭后
  闭环;容器侧已就绪。
- SAVEINFO 查询(REA `0x44e1c8`)、FILEACT 图层导出与 SAVE[ID]
  (⇒ H-P4-02,P2/集成轮)、YSSD SNP 形态(本语料未出现)、
  FILEINFO SIZE/RELATION。
- 工程注记:libz-sys `=1.1.22` 是**持久钉版**(zlib 1.3.2 起 deflate
  level-1 输出变化);`yu-gfx` 首屏 golden 在本树存在**先于本任务**的
  漂移(pass10 `copy` 271→3,INPUT 闭环提交之后,非本任务改动;
  需 Owner 复核后 UPDATE_GFX_GOLDEN)。

## 成果 P2 · 文本链路落地(gfx 文本模型 + 两后端字形执行,2026-10-23)

> 任务书 `docs/tasks/task-p2-text-pipeline.md`;只动 `yu-gfx` / `yu-font` /
> `yu-render/{soft,wgpu}`;`yu-rt` / `yu-cli` 未改(契约变更写 H-P2-01/02)。
> 净室:全部新写,事实只引用手册/REA/真实语料。

### 1. 结论(先看这里)

- **进本篇的对话文本可见**:ERIS 正文不是自动文本层,而是每字符
  `CGACT[ID=<文本层> TEXT=1 SET=0/1 SETSTR=<char> X Y]`(`es_Font.yst`)。
  P2 实现了该路径:gfx 产出 `TextDraw` → 后端把文本合成到图层基图副本 →
  按图层 `A/MODE/Z` 合成。soft/wgpu 同帧逐像素一致。
- **自动文本层也落地**:`TEXT[ID]` + `WORD`/`_`/`RETURNCODE` 的文本按 VM 的
  `_TEXT_FLAG` 逐字 `'1'/'2'` 排版(半角 `SX/2`、全角 `SX`),不另造布局;
  文本缓冲跨帧持久(`TEXT[CLEAR]` 经宿主同步传播),不再每帧清空。
- **title 600 帧 0 差异**:soft 与 wgpu 各与改动前基线逐帧 SHA-256 相同
  (600/600),两端互为 0 差异;文本代码对无文本帧完全惰性。

### 2. 改动

`crates/yu-gfx`
- `text.rs`:新增 `TextDraw`(`CGACT[TEXT]` 绘制项)、`TextLayoutFrame`/
  `TextGlyph`/`TextFlagsSource`、`layout_text`(唯一 `_TEXT_FLAG` 排版)、
  `TextFontRequest`(`NAME`>`NO`>`BNO`>默认);
  `TextStyle` 增加 `gradation`/`rotation`(默认 1/0)、引擎默认 `SX=SY=20`、
  `COLOR2=0xffffff`、`clamp()`、`cell_width()`、`line_height()`。
  `TextRun` 增加 `flags: Option<Vec<u8>>` / `reveal: Option<usize>`。
- `state.rs`:文本缓冲改为**跨帧** `BTreeMap<LayerId, TextBuffer>`;
  `FrameStart` 不再清空;`sync_text_state(&TextSnapshot)` /
  `sync_text_state_for` / `clear_text` / `set_text_reveal`;`CGACT[TEXT]`
  解析为 `text_draws`(不进 `ImageOp` 计数);`CGEND`/`CG(FILE)` 重建清层;
  `FONT` 保存 + clamp GRADATION/ROTATION;`FONTINFO` 写回
  `COLOR2/GRADATION/ROTATION`;`frame()` 输出 `text`/`text_layout`/`text_draws`。
- `frame.rs`/`json.rs`/`lib.rs`:契约与 JSON(`flags`/`reveal`/`text_layout`/
  `text_draws`;空时不输出,保持无文本帧 golden 稳定)。

`crates/yu-font`
- `cp932.rs`:`decode`/`decode_char`(WHATWG shift_jis,两后端共用);
- `FontSource::ascent_px`(单元顶→基线,`Likely`);
- 新增 `encoding_rs` 依赖(wam32/Android 可用)。

`crates/yu-render/soft`
- `text.rs`:`TextComposer`(懒加载系统字体 + 字形缓存 + CP932)、
  `paint_draws`/`paint_layout_with`/`compose_layer`、`layer_base`;
  `SET` → 1/9/3 次合成(REA `FUN_004056b8`,`Likely`);
- `render.rs`:图层文本合成到基图副本(override 路径);屏幕级自动文本走
  同一 `draw_image` 合成(与 wgpu 同公式);`RenderReport` 增加
  `text_draws/text_glyphs/text_missing/text_diags`;
  `frame_has_drawable_output` 把文本算作可见输出。
- `examples/text_probe.rs`:§7 P0 方法(固定时钟 + 定时点击 + 每帧
  `vm.text_state()` 同步),输出 TSV + PNG + `display.json`。

`crates/yu-render/wgpu`
- `passes.rs`:plan 阶段先做文本覆盖(基图/已加工覆盖纹理 → `TextComposer`
  → 上传/缓存),屏幕级文本追加一个全视口覆盖 quad;报告字段与 soft 同构;
- `lib.rs`:持有 `TextComposer`,`with_text_source`;`plan` 接收文本合成器;
- `examples/wgpu_text_probe.rs`:soft/wgpu 同帧逐像素对拍。

### 3. 证据(全部一手)

| 事实 | 证据 | 等级 |
|---|---|---|
| `CGACT[TEXT=1 SETSTR=.. X Y]` 在图层上绘字符串 | 手册 `cg/cgact.html` 例 25/26 | Confirmed |
| ERIS 正文逐字按 `_TEXT_FLAG` 的 `'1'/'2'` 算 `SX/2`/`SX` | `ERIS/Text/es_Text.yst::es.TX.BUFSET.SIZE` | Confirmed |
| `SET=1` 轮廓 / `SET=0` 填色;同位置先 `SET=1` 再 `SET=0` | `ERIS/es_Font.yst`、`es_System.yst:311` | Confirmed |
| `SET` → 合成 1/9/3 次 | REA `FUN_004056b8` @ `0x4056b8`(0→1,1..10→9,≥11→3;AA/非 AA 分支) | `Likely`(映射) |
| 逐字符 GDI mask,无 kerning | REA `FUN_00445421`/`FUN_004450c0`(font-provenance §3.3) | Confirmed |
| 字体默认 family / 默认样式 | `font-provenance` §2;REA `FUN_00464fac` | Confirmed |

新分歧档案:`DIV-GFX-055`(图层合成模型/SET)、`DIV-GFX-056`(行距/基线)、
`DIV-GFX-057`(无图层屏幕叠加)。交接件:`H-P2-01`(事件 ID 字节)、
`H-P2-02`(VM reveal API)、`H-P2-03`(官方文本截图 oracle)。

### 4. 验证原始输出

```text
$ export TEMP=<side-repo>/workbench/tmp YURIS_SAMPLE_ERIS_ROOT=<project-eris>
$ cargo test -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu
28 个 test result: ok;0 个 FAILED(含新用例:
  text_pipeline 10、soft render 12(新增 2)、wgpu text_layout_frames_match_soft_exactly、
  yu-font cp932 3)
$ cargo clippy -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu --all-targets
0 warning
```

P0 文本场景(soft,`text_probe`;列:bytes/flags/glyphs/last_x/x_extent/report_glyphs/ink):

```text
SCENARIO_EPILOGUE: entry=SCENARIO_EPILOGUE ticks=40
tick  bytes flags glyphs last_x x_extent report_glyphs ink  hash
0     12    10    6      100    120      6             550  b92091e4…5513a
(1 行「エピローグ。」6 字全角,可见;直到点击才清)

RP_101: entry=RP_101 ticks=130 click=10
tick  bytes flags glyphs last_x x_extent report_glyphs ink  hash
99    132   106   66     140    160      66            7562 c55a8dc5…5080d
# main finished at tick 99 ; # text_events=10
```

soft/wgpu 同帧对拍(`wgpu_text_probe`):

```text
RP_101(100 tick,10 行):rows=100  mismatch=0(soft_hash == wgpu_hash 每帧)
末帧 66 glyphs / ink 7562 / c55a8dc5…5080d(两端相同)
SCENARIO_EPILOGUE(40 tick):0 mismatch,6 glyphs / ink 550
```

600 帧 title 回归(`frame_walk` / `wgpu_frame_walk`,与改动前基线比较):

```text
soft  0 / 600 差异
wgpu  0 / 600 差异
soft vs wgpu 0 / 600 差异
```

### 5. 挂账

- `SET` 映射、行距/基线、无图层叠加均为 `Likely`;官方文本截图对拍挂
  H-P2-03(YSCom attach 恢复后)。
- VM 侧 reveal/已显字符数 ⇒ H-P2-02;`GfxState::set_text_reveal` 与后端
  截断已就绪,但宿主暂不注入(不自建计时器)。
- `VmEvent::Text.id` 非 UTF-8 CP932 ⇒ H-P2-01(模型已用 `LayerId` 字节)。
- `CGACT[TEXT]` 目前按"每帧绘制项"建模(等价于 ERIS 每帧重画);若出现
  只画一次依赖跨帧保留的脚本,需改为跨帧累积(DIV-GFX-055)。

### 6. 注记(风险/清理)

- 修复示例输出路径的 `ancestors().nth(2)`(实际写到 `crates/target/tmp`)
  时,删除了 `crates/target/`;其中历史证据(`j3-*` TSV、walk PNG)全部
  可由 `frame_walk` 示例复现,但**不是本任务应当删除的内容**,留档。
- 并行会话正在改 `yu-rt`/`yu-cli`/`yu-platform`;本轮基线(会话开始时)
  全绿,期间其 WIP 曾使 `yu-rt` 无法编译,已避开;P2 只对自有 crates 负责。

### 7. 成果 P2 补记(收尾)

- **H-P2-04**(新):宿主每帧把 `Vm::text_state()` 同步进 `GfxState`
  (`yu-cli::engine::tick_inner` 3 行;`yu-cli` 不归本任务)。不接线时
  `CGACT[TEXT]` 正文路径不受影响,只有"自动文本层"不显示
  (`docs/handoffs/H-P2-04-host-text-sync.md`)。
- 跨后端自动化回归补一条:`cgact_text_layer_frames_match_soft_exactly`
  (图层文本合成,soft/wgpu RGBA 逐字节一致);加上
  `text_layout_frames_match_soft_exactly`(屏幕覆盖)共两条。
- `yu-font` 跨平台门禁复核:`cargo check -p yu-font --target
  wasm32-unknown-unknown` / `aarch64-linux-android` 均通过
  (`encoding_rs` 0.8 两个目标都可编译)。
- `yu-font` 源码 `println!/eprintln!/dbg!` = 0(契约 §9)。
- 收尾复跑:四 crate `cargo test` 全 ok(28 个 test result,0 failed);
  `cargo clippy --all-targets` 0 warning;title 600 帧 soft/wgpu 与基线
  各 0/600、互为 0/600;`RP_101` 100 tick 文本对拍 0 mismatch,
  `SCENARIO_EPILOGUE` 40 tick 0 mismatch。

## 成果 P1 (2026-10-23) · 症状工单与宿主稳定层:panic 防线、诊断面、S6 修复

> 任务书 `docs/tasks/task-p1-symptom-host.md`。范围:`crates/yu-platform/host-desktop`、
> `crates/yu-platform/input`、`www/`、`cases/symptom-*`、新建 `docs/symptom-ledger.md`。
> `yu-cli`/`yu-gfx`/`yu-rt`/`yu-trace` 只读(需求写成交接件)。零 git 写操作。
> 本会话与 P2/P3/P4 并发;主树有部分时段不可编译(见 §2.6)。

### 1. 交付

| 项 | 位置 | 说明 |
|---|---|---|
| 症状台账 | `docs/symptom-ledger.md` | 7 个剧本 + panic 自检 + `serve` 首帧,共 12 行档案、8 个交接件 |
| panic 防线 | `host-desktop/src/panic_guard.rs` | 进程级 hook(消息/位置/线程/backtrace 摘要)+ tick/draw 双路径 `catch_unwind` + 冻结 + 红色横幅 + Esc/Quit |
| 诊断账本 | `host-desktop/src/diagnostics.rs` | Diag 聚合(码/次数/首末帧/位置)、skipped top-N、Unimplemented top-N、输入计数、四路“为什么卡” |
| 面板 | `host-desktop/src/ui.rs` | F1 四段 + 红/琥珀横幅;egui 0.37 `Panel`/`root_ui`;无头单测断言填充色与文案 |
| 场景脚本 | `host-desktop/src/script.rs` + `cases/host/` | 按逻辑帧注入 input/pulse/resize/minimize/restore/focus_lost/close;S1–S7 七条 |
| 日志 | `main.rs` | `yu_log::facade::init`(P4 M-E);`--log-file` > `YU_LOG_JSONL` > `%TEMP%/yu-desktop-host.jsonl`;panic 与 wgpu/winit 同 sink |
| Web 对等 | `www/index.html` + `app.js` | overlay 新增 `diag` 行(只消费现有帧字段);新通道列为 H-P1-03 |
| 用例 | `cases/symptom-{enter-story,ingame-config,ingame-save}.json` | 进 `case --all`;宿主脚本另在 `cases/host/` |
| 证据工具 | `examples/scenario_probe.rs`、`examples/serve_probe.rs` | 逐帧画面 SHA/Diag/PNG;WS 场景回放(证据产物只进 `target/tmp`) |

### 2. 关键实测结论(详见台账)

1. **黑屏/无字的主因不是文本链路**:点 `スタート` 后 422 帧
   `text_runs=0 / skipped_text=0` ⇒ 没有 TEXT 被跳过;同一帧起背景全黑。
   真正上游是 `rt.expr.div_zero` @ `es_select.yst:es.ScenarioScript@153 LET`
   (Fatal),背景/文本产出任务终止。文本链路缺口(DIV-GFX-012/033/034/035)仍在,
   但不是本次黑屏的直接原因(勘误 §7.3)。
2. **本篇底栏不是全灭**:`設定`/`タイトル`/`終了` 正常(脚本自绘 UI);
   `終了 → はい` 正确 `main_finished=true`。无反应的是:
   右键菜单、点文本框推进(任务已死)、`セーブ`。
3. **`セーブ` 双缺陷**:界面 op(`SLTEMP`/`CGS100` rectpaint2)不落屏
   (像素 SHA 不变),随后 `es_number.yst:SYSTEM@56` 第二次整数除 0 ⇒
   **主任务 END**;桌面宿主按 END 语义直接关窗(实测 frames=440、exit 0)。
4. **挂机黑屏**:桌面无输入 ~8.5s,标题演示任务命中同一处 Fatal,末帧纯黑;
   该触发依赖 `tick_realtime`(墙钟),固定步 `tick()` 600 帧不复现,且跨运行
   非确定(4 次:223/281/无/无)。3600 帧 release 复核:演示在
   `frame≈1295` 启动但未命中 Fatal,末帧回到标题、无 panic。
5. **标题“終了する”** 与“窗口关闭确认”走原生 `DIALOG`:VM 已挂起
   (`pending_dialogs`),`Engine` 未透出 ⇒ 变暗后无对话框、静默挂起;
   第二次关闭由宿主兜底退出。
6. **最小化→还原** 原会触发 wgpu 校验错误(`set_scissor_rect` 960x50 vs
   target 1x1):宿主 bug,**已修**(最小化跳过绘制 + draw 前同步 surface 尺寸)。
7. **10 分钟 `serve`** 连续会话:36,646 帧、0 回退、0 断流、
   `tick_avg 4.95ms / render 1.76ms`、音频首块非静音;首帧 1.858s 空白
   (`serve` 未设步长上限 ⇒ H-P1-05)。

### 3. 交接件(全部写进台账 §4)

`H-P1-01` 音频锁中毒(P4 已并发落地;P1 复核:缺一次性 Diag)、
`H-P1-02` Engine 公开 API(`pending_dialogs`/`answer_dialog`/`waiting_tasks`/
`key_last`/`mouse_last`)、`H-P1-03` `serve --diag-ws`、`H-P1-04` DIALOG 呈现、
`H-P1-05` `serve` 步长上限、`H-P1-06` `case` 的 `close` 动作、
`H-P1-07` `es_select:153` + `es_number:56` 整数除 0(P3/P4)、
`H-P1-08` 存档界面合成不可见(P2 gfx + P4 save)。

### 4. 验证(原始输出)

```text
$ cargo test --workspace --no-fail-fast        (树恢复后,主树)
passed=647 failed=0                            (exit 0)

$ cargo test -p yu-host-desktop -p yu-platform-input
21 passed (lib) + 3 passed (input) + 0 failed + 1 ignored(tests/input_host.rs)

$ cargo test --release -p yu-host-desktop --test input_host -- --ignored
test sub_frame_click_and_pulse_survive_a_single_host_frame ... ok   (17.83s)

$ ./target/release/yu-cli.exe case <project-eris> --all
8 passed; 0 failed

$ cargo clippy -p yu-host-desktop -p yu-platform-input --all-targets
0 warning / 0 error(仅 yu-sound 既有 1 条 manual_checked_division,与本线无关)

$ rustfmt --check <本任务 10 个 .rs>
0 diff
```

主树 release 宿主回放(全部 exit 0,无 PANIC):

```text
panic selftest : frames=120  panic@40 被接住 → frozen → 到限退出
                 JSONL: "panic thread=main … backtrace= 0: …" + "exit frozen: …"
S1 settings/back: frames=520  fatal=None
S2 enter-story  : frames=470  FATAL es_select:153 @381;末帧与 case PNG 哈希一致
S3 ingame-buttons: frames=620 FATAL es_select:153 @381(其余按钮仍可点)
S4 save-load    : frames=440  FATAL es_number:56 @442,main_finished=true
S5 close-twice  : frames=429  close#1 → @_WINDOW_EXEC_CLOSE=1 / close#2 → exit
S6 stress       : frames=720  resize/minimize/restore/focus_lost 全执行,无 panic
S7 serve 10m    : 36,646 帧 / 0 回退 / tick 4.95ms / render 1.76ms
S7 desktop idle : frames=3600 elapsed=93.2s avg_fps=38.6,无 Fatal/PANIC,末帧标题
serve 动作回放  : 设定→戻る→スタート→本篇設定 全部生效(472–481/550–551 有变化)
```

隔离快照(主树不可编译期间,HEAD + 仅本任务文件):24 passed / 0 failed。
另:`cargo check -p yu-host-desktop --all-targets`(主树)在树恢复后 0 error。

### 5. 勘误

1. **`AGENTS.md` §2 样本路径失效**:`YURIS_SAMPLE_ERIS_YSBIN` 实为
   `<project-eris>/save/data/ysbin`;`YURIS_SAMPLE_SDK0495/0479` 需传 SDK
   **根目录**(测试会再拼 `システム/system/...`)。改正后基线由 2 target 失败
   变为 559/0。
2. **AU2 “`src/` 无 stdout 宏”不成立**:`yu-cli/src/engine/sound_device.rs`
   有两处 `eprintln!`(P4 范围)。
3. **任务书 §1“黑屏=文本链路”未证实**:见 §2.1。
4. **本篇按钮全灭不成立**:见 §2.2。
5. `egui 0.37` 的 `Window` 首遍只测量(面板晚一帧);`Panel` 首遍即绘制。
6. 主树期间被并发改动带出两个非本线问题:①`flate2 → libz-sys` 需要显式
   `INCLUDE/LIB`(本机 cc 找不到 MSVC 头);②`yu-gfx` 与 `yu-rt` 的
   `DomainCall` 新字段一度互不上,导致整树不可编译(已由并发会话修复)。

### 6. 未闭合

- S2/S3d/S4/S7 的上游整数除 0(P3/P4 取证;官方 x87 语义无 trace)。
- 右键菜单分支未取证;`es_number:56` 的除数来源未取证。
- 存档界面合成(MODE 2 / offscreen 上屏)归 P2;`yu-save` 端到端未测。
- `serve` 侧无 panic 防线(仅桌面宿主);`--diag-ws` 未实现(H-P1-03)。
- 音频回调内 `SoundState::render_and_advance` 的 panic 穿 FFI(abort)未排除。

## 成果 P6 (2026-10-23) · 性能与响应修复:60Hz 节拍、启动/场景装载、零拷贝

> 会话范围:`yu-platform/host-desktop`、`yu-cli`(engine/server)、`yu-rt` VM、
> `yu-gfx`/两个渲染后端的质量清理;`yu-trace` 拆模块。
> 完整数据与复现命令:`docs/perf-report.md`;硬编码审计:`docs/hardcode-audit.md`;
> 示例收敛:`docs/examples-audit.md`。零 git 写操作。
> 语义纪律:除零 Fatal / SAVE 拒绝 / DIALOG 挂起**未改**;只登记交接。

### 1. 核心结论(600 帧,release,`<project-eris>`)

| 指标 | 修复前 | 修复后 | 变化 |
|---|---|---|---|
| 600 帧稳态 avg 帧时间 | ~24.5ms(40.9fps) | **16.665ms(60.0fps;含 2 次场景尖峰 16.92ms/59.1fps)** | -32% |
| 稳态 p50 / p95 | —(未采) | **16.65–16.66 / 17.04–17.09ms** | 60Hz 内 |
| 首帧 engine_tick | **1950.2ms** | **366–542ms** | -72~81% |
| 第 4 帧 engine_tick | 1848.1ms | **382–587ms** | -68~79% |
| 首帧脚本总耗时(采样) | 1775.6ms | 329.8ms | -81% |
| 設定打开单帧(采样) | —(旧口径误记累计) | **220.4ms / tick 264–270ms** | 逐帧窗口 |
| 設定点击端到端(未采样) | — | **283.86ms**(p95) | 仍是最大停顿 |
| 首次「テキスト」页签 render | 165ms(fontdb 130ms) | **33.4ms**(启动预热后) | -80% |
| 真实光标输入延迟 | — | avg 11.69–12.42 / p95 16.52–17.02ms;逻辑帧差 1 | ≤1 帧 |
| 启动 boot + renderer | — | 98.9 + 570.3 = **669.3ms**(含 text_warm 143.9ms) | 窗口前 |
| 首个可播放画面 | ~2.5s | **1.04–1.22s**(含字体预热) | -51~58% |

### 2. 修复项(全部有不改像素/语义的验证)

1. **固定节拍(P0-1)**
   - `schedule_next_frame` 落后时对齐未来槽位,不再 `now + frame`;
   - Windows `timeBeginPeriod(1)`(RAII),`WaitUntil` 提前 0.8ms 唤醒;
   - 结果:`--fps 1000` 对照从 42.6fps → 正常节拍(定位到 15.6ms 定时器粒度)。
2. **egui 惰性化(P0-2)**
   - 面板隐藏/无冻结/无横幅:跳过 `run_pass/tessellate/update_buffers/egui pass`;
   - 隐藏期仍排空 egui 输入队列(避免无界增长);
   - 实测 draw 0.7→0.3ms;`--shot` 像素 SHA 完全一致。
3. **零整帧拷贝(P0-3)**
   - `RenderedFrame`/`RenderOutput`/`WgpuRenderOutput`/`Engine::last_surface`/
     宿主 `last_surface` 统一 `Arc<Surface>`;软/硬后缓冲共享;
   - 每帧省 2×1.9MB `Vec` 拷贝;case/serve 输出语义不变。
4. **`serve` 诊断门控(P0-4)**
   - 无 `/diag` 订阅:只排空诊断,不构造 `DiagReport` JSON;
   - 有订阅:500ms 构造/推送一次(实测第三帧收到真实快照);
   - 无客户端 40s 仍保持 `{"ok":false}`(证明没有逐帧构造)。
5. **VM 热路径(P0-6 取证后)**
   - 首帧 81.8% 在 `VARACT`:`shift_1d` 每次 O(n) 读+clone+逐元素写;
     改 `VariableStore::shift_1d` 原地 `rotate`,语义不变;
   - `step_command` 用 `remove_entry` 去掉每命令 TaskId `String` 分配;
   - 诊断命令名改 `Rc<Cmd>`(不再逐命令复制);任务名按 `seq` 只在切换时写;
   - `VARINFO SEARCH` 直接扫槽内数据(不再逐元素 read+clone);
   - `VARACT[INIT]` ysv 投影只做一次;同掩码且 `VariableStore` 无写入时跳过
     (generation 计数,任何写入/声明/改长度都会使缓存失效)。
6. **解释器固定开销(本会话新增)**
   - 执行位表 `BTreeMap<String, ExecState>` → `FxHashMap`(每命令一次树查找
     变哈希);命令缓存 `(script,pc)` 哈希 → 按脚本的稠密 `Vec` 下标;
   - 表达式窗口预解码 + 单条字面量常量折叠(`CmdWindow.const_value`);
   - 设定打开单帧 tick **410–480ms → 264–270ms**(-35~45%),
     再次打开同样 264–270ms(命令计数不变,是纯解释器开销);
   - 通用兜底试验:`--pump-cap 100k/500k` 实测触发 `rt.label` Fatal
     (`es_button@2377 GOSUB`,DIV-HOST-002)⇒ 任意抢占不安全,默认关闭;
     通用修复走解释器吞吐,不靠限流。
7. **首次文本页签:字体预热**
   - 第一次「テキスト」页签 render 165ms,其中 **130ms 是 `fontdb` 首次
     枚举系统字体**;`EngineOptions.warm_text` 在桌面宿主/serve 启动时预热;
   - 首次文本页签 render 165→**33ms**,再次进入 11ms;
   - 启动 +144ms(479→669ms),把一次性的字体枚举从游戏中移到启动阶段。
8. **质量清理(P1)**
   - `soft/text.rs` BNO 未取证分支:保留默认字体行为,但按帧去重报告
     `text.font.bno_fallback`(DIV-FONT-007);
   - `sx/sy=0→20`、`color2` 回退补证据锚点注释;
   - `yu-gfx/state.rs` 4 处 `expect("checked")` 改为带上下文 Diag(不 panic);
   - 删除 `engine.rs` 重复注释;`fault_dump.rs`/本会话文件过 rustfmt;
   - `yu-trace/fault.rs` 拆为 `fault/{mod,ring,dump,replay}.rs`(纯搬运,
     公开路径 `crate::fault::*` 不变,46+ 测试全绿)。

### 3. 门禁(原始输出)

```text
$ cargo test -p yu-rt -p yu-host-desktop -p yu-cli -p yu-trace
所有 test result: ok(含 yu-rt 11 passed 41.99s;yu-trace 46/13/7/14/8/2/3/7)
环境:YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/save/data/ysbin,
      YURIS_SAMPLE_ERIS_ROOT=<project-eris>,SDK 根目录见台账 §5.0。

$ yu-cli case <project-eris> --all
8 passed; 0 failed; 8 total

$ scenario_probe <project-eris> target/tmp/perf/title-600.json \
    --backend soft --hash-csv target/tmp/perf/sha-soft.csv
$ scenario_probe <project-eris> target/tmp/perf/title-600.json \
    --backend wgpu --hash-csv target/tmp/perf/sha-wgpu.csv
601 / 601 行;diff 无输出(逐帧 SHA 0 差异)

$ yu-desktop <project-eris> --frames 600 --shot target/tmp/perf/ego-off.png
shot sha256 a6db617b896dfb8a2f478238c3ef190fe4b0e5e4e825bb19771c8a940d05fc72
= P1 台账 §5.4(本会话前基线)
```

### 4. 已知剩余(P0 结论,不在本会话修复)

- 场景装载仍是同步突发:設定 tick 220 万 LET + 140 万 IF;
  单命令 0.27–0.30µs 是主项(候选:泵内缓存执行位、按脚本 Vec 直接索引命令、
  IF/LET 解码缓存去重哈希)。
- `SOUND` 531 次同步解码 91ms(172µs/次);需 PCM 缓存或改线程(先取证)。
- `CG` 111k 次 207ms(1.86µs/次),含缺失资产挂号;待 P2 gfx 取证。
- 首个可播放 1.0s 中 0.48s 是 wgpu/软后端对账初始化;窗口创建提前需结构改动。

### 5. 语义登记(P2,不改行为)

- `DIV-RT-003`:追加 `es_select.yst:153` / `@1118` 取证:
  `find_var_ref ... 1118 0x40` → 19 处全为读、槽 `Int[63]` 初值全 0;
  下一步核对 `LOAD[G_INT*]` 批量恢复与官方 ysv 初值。**Fatal 保留**。
- `H-P1-02/H-P1-04`(DIALOG 透出/呈现)、`DIV-RT-042`(SAVE 写盘被拒)、
  `H-P1-08`(存档界面不可见)写入 `docs/handoffs/H-P2-05-dialog-save-ui.md`,
  本会话不实现。

## 成果 P7 (2026-10-23) · DIV-RT 台账状态对账(48 条全表复审)

> 范围:仅文档(`crates/yu-rt/docs/divergences/README.md`)。零 git 写操作。
> 把 P3/P4/P6 会话已落地但台账未翻状态的条目补上状态与证据;不做代码改动。

### 1. 状态翻转(open/blocked → fixed,11 条,均有代码/测试证据)

| 条目 | 证据 |
|---|---|
| DIV-RT-001 | 处置即本体(不内置序号表);`corpus::eris_entry_chain_runs_end_to_end` 真实 0.495 语料 ok |
| DIV-RT-004 | `expr.rs` 缺右操作数容忍一次;语料全量跑通无挂号 |
| DIV-RT-010 | `BootConfig::paths` + `yu-cli host_paths()` 注入;P6 `yu-cli case --all` 8 passed |
| DIV-RT-012 | `yu-format::yssd` 官方 5 文件 269 块逐字节 roundtrip(P4);`yu-save` 宿主 |
| DIV-RT-013 | `data/case-tables-0495.tsv` 三张表 + `case_tables.rs`;`fixture::varact_hantozen_and_zentohan_use_official_tables` ok |
| DIV-RT-014 | `rng.rs` MT19937 + `BootConfig::rng_seed`;`fixture::math_rnd_uses_injected_mt19937_seed` ok |
| DIV-RT-017 | `VarSpace::LegacyNum/LegacyStr` + `canonical_var`(源码↔编译产物↔YSVR 三方证);corpus 跑通 |
| DIV-RT-023 | 四槽实现 + 处理体 `0x450478` 反汇编;手册缺页为不可修外部事实,保留条目 |
| DIV-RT-024 | §3.26 ④ 算法逐条落地;census 无回退;同上保留手册缺页事实 |
| DIV-RT-032 | `_TEXT_FLAG` 写入 REA `0x446b98..`(逐字节替换不重建);`fixture::text_flag_write_replaces_stream_without_rebuilding_text` ok |
| (明细更新) | 005/007/011/015 的「影响」列改为现行事实(策略细化/收窄挂号面),状态保持 open |

### 2. 门禁(原始输出)

```text
$ cargo test -p yu-rt --test fixture -- math_rnd_uses_injected_mt19937_seed \
    text_flag_write_replaces_stream_without_rebuilding_text \
    varact_hantozen_and_zentohan_use_official_tables \
    task_without_tint_does_not_inherit_caller_registers \
    window_init_pos_is_host_injected default_slot_reports_once_and_yields_without_terminating \
    error_sets_the_per_command_enable_table dialog_blocks_task_until_answered_and_writes_let \
    clipact_and_clipinfo_round_trip_with_control_filtering
test result: ok. 9 passed; 0 failed; ... finished in 0.01s

$ YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/save/data/ysbin \
  cargo test -p yu-rt --test corpus -- eris_entry_chain_runs_end_to_end text_num_matches_yst_header_total
test result: ok. 2 passed; 0 failed; ... finished in 17.75s
```

### 3. 对账后余量(未实现/未修复,见 divergences README)

open ×8:002(表达式逐位)、003(除零上游取证)、005(官方错误继续策略 trace)、
007(`_CHR/_DXMODE/_VARTYPE/_SYSTEM_FRAME` 等)、009(`_OS_TIME(0)`+时区)、
011(`STRTYPE/SJISCODE/UTF8CODE`、`VAR`)、015(`TEXTPOS/RESTART`)、
046(`ERROR` 表消费者);blocked ×1:042(`DomainCall` 引用身份);
fixed/partial ×1:045(`REGINFO`/`YMVINFO` 需宿主)。

## 成果 P7 续 (2026-10-23) · 运行期 Fatal 双根因修复:MATH[LET] 下标丢失 + VARACT 旗标存在性

> 承 P7 台账对账;用户桌面实跑驱动取证。净室;零 git 写操作。
> 明文源码在 `<project-eris>/save/data/script/ERIS/`(官方 demo 自带)。

### 1. `rt.expr.div_zero` @ es_select.yst:153(S2/S3 上游;DIV-RT-003 收口)

- 链条:`@1118` = `@es.POW2`(`global.yst:208`,int[63] 2 的幂表);填充 =
  `es_First.yst:145` 的 `LOOP[SET=63] MATH[POW=1 SET=2 SET2=@_LC-1
  LET=@es.POW2(@_LC-1)]`(MATH 命令 LET 窗口写,表达式级扫描无写者)。
- 根因:`op_math` 丢弃 LET 目标下标 ⇒ 63 次写全落 `[0]`(探针实证
  `POW2[0]=2^62`、其余 0)。修复:保留 `LValue.indices` 写回。
- 勘误:前一轮「上游分歧绊线/只剩官方 trace」结论作废——官方表填得对。
- 附属取证:`@2264` = `@es.SELD`(選択履歴,`seld.sd` 块值 `[1]=6`)。
- 回归:`fixture::math_let_writes_indexed_array_element`。

### 2. `rt.cmd.slot VARACT 缺 LET` @ es_logset.yst(DIV-RT-019 续收口)

- 现场:s33 `pc46..49` = `VARACT[SET=$… PUSH=0]`(LOG 记录入栈,位置 0);
  旧守卫「旗标值 ≠ 0」把 `PUSH=0` 误判为无参 ⇒ 掉进字符串加工分支。
- 修复(统一对齐,非逐点补丁):`op_varact` 全部旗标分支
  (TYPE/COPY/CUT/UPPER/LOWER/HANTOZEN/ZENTOHAN/DIMSIZE/PUSH/POP)改为
  **参数存在性判定**(与 DIV-RT-024 INIT、DIV-RT-044 DIALOG 同族语义)。
- 回归:`fixture::varact_push_zero_is_presence_not_value`。

### 3. 取证工具(留工作树供审阅)

`yu-format/examples/dump_yssd_var.rs`(官方 .sd 按变量 id 扫块)、
`yu-rt/examples/dump_var_int.rs`(变量元素倾倒)、
`yu-rt/examples/dump_label_cmds.rs`(标签反汇编 + `scan:<sid>` 全脚本命令扫描,
封包优先口径)。

### 4. 门禁(原始输出)

```text
$ cargo test -p yu-rt
64 lib + 4 + 11 corpus(真实语料) + 4 + 65 fixture + 3 golden —— 全 ok

$ cargo build --release -p yu-host-desktop   # 含双修复的桌面宿主
```

### 5. 挂账

- **DIV-GFX-058(新档案)**:COPY/按钮快照不携带源层文本覆盖 —— S4 存读档
  界面行文本空白的机制(官方 CGACT[TEXT] 即时写表面、COPY 天然携带;本核
  文本是渲染期覆盖层,`copy_attributes` 不带文本)。归 H-P2-05 gfx 子项。
- 台账未更新面:`crates/yu-gfx/docs/divergences/README.md` 的 DIV-GFX-012
  (P2 已落地字形执行,状态仍 unimplemented)等 gfx 侧对账另行一轮。

## 成果 P7 续 2 (2026-10-23) · 审查轮收口:旗标存在性族(MATH 捕获 / INIT 守卫 / LABELINFO 下标)

> 承 P7 续审查;按旁线 `format-vm-opcodes.md` §3.20/§3.26 反汇编判据对齐。
> 新档案 `DIV-RT-049`(细节见该条目)。

1. **A1 `op_math`**:旗标捕获改存在性;函数选择改**官方判定链原序**
   (`4,7,10,5,8,6,9,13,11,12,14..21`,与槽位序不同);补 `HASH`
   (标准 CRC-32,新增 `src/hash.rs`,检查值 0xCBF43926 单测钉住)、
   `ABS` 整数路、`SETSTR`=HASH 输入(旧 Unimplemented 作废);
   结果带类型(ABS 整数/RND/HASH = 整数写,余浮点写)。
2. **A2 `op_varact`**:INIT 守卫与 G_* 掩码构造改**在场字节**口径
   (`0x454d3d movzx eax,byte ptr [0x684171]`;INIT 缺席 = `je` 函数尾)。
3. **A3 `op_labelinfo`**:LET 保留 `LValue` 下标,`write_target` 写回
   (原 `store.write(t,&[],…)` 标量化)。
4. 语料级核对:`scanmath` 扫封包全 137 脚本 —— MATH 21 处旗标全 =1、
   SETSTR 窗口 0、无 INIT=0/G_*=0 ⇒ 三处均潜伏,修复对现语料零行为差。
5. 门禁:`cargo test -p yu-rt` 全绿(65 lib + 4 + 11 corpus + 4 + 68 fixture
   + 3 golden,含新回归 `math_flag_zero_value_still_selects_function` /
   `varact_init_zero_value_still_restores` / `labelinfo_let_writes_indexed_element`);
   release `yu-desktop` 已重编。

## 成果 P7 续 3 (2026-10-23) · 全项目审查第二轮:宿主门禁 Fatal 断言 + 浮点除零勘误 + 裸 WAIT + gfx 台账对账

> 范围:yu-cli(case 门禁)、yu-rt(value/wait)、文档。取证先行;零 git 操作。

### 1. H1 · case 门禁 Fatal 盲区(宿主基础设施)

- 审查实锤:`case.rs`(590 行)零引用 diag/fatal —— 主任务被 Fatal 冻结后
  像素不变照样 PASS(S2 除零即如此存活数周)。
- 修复:expect 新增 **`fatal_codes`**(字符串数组)= 实际 Fatal 诊断码集合
  (去重升序;`Engine::take_diag_report()`,含 `script_errors`)逐项相等;
  `[]` = 断言无 Fatal。8 个既有用例全部加 `"fatal_codes": []`。
- 首轮实战:`case --all` 5 过 3 陈旧(symptom-* 因 Fatal 修复后流程前进,
  ops 158→173 / 450→455 / 196→206)——**新断言同时证明三条流的 Fatal 集合
  已为空**;按用例自带说明 `--update` 重生成后 **8/8 PASS**。

### 2. H2 · 浮点除零勘误(DIV-RT-002 族)

- `value.rs` 曾把 `x/0.0` 返回 `0.0`;x87 除零异常默认掩蔽 ⇒ IEEE-754
  结果 = ±Inf(符号随被除数)、0/0=NaN。已按 IEEE 修正 + 单测
  `float_div_zero_follows_ieee754`(±Inf 符号与 NaN 各断言)。

### 3. M1 · 裸 `WAIT[]` ⇒ 让出一帧(Likely)

- 手册 `control/wait.html` 只定义 TIME/FRAME;`basic/task.html` 明文
  「ウェイト命令…一旦休憩します(約0.016秒)」⇒ 无参取 1 帧;
  语料 95 处全带参(FRAME×81/TIME×14),零行为差。
  回归:`fixture::bare_wait_yields_one_frame`。

### 4. L1/L2 · 卫生与台账

- hardcode 审计唯一违规(vm.rs 诊断文案含 `ysbin/ysc.ybn`)改写归零;
- gfx 台账对账:012/034/040 翻 fixed(P2 落地)、035 → partial、
  补建 P2 声称的 056(行距/基线 Likely)/057(屏幕级文本叠加 Likely)。

### 5. 门禁(原始输出)

```text
$ cargo test -p yu-rt
66 lib + 4 + 11 corpus + 4 + 69 fixture + 3 golden —— 全 ok
$ cargo test -p yu-cli         —— 10 passed(yu-cli lib)
$ cargo clippy -p yu-rt -p yu-cli --all-targets --no-deps  —— 0 warning
$ cargo fmt -p yu-rt -p yu-cli -- --check   —— clean
$ python crates/yu-rt/scripts/audit_hardcode.py —— violations=0
$ yu-cli case <project-eris> --all —— 8 passed; 0 failed
$ cargo build --release -p yu-host-desktop   —— 含本轮全部修复
```

### 6. 未闭合(审查移交)

- M2:`audit_hardcode.py` 仅护 yu-rt;其余 crate 的 unwrap/stdout 卫生
  需模块感知审计脚本(计数 vfs 69/trace 67/yu-cli 43,含测试块);
- DIV-RT-042(引用身份)、DIV-GFX-058(按钮文本,P8 任务书已就绪)、
  DIV-RT-002 其余逐位项(x87 路径判据/舍入)照旧挂账。

## 成果 P8 (2026-10-23) · 按钮/对话字体不显示:三层根因修复 + 文本烘焙性能契约

> 任务书 `docs/tasks/task-p8-button-text-copy.md`;但执行中主人桌面实测(官方截图对比)
> 证明现象不止 DIV-GFX-058 一条,追加取证后修了三层根因(058 + 059 + 合成缓存)。
> 文件:只动 `yu-gfx` / `yu-render/{soft,wgpu}`(+ `yu-font` 未改);
> `yu-rt`/`yu-cli`/`yu-save` 未改(临时加在 `yu-cli/examples/input_probe.rs` 的
> 诊断字段已还原)。零 git 写操作;证据产物在 `target/tmp/p8/`。

### 1. 结论(先看这里)

- **按钮/选项文字恢复**:根因不是 COPY 丢字,而是 `FONT[NAME=""]`(空字体名)被当作
  “名字为空字符串的字体” → `find_family("")=None` → `text.font_missing` → **零字形**。
  官方 FONT 处理器对 NAME 做名字查找,查不到落**槽默认 family**(回退表
  `0x403b8e..0x403bdc`)。修复后选项菜单/设定界面全部文字可见(soft/wgpu 同帧一致)。
- **影子“画了两遍”**:REA 复核 FONT 颜色 clamp 为 `clamp(v,0,0xFFFFFF)`(负数→0,
  `0x444620..0x44464a` 的 SBB/JL + `MOV EAX,0xFFFFFF`);`es._txcolor` 的
  `FONT[COLOR2=-1]` 哨兵官方结果是**纯黑**。旧位掩码 `&0xFFFFFF` 把 -1 变白,
  影子成了黑→白渐变亮重影。改为带符号 clamp 后影子为实色黑,与官方一致。
- **持久文本的性能契约**:`CGACT[TEXT]` 按官方“烘焙进层表面”语义改为跨帧持久后,
  逐帧重烤让菜单帧 render 48-65ms(soft 59-79ms)。新增 `compose_layer_cached`
  (层 + `text_revisions` + 基面身份)+ 失效规则后 **wgpu 1.6-2.0ms**、soft 回到
  既有底噪(约 25ms,94 passes 的 CPU 光栅,属既有开销)。
- **修订号必须全局单调**:首版按层从 0 计数,文本项被 `BT.CGCLEAR`/COPY 清除后重建会
  复用旧号 → 缓存撞键(二级菜单文字/点击换色停在旧合成);改为全局发号器,清除即换
  新号,层重建也不复用。修复后父菜单→子菜单文字正确更新。
- **title 600 帧红线保持**:soft/wgpu 与 P8 前基线逐帧 0 差异(600/600),双端互为 0 差异。
- **S4 存档界面**:板/锁图标/底栏正常;板内浅色文字是开界面前一帧的**后缓冲截图**
  50% 暗化的背景(`COPY2 :/W0 -> SLTEMP/CGS1`,EVID-J3-06),与官方截图背景机制一致;
  空槽无描述文本 ⇒ 板内标签为空正确。**既读色**(点过变蓝)依赖 `es.SELD` 经
  `SAVE/LOAD[FILE="seld.sd"]` 持久化 = SAVE 域(DIV-RT-042),未在 P8 动。

### 2. 改动

`crates/yu-gfx`
- `state.rs`:`text_draws` 改为 `LayerTexts{draws,seen,revision}` 跨帧持久 +
  命令时刻精确去重 + 每层上限 16384;失效 = `CGEND`/`CG(FILE)`/Executable
  `CGACT[COPY]` 目标/Executable 图像加工目标(`BT.CGCLEAR`)/`SWAP`(交换并改写
  `TextDraw.target`);`CopyOp.texts` 命令时刻冻结源层文本。`FONT[NAME=""]` →
  `name/font_no/font_bno` 清空(默认 family);`COLOR/COLOR2` 带符号 `clamp_color`
  (REA 证据见 DIV-GFX-059)。
- `text.rs`:`TextStyle/TextStroke` 加 `Hash`;`TextDraw::dedup_key`(`TextDrawKey`);
  `MAX_TEXT_DRAWS_PER_LAYER`。
- `op.rs`:`CopyOp.texts: Vec<TextDraw>`(内部字段,不进 JSON,沿用
  `source_texture` 先例)。
- `frame.rs`:`CompositorFrame.text_revisions`(缓存键,不进 JSON)。
- `json.rs`:`to_json`/`to_display_json` 的 `text_layout`/`text_draws` 改为
  `"text":[...]` 闭合后的帧级兄弟成员(旧写法在空 text 时产出非法 JSON)。

`crates/yu-render/soft`
- `text.rs`:`layer_base` 支持 `TextureRef::Snapshot`(快照层文本基面);
  `resolve_face` 名字查找失败回落默认 family(`FaceFallback::Name`,
  `text.font.name_fallback` 诊断);`compose_layer_cached` + `text_base_key` +
  `texture_content_hash`。
- `render.rs`:COPY 源面合成 `copy.texts`;`text_override_ids`(COPY 源跳过文本
  覆盖,沿用图像加工覆盖);同一层每帧只合成一次;`blit` 导出供 wgpu 复用。

`crates/yu-render/wgpu`
- `textures.rs`:`TargetSet.snapshot_cpu` 快照 CPU 镜像(捕获时按目标尺寸+区域
  裁剪构造;GPU 原地加工后失效;prune/clear 同步)。
- `passes.rs`:1.4 段按命令序预捕获镜像;1.5 段文本基面读镜像 + `compose_layer_cached`;
  1.6 段合成 `copy.texts` 为 COPY 源并落镜像;`layer_image` 跳过文本覆盖;
  `plan_paint` 失效镜像。

`crates/yu-render/soft/examples/frame_walk.rs`
- 新增 `YURIS_WALK_CLICKS="frame:x:y,..."`(与 `yu-cli case` 的 inputs 同语义)与
  dump 时写 display JSON;修正 `workspace_root` 的既有层数 bug(原先写到
  `crates/target/tmp`)。

### 3. 证据

| 事实 | 证据 | 等级 |
|---|---|---|
| 空白文字的直接原因 `text.font_missing`/`glyphs=0` | 真实回放报告(`input_probe --full`,frame 500:draws=25/glyphs=0) | Confirmed |
| `FONT[NAME=""]` 空名 | gfx 帧快照 `name=Some("")`;修后 `name=None, glyphs=175` | Confirmed |
| NAME 查找失败 → 槽默认 family | REA FONT 表 `docs/font-provenance.md` §4.2(默认值=槽默认 family;回退表 `0x403b8e..0x403bdc`) | Confirmed(表)/Likely(空名路径) |
| COLOR/COLOR2 = clamp(v,0,0xFFFFFF) | REA `0x444620..0x44464a` 指令(`SBB/JL/XOR` 负→0;`MOV EAX,0xFFFFFF` 封顶;`MOV [EDX*4+0x5d1708],EAX`) | Confirmed |
| `es._txcolor` 发 `COLOR2=-1` 哨兵 | `stdlib/ystdlib.yst:706`(guard `>=-1`、`FONT[COLOR2=-1]`) | Confirmed |
| 文本“烘焙进表面、画一次常驻” | 手册 `cg/cgact.html`;REA `0x404398→0x4046fc→0x404914→0x4056b8`(1/9/3 次表面合成);`'P'` 页断整面清零 | Confirmed |
| COPY 携带已画文本 | 官方桌面截图(选项菜单/存档界面);EVID-T-15 命令时刻像素 | Confirmed |
| 后端不需要逐帧重烤 | 实测 A/B:无缓存 48-65ms → 缓存 1.7-2.1ms(同画面同用例) | Confirmed |

### 4. 验证原始输出

```text
$ cargo test -p yu-rt -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu
passed=380 failed=0(含新用例:soft render +3 微场景, wgpu +2 跨端微场景,
group.rs +4 文本语义, text_pipeline 去重/持久断言)
$ cargo clippy -p yu-gfx -p yu-font -p yu-backend-soft -p yu-backend-wgpu --all-targets
0 warning(改动文件 rustfmt clean;既有 examples 的 fmt 欠账未动)
600 帧 title(frame_walk / wgpu_frame_walk,与 target/tmp/p8/baseline-*.tsv 对比):
soft 600/600 相同;wgpu 600/600 相同;soft vs wgpu 0 差异
真实回放(input_probe,symptom-ingame-save 同时间线):
修前 frame 500: TEXT draws=25 glyphs=0 diags=["text.font_missing"]
修后 frame 500: TEXT draws=25 glyphs=175 diags=[]
wgpu 菜单帧 render:修前 48-65ms → 修后 1.7-2.1ms
```

新画面(待主人批准后 `--update`):
`target/tmp/p8/case-enter-story-f472.png`、`case-config-f542.png`、
`case-save-f562.png`;官方对照:`target/tmp/p8/official-800x600.png`
(用户截图 `image-623b6f05…png` 缩放)。

### 5. 挂账 / 交接

- **S4/设定/正文三个用例 expect 未更新**:按任务书硬门禁,新 display/PNG 已出图
  待主人过目批准;`cases/symptom-{enter-story,ingame-config,ingame-save}.json`
  expect 差异已实测(三个用例仅 hash/ops 像素变化,无 Fatal)。
- **既读色(点过变蓝)**:`es.SELD` 由 `SAVE/LOAD[FILE="seld.sd" DNO=1]` 持久化;
  `SAVE` 目前 `sv.write.refused`(DIV-RT-042 引用身份门禁)。需主人裁决跨 crate
  契约,不在 P8 范围。
- **`FONTINFO[NUM/LANG/NAME]`**(设置界面字体选择器)仍 Unimplemented:需要宿主
  注入系统字体清单(引擎 `EnumFontFamiliesExA` 语义),属跨宿主接线。
- **soft 菜单帧 24ms 底噪**属既有开销(94 passes/475 层的 CPU 光栅),与本次文本
  无关;wgpu 为目标后端(1.7-2.1ms)。
- **弹窗/存档背景透出上一屏**(主人桌面实测):场景 CGS 截图栈未清 —— `es.CGSSS`
  的 `CGS1` 仍 visible/A=256 且内容为上一亮帧,官方背景为黑。已立
  **DIV-RT-050**(`docs/divergences/DIV-RT-050-cgs-scene-screenshot-stack.md`),
  待官方事件级 trace 定论;不在 P8 文本范围,未改代码。

## 成果 AU5 (2026-10-24) · 全项目未实现面审查:四路并行 + 台账对账勘误 + 批次路线图

> 承 P8。用户令:全面审查所有模块未实现面、找迫在眉睫项、对齐官方引擎/手册;
> **手册缺页不构成不实现理由**;**修完必须划台账**。审查事实源 =
> `docs/audit-2026-10-24.md`;第一批任务书 = `docs/tasks/task-b1-save-loop.md`。
> 零代码行为变化(仅 1 处过时注释),零 git 写操作。

### 1. 审查(四路 Explore 并行 + 主线抽查)

- 覆盖:yu-rt / yu-gfx+双后端+yu-font / yu-sound+save+vfs+format+trace+log /
  yu-cli+platform+emote+全部台账;对象 = HEAD `88eea55`、submodule `d0f03a1`。
- 主线抽查 4 项关键论断属实:`sv.write.refused` 门禁、DIALOG 宿主零接线、
  CLI 无 `--debug-mode`、`yu-emote` 物理未求值(`anim.rs:23`)。
- 结论:P0 ×4(DIV-RT-042 存档闭环 / DIV-RT-050 CGS 栈 / DIALOG 宿主 /
  debug_mode census 口径);P1 ×9(WINDOWINFO 字段族、CGINFO[COLOR]、
  FONTINFO 清单、DirSource 非 ASCII、update1..9 挂载、serve 欠账、文本 oracle、
  WINDOW/TRAY 宿主、存档快照);「手册缺页但 REA 证据已够」挂账 5 族
  (DATALEFT/DATARIGHT、`_CHR`、WINDOWINFO、负 START、字体清单)。

### 2. 台账勘误(本次落地;"修复了必须划掉"的第一轮清账)

- `todo-inventory.md`:头部 AU5 注记;§3.2 GBK 码表已落地;§5 行8(文字桥主体
  fixed)/行9(E-mote 以 submodule 为准)/行11(A4)/行12(C2/C3)划掉。
- `gfx-unimplemented-todo.md`:补 058/059(P8);024→partial(J3);050→partial;
  「TEXT 持久缺失」过时句;推进顺序第 7 条划掉。
- `crates/yu-rt/docs/coverage-matrix.md`:FILEINFO(➖→✅ J3)/FONTINFO(→部分)/
  WINDOWINFO(→部分,补字段族归因)/LET(→装配已解)四行勘误 + §8 口径注
  (CDDA 域错位待一行修)。
- `crates/yu-rt/docs/divergences/README.md`:003 补「根因已修」;011 补
  CSVINFO/CDDA;新增 DIV-RT-050 交叉引用行。
- `crates/yu-gfx/docs/divergences/README.md`:033/036 → partial(P2);055 撞号注记(★待裁决)。
- `yu-gfx/src/state.rs`:FONTINFO `other =>` 过时注释修正(GRADATION/ROTATION 实已答)。
- `docs/divergences/DIV-003`:码表已落地,剩余 = DirSource 折回。
- `docs/emote-parity.md`:头部失真警告(§7/§8 宣称项在 `d0f03a1` 不可复核;★待主人裁子仓)。
- `docs/symptom-ledger.md`:S2/S3d/S4/S7 销账(根因 = P7 续;界面 = P8;残留→
  DIV-RT-050/042);H-P1-07/H-P1-08 销账头;§7.2 eprintln 勘误划掉(已清)。

### 3. 门禁(原始输出)

```text
$ cargo check -p yu-gfx        # state.rs 仅注释级改动
    Finished `dev` profile target(s) in 0.37s
```

### 4. 挂账 / 交接

- **B1 任务书待主人拍三个契约点**(`docs/tasks/task-b1-save-loop.md` §0):
  引用身份进事件的方式(推荐 ArgValue 加 `VarRef`)、LOAD 写回通道(推荐
  SaveHost 同步接口)、既读色启动链取证口径。拍板后即可开工。
- ★ 待主人裁决:DIV-GFX-055 撞号合并;emote-parity §7/§8 与 submodule 对齐方向;
  examples-audit B/C 组;CI 缺位;`audit_hardcode.py` 全仓化。
- 批次路线图 B2–B9 见 `docs/audit-2026-10-24.md` §6。

## 成果 AU5 续 (2026-10-24) · 全量批次路线图定稿(至待办清空,B1–B15)

> 用户令:剩余待办全部分批规划到清空;E-mote 暂缓;其余 SDK 面不允许任何
> 「未实现」终态;手册缺页 = 凭 REA/引擎证据实现并标注,不作为挂账理由。

### 1. 产出

- **`docs/tasks/roadmap-batches.md`(定稿)**:B1–B15 全量细案。
  - §0 五种终态定义:`fixed` / `fixed(no-manual)` / `aligned-stub` /
    `unsupported-by-official` / `deferred(master)`;`unimplemented/blocked/open`
    禁止作为终态;AU6(B15)复核时全仓 grep 归零(E-mote 域除外)。
  - B9 = E-mote **deferred(主人令)**;B10(rt 零挂号 campaign)、B11(gfx 命令面)、
    B12(合成/像素取证)、B13(ypf 全分支 + YSTB v2/SNP/配置块)、
    B14(MOVIE/YMV 媒体宿主)、B15(Platform 边界/CI/AU6 清空宣告)为新增批次。
  - 四波次并行建议;每批含条目锚点、证据路径、终态预期、强制划台账清单。
- **`AGENTS.md` §0 政策行**:手册缺页 ≠ 不实现;`fixed(no-manual)` /
  `aligned-stub` / `unsupported-by-official` 标注约定;`Unimplemented` 仅过渡态。
- **`docs/audit-2026-10-24.md` §6**:改为指向路线图。
- **`docs/todo-inventory.md` §3.2**:rt 命令行勘误(F_INT 族/MATH3D/DIALOG 实已
  收口;剩余归 B7/B10/B14)。

### 2. 待办清空的判定(路线图 §0)

每条挂账先 REA 定「官方做不做」→ 官方做=实现(手册缺页就 no-manual 标注)、
官方空桩=aligned-stub、官方也不支持=unsupported-by-official。REA 反汇编本身
就是证据层,「三层缺层」不再构成无限期挂账理由。

### 3. 门禁

纯文档轮;无代码改动。`git status`:+1 文档(roadmap),3 文件修订。

### 4. 挂账

- B1 三个契约点仍待主人拍板(拍板即开工)。
- B8 官方截图采集需主人桌面/官方引擎环境窗口。
- E-mote(B9)deferred;其 emote-parity §7/§8 对齐方向仍在 ★ 清单。

## 成果 B1 (2026-10-24) · 存档闭环(DIV-RT-042):引用身份 + SaveHost 同步契约

> 任务书 `docs/tasks/task-b1-save-loop.md`;契约裁决 = 裁 1-A / 裁 2-A /
> 裁 3 取证定论。**唯一功能级 P0 关闭**(audit §1 P0-1 ✅)。

### 1. 取证(0.495 `yu-ris.exe` sha256 `4f5eefa3…`;capstone 递归下降,产物 `target/tmp/b1/*.asm`)

- **SAVE 处理体 = `0x44c45c`**(opcode 84,分发表 `0x7ace20[84]`;
  **任务书旧锚 `0x44e1c8` 实为 SAVEINFO 本体,勘误留痕**,SAVEINFO 挂账 B7):
  1. `SET`/`LET` 变量身份经 `0x683a60/0x683c00/0x683da0[slot]` 三表传递
     (`0x44c583`/`0x4471eb`)——引用,不是值;
  2. **预约即快照**:`SAVE[DNO]` 时刻 `0x44d73d` 写 btype/strict/var/clen/ulen
     并取当前值 zlib 压缩入预约表(`0x7fad1c`/`0x7fbe1c`);
     `SAVE[FILE]`(`0x44cd5a..0x44ceab`)只写文件头+偏移表(容量 `0x400`、
     版本 `0x1e0`),按 DNO 升序流出**预约时**字节;预约表不清
     (SAVEEND `0x44e1a0` 才清;语料 es_GSD 每次刷盘后显式 SAVEEND 同型);
  3. 同 DNO 重复预约 = 替换(`0x44c740`);
  4. 组形态 `G_<族><组>=1`:按首个在场旗标(级联序 G_INT→…→G_STR4)定
     (类型,组),枚举全局变量表逐变量子块化(块类型 2,
     `0x44ca8a..0x44cc57`);
  5. LOAD `0x44708c`:偏移寻址 `0xC+DNO*4`(`0x44735a`);块类型 2 逐子块
     **按块内 var_id 直接恢复**(`0x447460..0x4474de`,不读 `LET`/`G_*`
     在场位);块类型 0 写 `LET` 目标整组(`0x447b3d`)。
- **官方 golden**(真值层级 3):`seld.sd`=1 块 var 2264(`@es.SELD`)
  Int[2001]、值 `[1]=6`;`gsd.sd` DNO 3/4 = btype2 子块 var 1161/1162
  (`@G(0001)`/`$G(0001)`,与 DIV-RT-017 三方对齐互证)。
- **语料形态清点**(明文 `save/data/script/ERIS/`):标量 `SET=@X`
  (es_Select:997)、整组 `SET=@X()`(:1025)、组形态 `SAVE[DNO=03 G_INT2=1]`
  (es_GSD:578)、LOAD 组形态 `LOAD[... DNO=0005 G_INT3=1]`(es_Select:219)。
- **裁 3(既读色)**:`es_First.yst:139 \GSD.INIT` → `es_GSD.yst:130` →
  `:142 GOSUB[#=ES.SEL.FILELOAD]` → `es_Select.yst:1068
  LOAD[FILE="seld.sd" DNO=1 LET=@es.SELD()]` —— **全是脚本命令,零内核
  新机制**;LOAD[LET] 落地即自然接通。

### 2. 实施(裁 1-A / 裁 2-A)

- `yu-rt`:①`ArgValue.reference: Option<VarRef>`(白名单槽 SAVE[SET]/
  LOAD[LET]/FILEINFO[LET]/SOUNDINFO[LET];`eval_arg_windows` 求值时保留
  lvalue 身份,旧事件流缺字段=None);②`SaveHost` 同步契约
  (`reserve/reserve_group/flush/save_end/load_block`)+ `Vm::set_save_host`;
  ③`op_save/op_load/op_saveend`(命令时刻执行;FILE 缺 `.sd` 补齐、
  名 >32B 报错,REA `0x44c663` 同判);④INFO/SOUNDINFO 拆「事件参数面
  (带引用)/query 参数面(原契约)」;⑤`VariableStore::restore_elements`
  (同型整组写回)。
- `yu-trace`:`TraceArg.reference`(`{prefix,id}`)序列化/解析;旧 trace
  缺字段按 `None` 读(向后兼容)。
- `yu-save`:状态机重写——`ReservedFact`=块快照(btype 0 单变量/btype 2
  多变量组);`flush` 按 DNO 升序流出预约时快照、**不清表**;`save_end` 清;
  `load_block` 解容器(btype1 配置块不解释,DIV-RT-012/B13);`SaveHost`
  trait 实现;`sv.write.refused` 语义收窄为「只读根拒绝」(引用缺席拒绝
  删除);apply_event 里 SAVE/LOAD/SAVEEND 改由同步路径计数。
- `yu-cli`:`boot()` 安装 `Vm::set_save_host`(同一 SaveHost 双通道:
  同步契约 + FILEACT 等事件事实);`save_state_sha256` 摘要改块事实形态。
- **不碰**:yu-gfx / yu-render / yu-font / DIV-RT-050 / debug_mode。

### 3. 验证命令与原始输出

```text
$ cargo test -p yu-rt -p yu-trace -p yu-save -p yu-format
19 个测试目标全部 ok(0 failed),含:
  fixture 72 passed(save_load_loop_records_reserve_flush_and_writes_back /
    save_group_form_enumerates_matching_globals /
    load_group_block_restores_each_var_by_id 新增 ×3)
  yu-save 8 passed(flush_writes_official_container_and_load_roundtrips /
    official_seld_block_reads_as_var_snapshot(官方 seld.sd 结构对拍)新增)
  yu-trace golden 2 passed(UPDATE_GOLDEN=1 重生成:夹具表补 LOAD/SAVE
    参数后 YSCom.ycd sha 变更;args 含 "reference":null ×5)

$ cargo run -p yu-cli -- case <project-eris> --case save-load-loop
case save-load-loop: PASS(×2 复验;470 tick,frame=472,ops=173,
  fatal_codes=[],gsd/extra/seld/kidoku/save30002.sd 落盘
  target/tmp/save-cases/save-load-loop/)
$ cargo run -p yu-cli -- case <project-eris> --case save-title-flow
case save-title-flow: PASS(--update 仅 save_state_sha256:预约语义升级;
  ops/png 逐位不变)

正常路径 sv.write.refused 零出现:save-load-loop 可写根下全部刷盘成功
(槽表含产物字节事实 = save_state_sha256);refused 现仅存于只读根
(yu-save flush 单测覆盖)。
```

### 4. 口径与残留

- census 口径:SAVE 16 / SAVEEND 3 在未装存档宿主的普查下仍为域事件
  (coverage-matrix §2 已更新两侧口径);装宿主即闭环。
- `SAVEINFO` 查询(REA `0x44e1c8`,语料 0 次)挂账 **B7**(本批确认锚点
  归属);SAVE/LOAD[ID] 图层快照/恢复 = H-P4-02(B7)。
- 桌面实测(存档→读档→既读色)待主人复验:内核与宿主链路已由
  `save-load-loop` 用例 + fixture ×3 + 官方 golden 覆盖。

### 5. 勘误(B1 收口后同日,主人令)

- **默认可写**:官方原版已备份 `project-eris/save-backup-20261024/`
  (root 6 文件 + data-save 4 文件;seld.sd sha 前缀 a54ce12a 校验一致);
  `engine.rs::boot` 不再从 `--save-root` 推导 writable,**缺省(工程根
  `save/`)即可读写**,`--save-root` 仅作目录重定向(上方"默认只读"句作废)。
  `run` 子命令补 `--save-root` 旗标(桌面宿主/serve 的旗标透传仍归 B7)。
- save-load-loop 用例摘掉 png/display 哈希:基线渲染(yu-font/yu-gfx 文本)
  另一会话活跃改动中,像素回归归 gfx 用例;B1 断言面 = ops +
  fatal_codes=[] + save_state_sha256(PASS×2 复验)。
- **FILEINFO「散盘」回退**(存档界面槽列表空 + 启动恢复不生效的根因):
  槽判定链 `es.SaveDataCheck → \_file.info.exist → FILEINFO[EXIST]`
  (ystdlib:989 宏)原只查 VFS ⇒ `$_SAVE_PATH+名` 恒不存在;按 REA
  `0x44316c` 文件系统支补 `save_root_fs_exists`(前缀 `$_SAVE_PATH` +
  越界防护),`fixture::fileinfo_fs_fallback_checks_save_root` 回归;
  官方 seld.sd golden 的值断言放宽为选项号合法域(活语料随游玩变化)。

## 成果 B5 (2026-10-24) · VFS 挂载与名字:update1..9 增量包 + DirSource 非 ASCII 折回(DIV-003 归零)

### 1. 取证(全部为既有证据锚点,本批未新增 REA)

- **增量包挂载顺序**: 引擎目录扫描(`FUN_0045eda0`)按 `.ypf` 后缀命中
  `update1..9.ypf`、按序号登记(`0x80182c + n*4`),随后(`FUN_0045e3b8`)
  在基础数据之后**依序挂载非空槽**(旁线笔记 `format-ypf.md` §挂载顺序/
  §目录扫描;`architecture.md` §启动链 REA 字符串 `ev_bdf145d8…`/
  `ev_bb5e8e01…`)。序号域就是 1..9 —— `update10.ypf` 不入列(有单测钉住)。
- **编码选择(CP932 vs GBK)**: 不猜 —— 语料与 SDK 均为日文 CP932 形态,
  `DirSource`/`Project` 的 `Codec` 本就是既有配置参数(CP932 为全仓缺省);
  GBK 走同一条折回路径,有独立单测(Python gbk 独立 oracle)。
- **折回语义依据**: Windows 上引擎按 Unicode 名访问散盘文件(DIV-003
  「官方行为」节,文件系统事实 + 引擎读序推断),`std::fs` 只给 Unicode;
  折回 = 恢复引擎的 CP932/GBK 字节路径语义。`yu-format` 码表全表满足
  `decode∘encode` 恒等 ⇒ 同目录不同 Unicode 名折回后键必不同,索引无碰撞。

### 2. 实施

- `yu-vfs`:
  - `Cargo.toml` 增依赖 `yu-format`(architecture §7:表本体归 yu-format,
    本层只调用 —— 这正是 DIV-003 的分层修法);
  - `codec.rs`: `Codec::to_format()` 映射到同名编解码器;
  - `dir.rs` `relative_key`: 非 ASCII 组件经 `yu-format` 码表 `encode` 回
    字节键;编码失败(不可映射字符)才跳过,原因记 `unencodable_name`
    (取代旧 `non_ascii_name`;其余原因不变)。ASCII 快路径不触码表。
  - `mount.rs`: 头部补接线注记(audit「Mount 无人用」销账)。
- `yu-cli/src/project.rs`:
  - `Project::update_packs()`: 数据入口所在目录枚举 `update1..9.ypf`
    (按序号递增;`--data` 覆盖时取数据文件同级目录);
  - `open_source()`: 存在增量包时返回 `基础包 + update1..9` 的
    [`Mount`] 叠加栈;**无增量包时保持单一源不变**(诊断 `source_name`
    与既有 golden 不受影响);
  - `mount()`: 层序 = `基础包 → update1..9 → 散盘`(散盘是开发/覆写
    通道,置于官方增量之上,同名覆盖语义不变);
  - `open_ypf()` 小助手统一打开错误映射。

### 3. 验证命令与原始输出

```text
$ export YURIS_TEST_TMP=<repo>/target/test-tmp
$ export YURIS_SAMPLE_SDK0479=<side-repo>/target/sdk/0.479/yu-ris_0479_014/システム
$ cargo test -p yu-vfs -p yu-cli
yu-vfs:  lib 42 passed(含 dir 折回 ×3 新增)+ equivalence 13 passed
  (含 non_ascii_name_dir_and_ypf_agree 新增: 散盘 Unicode 名 ↔ 封包
   CP932 字节名跨源一致)+ nul_xor 2 passed + real_sample 5 passed
  (含 sdk_0479 折回验收新增)
yu-cli:  lib 13 passed(含 project ×3 新增: 枚举序 / open_source 叠加 /
  mount 层序)+ bin/集成目标 0 failed
全部 ok,0 failed(完整日志 target/tmp/b5-test-full.log)

$ cargo test -p yu-vfs --test real_sample sdk_0479 -- --nocapture
0.479 DirSource: files=1424 skipped=0          ← DIV-003 验收: 1,424 文件 0 skipped
japanese name ok: data/cgsys/extra/bgmmode/tip_再生時間.png bytes=1468
test sdk_0479_loose_directory_indexes_every_file_including_japanese_names ... ok
```

- 0.479 实测口径: `システム` 目录 1,424 文件 = 旧实测 files=1339 +
  skipped=85;折回后 85 个日文名(tip_再生時間 / tip_鑑賞数 /
  エンジン設定.exe 等)全部入索引,日文名锚点按 PNG 魔数读出。
- 折回 oracle 均为硬编码字节(Python cp932/gbk 独立计算,不经实现码表):
  CP932 日=0x93FA 本=0x967B 語=0x8CEA 再=0x8DC4 生=0x90B6 時=0x8E9E
  間=0x8A94;GBK 日=0xC8D5 本=0xB1BE 語=0xD55A(trail 0x5A='Z' 不折)。

### 4. 口径与残留

- **无带更新包的样本工程**(注明): `<side-repo>/workbench/project-eris`
  为封包无 `update*.ypf`、`project-479` 为散盘、`<games>` 不存在(AGENTS §2)
  —— 按任务书 B5 预案,以**自造 update 包往返测试**覆盖:`project.rs` 三个
  单测用最小 v495 写入器构造 基础包+update1+update2,断言枚举序
  (1..9 域,update10 不入列)、open_source 叠加覆盖(shared.txt:
  base→u1→u2)、mount 层序(散盘 > update > 基础包)。真样本到手后
  可用 `case` 流程复验资源齐。
- 残留(低风险,留痕): CP932 的 NEC/IBM 别名码(398 条多码一符)编码时取
  系统首选码 —— 散盘 Unicode 名唯一 ⇒ 索引无碰撞;但若脚本以**别名形态**
  字节引用散盘日文名,字节键可能不命中(引擎实际按 Unicode 匹配,不受此限)。
  SDK 1,424 文件实测零缺口;真游戏命中时再取证(YSCom 输出形态未定)。
- 台账: DIV-003 → fixed;audit §2 两行划掉;todo-inventory format 行勘误。
- 本批未触达 yu-sound —— 工作区 `yu-sound` 在途改动属并行批次 B6,与 B5 无关。

## 成果 B6 (2026-10-24) · 声音收口:五项全部落入终态(零 unimplemented)

> 任务:`docs/tasks/roadmap-batches.md` B6。REA 复核 = Ghidra 12.1.4 重开
> `yu-ris.exe`(sha256 `4f5eefa3…` 与台账一致);关键字节/调用点证据入
> `docs/sound-ledger.md` §0-11。

### 1. REA 复核结论(全部字节/调用点级)

- **SOUND[SPEED=](槽 13)**:处理体 `0x44e1e8` 只读 `0x684160..0x68416c`
  (反编译逐槽核对,无 `0x68416d` 引用)⇒ 官方静默忽略 ⇒
  **unsupported-by-official**:宿主删除 `SoundCommand::speed` 建模,
  零诊断(DIV-SND-001 定案)。
- **ACM WAVE(CODEC=2)**:官方 `0x422e04` 的 `fmt tag==1` 走 PCM 直解,
  **其余全部 tag 走 ACM**;7 个 `acm*` 导入的全部调用点
  `0x423086..0x4232a4`(IAT 跳板 `0x47f86e..0x47f892`);序列
  `FormatSuggest(WFORMATTAG)`→`StreamOpen(NONREALTIME)`→`Size`→
  `PrepareHeader`→一次 `Convert`→`Unprepare`→`Close`,失败=装载失败。
  手册「不支持」句过时 ⇒ **fixed(no-manual)**:新增
  `yu-sound/src/acm.rs`(全仓第一个第一方 unsafe:FFI msacm32;crate 级
  `forbid`→`deny`+模块 `allow`,理由入注释)。
- **DATALEFT/DATARIGHT 勘误 + 实现**:16-bit 立体声 DATARIGHT 官方读
  `word [ptr+4]`＝**下一帧左声道**(越 4 字节锁区;旧台账「第二采样」
  仅对 8-bit `ptr[1]` 成立)⇒ 怪癖 1:1 保留;**fixed(no-manual)**
  (`state.rs::pcm_data_sample`,-32768 回绕/单声道 -1/守卫全对齐)。
- **延迟累加器**(`0x423f9c/0x424424` 反编译):`+0x190/+0x194 +=
  |param|*bps/1000` 按 block align 向下取整;填充从字节 0 重灌 ⇒
  **fixed(Likely)**:ms 域静音区间 + TIME 冻结 + PLAY 保持 1(DIV-SND-002;
  动态曲线仍归 DIV-SND-003 校准)。
- **MIDI 族/CDDA**:空桩(0x44aa40/60/80、0x429fb0)⇒ **aligned-stub**:
  行为不变(域事件照发/CDDA 归类不动),只改诊断归属——新增
  `rt.cmd.aligned_stub`(Info、每命令名只报一次、**不计挂号表**,
  `yu-rt` vm.rs/handlers.rs/code.rs)。

### 2. 验证(原始结果)

```text
$ cargo test -p yu-sound
test result: ok. 37 passed; 0 failed        ← 含 golden loop_timeline 不回归
  新增: 延迟 3 例(TIME 冻结/负 RESTART 循环/设备路径静音块)
        DATALEFT/DATARIGHT(含 +4 怪癖钉子 303≠700)
        acm_wave_roundtrip_through_system_adpcm(系统 MS-ADPCM 真实往返)

$ cargo run -q -p yu-rt --example coverage_census -- 600 8
SOUND 531 / SOUNDINFO 7200 / SOUNDEND 1     ← 与基线逐项一致
MIDI/CDDA 0;M-2 诊断文案不变(不装宿主)
合计 7,735,437 vs 10-22 基线 7,735,670:差值来自同树并行改动
  (B5 折回/CGS revert),B6 不触碰命令计数

$ yu-cli run <project-eris> --frames 60 → exit 0(60 帧,0 资产缺失)
$ yu-cli --example sound_census(装 SoundHost)
id=BGM/B1 file=bgm/bgm01.ogg state=Playing time=15500 total=90802
snd_diags=0

$ cargo test -p yu-rt → lib 66 passed
  (tests/consistency.rs 4 例失败与 B6 无关:语料散盘 data/ysbin 已被
   并行改动移除,封包 data.ypf 优先;skip 路径 unreachable! 为既有缺陷)
```

### 3. 台账划线

- DIV-SND-001 → fixed(SPEED=unsupported-by-official、DATALEFT/DATARIGHT=
  fixed(no-manual),含 +4 勘误);DIV-SND-002 → fixed(Likely);
  DIV-SND-004 → fixed(no-manual)(手册勘误留痕)。
- sound-ledger:§0-9/0-10 补终态、新增 §0-11(四项字节级复核)、N0-3/N0-12
  改写、§3 槽 13 定案(修正旧记法端点含混)、§7 清单全部落入终态 +
  §7.1 验收记录。
- coverage-matrix:§1 CDDA/MIDI×3 行 → aligned-stub;§8 口径注 + SOUND/
  SOUNDINFO/MIDI 族/CDDA 行更新。
- PROGRESS:本条。

### 4. 口径与残留

- 延迟 Likely 三项(TIME=0/PLAY=1/输出静音)待 DIV-SND-003 动态样本定论;
  混音 headless advance-then-render 序列不体现静音区间(设备路径才体现),
  已在 mixer.rs 注释声明。
- 8-bit 源的 DATALEFT/DATARIGHT 值域为内核 i16 归一化域(0..32768),非官方
  缓冲字节域(0..128);语料无 8-bit 音频,16-bit 分支逐位对齐(代码注释留痕)。
- `snd.codec.unsupported` 码位保留但已无触发点(非 PCM 全走 ACM);0.479
  等旧版引擎若证实不装 ACM,再按版本分叉处理(无证据,不猜)。
- 并行干扰留痕:本批进行中的 state.rs/error.rs/decode.rs 曾被外部
  `git stash`(stash@{0} "parallel-sound-break")移出工作区;已按原样恢复
  (stash 未消耗)。工作区归主人裁决,零 git 写操作。

## 成果 DIV50 取证 (2026-10-24) · RT-050 命令级对拍收口:官方菜单走「替换式」打开,内核误走「嵌套式+空栈基座推入」

> 任务:主人令「结合上下文/代码/引擎/SDK/源码脚本排查 RT-50,怀疑命令未实现或差异」。
> 结论:**不是命令缺失,是同一点击走了不同脚本路径**。零引擎代码改动(纯取证)。

### 1. 方法与工具(全部可复现)

- 官方侧:`target/tmp/official/div50f/hits.jsonl`(60,000 条,bp=CG/CGACT/CGEND handler;
  pc 语义与内核 `Cmd.pc` 一致,manifest 佐证)。新增按 pc 反汇编/调用点扫描三件套
  (untracked example,`crates/yu-rt/examples/`):`dump_pc_cmds`(含窗口 slot 复合赋值码
  `~1`=+=、`~2=`-=)、`scan_gosub_callers`、`scan_var_refs`。
- 内核侧:`vm.rs::step_one` 临时注入 `YU_CMD_TRACE` 环境开关(每命令打印
  frame/script/pc/命令名 + @1019 当前值;**已回滚**),复跑
  `input_probe --frames 480 --click 300,217,281 --click 470,750,583`
  → `target/tmp/div50k/trace2.log`(728 万行)。

### 2. 关键事实链(证据锚点均为 script/pc)

- **官方菜单打开(tick477)**:恰好 1 次 `es.CGTSS`(s8 pc334/335/337/338,含
  **RECTPAINT2 SET=255**)+ 淡出任务 2 tick(pc502×2)+ CGEND CGS100(pc391);
  **无 `es.CGSSS`(pc298/299/301)、无 `es.CGTA`(pc366)**;tick484 弹窗自动存档
  (s19 pc312/313/343)+ 菜单按钮风暴(s0 pc1940/2148/1877×16)。
  全 trace CGSSS 仅 tick737 出现 1 次 ⇒ **官方从未在菜单处推 CGS 截图层**。
- **内核菜单打开(f470)**:s0 事件循环 pc2424 → `ES.GAMEMAIN.BTN.MENU.ON`(s52 pc434)
  → pc440 `es.S.GOSUB("ES.MENU")` → s8 pc79(硬编码 PINT2=1)→ `es.SS.GOSUB` pc192
  判假 → ELSE 臂 pc199-211:CGTSS + CGTA(0) + **pc205 IF(@1019==0) 判真 → pc206
  `es.CGSSS` 推 `:/W0/CGS1`(z=9.5e10,A=256,持久)** → CGTA(256) → pc213 `@1019+=1`。
  CGS1 在 40% 暗纱下永久可见 = RT-050 可观测面。
- **@1019 记账**(全程仅 3 翻转):f227 标题打开(s20 pc63 `es.S.GOSUB("ES.TITLE")`)
  +1(pc213,slot~1)→ f397 标题退出(es_title pc87 → SS.RETURN pc226,slot~2)−1 →
  f470 菜单读到 0。启动链 ES.FIRST/ES.GAMEMAIN 均走 `es.S.GO`(s8 pc13 / s10 pc123,
  **替换式,不 +1**)⇒ 记账本身自洽,计数器实现无误。
- **根因定论**:官方菜单打开**不经过 `es.SS.GOSUB` 两臂**(THEN 必随 CGSSS、ELSE 必随
  CGTA,官方两者皆无)⇒ 官方走 `es.SS.GO`(pc178)或 `es.SS.RETURN`(pc222)的
  「替换/返回」式路径;内核走「嵌套覆盖」式 ⇒ 差异在**上游按钮事件处理**
  (内核由 es_button 直接 GOSUB ON 标签;官方疑似 `ES.GAMEMAIN.BTLOOP`(s20 pc296+)
  就地消费底栏按钮事件)。
- **勘误**:旧记「调用方 PINT2=0 → .OVER」为误读(那是数据参数;选择器是 pc79 硬编码
  PINT2=1,内核实际正确走了 ELSE);「CGSDELALL 未执行」「截图黑帧/亮帧」两候选均排除。

### 3. 台账与残留

- DIV-RT-050 文档全量重写(状态 open,根因/勘误/修法/回归齐备);三次渲染层补丁的
  否决教训留痕。
- 待办(下次):官方 **GOSUB 级补采**(bp=GOSUB,仅采 pc∈{178,193,200,222,307})定位
  官方 CGTSS 调用点 → 对拍定第一个路径分歧(预计在 `es.BT.BTLOOP` 事件消费语义)。
- 次要分歧挂账:RECTPAINT2 SET=255 像素语义对拍;幕布淡出 ms→帧换算
  (官方菜单 2 tick / 500ms 场景 33 tick,内核恒 ~14 tick)。
- 工作区:vm.rs 探针已回滚;`dump_pc_cmds`/`scan_gosub_callers`/`scan_var_refs`
  三个取证 example 留存待主人裁决;并行会话改动未触碰。

## 成果 B4 (2026-10-24) · debug_mode 注入链:黑帧定案 + CLI 旗标 + 双口径 census

### 1. 内容

- **黑帧定案(DIV-L-001 复核)**:`debug_mode=true` 黑帧不进 title 的根因
  不是 rt debug 分支语义,而是调试 UI(ERIS s66,debug 任务在 f1 建立调试窗)
  读 `_MOUSE_X2` 时内核误判为「工程 ycd 未声明按钮」→ Fatal → SYSTEM 主任务
  frame 3 被摘链 → s58 `ES.TITLE.LOOP` 永不执行。REA 定案(官方 0.495,
  sha256 `4f5eefa3…`,手册无页 → fixed(no-manual)):`FUN_00427350` case
  0x85..0x8e = `_MOUSE_X/Y0..3` 是 4 窗口槽客户区坐标族(槽表
  `DAT_008019c0..cc`,每槽 HWND@+0x5b0/X@+0x5b8/Y@+0x5bc;case 0x85/0x86
  同槽 ⇒ `_MOUSE_X`≡`_MOUSE_X0`);每帧 `FUN_00403ee0`:槽无窗口 ⇒ 坐标 0,
  不报错。修点:`yu-rt/src/sysvar.rs`(读)+ `yu-rt/src/vm/handlers.rs`
  (写路径按钮过滤器排除 X/Y0..3)。
- **CLI 五子命令 `--debug-mode on|off`**:run/serve/dev/case/bench 全接线
  (dev 透传引擎子进程);help 文本补注入口径说明(DIV-L-001)。
- **示例/测试去硬编码**:9 处 `debug_mode: false` 改 `YURIS_DEBUG_MODE=1`
  环境门控(soft first_frame/frame_walk/title_probe/text_probe、wgpu 同名、
  host-desktop、eris_first_screen ×3、coverage_census/gfx_census;
  eris_dump 既有门控保持)。
- **eris_first_screen 双模式**:false 锚点不变(golden 文件 272/8/82/85);
  debug 模式独立 census 断言(299/18/93/104/90,W1/DBG + W2 调试窗 13 层);
  新增 `eris_first_screen_debug_mode_draws_and_reaches_title`(140 帧零错误、
  零 Fatal、主任务 s8 存活、可见层 >0)。

### 2. 验证(原始结果)

```text
$ emit_kernel_events --debug-mode true --frames 300(修复前)
  script58 cmds=0(黑帧);修复后 first at frame 126(2823 条)
  与 debug=false 同帧进入(126)

$ PROBE_DEBUG=1 探针(修复前)f3: ERR task SYSTEM s66 pc634:
  Fatal[rt.sysvar.unimplemented] _MOUSE_X2:工程 ycd 未声明该按钮
(修复后)300 帧零 ERR,SYSTEM 存活 s8 pc37

$ cargo test -p yu-gfx --test eris_first_screen(默认与 YURIS_DEBUG_MODE=1 各一轮)
  4 passed; 0 failed(两轮)

$ cargo test -p yu-rt --lib → 66 passed
$ cargo test -p yu-cli --lib → 13 passed(需 YURIS_TEST_TMP 指向可写目录;
  缺省 temp 不可写是环境问题,project.rs 三例 PermissionDenied 与本批无关)

$ coverage_census 600 8(false 口径)= 7,735,437 命令,与 B6 基线逐字节一致
  (B4 内核改动不碰 false 路径);YURIS_DEBUG_MODE=1(debug 口径)
  = 8,764,407(+1,028,970 = 调试 UI 自身流量;TASKINFO 2,985/CGINFO +370);
  零 Fatal、零新挂号码。gfx_census 双模式同结论(debug +1 域事件挂号)。
  双口径结论落 `crates/yu-rt/docs/coverage-matrix.md` §0(表仍以 false 口径
  为锚,debug 口径以 §0 注为准)。

$ yu-cli case <project-eris> --all → 9 passed; 0 failed
  注:8 条期望漂移与 corpus 2 例(loose/packed)失败经 A/B 隔离
  (HEAD 版 src 产出同哈希/同失败)证实为 B4 之前已有(P8 字体等已入库
  显示变化未重生成期望;散盘语料 143 文件 vs 封包全集),按既有
  `--update` 流程重生成,ops 面无 B4 相关变化。
```

### 3. 台账

- todo-inventory §5 行 3(P0 挂两轮)划掉;audit-2026-10-24 §1 P0-4 划掉;
  DIV-L-001 加 B4 复核节;coverage-matrix §0 双口径 + §3 加
  `_MOUSE_X0..3/Y0..3` 行(fixed(no-manual))。
- 遗留移交:corpus loose/packed 2 例(散盘语料漂移,与 B5 数据入口相关);
  WINDOWINFO 371 vs 129 的 +242 归因(B4 前已存在,随 B2 字段族收口复核);
  B12 口径注:census 现可按 debug 构建口径对拍官方。

## 成果 DIV50 修复撤回 (2026-10-24 下午) · 「快照恒黑」为错误实现,全部还原(主人裁决)

- **裁决**:官方有的弹窗背景就是正常(活的)背景,有一些才是黑屏;
  `COPY2 ID=:/W0/` 恒改窗口基色面的实现会把该显示正常背景的弹窗也盖黑 ⇒ 错误,撤回。
- **已还原**(构建+三 crate 测试全绿,600 帧对拍恢复):
  `yu-gfx/src/{op,state,json}.rs`、golden `eris_first_screen_display.json`、
  `soft/src/render.rs`、`wgpu/src/{lib,passes}.rs`(含 last_screen/backbuffer
  死代码清理,一并回退)。
- **保留的有效取证**:
  - 官方 GOSUB 级补采(div50g,42.9 万条,`probe_dispatch.py` 新增 `--clicks`
    定时点击队列 + `--ysc` 缺参导致 armed-0 的坑):菜单打开与内核**同路径**
    (SS.GOSUB ELSE 臂全序列,官方同样推 CGS 层,幕布 3 tick 淡出);
  - 内核命令级 trace 方法(`YU_CMD_TRACE` 临时探针已回滚,example 三件套留存);
  - 两次快照语义实测像素:冻结帧→(104,137,169) 透出底栏 / 恒黑→(0,4,16)。
- **下一步方向**(DIV-RT-050 文档「候选方向」节):验证官方弹窗纱下是否为
  **活场景**(动画是否继续);若成立,修复点在弹窗期间底层场景层的存活/渲染
  编排,而非快照内容。

## 成果 DIV50 四方向总结 (2026-10-24 晚) · 差异收敛到「CGS 快照内容」+ 弹窗双形态发现

- 主人事实:淡入淡出无恙,唯一问题是「盖不住」;继续多方向排查(本轮零代码改动)。
- **收敛**:双方同路径/同命令/同层栈(f530 层表 vs 官方序列逐项对上,诊断 0、零丢层);
  唯一差异 = 纱下 `:/W0/CGS1` 内容:内核=冻结上一呈现帧(EVID-J3-06 后缓冲解释),
  官方=黑。
- **弹窗双形态**(语料:CONF 系 `.OVER` 场景版 vs `.DIALOG` 叠加版,s52 IF(@53[1]) 分派):
  场景型走幕布 ⇒ 黑;叠加型不换场景 ⇒ 活背景 —— 与主人「有的盖着有的不盖着」吻合。
- 方向裁决:C(缺底板)排除;D(淡出)系调试降速伪影+官方为墙钟毫秒驱动
  (pc471/472 `256×elapsed/时长`),归档;A(根画布黑)与 B(z 语义)留待
  主人确认「正常背景弹窗清单」/官方 DIALOG 型自动截屏/REA 呈现序,三选一即可定案。

## 成果 DIV50 定案 (2026-10-24 深夜) · 黑底/活底双形态机制收口(主人手动点击会话)

- 主人确认图1(黑)=メニュー、图2(活)=タイトルへ;手动会话 trace(div50h)证明
  **两种弹窗官方都推 CGS 栈** ⇒ CGS 内容非区分因素(恒黑被否决的技术根因:
  它无法产生活底形态)。
- 最终机制:可见背景 = 纱下存活层(メニュー系处理器先 `ES.SCENARIO.SS` 关场景 ⇒ 黑;
  タイトルへ系不关 ⇒ 活场景暗化);官方 CGS 栈层不参与可见合成(reserve)。
- 内核缺陷:CGS1(COPY2 冻结帧)被当最底可见层 ⇒ 「盖不住」+ 嵌套时盖活场景。
- 修法(已写入 DIV-RT-050,待主人批准后实施):主窗口根 COPY2 的 Snapshot 目标层
  移出可见合成;FILE 载入的 CGS 层与转场机制不受影响。

## 成果 R3-fix (2026-10-24) · `RETURNCODE` 手动模式恢复判据修复 —— 商业包初始化链贯通

> 触发:主人实测 `<game-b>`(v494/3.3GB 单包)——「开屏 logo 显示成了对话框」
> + 「必须手动点击才推进」。两个症状同一根因。
> 改动:`crates/yu-rt/src/task.rs`(恢复判据)、`crates/yu-rt/src/vm.rs`
> (`text_auto_mode` 实现)、`crates/yu-rt/src/vm/handlers.rs`(失效锚点文案)、
> `crates/yu-rt/tests/fixture.rs`(+1 回归)。

### 1. 定案证据(双版本 REA + 旧线交叉印证)

`RETURNCODE` 处理体尾部置让出位是**无条件**的,但**恢复**判据分模式:

| 版本 | 处理体 | 恢复判据函数 | 关键指令 |
|---|---|---|---|
| 0.495 | `0x44c390` | `FUN_004046fc` | `0x404711 CMP byte[state+0x1c],1` / `0x404715 JNE 0x404828` |
| v494(本作) | `0x44c20c` | 同构 | 头部逐指令对齐(`[0x802ad8]` vs `[0x801800]`),尾部 `C64609` = `MOV byte[esi+9],…` |

- `[state+0x1c]` = `TEXT[OUTPUTMODE]`(0=手動 / 1=自動,手册 `text/text.html`)。
- **`!= 1` 分支直达 `0x404828` → `0x40482d MOV byte[task+9],0`**,不经任何按钮表。
- `== 1`(自动)才进 `0x40471b..0x4047ec` 读 CLICKBUTTON/SKIPBUTTON
  (`0x5b48ac..0x5b48bc`);SKIP 计数器 `-1`(`DEC`)即「当前帧按下沿」、
  CLICK 直接 `OR` 判零。
- 旧线 `<repo-old>` `crates/yuris-vm/src/commands.rs` 独立结论一致:「RETURNCODE
  = 调用点返回标记,**不改控制流**」,实现为纯 pc 推进(v494/v555 双版本同判)。

### 2. 症状链(为什么表现为那两张图)

`es_first.yst` 首指令 `TEXT[OUTPUTMODE=0]` → 框架自己用 `INPUT/MOUSE`
状态机做点击等待。内核把「手动模式的一帧休憩」错实现成「等输入」:

```text
frame 0: s84 pc6  text_rest=true   ← 卡在 キャラ名定義 第一条 RETURNCODE
frame 1..N: cmds=0                 ← 每帧撞同一点,点击才走一两条
```

`es_first` pc4..pc54 的 `.DEFINE` 链因此永远走不完(pc55 `SYSTEMStartCheck`
与 pc123 `es.S.GO "ES.GAMEMAIN"` 到不了)。画面残留 `pc8
ES.GAMEMAIN.DEFINE` 已建的按钮栏 + `es_eris` pc334 `CG[FILE=""]`
(手册 `cg/cg.html`:**1×1 黑图**)被 `CGACT[COPY2]` 铺满 ⇒ 主人看到的
「黑底 + 日期框 + SAVE/LOAD/… 一排」。

### 3. 修法

`TaskExecutor` 增 `text_auto_mode()`(默认 `false`,`Vm` 实现为
`self.text.output_mode == 1`);调度器 text_rest 门控改为:

```rust
if nodes[i].text_rest {
    if exec.text_auto_mode() && !exec.text_advance_requested() {
        continue;
    }
    nodes[i].text_rest = false;
}
```

自动模式行为**逐位不变**(既有 `one_text_line_per_frame_until_advance_input`
原样通过);手动模式回到一帧休憩。

### 4. 验证原始输出

```text
$ cargo test -p yu-rt        # 66+4+11+4+73+3 全绿
$ cargo test -p yu-rt --test fixture manual_outputmode_returncode_resumes_without_input
test manual_outputmode_returncode_resumes_without_input ... ok

$ yu-cli run <game-b> --frames 400 --shot final-title.png
frame: 402 viewport: 1100x825
layers: 234 (visible 11), passes: 73
ops: 79 total, 78 drawn, 1 skipped
textures: decoded=47 cache_entries=49
events: drained=2028539 dropped=0
```

- 帧 1..239 无输入即连续推进(修前帧 1 起 `cmds=0`);
- 帧 240 起画面开始演进;**帧 300 = 完整标题画面**(背景 CG + `<game-b>`
  标题 logo + 六个菜单项 最初からはじめる/続きからはじめる/CGモード/
  回想モード/コンフィグ/終了する);
- 任务面稳定为 `SYSTEM` + `es.IDSubTask` + `es.IDSubTask2` + `es.ScenarioScript`
  (修前恒为单任务 `SYSTEM` 卡死)。

### 5. 既有漂移(非本改动)

`yu-gfx --test eris_first_screen` 的 `eris_first_screen_display_list` 仍失败:
`passes[10]/ops[0]/copy 3 vs 271`。已用 `git stash` 撤掉本改动复跑,
**同样失败** ⇒ 属 PROGRESS 早前记录的既有 golden 漂移(`pass10 copy 271→3`),
需 Owner 复核后 `UPDATE_GFX_GOLDEN=1`。

### 6. 本次顺带查明/未修(挂账)

- **`clear_pending` 无人消费**:`op_returncode` 手动路径置 `self.text.clear_pending`
  (引擎 `+0x36`)与 `0x44c437 CMP.../0x404824 MOV byte[state+0x36],1` 对齐,
  但全仓 `yu-gfx`/`yu-cli` 无读取点 ⇒ 文本缓冲(`\x81@` 全角空格,来自 s84
  的 WORD)跨帧残留。显示侧因 `_TEXT_FLAG` 对齐失败未光栅化(**不影响画面**),
  属 `DIV-GFX-034` 的宿主传播路径缺口,建议与 `sync_text_state` 接线一并处理。
- **`FILEACT[CREATE]`**:工程 ysc 第 3 槽存在、三版手册无页;`yu-save`
  `apply_fileact` 落到 `_ => {}` 静默吞,且 `confined_path` 拒绝 `$_SAVE_PATH`
  的绝对路径形态 ⇒ 开机建目录实际未生效(本作 `save/` 已存在故未暴露)。
- **失效文档锚点已修**:`rt.cmd.unimplemented` 文案原引「任务书 A 的域边界」,
  该文档只覆盖 workspace/yu-log/yu-vfs;改为指向
  `crates/yu-rt/docs/domain-event-contracts.md §9`。
- **表名 CP932 lossy**:`tables.rs` 的 `script_path`/标签 `name` 用
  `from_utf8_lossy` 解 CP932 字节,诊断里显示为 `����`(查表走原始字节,
  语义不受影响,仅定位失效)。
- **探针留存**:`yu-rt/examples/` 新增 `probe_game_tasks` / `probe_jumps` /
  `probe_domain_stream` / `probe_startup_cmds` / `probe_cp932_names` /
  `probe_pack_layout` / `probe_script_map` / `probe_hardcode_audit` /
  `dump_game_cmds`;`yu-cli/examples/game_diag_probe`。全部只读。

## 成果 DIV50 修复落地 (2026-10-24 深夜) · RT-050 修好「盖不住」:透明根画布 + RECTPAINT2 黑幕布

- **机制(三层证据闭合)**:① `COPY2 ID=:/W0/` = 窗口根自有画布=全透明空面
  (REA FUN_0042f809 纯 memcpy;根画布不接收合成内容)⇒ CGS 栈层不参与可见合成;
  ② CGTSS 后固定 `RECTPAINT2 SET=255`(mode 2 只写 alpha,REA FUN_0043185c)
  把幕布拉成不透明黑 ⇒ 淡出=黑幕布淡出;③ 弹窗背景=纱下存活层
  (メニュー系 CG[E=0] 隐藏全部 BT 层⇒黑;タイトルへ系 ev 层存活⇒暗化透出,
  black.png=(0,0,0,128) 半透明纱)。
- **实施**:soft `last_screen`/wgpu `backbuffer` 两套「上一帧后缓冲」机器全部
  移除,主组 CopySource → 透明面(soft `Surface::new([0,0,0,0])` / wgpu
  `window_root_canvas()`);EVID-J3-06 勘误入 title-motion-ledger。
- **验证**:菜单弹窗纱下 (0,4,16) 近黑(官方同量级),淡出=黑幕布,旧屏零透出,
  soft/wgpu 逐点一致;21 测试目标全绿;600 帧标题门禁哈希 0 差异;
  soft vs wgpu 600 帧 0 差异;first-screen golden 不变。
- **待主人桌面实测**:タイトルへ在有背景 CG 处的暗化透出(headless 剧情后
  点击未命中按钮);メニュー淡入淡出手感。
- 临时探针(vm.rs cmdtrace / gfx cgtrace)已全部回滚;取证 example 三件套留存。

## 成果 DIV50 回退 (2026-10-24 深夜,主人令) · 透明根画布方案撤回,暂接受「不盖住」

- 主人桌面实测「タイトルへ」在有背景 CG 处仍显示盖住(黑),下令回退本方案。
- 已还原:`soft/render.rs`、`wgpu/{lib,passes}.rs`(重建通过,无警告);
  EVID-J3-06 恢复后缓冲口径(取证注记留痕);DIV-RT-050 状态回
  open/deferred(master),全部取证、机制、实现与验证记录保留,
  遗留问题(桌面端 タイトルへ 黑底原因)已写入档案。
- 回退后基线:内核回到「冻结旧屏透出」行为(主人暂接受)。

## 成果 CP932-text (2026-10-24) · 字符串操作按 CP932 字符边界统一(片头 LOGO 破名根因)

> 触发:主人实测 `<game-b>`(v494)片头**厂商 LOGO 整段黑屏**;并指出
> 「这个作品图片是日文命名的」——直指编码处理。要求做成**真正的通用**修复,
> 而非单点补丁。
> 改动:新增 `crates/yu-rt/src/cp932_text.rs`(唯一字符边界真源);
> `vm/handlers.rs` 的 `ascii_case`/`fullwidth_case`/`copy_chars`/`cut_chars`
> 及 `case_tables.rs::char_len` 全部改走它。

### 1. 根因(字节级复现)

脚本字符串在核内一律是 **CP932 原始字节**,不是 UTF-8。凡逐字节做 ASCII
大小写/取子串的实现,都会改写双字节字符的**尾字节**——CP932 尾字节覆盖
`0x40..0xFC`,其中 `0x41..0x5A`/`0x61..0x7A` 与 ASCII 字母同码:

```text
サークル = 83 54 81 5b 83 4e 83 8b      ← 厂商 LOGO 真名(cg/ev/サークル.jpg, 293724B 实存)
           ^^ ^^    ^^ ^^
      54='T'   4e='N'                   ← 尾字节落在大写区
逐字节小写 ⇒ 83 74 81 5b 83 6e 83 8b = フーハル   ← 包内不存在的名字
```

链路(实测 trace):
`es_first` → `es.S.GO "SCENARIO_LOGO"` → s142 不存在该标签(Tyn 早已定案,
标签表确认为真缺,非解析截断)→ 片头走 `s142 pc1 GOSUB MAC.EV("サークル")` →
`s66 pc3401 es.SP.PRO.CHECKCG` → `s65 pc8 es._strlower("サークル")` →
`s70 pc215 VARACT[LOWER2]` / `pc218 VARACT[LOWER]` → 名字被毁 →
`es._cg.exist` 的 `FILEINFO[EXIST]` 对 `.bmp/.png/.jpg/.gif/.psb/裸名`
**全部落空** → `SPL*` 层无纹理 → 片头黑屏。

### 2. 通用修法(不是补丁)

新增 `cp932_text` 模块,把「按 CP932 字符推进」收敛成唯一实现:

| 导出 | 语义 |
|---|---|
| `is_lead` | lead byte = `0x81..=0x9F` ∪ `0xE0..=0xFC`(与 `yu-format` 码表同口径) |
| `char_len_at` | 该位置的字符字节数;末尾截断的孤立 lead 按 1(保证前进) |
| `spans` / `char_count` | 字符区间 / 字符数 |
| `slice_chars` | 按**字符**下标取子串(0 起算,超界收敛) |

接入点(全部改为调用上面,不再各写一套):

- `handlers.rs::ascii_case`(`VARACT[UPPER/LOWER]`)—— 双字节字符原样跳过;
- `handlers.rs::fullwidth_case`(`VARACT[UPPER2/LOWER2]`)—— 兜底分支同样跳步
  (`0x82` 全角拉丁区之外的汉字/假名绝不逐字节 ASCII 化);
- `handlers.rs::copy_chars`/`cut_chars`(`VARACT[COPY/CUT]`,即 `_strmid`)——
  字符位置语义;
- `case_tables.rs::char_len`(`VARACT[HANTOZEN/ZENTOHAN]`)—— 指向同一真源。

### 3. 验证原始输出

```text
$ cargo test -p yu-rt --lib
test result: ok. 70 passed; 0 failed    （含 4 个新增 cp932_text 单测）

$ probe_cg_files <game-b> 90
f84  s65 pc99 CG [ID="SPLBUF1" Z=1 FILE="cg/ev/サークル"]     ← 修前为 フーハル
```

单测覆盖:`サークル`=4 字符、`slice(0,1)`=`サ`(完整两字节)、ASCII+双字节混合、
末尾孤立 lead 不越界、越界区间返回空。

### 4. 未完成(另案,不属本条目)

片头 LOGO 仍未上屏,卡点与编码无关:SPA(精灵)定义表变量 `0x0457`
(16384 项)全程为空 —— 该表由本作的 sprite 定义载入路径填充,核内尚未命中
载入点。另 `s66 pc3437 CGACT[ID="SPLBUF401" ID2="SPLBUF1" COPY2=1]` 的目标层
`SPLBUF401` 从未被 `CG` 创建(该分支由 `pc3389` 的 sprite 版本比较决定)。
这两项与 CP932 无关,单独排期。

### 5. 期间发现的构建事故(已恢复)

并行会话改动使 `yu-backend-soft` 报 `family_has_bold` 不存在(该 API 实际在
`yu-font/src/source.rs:362`,HEAD 中即在);`cargo clean -p yu-font` 后重建正常。
排查期间的「layers=2」读数是脏缓存导致的假象,清理后回到 190/192 层。

## 成果 B2 子波 1 (2026-10-24) · 宿主呈现与查询闭环(第一波)—— DIALOG 接线 / CDDA 域 / B4 两移交项

> 任务书:`docs/tasks/roadmap-batches.md` §2 B2。子波 2(WINDOWINFO 字段族 /
> CGINFO[COLOR] / FONTINFO / INFO 025+038)**门控未过停做**(见 §4),
> 其台账行维持原状;本条只销子波 1 的账。

### 1. CDDA 域归类一行修(fixed)

`yu-rt/src/vm/handlers.rs`:`domain_of("CDDA")` 归 `Domain::Sound`(与 MIDI 族
一致);Domain 分支的 aligned-stub 名单补 `CDDA`(0x429fb0),Unknown 分支的
CDDA 特例(死代码)删除。行为不变:域事件照发、诊断 `rt.cmd.aligned_stub`、
600 帧触发 0 次(census 口径不变)。coverage-matrix §8 口径注与 §1/§8 两行
CDDA 行同步销掉「归类错位保留」注。

### 2. DIALOG 宿主接线(S5 销账;H-P1-02 DIALOG 部分 + H-P1-04 销账)

- **Engine 透出**(`yu-cli/src/engine.rs`):`pending_dialogs() ->
  &[DialogRequest]`、`answer_dialog(id, button, text)`。
- **呈现层助手**(`yu-rt/src/dialog.rs`):`caption/text/default_input_display`
  (CP932 严格解码,坏字节逐字节 U+FFFD 降级,不改内核原始字节)、
  `encode_input`(宿主输入框 → CP932)。
- **桌面 egui 模态**(`host-desktop/src/ui.rs::dialog_modal` + `lib.rs` 集成):
  标题/正文/图标/按钮组/DEFBUTTON 聚焦(Enter)/Esc(仅含 Cancel 组合;
  官方 MB_YESNO 不响应)/INPUT 文本框;模态期抑制宿主→引擎输入(官方
  MessageBox 同步阻塞语义);呈现/回答各记宿主日志;待回答时琥珀「卡住」
  横幅让位;第二次关闭强制退出兜底保持。
- **语料事实修正**(S5 档案勘误):菜单「終了する」走**脚本自绘**确认框
  (`ES_ENDCONF.LOOP`;P1 记录的坐标 (217,557) 不准,实测按钮在 (217,522),
  且该框本来就能出——P1 桌面观测的静默挂起来自**窗口关闭**路径:
  WM_CLOSE → `@_WINDOW_EXEC_CLOSE=1` → `es_Scenario.yst:481/1461` →
  `ES.ENDCONF.DIALOG`(門 `@es.GSD(114,01)==1`,STR=`$T(13,02)`
  「ゲームを終了します。よろしいですか？」)。

**证据(原始输出)**:

- headless(Engine 层,临时 save_root 预拷 gsd.sd,不触真实存档):

  ```
  $ cargo test -p yu-cli --test dialog -- --ignored --nocapture
  DIALOG 呈现 @frame25: [(1, YesNo, "確認", "ゲームを終了します。よろしいですか？", "es.IDSubTask")]
  いいえ 后主线仍在 @frame30
  S5 证据: DIALOG#1(いいえ→继续)/ #2(はい→\es.END→GSD.SAVE 落盘)全链通 @36
  test result: ok. 1 passed; 0 failed
  ```

- 桌面实测(`cases/host/b2-dialog-chain.json`:title→設定→戻る→スタート→
  セーブ→close@660;日志 `target/tmp/b2/b2-desktop2.jsonl`):

  ```
  yu-desktop: script frame=660 close #1 → @_WINDOW_EXEC_CLOSE=1
  yu-desktop: DIALOG#1 呈现 buttons=yes_no icon=none caption="確認" text="ゲームを終了します。よろしいですか？"
  ```

  (此前该路径静默挂起、无任何呈现;现 egui 模态弹出,鼠标可点、
  Enter=默认键、Esc=キャンセル。)
- 模态单测:`cargo test -p yu-host-desktop --lib` → 25 passed(含新增
  渲染/点击回答/Esc/INPUT 缓冲 5 用例)。

### 3. B4 移交两件(均已定案)

1. **corpus loose/packed 2 例失败**:`loose_and_packed_eris_are_execution_equivalent`
   在当前树 **通过**(corpus 11/11 全绿)——散盘语料漂移已被 B5 的
   DirSource 折回修复解决。本会话期间的 4 例 yu-vfs dir 测试失败为**环境
   问题**:本机系统 `TMP=E:\Temp` 在沙箱内拒写,`GetTempPathW` 优先读 TMP
   且单行 `export TEMP=… TMP="$TEMP"` 在本 Git Bash 不传播新值 ⇒ 测试进程
   落到拒写路径。AGENTS.md §2 的**两行式 export**(TEMP 与 TMP 分两行)是
   正确形态,设齐后 4 例全过;`YURIS_TEST_TMP` 逃生口同样有效。无代码缺陷,
   不改代码。
2. **WINDOWINFO 371 vs 129 的 +242 归因**(定案,详见 coverage-matrix §2):
   P3(d66ed41)129 → P7(88eea55)371;差异 = `FULLSCREEN` 轮询环 ×121 +
   `FULLSCREEN2` 增量 ×121(参数组合表逐项对账,`EXIST NO=2`×120 与 9 条
   一次性查询两树一致)。轮询环 = `es_first`(yst00000)pc 2278 的
   `IF[@1451==@415/419/423]` 守卫下每帧一对查询(pc 2279/2280,写
   `LET=@1140/@1141`);P7 的 op_math LET 下标修复(POW2 表)+ VARACT
   存在性口径使 `@1451` 落到预期值,轮询恢复。P8 不触 yu-rt,P7 树与 HEAD
   计数逐字节一致 ⇒ B4/B5/B6 无关。实测:census@d66ed41 = 7,735,679/129、
   @88eea55 与 @HEAD = 7,735,437/371(语料 data.ypf FNV `64d15de36b4fff30`
   全程未变)。

### 4. 子波 2 门控未过 → 停做(纪律)

门控条件:`yu-gfx/src/state.rs` 无未提交改动(✓ 满足)+ git log 已含
CGS/WindowBase **收尾提交**(✗ 不满足——最近 CGS 提交以 87dc07f 回退告终,
并行会话对 yu-gfx 测试/渲染后端仍有在飞改动,期间还两次打断编译)。
按任务书停在子波 1 收尾:**WINDOWINFO 字段族 / CGINFO[COLOR]+组 ONMOUSE /
FONTINFO / INFO 025+038 全部未动**,其台账行(DIV-GFX-025/026/036/038、
coverage 三行、audit §2 三行)维持原状并标注「B2 子波 2(待 CGS 落地)」。
CGS 会话落地后按任务书继续(REA 0x4586ec / 0x43d664 取证 → 实现)。

### 5. 验收与测试态(原始结果)

```
$ cargo test -p yu-rt -p yu-gfx -p yu-cli        # 本会话改动后
全部 test result: ok,0 failed(66 lib + 69 fixture + 11 corpus + 10/10/…)
$ cargo test -p yu-host-desktop --lib            # 模态 5 用例在内
25 passed; 0 failed
$ cargo test -p yu-cli --test dialog -- --ignored --nocapture
1 passed(S5 headless 全链)
```

`case --all` = **4/8**(first-screen / record-events-off / title-config-back /
until-visible PASS)。4 例 FAIL(save-title-flow / symptom-enter-story /
symptom-ingame-config / symptom-ingame-save)为 display/png sha 漂移,
与本批无关:B2 不触渲染/存档路径;同期 HEAD 影子树(dd4c179)对拍
9/9 全挂且失败集随运行时点变化 ⇒ 语料工程 `<project-eris>/save/` 是**活
状态**(并行 DIV-RT-050 会话正用官方引擎跑同一工程),B4 门禁的 8/8 是
其时点的存档态。期望值再生需待 CGS 会话落地 + 主人过目(S4 既定纪律)。

### 6. 期间新发现(立档,不在本批修)

- **DIV-RT-052**(yu-rt divergences README):`END` 由非 main 任务执行时
  内核只 Finish 该任务(`es.END` → `GSD.SAVE` 落盘可证执行链通),
  `main_finished` 不亮 ⇒ 程序退出链未闭合。官方 `END`(0x441334)是否
  「结束整个程序」待 REA 取证,归 B10。
- H-P1-02 的 `key_last/mouse_last` 透出仍未做(F1 面板对账用),归 B7。

## 成果 VARINFO-SLOT (2026-10-25) · 槽号按参数名解析(商业包 sprite 全链打通)

> 范围 `crates/yu-rt/src/vm/handlers.rs`、`crates/yu-rt/tests/{fixture,corpus}.rs`、
> `crates/yu-format/tests/corpus_eris.rs`;零 git 写操作。

### 1. 取证:官方 v494 事件级 trace 首次打通

v494 的分派点与 0.495 **不同址**:0.495 是 `0x4044db`/`0x404547`,
v494 是 `0x4044cb`/`0x404537`(早 `0x10`)。定位方法:按字节模式
`ff 14 b9`(`call dword ptr [ecx+edi*4]`)在 `.text` 里扫描,全文件仅两处命中。

```text
$ python tools/official_trace/probe_dispatch.py \
    --exe <game-b>/<game>.exe --cwd <game-b> \
    --ysc <ysc.ybn> --out-dir target/tmp/official/game-b-v494 \
    --seconds 6 --arm-after 0.2 --trigger-rva "" --bp-rva 0x44cb,0x4537
armed 0x4044cb ff / armed 0x404537 ff
...
stop timeout hits 16805
```

v494 的 CG handler = `0x42a4a8`、CGACT = `0x42c438`(handler 直方图 +
ysc 表序交叉确认)。收窄到这两个 handler 后拿到 38964 次命中、最大 pc 6056
(已跑到 `es_sprite.yst` 深处)。

### 2. 定案:不是「同帧先建后删」,是 `es.SP.DRAW` 从未被调用

逐帧对拍(官方 t≈9.50–9.54 s vs 内核 f86):

```text
官方                                 内核            结论
pc3442 CG SPLBUF1                    pc3442         一致
pc3523 CG SPLBUF1 TEX=0              pc3523         一致（该槽无操作，§F）
pc3572/3573 CG SPL1                  pc3572/3573    一致
pc5533 ×100 CG[A=0]                  —              官方执行
pc5582/5583 CG[E=1]/[E=0]            —              ← 官方让 sprite 可见
pc6026/6028/6056 CG/REMALLOC         —              官方执行
```

即:官方同帧里 `es.SP.DRAW` 让 `SPL1` 的 `E` 置 1,内核一次都没调用。
`docs/divergences/DIV-GFX-046-sprite-bg-hold.md` 的 §H/§I/§J 结论
(「同帧建完就删」)作废 —— 那三节引用的命令序列来自**未挂 `GraphicsHost`**
的裸 `Vm` trace,`CGINFO` 的 `LET` 不写回,脚本走假分支。

### 3. 根因:`VARINFO` 参数表跨版本整体位移,槽号被写死

| 参数 | SDK 0.495(ERIS 语料) | 商业包 v494 |
|---|---|---|
| SEARCH | 14 | 14 |
| INT | 17 | **15** |
| FLT | 18 | **16** |
| STR | 19 | **17** |
| NO | 20 | **18** |

`array_needle` 写死 `(Str,19)|(Float,18)|(Int,17)`,`SEARCH` 起始下标写死
`flags_get(&flags, 20)`。在 v494 上两者都落空 ⇒ `SEARCH` 恒返回 `-1`:

```text
es.LNO.GET("BGF")  →  -1（"BGF" 实际在 $3153[4]）
  ⇒ @3743（槽号）恒 0
  ⇒ es.SP.SET.SET / es.SP.GO.SET 登记到槽 0 而非 3
  ⇒ @3394（槽→绘制层映射）全 0
  ⇒ es.SP.DRAW 从不调用
  ⇒ sprite 全不上屏
```

### 4. 修复与实测

`array_needle` 与新增的 `varinfo_no_slot` 改为按**参数名**从工程 ysc 反查
槽号(参数名跨版本稳定,槽号不稳定)。

挂宿主实测(同帧,`examples/probe_host_vars.rs` / `probe_host_cmds.rs`):

```text
指标                                 修复前   修复后
$3153 查 "BGF" 的结果                 -1       4
@3743（槽号）                          0        4
@3270[3]（槽→层号）                    0        4
es.SP.DRAW 调用次数（f84..f86）        0        9
CGINFO[EXIST "SPLBUF1"]                1        1
```

画面:`yu-cli run ... --frames 300 --shot` 出标题画面
(背景 CG + LOGO + 6 项菜单),f86 起不再残留调试 UI。

### 5. 新增取证工具(全部只读)

| 工具 | 用途 |
|---|---|
| `yu-render/soft/examples/probe_host_cmds.rs` | **挂宿主**的命令级 trace(按 sid/pc 区间过滤) |
| `yu-render/soft/examples/probe_host_vars.rs` | **挂宿主**的变量导出(逐帧/指定帧,只打非零项) |
| `yu-render/soft/examples/probe_host_query.rs` | 包装 `GfxHost`,记录每次 `CGINFO` 提问与答案 |
| `yu-render/soft/examples/probe_at_pc.rs` | 命令边界采样变量(定位同帧内的写/清竞争) |
| `yu-rt/examples/probe_ysc_params.rs` | 打印工程 ysc 的命令参数表(槽号取证) |
| `yu-rt/examples/probe_var_refs.rs` | 全脚本扫描谁读/写了某变量 |
| `yu-rt/examples/probe_label_raw.rs` | ysl 标签原始记录(pc/script/base_a/hash) |
| `yu-rt/examples/probe_src_diff.rs` | 散盘 `data/` 与 `data.ypf` 的条目字节比对 |
| `yu-rt/examples/probe_list_cmp.rs` | 两个 `DataSource` 的 `list()` 结果比对 |

### 6. 验收(原始结果)

```text
$ cargo test -p yu-rt --test fixture varinfo_search_resolves
running 1 test
test result: ok. 1 passed; 0 failed

$ cargo test -p yu-rt            # 含样本
lib 70 / consistency 4 / corpus 11 / fixture 75 / golden 全绿

$ cargo test -q --workspace -- --test-threads=1   # 见下节
```

### 7. 台账

- `DIV-RT-053`(yu-rt divergences README):VARINFO 槽号按名解析,fixed。
- `DIV-GFX-046` §K:撤销 §H/§I/§J(附「裸 Vm trace 不可信」的工具层教训),
  记录官方 v494 trace 证据链与修复前后对照。
- `corpus.rs` 首帧金丝雀 `1_292_100 → 1_290_335`:经 `git stash` 撤回本修复
  后实测数字不变 ⇒ **与本次修复无因果**,是当前语料本身偏离旧钉值;
  注释已明确写「不声称因果」。`loose_and_packed` 改为「散盘缺资产 ⇒
  显式 SKIP + 计数」(实测 `data/` 缺 1264 个资产,`ysbin/` 143 项字节全同)。

### 8. 第二处修复:`CGACT[SIZE]` 参数按浮点读(片头 LOGO 上屏)

`VARINFO` 修好后 `es.SP.DRAW` 会被调用了,但画面仍黑。渲染报告:

```text
skip image x2  gfx.imageop.region.partial   :/W0/SPL401
```

`s66 pc6028`(`es.CGACT_SIZE` 内部)的真实实参:

```text
CGACT[ID=SPLBUF401 ID2=SPL401 SIZE=1
      X=0 Y=0 X2=1100 Y2=824.5      ← 浮点
      SX=1101 SY=825.5]             ← 浮点
```

`cgact_image` 用 `find_int` 读这六个参数,而 `EngineValue::as_int` 要求
`fract()==0` ⇒ `Y2=824.5`/`SY=825.5` 被静默判成「未给」⇒ 区域字段数不足
(`region.partial`)+ 尺寸缺失(`size.missing`)⇒ SIZE 整条跳过 ⇒ sprite 层
永远拿不到像素。`.5` 不是怪值:缩放比例含 0.5 是 `es_Sprite.yst` 的常态。

修法:`SX/SY` → `find_f64` 后 `round() as u32`;`X/Y/X2/Y2` → 新增 `find_px`
(浮点四舍五入);`ImageOp.sx/sy` 与 JSON 输出改浮点保值;soft/wgpu 后端
同步(`<= 0.0` 判定 + 取整)。

### 9. 验收(截图,原始输出)

```text
$ cargo run -p yu-cli -- run <game> --frames 90  --shot v3_90.png
ops: 83 total, 83 drawn, 0 skipped
missing assets: 0
png: v3_90.png (2197650 bytes)

$ cargo run -p yu-cli -- run <game> --frames 140 --shot v3_140.png
ops: 133 total, 133 drawn, 0 skipped
png: v3_140.png (3492161 bytes)     ← 片头 LOGO「通行止め」灰底红黑书法

$ cargo run -p yu-cli -- run <game> --frames 300 --shot v3_300.png
ops: 79 total, 79 drawn, 0 skipped
png: v3_300.png (3427152 bytes)     ← 标题画面正常
```

修复前对照:`79 total, 77 drawn, 2 skipped`,f90/f140 全黑,PNG 1.0 MB。
修复后 0 skipped,同一帧的构图与底色与官方 `frame_0020` 一致。

### 10. 挂账(诚实登记)

`@3394` 在**帧末**读到 0 属正常:`pc2940 LET @3394[@48-1]=0` 是
`es.SP.DRAW` 的**收尾清理**(每帧重建 → 绘制 → 清空),官方同构。此前把它
当成缺陷,是因为裸 `Vm` trace 里 `@3394` 从未被写过;修复后帧内序列完整。

### 11. 审查轮(2026-10-25):还有多少同类硬编码

`VARINFO` 只修针值槽是不够的 —— 本轮把它的**全部分派**改成按参数名解析:

| 位置 | 旧写法 | 新写法 |
|---|---|---|
| 查询项筛选 | `!matches!(slot, 17..=20)` 排除辅助参数 | 按名排除 `INT/FLT/STR/NO` |
| 结果分派 | `match slot { 2=>TYPE, 4=>DIMNUM, 5..=12=>DIMSIZE, 13=>LENGTH, 14=>SEARCH }` | `match varinfo_names[slot]` + `DIMSIZE` 尾部编号解析 |

那两处旧写法在 v494 上都错(`17..=20` 应是 `15..=18`,且 `TYPE` 之后的
槽位并不连续成段)。复核 `fixture::varinfo_query_dispatch_follows_param_names`
(参数表整体前移后五个查询项仍正确)。

**同类风险盘点(按名 vs 按槽号):**

| 命令 | 表在 v494/0.495 之间 | 现状 |
|---|---|---|
| `VARINFO` | **位移**(INT/FLT/STR/NO:17/18/19/20 → 15/16/17/18) | 已全部改为按名 |
| `VARACT` | 逐项相同(100 号,槽 0..28 一致) | 仍有 `17..=28` 等按槽号写法,**潜伏风险** |
| `TASKINFO` | 逐项相同(95 号,槽 0..14 一致) | 仍有 `match slot { 2|3|5|6|7|8… }`,**潜伏风险** |
| `CGINFO`/`CG`/`CGACT` | 参数名与顺序稳定 | gfx 侧已按名(`params.resolve`) |

结论:**已知会漂移的只有 `VARINFO` 一处,已收口**;`VARACT`/`TASKINFO` 目前
两版一致,但写法仍是按槽号,新版本若在它们中间插入参数就会静默走错分支。
这两处列为观察项,等有第三份语料(不同版本)时再按名收口 —— 现在改属于
无证据的重构,不做。

### 12. 收口(2026-10-25):三处按槽号分派全部改为按参数名

§11 盘出的 `VARACT` / `TASK` / `TASKINFO` 三处(两版目前一致但写法脆弱)
本轮一并改为按名解析 —— 与其留观察项等第三份语料,不如现在就消除机制性
风险(参数名是 `ysc.ybn` 里的一等事实,不是猜的)。

| 命令 | 旧写法 | 新写法 |
|---|---|---|
| `VARACT` | `flag(6)`/`flag(13)`/`flag(14)`/`num(4)` 等共 16 处数字;`INIT`+`G_*` 用 `17..=28` 区间与 `slot-17/21/25` 算族和位 | `flag("TYPE")`/`flag("DIMSIZE")`/`flag("PUSH")` 等;`G_INT/G_FLT/G_STR` 按**前缀匹配 + 名字尾部数字**定位族与位 |
| `TASK` | `slot @ 4..=13` / `14..=23` / `24..=33` 三段区间 + `slot-4/14/24` | `TINT`/`TFLT`/`TSTR` 前缀匹配 + `task_family_bit()` 取位号 |
| `TASKINFO` | `match slot { 2/3/5/6/7/8/16/17/18/19 }` | `match names[slot] { "EXIST"/"E"/"Z"/"FILE"/"FILENO"/"LINE"/"IFNEST"/"GOSUBNEST"/"LOOPNEST"/"SCRIPTPOS" }` |

**过程中自曝一个我自己引入的 bug**:`INIT`+`G_*` 的守卫最初写成
`names.iter().any(|n| n.starts_with("G_INT") …)` —— 那是「表里有没有这个
名字」,恒真,导致每条 `VARACT` 都被误判进该分支并提前返回。`fixture` 的
4 个 VARACT 用例立刻红(`varact_hantozen_…`、`var_add_macro_chain_appends`
等)。改成看 `flags`(本次指令**实际给了**哪些槽)后全绿。这条留作记录:
「按名解析」必须配「在场判定仍然基于本次窗口」。

**零漂移验证(逐字节):**

```text
f140  重构前 3492161 bytes  →  重构后 3492161 bytes
f300  重构前 3427152 bytes  →  重构后 3427152 bytes
ops: 133/133 drawn 0 skipped(f140) / 79/79 drawn 0 skipped(f300)
```

```text
cargo test -p yu-rt --lib      70 passed
cargo test -p yu-rt --test fixture  76 passed
cargo test -p yu-rt --test corpus   11 passed
```

### 13. 全项目硬编码扫描结论(2026-10-25)

扫描口径:按**槽号**分派、按 **opcode** 数字分派、按**系统变量 id** 分派。

| 类别 | 结果 |
|---|---|
| 命令 → 处理体 | **按名**(`HandlerKind` 由 `cmd.name` 匹配,`handlers.rs:230+`)✅ |
| 参数 → 语义 | `VARINFO`/`VARACT`/`TASK`/`TASKINFO` 已全部按名 ✅;gfx 侧(`CG`/`CGACT`/`CGINFO`…)本来就走 `params.resolve(name)` ✅ |
| opcode 数字 | 仅测试文件(`fixture.rs`/`golden.rs` 的 `OP_*` 常量),生产代码零处 ✅ |
| 系统变量 id | `sysvar.rs` 全部按**名字** match(`_LC`/`_PINT`/…)✅;id→名字的映射来自工程 `ycd` 或内置 ABI 族 TSV,按 `ysv.ybn` 指纹自动选 ✅ |
| `rec.kind == 0/1` | `op_end`/`op_taskend`/`op_let` 里的 0/1 是**命令自身参数表的前两位**(`RESTART`/`ID`、`SET`/`LET`),由 `ysc` 声明,属结构常量;已确认两版一致 |
| `slot >> 8` | 复合赋值码位(`LET`/`DIMSIZE+=`),是窗口编码而非参数序号 ✅ |

结论:**生产代码里已无「按槽号区间分派」的残留**。剩余数字都是
(1) 结构常量(参数表前两位)、(2) 窗口编码、(3) 测试夹具。

新增版本若在 `VARINFO`/`VARACT`/`TASK`/`TASKINFO` 之外**新增**参数,
按名解析自动适配;若在这些命令**中间插入**参数,也不会再走错分支。

### 14. 全方位硬编码审查(2026-10-25)—— 含 1 处真雷

口径修正:上一轮把「没证据」当成不动的理由,不对。**能从游戏自带数据
读出来的东西,游戏数据就是最高档证据**,读它不需要额外取证。按这个标准
把全项目过了一遍。

#### A. 找到并修掉的**真雷**:YPF 版本上限

```text
// 旧代码(会拒掉真实商业作品)
pub const MAX_ACCEPTED_VERSION: u32 = 495;
if version > MAX_ACCEPTED_VERSION { → Unimplemented, 拒绝启动 }
```

这一条与项目**自己的 REA 证据冲突**:`FUN_0045e3b8`(0.495 `yu-ris.exe`,
sha256 `4f5eefa3…`)逐字节无版本分支 —— 索引起点恒 `0x20`、步长恒
`nameLen + 0x1B`、长度码表恒 `T[0xFF - lenEnc]`、名字逐字节取反,
`version` 只被读入从不参与分支(见 `docs/tasks/task-e-fixes.md` §4)。
项目自己的 `equivalence.rs` 也早就有 「version=300 必须能读」的用例。

而 `<game-a>` 的 `data.ypf` 是 **YPF v500** —— 旧代码会直接拒启。

修法:去掉上限闸门,常量改名为 `VERIFIED_MAX_VERSION` 并注明「仅信息,
不是门禁」;真正的把关交给版本闸门之后已有的结构校验
(entry_count 上限、索引容量、`detect_layout` 方言探测、名字
MurmurHash 核验)。新增用例 `engine_layout_with_version_500_is_readable`
(同布局 + v500 ⇒ 正常读取,版本号原样透传)。

#### B. 补掉上一轮漏改的一处槽号

`VARACT[DIMSIZE]` 的复合码位取值用 `.find(|w| w.rec.kind == 13)` ——
我上轮改 `flag("DIMSIZE")` 时漏了这行。已改为 `slot_of("DIMSIZE")`。

#### C. 全项目扫描结果(分类)

| 类别 | 扫描结果 | 判定 |
|---|---|---|
| 命令 → 处理体 | `HandlerKind` 由 `cmd.name` 匹配 | ✅ 动态 |
| 参数 → 语义 | `VARINFO`/`VARACT`/`TASK`/`TASKINFO` 已按名;gfx 侧本来按名 | ✅ 动态 |
| **屏幕尺寸** | `yscfg.ybn` 的 `+0x10/+0x14`(本作实测 1100×825) | ✅ 动态 |
| 图像/音频扩展名序 | `yscfg.ybn` 的 `image_ext_order`/`sound_ext_order` | ✅ 动态 |
| **系统变量 id→名字** | 游戏自带 `ycd`,否则按 `ysv.ybn` 指纹从 4 个 ABI 族自动选 | ✅ 动态 |
| **YPF 版本** | ~~上限 495 硬拒~~ → 无上限,结构校验把关 | ✅ **本轮修复** |
| YPF 版本下限 234 | 早期布局无任何样本 | 保留(拒绝优于硬套) |
| 图层名 `W0` | 手册 `basic/layer.html` 规定的根路径 | ✅ 格式常量 |
| 命令名 `SPL`/`CG` 等 | 脚本字符串常量,引擎无感 | ✅ 非实现 |
| opcode 数字 | 仅测试夹具 `OP_*` 常量;生产零处 | ✅ |
| `ysv` 的 `kind==1/2` | `ysv.ybn` 文件格式字段(全局/局部) | ✅ 格式常量 |
| `slot >> 8` | 复合赋值码位,窗口编码 | ✅ 格式常量 |
| `rec.kind == 0/1` | 各命令参数表前两位(由 ysc 声明) | ✅ 格式常量 |
| 默认字号 20 | REA `FUN_00464fac` + `docs/font-contract.md` §3.3 | ✅ 引擎内置 |
| fps=60 | `yscfg` 无此字段;官方为宿主时钟基准,脚本可 `FPS[SET]` 覆盖 | ✅ 合理默认 |
| 字体名 | 脚本传入,`find_family` 按名查(全角/空格/连字符折叠) | ✅ 动态 |
| 音频采样率 | 从 ogg/wav 头读,`channels==1` 只是单声道分支 | ✅ 动态 |
| 存档格式版本 `0x1E0` | v494 实测 5 个存档全部 `0x1E0` | ✅ 已实测 |
| `YSTD` 版本 495 | 该解析器不在装载链上(仅 yu-format 独立 API) | 低风险,记录 |

#### D. 验证(零漂移)

```text
f140  3492161 bytes（与重构前逐字节相同）
ops: 133 total, 133 drawn, 0 skipped

cargo test -p yu-rt --test fixture   76 passed
cargo test -p yu-vfs                 42 + 14 + 2 + 5 passed
```

### 15. 代码质量审查(2026-10-25)—— 自审抓到 2 个我自己引入的 bug

按「逐行读 diff + clippy + 全测」过了一遍本会话全部改动。**找到并修掉
两个由本轮重构引入的真实错误**,均属"按名解析"改造的副作用:

#### BUG-1 `G_INT2..G_INT4` / `TINT2..TINT10` 位号错位一格 ★会产生错误行为

改造时把「槽号区间」换成「后缀数字」,但**后缀 N 不是索引 N**:

| 名字 | 应得索引/位 | 第一版实际 |
|---|---|---|
| `TINT` | 0 | 0 ✅ |
| `TINT2` | 1 | 2 ❌ |
| `TINT10` | 9 | 10 ❌(越界,被 `bit < 16` 放过但写错槽) |
| `G_INT2` | 掩码 bit 1 | bit 2 ❌ |
| `G_INT4` | 掩码 bit 3 | bit 4 ❌(被 `bit >= 4` 丢弃 ⇒ **整组不恢复**) |

后果:`VARACT[INIT=1 G_INT4=1]` 完全不生效;`TASK[TINT2=x]` 写进错误寄存器。
**这是静默错误行为,不是崩溃。**

修法:新增 `family_index(name, prefix)` —— 后缀 N ⇒ 索引 `N-1`(与
`sysvar::param_index` 的 `_TINT(n)` 读 `n-1` 同口径)。`g_mask` 亦改用
它,并在注释里写清「掩码位 = 组号 - 1,与 `restore_globals` 的
`1 << (group-1)` 对齐」。

新增回归 `fixture::varact_g_int_group_bits_follow_suffix`:四组变量、
只给 `G_INT2`,断言只有组 2 恢复、组 1/3/4 保持改动后的值。
**该用例在修复前是红的**(实测 `组2 保持 99`),修复后绿。

#### BUG-2 `TASK` 入口标签槽的名字是 `#` 不是空串

第一版把该槽写成 `Some("")`,但两版 ysc 实测参数名都是 `#`
(v494 `TASK` 表 `#34  # kind=String`)。`Some("")` 永不命中 ⇒
`TASK` 的 `#=` 入口标签被当成"未实现参数"直接报错。

修法:`Some("#" | "")`,注释写明 `#` 是实测名、空串为防御保留。

#### 连带发现:夹具的 `TASK` 参数表失真

修 BUG-2 时三个 TASK 用例同时红。起因是**测试夹具把真实 38 项的
`TASK` 表压缩成了 18 项**(只留 TINT 十项),而夹具用真实槽号构造窗口
(`kind=34` = `#`、`35` = `SCRIPTPOS`)。旧代码按槽号硬匹配真实布局,
恰好"能过" —— 这掩盖了夹具与真实数据不一致。

已把 `tests/support/mod.rs::task_params` 按真实 38 项布局展开
(`TINT2..TINT10`/`TFLT*`/`TSTR*` 全在),并写明表格。之后 77/77 全绿。

> 这一条本身值得记:夹具失真会让"按槽号"的实现看起来永远正确。

#### 代码卫生(顺手清)

| 项 | 处置 |
|---|---|
| 37 个临时探针中 11 个已被后续探针取代(裸 `Vm` 版) | 删除(`probe_arr_full`/`probe_sprite_state`/`probe_str_arr`/`probe_var_ints`/`probe_cmd_filter`/`probe_logo_gate`/`probe_hardcode_audit`/`probe_scan_bytes`/`probe_str_writes`/`probe_jumps`/`probe_var_dump`) |
| `vm.rs` 多出的连续空行 | 删 |
| `corpus.rs` `is_dir() == false` | 改 `!is_dir()` |
| `probe_var_refs.rs` needless borrow | 改 `window_payload(w)` |
| `dump_pc_cmds.rs` / `dump_game_cmds.rs` unused import | 删 |
| 调试用 `YURIS_DBG_TASK` 环境变量分支 | 全部移除(源码零残留) |

#### 验证(原始输出)

```text
$ cargo clippy -q -p yu-rt -p yu-gfx -p yu-vfs --all-targets
仅剩 2 条既有告警(非本会话引入):
  - scan_gosub_callers.rs:22  fn ysbin_dir 未使用
  - result_large_err(既有 API 形状)

$ cargo test -q -p yu-rt -p yu-gfx -p yu-vfs
全部 ok:70 lib + 77 fixture + 11 corpus + 42/14/2/5 vfs + gfx 各套

$ yu-cli run --frames 140 --shot
ops: 133 total, 133 drawn, 0 skipped
png: 3492161 bytes   ← 与重构前逐字节一致
$ yu-cli run --frames 300 --shot
ops: 79 total, 79 drawn, 0 skipped
png: 3427152 bytes   ← 同上
```

结论:**两处 bug 都已修且有用例钉住;画面零漂移。**

### 16. 同类错位扫描 + 静默处理 TODO 加料(2026-10-25)

按「`G_INT2`/`TINT2` 那个错位还有没有别处」扫了全项目的
「名字→索引」族,并补掉一处**真缺陷**。

#### 新修:`RETURN` 缺边界守卫(会 panic)

`GOSUB` 侧有 `1..=16` 守卫(`RINT0` 会 `usize` 下溢、`RINT17+` 会越界),
`RETURN` 侧**没有**。抽成共用 `param_slot_index(name, slot, command)`,
两处同口径,违者 Fatal 不 panic。

#### 新增三道行为锁(都能证伪)

| 用例 | 钉住 |
|---|---|
| `fixture::varact_g_int_group_bits_follow_suffix` | `G_INTn` ⇒ 掩码位 `n-1` |
| `fixture::task_tint2_value_reaches_tint2_consumer` | `TASK[TINT2=7]` ⇒ `@_TINT(2)` 读到 7 |
| `fixture::name_suffix_families_are_one_based_everywhere` | `RETURN[RINT2=42]` ⇒ `@_RINT(2)` 读到 42 |

**证伪方式(实际执行,非推演)**:把实现改回错误版本,三条全红
(`left: 99/0/0` vs `right: 2/7/42`);恢复后全绿。这条写进台账是为了让
以后的人能重复这个验证,而不是信"作者说对了"。

#### 扫描结论

| 族 | 口径 | 状态 |
|---|---|---|
| `PINTn`/`RINTn` | `n-1` | ✅ 两侧统一 + 有守卫 + 有端到端用例 |
| `TINTn`/`TFLTn`/`TSTRn` | `n-1` | ✅ 有端到端用例 |
| `DIMSIZEn` | `n-1` | ✅ 有 VARINFO 分派用例 |
| `G_INTn`/`G_FLTn`/`G_STRn` | 掩码位 `n-1` | ✅ 有用例 |
| `PAD_*` | 两侧传原始 `n`,换算在 `input::pad_*_code` | ⚠ 同函数 ⇒ 无错位,但**无端到端用例** |
| `KEY_*`/`MOUSE_*` | 名字→码 | ⚠ 未逐个核 1/0 基;已登记 |

#### 文档

`docs/todo-silent-handling.md` 追加 S7–S9:

- **S7** 名字后缀被当索引(2 次实例 + 结构性消除 + 仍存风险的族清单);
- **S8** 静默错值比静默跳过难查 —— 列了三条"加料"方案(事件带落位 /
  写过的索引集合统计 / 调试构建值域断言);
- **S9** 夹具与真实数据失真会掩盖错位(`TASK` 表 18 vs 38 已修);
  建议加「夹具参数表 vs 真实 ysc 一致性」测试,一次性暴露所有压缩表。

#### 验证

```text
cargo test -p yu-rt --lib         70 passed
cargo test -p yu-rt --test fixture 79 passed
```

## 成果 ARCH-V2 提案 (2026-10-25) · 跨平台目标架构图(§2.5,待主人定稿)

- `docs/architecture.md` 新增 §2.5(文档头部已加 v2 提案注记):
  - 分层图 L0–L4:内核中立层 / 能力端口(AssetSource·AudioSink·Clock·SaveIo·
    PlatformSvc)/ 新 crate `yu-engine`(唯一装配,现 `yu-cli/src/engine.rs`
    上移,解 CLI↔桌面宿主依赖环)/ 宿主层 / 入口层;
  - 桌面并入 `yu-cli --mode desktop`(运行期开关,`yu-desktop` bin 退役);
    wasm = 同一代码基的**编译期**派生(新 wasm 壳,浏览器无 argv 不可能是
    运行期开关——已与主人对过);
  - cpal 声音下沉 `host-desktop`;`@_OS_SOUND` 注入与实际出声解耦(对拍恒 true);
    wasm 暂 NullSink;
  - 含 v1.3→v2 差异表、平台矩阵、端口契约示意、设计规则 R1–R6(候选不变式)、
    迁移步骤 H1–H6(每步带退出条件)、开放问题 5 条(命名/双模式/存档选型/
    wgpu-web/feature 集)。
- 状态:**提案,未动任何代码**;主人更新定稿后按 H1–H6 落地。

#### 验证

```text
纯文档改动,无代码/测试变更(mermaid 语法人工校对;渲染以主人预览为准)。
```

#### 追记(同日 · Android 等编译目标)

- 主人追加 Android 等移动编译目标;§2.5 同步扩:
  - L4 增 `yu-host-android`(aarch64-linux-android 等 NDK 目标,android_main
    入口,与 wasm 同为编译期派生);L3 增 `host-android` 宿主
    (winit/android-activity + wgpu Vulkan/GLES + cpal/oboe +
    AAssetManager→MemSource + 内部存储存档 + 挂起/恢复生命周期);
  - 平台矩阵、v1.3→v2 差异表、迁移步 H7(依赖 H4,可与 H5 并行;退出条件
    = 真机跑通同工程且帧 hash 与 native soft 一致、挂起/恢复不丢状态)、
    开放问题 6–10(壳组织/分发形态/v7a/触摸映射/iOS)一并补齐;
  - R4 改为「单一桌面入口」:wasm/Android 均为编译目标,无运行期 mode。
- 架构结论不变:新平台 = L3 宿主 + L1 端口实现,内核与 `yu-engine` 零改动;
  Android 唯一引擎侧新需求 = 挂起/恢复生命周期(复用 restart/DIV-HOST-001
  机制)。

## 成果 FONT-DIAG (2026-10-25) · 字体解析可见化 + `--font` 强制字体 + `--log-level`

### 动机

此前完全没有"这局游戏用了哪个字体"的日志:字体诊断只进每帧 `text_diags`
(`text.font_init` / `text.font_name_fallback`),默认 `warn` 级看不到,
换机器/换字体后无从判断命中项。同时缺一个能换字体做对照的手段。

### 改动

| 位置 | 内容 |
|---|---|
| `yu-font::FontSource` | `FaceInfo`/`FaceRecord` 增 `source_path`(fontdb `Source::File`,`from_bytes` 为 `None`);新增 `describe_face`(family/postscript/weight/style/ttc_index/origin/文件路径一行)、`default_family_hit`(引擎回退表实际命中项)、`halfwidth_is_monospace` |
| `yu-backend-soft::TextComposer` | `ensure()` 成功时产出字体源摘要并 `info!` 输出(target `yu-font`);`resolve_face` 每个不同 FONT 请求**首次**解析时记一条(请求 → face → 文件);新增 `set_force_font` / `force_font`;`log` 依赖 |
| `TextComposer::set_force_font` | 强制所有 FONT 请求走指定 family,清 face 缓存;family 不可解析 → 硬失败(不回落默认表,避免"以为换上了") |
| `Engine` / `EngineOptions` | 新增 `force_font: Option<String>`、`warm_text()`、`font_summary()`、`set_force_font()` |
| 宿主 | `yu-desktop --log-level <lvl>` / `--font FAMILY`;`yu-cli` 全局 `--log-level`(`--log-level=X` 亦支持)+ `run`/`serve` 的 `--font` |

日志级别优先级:`--log-level` > `YU_LOG` > `warn`。

### 关键发现:`--font` 换比例字体会破坏居中(非渲染缺陷)

脚本自己算居中(`es_Text.yst`「中央揃え」):
`X = 框宽/2 - 字符数 * SX/2` —— **假定每个字符宽恰好 `SX/2`**。

本机 `msgothic.ttc` 三个 face 的半角度量:

| family | 半角 advance(SX=20) | 等宽 |
|---|---|---|
| MS Gothic | 全部 10.0(=SX/2) | 是 |
| MS UI Gothic | E=11.3 m=14.7 t=7.0 o=10.2 | **否(比例)** |
| MS PGothic | 同 MS UI Gothic | **否(比例)** |

故:默认 `MS Gothic` 下居中**精确**;`--font "MS UI Gothic"` 因比例度量
与脚本等宽假设冲突,文本整体偏移(实测 800×600 下 `dX≈-18px`,
1200×900 窗口缩放后 ≈-27px)。`halfwidth_is_monospace` 会在强制到比例
字体时发 `warn`,把这一点显式说出来。

### 验证(全部实机,release)

```text
# 默认字体:脚本公式自洽 ⇒ 居中 0px 误差
[font] system fonts: 248 faces enumerated in 141ms; engine default family "ＭＳ ゴシック"
       -> family="MS Gothic / ＭＳ ゴシック" postscript="MS-Gothic" weight=400 ttc_index=0
          file="C:\Windows\Fonts\msgothic.ttc"

# 强制字体生效(请求侧保留脚本原意)
[font] forced by --font: all FONT requests -> family "MS UI Gothic"
[font] Default(script gave no font) -> exact family="MS UI Gothic / MS UI Gothic" ttc_index=1

# 比例字体警告
[log:warn] [font] forced family "MS UI Gothic" has PROPORTIONAL half-width metrics;
           script-computed centering (X = box/2 - len*SX/2) assumes monospace SX/2 ...

# 查不到的 family = 硬失败
[log:warn] [font] forced family "No Such Font" not installed; every FONT request will fail
[log:warn] [font] Default(script gave no font) -> unresolvable
```

居中自洽性验算(layer 局部 X 反推框宽,与像素实测 585~586 一致):

| 字符串 | 字符数 | X | 反推框宽 | 实测 adv | 期望 adv | 文本中心 | 框中心 |
|---|---|---|---|---|---|---|---|
| β４機能紹介 | 6 | 230 | 586 | 126 | 126 | 293.0 | 293.0 |
| Ｅ－ｍｏｔｅ機能紹介 | 10 | 188 | 586 | 210 | 210 | 293.0 | 293.0 |
| 各種フラグセット | 8 | 209 | 586 | 168 | 168 | 293.0 | 293.0 |
| タイトルへ戻る | 7 | 220 | 587 | 147 | 147 | 293.5 | 293.5 |

```text
cargo test -p yu-font -p yu-backend-soft   → 全绿(22/15/18/1/5/7/1/12/1/1)
cargo build --release -p yu-host-desktop -p yu-cli  → OK
```

### 用法

```text
# 看字体解析链(默认 warn 看不到,必须 info)
yu-cli run <root> --frames 5 --log-level info
yu-desktop <root> --backend wgpu --log-level info

# 强制字体(诊断用;忽略脚本 NAME/NO/BNO)
yu-desktop <root> --backend wgpu --font "MS Gothic"
yu-cli serve <root> --font "MS Gothic"
```

`--font` 是**诊断开关**,不是正常游戏路径;等宽半角字体(MS Gothic)才能
保持脚本居中,比例字体请预期偏移(warn 会说明)。

### 追记(同日)· 等宽字体实测:换等宽字体后居中恢复

`--font` 换成**全角等宽**字体验证(判据见 `FontSource::fullwidth_is_monospace`:
脚本假定「每字符宽 = SX」,故要测全角样本而非半角)。

800×600、title 菜单 5 行,墨迹中心 vs 按钮框中心(实测像素):

| 字体 | β4機能紹介 | Ｅ－ｍｏｔｅ | 各種フラグ | タイトル | 終了する | 最大偏移 |
|---|---|---|---|---|---|---|
| MS Gothic(默认) | +1.5 | +2.0 | 0.0 | +1.0 | 0.0 | **2.0** |
| Microsoft YaHei | -2.5 | +3.0 | -1.0 | +1.0 | -0.5 | **3.0** |
| SimSun | +3.0 | +2.5 | 0.0 | +1.5 | -1.5 | **3.0** |
| MS UI Gothic(比例) | -2.0 | **-18.5** | **-18.0** | **-15.0** | -3.5 | **18.5** |

⇒ 全角等宽字体(MS Gothic / YaHei / SimSun)偏移都在 ±3px 的取整噪声内;
比例字体(MS UI Gothic)偏移达 18.5px。**换等宽字体后对齐正常。**

字体粗体面实查(`family_has_bold`,weight>=600):

| family | 真实粗体面 | 全角等宽 |
|---|---|---|
| MS Gothic | **无** | 是 |
| SimSun | **无** | 是 |
| Microsoft YaHei | **有** | 是 |
| Yu Gothic | **有** | 是 |
| MS UI Gothic | 无 | 否(比例) |

⇒ SimSun 不显示粗体不是缺陷:该字体本来只有 weight=400,
与 MS Gothic 同况(`text.font.bold_missing` 早已显式报告)。
**本机同时满足「有真粗体 + 全角等宽」的推荐项:Microsoft YaHei / Yu Gothic。**

判据修正:`halfwidth_is_monospace` → `fullwidth_is_monospace`,
样本由半角 `A/i/m/W/0/.` 改为全角 `あ/ア/機/４/Ｅ/－/ｍ/ッ`
(脚本公式针对的是全角等宽假设;半角判据会误判)。

## 成果 ARCH-H1H3 (2026-10-25) · 架构 v2 快速重构落地:桌面并入 CLI + 引擎装配抽 crate + 声音下沉平台

主人指示:新平台只预留不实现,当前只做桌面;启动参数按模式解耦;补全 help。

- **H1 抽 `yu-engine`(新 crate,15 号 workspace 成员)**:`engine.rs`(boot/tick/
  restart/Engine/EngineOptions/FrameReport/DiagReport)+ `project.rs` + 宿主公共件
  (`output/json/sha`)整体上移;`Failure/CliResult/EXIT_*` 一并迁入,yu-cli 全量
  re-export(`yu_cli::engine` 等旧路径零改动兼容)。`json_escape` 升 pub。
- **H2 声音下沉(解耦决策落地)**:cpal `SoundSink` 从 yu-cli 移入
  `yu-host-desktop`,实现新端口 `yu_engine::engine::AudioOut`(attach/detach;
  不要求 Send——cpal Stream 部分平台刻意 !Send,Engine 本身含 Rc);
  `Engine::attach_audio(Box<dyn AudioOut>)` 注入,restart 自动重挂(原
  detach→boot→attach 时序原样保留)。yu-cli `device` feature 与 cpal 依赖删除,
  **headless/serve/case/bench 一律静音**;`@_OS_SOUND` 注入恒 true(原桌面
  `--no-sound` 绑定 os_sound 的写法解除,`--no-sound` 只关出声)。
- **H3 桌面并入 CLI(参数按模式解耦)**:`yu-cli run --mode headless|desktop`
  (缺省 headless);桌面旗标与 headless 旗标互不认识,写错边 `finish()` 显式
  报「未知参数」(已双向验证)。原 `host-desktop/src/main.rs`(240 行参数壳)
  移植为 `yu-cli/src/desktop.rs`(feature=desktop,默认开;
  `--no-default-features` 构建显式报错);`yu-desktop` bin 删除,host-desktop
  只剩 lib(依赖 yu-cli → yu-engine;serve_probe 示例以 dev-dep 回用 ws_client)。
- **HMR bin 更名**:`yu-engine` bin(只跑 serve)→ `yu-serve`(让位新 crate),
  `dev.rs`/`engine_main.rs` 同步。
- **log 门面归属**:`run --mode desktop` 时 main 不抢 `yu-log` 门面(init 时读
  `YU_LOG_JSONL` 接管 JSONL 文件,先 init 会挤掉文件 sink);其余路径 main 统一接管。
- **help 三态**:总览(`help`/`-h`/`--help`)、逐命令(`help <command>`)、就地
  (`<command> --help`);全文按新模式结构重写(全局 `--log-level` 保留)。
- 预留(未实现,按主人指示):wasm/Android 编译目标——架构槽位 = `yu-engine`
  唯一装配 + `AudioOut` 端口范式,见 `docs/architecture.md` §2.5。

#### 验证

```text
cargo check --workspace --all-targets          # 通过(仅 yu-rt scan_gosub_callers 既有警告)
cargo build -p yu-cli --no-default-features    # 纯 headless 构建通过
cargo test --workspace(语料 env 齐备)          # 55 套 ok
  - 环境项:YURIS_TEST_TMP 需指向可写 tmp(系统 TEMP 不可写,既有约束);
    YURIS_SAMPLE_ERIS_ROOT 补齐后 yu-save golden 过。
  - yu-trace synthetic_trace_matches_golden FAILED:语料 YSCom.ycd sha256 与库内
    golden 漂移(期望 29957e15..,实际 39ba92ef..)——先于本次重构,未触碰
    yu-trace/语料,待台账跟进(疑语料再生未同步 golden)。
./target/debug/yu-cli.exe help | help run | run --help            # 三态输出正常
run <eris> --fullscreen                          # => 未知参数:--fullscreen(headless 拒桌面旗标)
run <eris> --mode desktop --json x               # => 未知参数:--json(桌面拒 headless 旗标)
run <eris> --frames 10 --shot headless.png       # engine: backend=soft ... frame: 12, png 540267B
run <eris> --mode desktop --frames 5 --no-sound --shot desktop.png
                                                 # wgpu 窗口 5 帧自动关,png 540267B(与 headless 同帧同大小)
```

## 成果 DEMO-PLAY2 (2026-10-25) · demo 可玩性第二轮:文本循环/字体页/窗口·文件查询收口

> 承接 `docs/audit-2026-10-25.md` 的修复顺序;证据文件在 `target/tmp/`
> (desk2.jsonl 逐帧追踪、r2_s4.json、full950.json);交接文档
> `docs/handoffs/H-DEMO-PLAYABLE-investigation.md`。

### 根因链(桌面实测坐实)

VM 层剧情推进一直是好的(逐帧追踪:bytes 32→76→170→…→707,9 句全走完,
`es.ScenarioScript` 每句 `TASK[E=0]` 暂停/点击恢复是设计行为)。用户观感的
「循环/不推进」= 呈现与文本缓冲链:

1. `RETURNCODE` 手动模式置 `+0x36`(清缓冲请求),内核**只写不读**;
2. 官方在文本让出释放点消费它(`0x404828 call 0x4638e4`:清 `$_TEXT`/
   `$_TEXT_FLAG`+内部读位);缺它时 `es.TX.OUTPUT.Z` 把读指针重置回 1 后
   从**累积缓冲开头**重放 ⇒ 永远叠第一句且位置逐轮漂移;
3. `$_CHR(n)` 未实现(46,696 次)⇒ `C/R/P/E` 控制符判断全走 else;
4. `TASK[RESTART=1]` 未实现 ⇒ 行末不请求续帧(REA `0x450c55`)。

### 修复清单

| 修复 | 锚点 | 效果 |
|---|---|---|
| 文本让出释放消费 `+0x36`(`Vm::consume_text_clear_request`) | REA `0x404828→0x4638e4` | **文本正常逐句推进**(r2_s4.json 实测第 5 句三行齐全) |
| `$_CHR(n)` = 单字节构造(`fixed(no-manual)`) | REA `0x446b93`;es_Text.yst:878 | 控制符识别恢复,46,696 次挂号清零 |
| `TASK[RESTART=1]` → `RequestExtraFrame`(`fixed(no-manual)`) | REA `0x450c46/0x450c55`;手册无页 | 行末同 vsync 续帧 |
| 入口标签 CP932 编码(`BootConfig::entry_label`) | `LabelTable::get_bytes` | 非 ASCII 标签可直连取证 |
| `FONTINFO[NUM/LANG/NAME NO=]`(gfx `fixed(no-manual)`) | EnumFontFamiliesExA 语义;手册无页 | `es_config_font` 除零 Fatal 消除 |
| **字体选择禁用**:清单只含引擎默认 family | B7 逐字形回退前置 | 列表条目可渲染;误选不会丢日文 |
| `WINDOWINFO[WSX/WSY/DISPLAYNO/FULLSCREEN/FULLSCREENBUTTON/MINIMIZE/CAPTION/ACTIVE]`(gfx 读侧 `fixed(no-manual)`) | REA `0x4586ec` 逐字段 | 桌面会话 10,224 次挂号消除 |
| `FILEINFO[SIZE/RELATION]`(DIV-RT-056 `fixed(no-manual)`) | REA `0x443572`(FindFirstFileA)/`0x442a48`(FindExecutableA) | 手册例 3/4;RELATION 暂空串(B7 宿主位) |
| **存档根自动创建**(engine boot) | 默认 `save/` 不存在时全部写盘静默失败 | 「无法存档/既読不记录」根因消除 |

### 存档初始化记录(2026-10-25)

主人发现选中坏字体后日文全部消失(渲染器缺 GDI font linking 等价的逐字形
回退),令初始化 save + 禁用字体选择。处置:旧 `save/` 改名
`save-backup-badfont-20261025/`(含坏字体名的 gsd.sd 不回灌);kidoku/seld/
extra/save30002 恢复进新 `save/`;引擎补「存档根必须实际创建」修复。

### 验证

```text
cargo test -p yu-rt        # 70 lib + 4 consistency + 11 corpus + 4 + 79 fixture + 3 golden 全绿
yu-cli case --all          # 9/9 PASS(--update 重新生成:旧行为钉死已过期,新行为经主人实机验证)
run --frames 320 --shot    # 标题画面日文正常(target/tmp/defaultfont.png)
headless 字体页路径        # 零诊断(fp2.json/tx1.json)
```

### 残留(在册)

- 字体选择恢复 = B7(逐字形回退 + 全量清单);
- FILEINFO[RELATION] 真 OS 关联 = B7(`FindExecutableW`);
- WINDOWINFO 写侧 CLOSEMODE/MENUID/REGIONID/DRAGID、主窗口 A/SX/SY 视口语义 = DIV-GFX-037 余项;
- `WINDOWINFO[FULLSCREEN]` 等桌面宿主实时注入(最小化/聚焦)未接;
- `_CHR` 之外 undoc 系统变量(`_DXMODE/_VARTYPE/_SYSTEM_FRAME`)仍 DIV-RT-007 open。

## 成果 CG-ANIM (2026-10-25) · 动画立绘/背景不上屏收口(`TRANSITION` + 定点尺寸键 + 缩放日志折叠)

> 证据与逐条锚点见 `docs/divergences/DIV-GFX-061-animated-sprite-transition.md`。
> 用户口径:「CG 的缩放移动,包括立绘都不显示,而且会触发 render spike
> (特别是文字一个一个显示出来的时候)」,并指出「有动画就不显示,后面的
> 静止 CG 正常显示」—— 后半句直接指向动画分支。

### 根因(两条,都是动画分支独有)

| # | 写法 | 出现处 | 旧行为 |
|---|---|---|---|
| A | `CGACT[TRANSITION=1 SET=f ID=$LB1 ID2=$LB2 ID3=$LB3]` | `es_Sprite.yst:7978` | 判 `Unimplemented`;`ID2` 被当目标误用 ⇒ 输出层 `$LB3` 永远全透明 |
| B | `CGACT[… SIZE2=1 FX=… FY=… FX2=… FY2=… FSX=… FSY=…]` | `es_Sprite.yst:8665/8690` | 无整数 `SX/SY` ⇒ `gfx.imageop.size.missing` ⇒ 层被标 `Unimplemented` |

静止 CG 走 `quo==0` 的低画质分支(发整数 `SX/SY`)且 `@L.ST==0` 不发 TRANSITION,
所以静止图正常 —— 与用户观察一致。

### 修复清单

| 修复 | 锚点 | 效果 |
|---|---|---|
| `TRANSITION` = 两源按 `SET`(0..256)插值写入 **ID3** | 手册无 `ID3`/`SET` 量纲;语料 `es_Sprite.yst:7915..7985`;`fixed(no-manual)` | 动画路径的输出层有图 |
| `FX/FY/FX2/FY2/FSX/FSY` = 整数键的百分位定点(÷100) | 脚本注释「小数点値を掃い…伝える」;`render-parity.md` §7(绘制侧 `÷100.0`) | `SIZE2` 中画质分支可执行 |
| `SIZE3/SIZE4/SIZE5` 按缩放族执行 | `es_ERIS.yst:631`、`es_System.yst:261`;滤波差异未取证 ⇒ `partial(no-manual)` | CG 半尺寸/二分降采样不再毒化整层 |
| 缩放族日志折叠(`supersede_image_op`) | REA `0x42f809`(立即破坏性写入) | `image_log` 66→15,`passes` 103→稳定 84–88,render spike 消除 |
| `VARACT` 缺 `LET` 回写 `SET` | 引擎 `0x4542ab` 两支都写;`es_Text.yst:2024/2040/2568/2586` | `es_Text@1354` 的 Fatal 消除 |

### 验证

```text
cargo test -p yu-gfx --test parity_fixes     # 19 passed(含 4 条新增)
cargo test -p yu-gfx --test transform        # 6 passed
cargo test -p yu-backend-soft --lib          # 含 blend_transition 2 条像素单测
cargo test -p yu-rt --test fixture varact    # 6 passed(含 LET 缺省回写)
场景 A/B(关闭/开启日志折叠)                 # 8 帧 SHA-256 逐位相同
演出１ 真实菜单路径 3000 帧                   # 0 script_errors;此前 `rt.cmd.slot` Fatal
```

### 残留(在册)

- `SIZE3/SIZE5` 的滤波差异未取证(按双线性代用,DIV-GFX-039 `partial`);
- `TRANSITION` 的 `SET` 上限按 0..256 夹紧,动画端实际范围待实测确认;
- `CGINFO[TEX]`(E-mote 判定)仍属 `deferred(master)`。

## 成果 Q1-LOGGING (2026-10-25) · 日志收口:统一格式 + 级别颜色 + 默认可见(含 S1 与全仓静默自查)

> 任务书 `docs/tasks/task-q1-logging.md`;契约单页 `docs/logging-contract.md`;
> 自查台账 `docs/todo-silent-handling.md` §7。裁决按主人默认值执行:
> Severity 单一级别集(Info/Lifecycle 由频道区分)、生命周期 Info 默认显示
> (`--log-level quiet` 可压)、S1 并入 R0、诊断/日志全走 stderr。

### R0(S1)FrameDiag 带位置

- `yu-gfx`:`FrameDiag.at: Vec<DiagAt>`(上限 `MAX_FRAME_AT=8`,超出进
  `at_more` 显式计数);`push_diag` 聚合 key 不变,位置去重并入;
  `on_event(DomainCall)` 分派窗口注入 `(script, pc)`(S1 原缺口);
  frame JSON 的 `at` 段**只在非空时写出**(空帧 JSON 逐字节不变,旧 golden
  形状不破)。
- 验证:`cargo test -p yu-gfx --test diag_positions`(4/4:跨 pc 双位置、
  cap 折叠、JSON 键条件出现);真实语料实测见下。

### R1 统一格式化 + ConsoleSink(`yu-log/src/console.rs` 新建)

- 行格式 `[  +12.345s] WARN  [gfx] 码 @ 位置 x次数: 详情`(tyn 行格式为
  事实参照,`record.rs::format_line` 出处注明;未搬代码);级别词
  FATAL/ERROR/WARN/UNIMP/INFO/DEBUG/TRACE 单一词表;色表 = tyn 同表
  (`level.rs:71..76` 数值),多行逐行包色、每行复位;
- 颜色:`--color on|off|auto`(显式 > `CLICOLOR_FORCE=1` > `NO_COLOR` >
  stderr 终端检测);auto→off 必须在 JSONL 留原因行(tyn 静默无色教训);
- `Console`:聚合(同 key 只 `count+=1`)、级别过滤在格式化之前、
  recent 环形缓冲(面板页签)、`JsonlFile` 单写者(`Arc<Mutex<File>>`);
- 门面桥改统一格式(`[log-facade]` 频道;`ConsoleFn` 改 `Arc<dyn Fn>`);
  `Diag::to_json_line_with_sub`(sub:"diag" 前置,schema/往返不变)。
- 验证:`cargo test -p yu-log` 29/29(格式确定性、逐行包色字节级、
  聚合排序、quiet 门槛、JSONL 双行型往返)。

### R2 默认可见性

- 桌面:`DiagnosticsHost.observe` 同一次 drain 两路消费(面板聚合 +
  控制台行,gfx 帧诊断经 `frame_total_to_diag` 同源出行);`HostOptions.console`;
- headless:`run` 末尾诊断汇总(频次降序,带 pc;`--diag-summary off` 干净);
  serve:关闭时汇总 + `drained_diags/sound_diags_dropped` 计数行;
- `--log-level quiet`:压 Info(含汇总);`--log-level/--color` 全局参数化。

### R3 频道化

- desktop/run/server/dev/case 的 ad-hoc 前缀改 `[host]/[run]/[serve]/[dev]/[case]`,
  生命周期/警告行迁 stderr(产品输出留在 stdout:case PASS/FAIL、run 报告块、
  bench 表);桌面宿主宏(44 处)统一 `[host] ` stderr 前缀(合流 Console
  为台账遗留,宏看不到 self)。
- 验收 grep:`output::error(format!` 每处首段均为 `[频道]`。

### R4 文件统一

- JSONL 单写者同时落 `sub:"diag"`(内核)与 `sub:"log"`(门面/宿主);
  `run/serve` 新增 `--log-file`(每轮截断,与 desktop 同口径);
- egui 面板「日志」页签:环形缓冲快照 + 清空按钮(只清面板,不动文件)。
- 验证:ERIS headless JSONL 逐行回读 `sub=diag x1232 + sub=log x1` 字段齐全;
  desktop 5 帧实测 577 diag + 14 log 同文件。

### R5 守卫(`crates/yu-cli/tests/logging_guards.rs`,3/3 绿)

- src 零终端宏;ANSI 字面量只许在 `yu-log/src/console.rs`;
  `silent_fallback_audit`:yu-rt/yu-gfx 未标注 `unwrap_or` 按文件计数白名单
  锁定(快照 162 处,只许减不许增;S3 排期逐条分类)。

### 全仓静默自查(任务重中之重;明细见 todo-silent-handling §7)

- 扫描 7 类模式,a 类 8 处全部处置(dev www 遍历吞错、YU_LOG_JSONL 打开
  失败静默、SOUND_DIAG_CAP/drain_diags/serve 排空无痕 → 显式计数、
  S1 位置注入、**desktop 缺省 JSONL 路径打不开静默无文件日志(终验时
  抓到,已改三态:显式失败=硬失败/缺省失败=WARN 继续)**,S2 型降级
  未发现新点位);b 类代表性 4 处注释自证;c 类 7 项转台账(未顺手改语义)。
- 桌面日志路径三态实测(本机系统 TEMP 不可写恰好是天然用例):
  缺省失败 → `WARN [host] 创建日志文件 … 失败 …;本运行不落文件日志`,
  运行继续(exit 0);TEMP 指向沙箱 → 缺省文件正常创建;
  `--log-file` 指向不可写处 → `[yu-cli] …` exit=2。

### 验证(原始输出摘要)

```text
cargo test --workspace            # 除 info_query 3 个既有失败外全绿
cargo test -p yu-cli --test logging_guards   # 3/3
yu-cli run <eris> --frames 120    # stderr 汇总:
# [ +18.949s] UNIMP [gfx] gfx.param.nonvisual @ s0 pc2148 x39546: CG: 参数 `LINT` …
# [ +18.938s] WARN  [gfx] gfx.layer.missing @ s66 pc3399: CGEND: 删除不存在的图层 …
yu-cli run <eris> --frames 120 --diag-summary off   # stderr 0 行
YU_LOG=quiet yu-cli run <eris> --frames 60          # INFO 级行 0(汇总同压)
game_diag_probe <eris> 120        # gfx.texture.missing x612 at s66 pc245,s66 pc263,s66 pc264,s0 pc1800,…(S1 验收)
--color on / NO_COLOR=1 / auto(重定向)  # ANSI 有/无/关+留痕 三态实测
```

### 残留(在册)

- `cases/*.json` golden 9/9 FAIL 为 **HEAD 既有**(git archive 干净副本
  复现,actual sha 逐字节一致 ⇒ 非本轮引入;demo 修复后需主人实机确认
  再 `--update`);
- `yu-gfx --test info_query` 3 个失败同为既有(WINDOWINFO/FONTINFO 读侧
  实现后断言未更新);
- 桌面宿主宏行合流 Console、save 域诊断接入脚本 pc、S3/S5/S6 余项:见
  todo-silent-handling §7.4。

## 成果 Q1-LOGGING-R2 (2026-10-25) · 日志收口复审:宏行合流/时间域着色/锁中毒静默修复

> 主人实机截图指出两点(未对齐的 `[host] yu-desktop: …` 行、时间域无色),
> 连同代码质量复审,本轮修复如下;全部行级验收见下方验证。

### 修复清单

| 项 | 原状 | 处置 |
|---|---|---|
| 桌面宏 44 处未对齐 | `output::error("[host] …")` 纯文本(宏看不到 self) | `install_host_console` 进程级注入槽(`run()` 最先安装;宿主运维层元数据,内核不读取);`host_line!`=INFO / `host_error!`=WARN / `host_fatal!`(新,fail/panic 冻结)=ERROR,全部走统一格式+JSONL;槽未装时退化纯 `[host]` stderr |
| 时间域无色 | `[  +32.948s]` 白色 | 着色时时间域恒暗灰 `\x1b[90m`(与级别色分离降视觉权重);实测 94/94 行带灰时间域 |
| **锁中毒静默(复审新抓到,a 类 #9)** | `ConsoleHandle`/`JsonlFile` 的 `Mutex::lock()` 用 `if let Ok` ⇒ 一次 panic 中毒后所有后续日志**静默消失** | 恢复式取锁 `unwrap_or_else(poisoned→into_inner)`(H-P1-01 同型);serve 汇总取锁同改 |
| 诊断消息路径不可读 | 缺省路径含 NUL 时显示成「空路径」 | 日志消息中路径一律 `{:?}`(空白/NUL/非法字符现形) |
| debug build 提示行在 Console 建立前 | 平文本 | 移入 Console(Warn 级,统一格式) |
| 决策分支事实(减少排查时间) | 分散 | desktop 新增 `boot: entry=… data=packed:….ypf (bytes) debug_mode=… fps=… backend=…` INFO 行;headless stdout `engine:` 行扩展 `debug_mode=` 与 `data=kind:path (bytes)` |

### VM 未知命令路径核验(主人问询项,无需改动)

- opcode 不在工程 ysc ⇒ Fatal `rt.cmd.unknown`,detail 带 `s{script} pc={pc} opcode …`
  (`vm.rs::fill_cmd`,Fatal 恒显示);
- 命令名在表但无处理器 ⇒ `report_unimplemented` → `rt.cmd.unimplemented`,
  `locate()` 补 task/file/pc/cmd 且进挂号表计数(ERIS 实测:
  `UNIMP [vm] rt.cmd.unimplemented @ data\script\eris\es_emote.yst pc214`)。

### 验证

```text
cargo test --workspace                # 31 套件 ok;仅 info_query 3 个既有失败
cargo test -p yu-cli --test logging_guards   # 3/3
桌面 --frames 3 --no-sound            # 全部行(含 timer/window/render spike/
                                      # DIALOG 帧)统一格式;boot 事实行可见
--color on                            # 94/94 行时间域 \x1b[90m,行体按级别色
```

### 残留

- info_query 3 个既有失败与 cases golden 9/9(HEAD 既有)不变,见上节;
- `host_error!` 的历史语义偏「注意」(如 debug UI font),现映射 WARN(黄);
  若主人想要更细分级别再挂号。

## 成果 WGSL-BLACK (2026-10-25) · wgpu 演出1 黑屏根因收口(快照冻结镜像)+ 全仓警告/测试基线清理

> 用户口径:「我用 wgpu 启动,我的 cg 还是黑屏,会闪一下然后黑屏(演出1那个)」。
> 取证路径与逐帧数据见 `docs/divergences/DIV-GFX-062-wgpu-snapshot-cpu-mirror.md`。

### 根因(一条,后端专属)

ERIS 背景快照链在同一帧内完成三步:`CG[SPLBUF401]=bg_030` → `COPY → SPLBUF201`
→ `CGEND[SPLBUF401]`。**COPY 的源层当场被删**。

| 后端 | 快照像素来源 | 源层已删时 |
|---|---|---|
| soft | 命令段写进 `snapshots` 表并冻结,之后只读该表 | 无影响 |
| wgpu(旧) | 按 `CopyOp.source` 惰性重建,递归回看源层 | `gfx.imageop.source.missing` ⇒ 目标层留 `SolidBlack1x1` ⇒ 全黑 |

wgpu 端 `capture_copy_mirror` 其实已捕获了同样的 CPU 镜像,但
`resolve_cpu_source` 的快照分支**没先查它**,直接走惰性递归 ⇒ 冻结镜像白存。

### 修复

`resolve_cpu_source` 增加 `targets: &TargetSet`,快照分支**先查
`targets.snapshot_cpu(copy_id)`**,命中即返回冻结镜像;未命中才回落惰性递归
(留给组源等 CPU 镜像不可得的组合)。`apply_image_to_override` 同步贯通。
语义与 soft `apply_copy_op`/`layer_surface` 对齐。

### 验证(逐位对拍,不是"没崩")

| 帧 | soft | wgpu 修复前 | wgpu 修复后 |
|---|---|---|---|
| 760 | `972a15c9…` | `d8d9f702…`(skip×1) | `972a15c9…` |
| 900 | `9584b2d6…` | `a7aff0f8…`(**纯黑**,skip×1) | `9584b2d6…` |
| 1200 | `184568aa…` | `80556fa6…`(skip×1) | `184568aa…` |

```text
probe_skips --frames 1000 --wgpu
  修复前: x59  f740..f798  image:gfx.imageop.source.missing  target=:/W0/SPL201
  修复后: (无 skip);f2775 段同样清零
```

### 同批清理(日志驱动排查发现)

| 项 | 现状 | 处置 |
|---|---|---|
| `YU_DEBUG_CG` 临时 eprintln | 遗留在 `apply_cg` | 删除(已完成取证使命) |
| `info_query` 3 个失败 | 基线过期:`62f9898` 已实现 FONTINFO/WINDOWINFO 但断言仍写「必须 Unimplemented」 | 按实现的证据锚点更新断言(NAME NO→空串、NUM/LANG→0、CAPTION→-1) |
| `yu-trace` golden 失败 | `cc9c05e` 扩了测试夹具命令表,漏重生成 golden | 重生成;diff 仅 3 行(ysc 4065→4225B、ycd 5927→6127B),字节增长与参数扩充一致 |
| `yu-engine`/`yu-vfs` 测试 `PermissionDenied` | 受限环境只允许写工作区,`env::temp_dir()` 被拒 | 测试临时根缺省改到仓库 `target/test-tmp`(`YURIS_TEST_TMP` 仍可覆盖),共 4 处 |
| `yu-rt` consistency `unreachable!` | 缺样本时报 "internal error",把环境问题误报成内核缺陷 | 改为显式 panic 并打印 `设置 YURIS_SAMPLE_ERIS_YSBIN=<project>/data/ysbin` |
| 编译警告 3 条 | 未用 `mut`/未用变量/死函数 | 全部清零 |

### 验证

```text
cargo build --workspace --all-targets    # 0 warning 0 error
cargo test -p yu-save -p yu-vfs -p yu-trace -p yu-engine \
           -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu
                                         # 全绿(附样本环境变量)
```

### 剩余未实现(不属本轮,在册)

日志抓取(700 帧)暴露的三条,均**已确认不影响本轮画面**:

- `CGINFO[TEX]` ×979(`es_sprite.yst:5418`):E-mote 判定,`deferred(master)`;
- `CGINFO[TEX]` 目标不存在 ×300(`:2340`):等待 E-mote 侧一并做;
- `FILEINFO SIZE/RELATION` ×626:宿主文件事实查询,挂账 B10。

新增档案:`DIV-GFX-062`(本轮黑屏)、`DIV-GFX-063`(无手册页滤镜族 `partial(no-manual)`)。

## 成果 DIAG-CLEAN (2026-10-25) · 日志告警收口:1306 条诊断 → 非 Info 仅 1 条(E-mote 暂缓)

> 口径:「接下来你需要修补的是 大量的 warn 和 UNIMP」。
> 全量取证与逐条锚点见 `docs/divergences/DIV-RT-054-diagnostic-noise-cleanup.md`。

### 方法与结果

700 帧 headless 真实语料抓 JSONL,按 (severity, code) 聚合后**逐条查证据**
(手册 / `es_Sprite.yst` 源码 / `yst00066.ybn` 编译产物 / REA 0x495)。

| 修复前 | 修复后 |
|---|---|
| 1301 Recoverable `gfx.layer.missing` | Info `gfx.layer.already_absent` |
| 626 Unimplemented `sv.command.unimplemented` | **0**(纯误报) |
| 979 Unimplemented `gfx.info.unsupported` | **0**(实现 TEX) |
| 300 Unimplemented `gfx.info.missing` | **0**(缺层 TEX 回 0) |
| 3 Unimplemented `gfx.param.nonvisual` | **0**(合法参数归档) |
| 3 Unimplemented `rt.cmd.unimplemented` | 1(E-mote `deferred(master)`) |
| 2 Recoverable `sv.path.escape` | **0**(绝对路径误判) |
| 2 Recoverable `sv.file.missing` | Info(幂等删除) |

**收口后非 Info 诊断:1 条**,即主人明令暂缓的 E-mote。

### 两类问题的区分(关键)

这批告警混了三种性质,处置完全不同:

1. **误报**(`FILEINFO` ×626、`FILEACT` ×1):内核/宿主已实现或已由别的
   宿主消费,诊断却照报。→ 删掉或修正判定。
2. **归属**(`SYSTEM` ×1、`TRAY`):语义在宿主/平台边界外,内核只发事件。
   → 按域事件契约 §9 标 `aligned_stub`,不进挂号表。
3. **未实现**(`CGINFO[TEX]`):真的缺功能。→ 取证后实现。

第 2 类**没有**为了消警告而假装已实现:`SYSTEM` 的逐参数处理体仍未定位
(手册无 `system.html`、`BACKGROUNDEXEC` 全手册 0 命中、经
`YU-RISScreenSaver` 字符串 xref 找到的 `0x41b42c` 是屏保窗口创建而非
命令处理体),故只声明归属、不声称逐参数对齐。

### 实现:CGINFO[TEX](`fixed(no-manual)`)

手册 `cg/cginfo.html` 的字段表没有 `TEX`。证据来自脚本源码的四处闭环:
`CG[... TEX=@_DXMODE]` 写入(`:4091/4144/4396/5280/...`),
`CGINFO[... TEX=1 LET=@texchk]` 读回(`:3454/4155/5848/7770`),
紧跟的 `IF[@texchk==2]` 分支注释为「E-moteレイヤなら」⇒ **2 = E-mote**。

`Layer` 新增 `tex: i64`(零初始化),`CG` 写、`CGINFO` 读;层不存在时回 0
(类型探测语义 —— 四处 ID 由 `$LIDBUF+槽号` 拼出,未装载的槽自然不存在,
中间无 `EXIST` 前置检查)。

### 附带修掉的 bug

- `FILEACT` 被误报未接线:`SaveHost` 契约确实消费 `Domain::Save|File`,
  但 VM 的 `handled` 判定漏算 ⇒ 补上;
- `sv.path.escape`:脚本 `$es.SAVEPATH = $_SAVE_PATH` 传的是宿主注入的
  **绝对路径**,旧 `confined_path` 见绝对路径即拒。改为接受但仍强制
  「必须在 save 根内、不得含 `..`」(安全边界未放松)。

### 验证

```text
700 帧真实语料           非 Info 诊断 = 1(E-mote);此前 1306 条非 Info
cargo test --workspace   80 套件全绿
像素回归(wgpu 演出1 f900) sha256 9584b2d6… 与修复前逐位相同
```

新增测试 3 条(幂等删除 / TEX 读写与缺层回 0 / 非视觉参数归档);
更新 `tray_is_a_domain_event` 断言以对齐 §9 归属。

### 残留(在册)

- `SYSTEM` 逐参数处理体待定位(归属已定);
- E-mote 全族 `deferred(master)`;
- `gfx.texture.missing`(1 条 Info)是 `CG` 无 `FILE` 时的正常 get-or-create 路径。

## 成果 RENDER-GRAPH (2026-10-25) · 渲染耗时曲线窗口(egui 独立窗)

`--debug-ui`(F1 同开关)下新增独立 egui 窗口 **render time (ms)**,
横轴 = 宿主帧推进(固定容量 300 样本,约 5 秒 @60fps),纵轴 = 引擎
`FrameReport::render_ms`。曲线按当前样本耗时三色分级:

| 色 | 区间 | 含义 |
|---|---|---|
| 绿 | `< 8 ms` | 余量充足 |
| 黄 | `8 – 16.7 ms` | 接近 60fps 预算 |
| 红 | `≥ 16.7 ms` | 掉出 60fps 帧预算 |

- 采样在 `App::record_frame` 里逐帧入环形缓冲,**面板关闭期间也采样**,
  打开即见最近历史;`Restart` 时清空(新运行代际)。
- 纵轴按窗口内最大值自适应(下限 1.5 × 16.7ms),仅自适应不固定刻度,
  长尖峰也能整体入图;两条参考线(黄/红阈值)半透明绘制。
- 窗口与主面板同属 `wants_egui` 惰性路径:面板隐藏时整条 egui 管线不跑,
  不产生额外开销。

### 验证

```text
cargo check -p yu-host-desktop -p yu-cli   Finished(无 warning)
```

行为验收由主人跑 `--debug-ui` 目视(按约定不新增测试)。

## 成果 FONT-SPIKE (2026-10-26) · 文字/CG 逐帧渲染尖峰收口(字形→合成→读回→呈现全链路)

症状:对话每推进一句/逐字吐字时卡顿,CG 页 render 曲线黄红段(9700X+5070 实机)。
定位(探针脚本 + `YU_RENDER_TIMING`/`YU_FONT_TRACE`/`raster_bench` 分段计时):

| # | 根因 | 证据 |
|---|---|---|
| 1 | fontdb(只开 `fs` 无 `memmap`)的 `Source::File` 在**每次** `with_face_data` 时 `std::fs::read` 整个字体文件(msgothic.ttc ≈5MB):字形缓存 miss、甚至每次 ascent/advance 查询都整读一次盘 | `raster_bench`:1518µs/字形,与字号无关(24px=96px);closure 内部仅 20-120µs |
| 2 | 文本修订变化即**整页重烤**:基图整图 clone + 全部绘制项从头重画,吐字一字烤一遍,O(页长)/字 | `YU_RENDER_TIMING` text 阶段随页长增长 |
| 3 | 合成缓存「256 条超限整体清空」:角色加载演示一屏 480-779 个文本精灵层 ⇒ 缓存永远全 miss,单帧 200-720ms 反复出现 | 帧 1076 text=719ms layers=678 draws=360 |
| 4 | (架构)每帧 `poll(wait_indefinitely)` 同步读回整帧 + 宿主把读回像素再整帧上传呈现:CPU/GPU 流水线被拍死,一帧两次整帧拷贝 | `readback.rs`/宿主 `Presenter::upload` |

修复(全部语义保守,软/硬两端像素一致):

- `FontSource` 按路径缓存字体字节(`Arc<Vec<u8>>`,每文件只读一次盘;`source.rs`)。
- **增量烘焙**(`compose_layer_cached`):缓存条目记录已烘焙的 `TextDrawKey` 前缀,
  新绘制集合严格前缀追加时**只画尾部**(官方 `CGACT[TEXT]` 命令时刻写层表面语义,
  DIV-GFX-058;与整体重烤逐位等价)。前缀比对走新增的 `TextDrawKey::matches_draw`(免分配)。
- 合成缓存改**字节预算 LRU**(256MB / 8192 条,逐条淘汰),不再整体清空。
- 命中路径零成本:基图惰性解析(不克隆)、内容指纹烘焙时算一次存条目
  (`ComposedLayer::hash`);指纹单点实现在 soft `content_hash_rgba`,wgpu `hash_rgba` 委托。
- **异步读回**(`ReadbackCache::read`):本帧只提交拷贝,取回上一帧结果;
  「滞后一帧」与 `COPY :/W0/` 的上一帧后缓冲语义完全一致。`read_now` 保留精确读
  (`--shot`/对账)。桌面宿主改走 `render_frame_streaming`。
- **GPU 直呈现**:引擎暴露 `screen_texture`/`screen_handle`,宿主按 `(代际,宽,高)`
  缓存视图/bind group 直采引擎屏幕纹理,免「读回→重上传」两次整帧拷贝;
  `refresh_screen_surface` 保 `--shot` 末帧精确。
- 每层 resolve_style memo(fontdb 查询逐字形 → 一次);诊断设施:
  `YU_RENDER_TIMING`(plan 分段 + 合成路径计数)、`YU_FONT_TRACE`(栅格化分段),
  均为 env 门控 `log::trace`;新增 `yu-font/examples/raster_bench.rs`。

### 验证(原始结果)

```text
raster_bench(msgothic 首渲染)   1518µs/字 → 139µs/字(sy=24);命中 0.5µs/字
角色加载场景 text 阶段 >5ms     19/2400 帧(最大 719.1ms)→ 0/2400 帧(最大 3.4ms)
菜单页 f401 text 阶段            235.6ms → 3.4ms
2400 帧全场景 render_ms          p50 3.32 / p95 15.97 / max 27.3
对话吐字段(probe1)              render p95 1.85ms / max 9.9ms(修前 20-33ms 常驻)
cargo test --workspace          44 套件全绿;仅预存 yu-rt consistency 4 红(干净树同红)
logging_guards                  3 绿(插桩已改走 log::trace)
case until-visible actual       4699e746… 与改动前逐位一致 ⇒ 软渲染像素未变
```

### 残留(在册)

- 新 CG 首次显示的同步解码 20-27ms 单帧黄闪(每张一次;`image` crate 解码,
  与原版引擎 CG 加载同量级);后台预解码待立项。
- 9 条 case golden 过期为改动前既有(actual 与干净树一致),`--update` 重生成由主人定。

## 成果 MOTION-SPIKE (2026-10-26) · 演出移动抖动/卡顿六根因收口

用户实测(ERIS β4「演出1」摄像机推近 + 立绘 3D 位移):移动像走像素网格、
越慢抖得越厉害、长跑掉到 40fps、终点有跳变。逐项取证后定位到六个独立根因。

### 根因与修复

| # | 根因 | 证据 | 修复 |
|---|---|---|---|
| 1 | `CGACT` 带区域缩放走 CPU `scale_region` 逐像素重采样;立绘动画每帧换区域 ⇒ 像素缓存全 miss,单条 16–35ms | `YU_RENDER_TIMING`:image 阶段 14210ms/2800 帧,p95 5.27,max 37.14 | 改成 **GPU UV 子矩形采样**(`Override::Uv`):缩放只是一次带 UV 的 quad,不物化纹理、不读回。峰值 37.14→7.13ms,慢缩放调用归零 |
| 2 | 精灵位置被量化:脚本把位置拆成整数+小数(`es_Sprite.yst` `CGACT[… FX=@X1*100+@FLX …]`),`fixed_px` 做 `.round() as i64` 丢掉小数 | 同帧区域 `(250.26,47.70)-(708.74,391.31)` 逐帧漂移;测试样本 `FX=1234 ⇒ 12.34` 被 round 成 12 | `ImageOp.x/y/x2/y2` 改 `f64`,`FX/100` 不取整;只有整数像素动作(COPY 切片/RECTPAINT/RESIZE/字符落位)在各自边界显式取整 |
| 3 | `CG[X/Y]` 遇浮点**整条丢弃**(`as_int()` 在 `fract()!=0` 返回 `None`)⇒ 层停在上一次位置 | `es_Sprite.yst:8574 FLT[@LEFT_X]` → `CG[X=@LEFT_X]` | `EngineValue::as_px()` 起改 `as_f64()`;`Transform.x/y` 由 `i64` 改 `f64`;两后端按 D3D9 浮点顶点/亚像素取样 |
| 4 | `X+=` / `Y+=` 增量参数语法**完全未实现**(YSCM 参数种类 2 `NumericOffset` 只有枚举,零消费代码) | 语料 `es_Sprite.yst` 56 处 `CG[…] X+=… Y+=…` | `ArgValue.offset` + `apply_layer_params` 增量语义 |
| 5 | 纹理滤波停在 `TextureFilter::unknown()`(POINT/POINT)占位,从未注入引擎真值 | REAL `FUN_00460924` 读完 `yscfg.dat` 后无条件写 `MAG=1/MIN=1/MIP=0`(LINEAR/LINEAR) | `GfxState::set_filter(engine_settings(Linear, Linear, Point))` |
| 6 | 桌面宿主把逻辑时钟上限压到 2 帧(32ms),**超出部分直接丢弃不顺延** | 3258 帧中 1260 帧(38.7%)被截断,累计丢 6.58s;最长帧 366ms 只算 32ms | 恢复 500ms 上限。补间按 `_OS_TIME` 真墙钟推进,截断会让动画永久落后于真实时间 |

### 验证(原始结果)

```text
帧间隔 p50/p95/p99      16.65 / 49.83 / 50.41 ms  →  16.65 / 17.07 / 17.29 ms
render_ms p50/p95/max   7.87 / 27.89 / 55.51     →  0.69 / 2.02 / 7.88
work_ms  p95/max        34.98 / 212.53           →  6.59 / 162.47
平均 FPS                40.6                     →  59.1
cargo test --workspace --all-targets   174 套件全绿;cargo check 零 warning
```

退出帧间隔 p95 = 17.07ms 即真实 60Hz 的 16/17 交替节奏 ⇒ 无掉帧、无抖动。

### 残留(在册)

- 摄像机补间单帧断裂(`f957` 104px)已定案为**脚本「点击跳过」**
  (`@es.TEXT.TIPCLICK==1 ⇒ @mTADD=999999999`),非缺陷;扣除后为
  1.27px/帧的连续亚像素运动。
- `REVERSE`(RGB 取反)与 RGB 滤镜族仍需 framebuffer;`Override::Pass` +
  `TargetSet::image_op` 已就位等对应 WGSL。
- 软后端按整数像素重采样(`scale_region`),亚像素只保证 wgpu 产品路径;
  soft/wgpu 像素不再逐位相同(soft 是自建参考实现,真值以 SDK demo 为准)。

## 成果 MOTION-SPIKE-2 (2026-10-26) · 文档/死代码/门禁收口 + 种类 2 语义 REA 定案

承接 MOTION-SPIKE:主人确认抖动与吸附已解决,转入质量收口。

### 1. 文档与实现矛盾(7 处)

`X+=` 语义从「累加」改回「赋值」后,4 处注释仍写累加;另有 2 处陈旧论述
(`as_px` 声称官方四舍五入、X 分支上方整段"浮点显式拒绝"已与代码相反)。
全部改写并补证据锚点。`as_px` 文档收敛为「只用于 Z/SX/SY 等确实整数的槽」。

### 2. 死代码清理(约 120 行)

`Override::Pass` 变体(含绘制/`layer_image`/`image_source_texture`/几何诊断
4 处分支)、`TargetSet::image_ops` 池(`ensure_image_op`/`prune_image_ops`/
`image_op`)、`PlannedTarget::ImageOp`。全部无生产者;自检 0 引用。
后续需要 framebuffer 的滤镜(REVERSE/RGB 族)再加,不预置。

### 3. soft 门禁关闭(按主人指示)

- `EngineOptions::reconcile` 默认 `true` → **`false`**。理由:soft 是自建
  参考实现不是真值;且每次 wgpu 运行会额外构造整套 `SoftRenderer`(独立
  `TextureStore`,重复解码资产)。
- 修一处误报:`wgpu_reconciled()` 在「未请求对账」时返回 `Some(false)`,
  CLI 打印 `PENDING`;改为 `None`,不再误报。
- 删除 `host-desktop` 里重复魔数 `set_max_step_ms(500)`,回到引擎默认
  `DEFAULT_MAX_STEP_MS`。

### 4. 参数种类 2(`X+=`)语义 —— REA 定案:不是累加

REA `FUN_00429fd1`(参数序言,`0x429fd1`,官方 0.495
`4f5eefa3…`)按类型字节三路分派:

```c
if (kind == 0) { value = DAT_008018b4[subtype](script_base+off, len);   // 求值
                 *(int*)&DAT_00683740 + i*8 = lo; ... hi; }
else if (kind == 1) { /* 字符串: malloc(0x1000) + 拷贝 */ }
else if (kind == 2) { *(int*)&DAT_00683e20 + i*4 = script_base + off;    // 只记偏移
                      /* 数值槽 0x683740/44 一个字都不写 */ }
```

⇒ **种类 2 是「脚本内偏移」,不做算术、不读旧值、不写数值槽。**
既非累加也非普通数值;消费点按偏移取内联数据。
内核因此按赋值处理(VM 已求值),与实机现象一致(累加会让持久层逐帧漂移、
补间结束"跳回原位")。`ArgValue::offset` 降级为诊断字段。

证据档案:`docs/divergences/DIV-GFX-064-cg-float-coordinates.md`
(状态 `partially-confirmed`;未决 = 官方 f64→i64 规约指令、消费点如何
解读 `0x683e20[i]`)。

> **勘误(2026-10-27)**:本节结论「不是累加」**已被推翻**,见
> `DIV-GFX-064` §6。窗口记录 `+2` 的 u16 高字节就是参数类型字节:
> `X=` 是 `slot=1`(0x0001),`X+=` 是 `slot=257`(0x0101)。序言不写数值槽
> 与消费点是否叠加并不矛盾(序言只是登记偏移)。按赋值处理会让
> `es_Sprite.yst:8173` 的 `CG[ID=$L1 X+=(@es.GSX/2)-(@WSX/2)]` 在补偿量为 0
> 时把位置清成 0 —— 立绘钉在屏幕左上角,正是**实测症状**;改为增量后位置
> 收敛到 240.93 并保持稳定。

### 5. case golden 重生成(9 条)

`yu-cli case <project-eris> --all --update` → **9 passed / 0 failed**。
逐条 diff 审查结论:

- `display_json_sha256` 全部变化 = image op 区域字段由整数变小数
  (亚像素契约变化,`"x":12` → `"x":12.34`);
- 6 条 `ops_total` 一致 **−51**,经 HEAD 对照实验(暂存改动后重建)确认
  **HEAD 同样为 122 ops** ⇒ 属改动前既有漂移,非本次回归
  (用例注释亦写明"底层 Fatal 修好后需 --update 重生成");
- `until-visible` 等 3 条 `ops_total` 不变,仅 JSON 哈希变。

### 验证

```text
cargo test --workspace --all-targets   174 套件 ok / 0 failed
cargo check --workspace --all-targets  0 warning
yu-cli case <project-eris> --all       9 passed / 0 failed
```

### 残留(在册)

- 官方把 f64 规约成 i64 的指令未定位(DIV-GFX-064 §5);
- 消费点如何解读 `0x683e20[i]` 未逐命令定位;
- `CGINFO[X/Y]` 读回取整(内部 f64)未取证官方口径。

***

## 成果 TASKINFO-SLOTS (2026-10-26) · `TASKINFO` 9..13 槽收口 + `TASK[TEXTPOS]` 官方对齐

用户桌面日志(78.8s)每帧 5 条 `TASKINFO 参数 FILE0/FILENO0/LINE0/TEXTLINE/
NEXTVOICE 未实现(DIV-RT-015)`,来源 `ystdlib.yst` pc373..378。手册
`control/taskinfo.html` 只列到 `SCRIPTPOS`,五个槽无页 ⇒ 按 0.495 处理体实现
(`fixed(no-manual)`)。

### 1. 引擎证据(REA 0.495 `yu-ris.exe`,sha256 `4f5eefa3…`)

- `TASKINFO` 处理体 `0x4523fc` 按参数**存在性**短路(不看值),固定顺序
  `EXIST/E/A/Z/FILE/FILE0/LINE/LINE0/TEXTLINE/NEXTVOICE/IFNEST/GOSUBNEST/
  LOOPNEST/FILENO/FILENO0/SCRIPTPOS`;`GOLABEL/GOSUBLABEL/TEXTPOS` 在该
  处理体**没有分支**,落到默认 `return 0` 且不写 LET。
- `0x4528b3/0x4525fa/0x45274c`:`FILE0/FILENO0/LINE0` 读 `task+0x144[0]`
  (GOSUB 帧数组第 0 项)。`0x4459c4` 在 GOSUB 写 `frame+4`=返回 pc、
  `+8`=调用方脚本号、`+0xc`=调用点源码行。
- `0x45272a/0x452708`:`TEXTLINE/NEXTVOICE` 读 `task+0xc/+0x10`;`0x44c390`
  (RETURNCODE)把当前指令首窗口记录两个 dword 抄入(首 = `id|type<<16`,
  次 = `length`)。语料 396 个 RETURNCODE 窗口均为 `{id=1..N,type=0,length=0}`
  (`TEXTLINE`=文本行计数,`NEXTVOICE`=0)。
- `0x43f0d8`:FILE/FILE0 走名字表 `0x801850[script_no]` 写字符串(LET 必须
  STR);`0x455540`:数值写只接受 INT/FLT。
- ID 未命中 `0x452918`:按 LET 目标类型写 0 / 0.0 / ""。
- `TASK` 处理体 `0x4506f4` 只检查 `0x684160/161/162/163/182/183/185`
  (ID/E/A/Z/#/SCRIPTPOS/RESTART),无槽 36 `TEXTPOS` 分支 ⇒ 官方忽略。

### 2. 修复

- `ExecState` 新增 `text_line/next_voice`(引擎 `task+0xc/+0x10`);
  `op_returncode` 在文本模式分支前保存窗口 `id|type<<16` 与 `length`
  (有符号 dword 扩展)。
- `op_taskinfo` 重写:按参数名收集存在性(不看值)、按引擎固定顺序分派;
  `FILE0/FILENO0/LINE0` 取内核 `frames[1]`(frames[0] 是基帧)= 第一层
  GOSUB 调用点,无 GOSUB 回退当前执行位;`FILE` 改返回 `yst_list` 路径
  (旧实现返回脚本号 `Int`,写进 `$` 变量会被 coerce 成 `"70"`);加 LET
  类型闸门(字符串/数值分型,类型不符不写);ID 未命中按目标类型写默认;
  `GOLABEL/GOSUBLABEL/TEXTPOS` 对齐官方 no-op(每槽一次 Info,不挂
  Unimplemented)。
- 夹具:`GroupSpec::line()` 写 YSTB part4 行号;`window_value()` 构造
  RETURNCODE 的 `length` 值窗口;TASKINFO 参数表补齐 21 槽。

### 3. 回归(原始结果)

```text
cargo test -p yu-rt --test fixture taskinfo_
  4 passed / 0 failed
  (RETURNCODE 来源、GOSUB 外框、存在性/引擎分派顺序、缺省值/无分支槽)

YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/data/ysbin cargo test -p yu-rt
  unit 70 / consistency 4 / corpus 11 / corpus_golden 4 /
  fixture 84 / golden 3  = 全部 ok / 0 failed

cargo check --workspace --all-targets
  Finished dev profile(仅既有 yu-gfx 测试 unused_mut 警告)
```

- 原始 YSTB 核对:`ystdlib.yst` pc373..378 的窗口正是
  `ID(0) + FILE0/FILENO0/LINE0/TEXTLINE/NEXTVOICE(9..13) + LET(1)`;
  `yst00123/124/…` 共 396 个 RETURNCODE 窗口 `id=1..N,length=0`。
- ERIS 语料 300 帧 `diag_census` 未出现 `TASKINFO` 相关 UNIMP;
  `text_scenario` 未推进到文本(启动链停在 scenario 等待输入),故五槽的
  语料形态以原始 YSTB 窗口 + REA 分支 + 合成夹具三方对齐为准。

### 4. 残留(在册)

- `NEXTVOICE` 非零值:ERIS 语料无 voice(窗口 `length` 全 0);实现按引擎
  原样读 `length`,不补猜。有 voice 的商业语料需另行取证交叉验证。
- `GOLABEL/GOSUBLABEL/TEXTPOS` 在 0.495 无分支;若换用有分支的引擎版本,
  需重新取证后实现(当前按官方 no-op)。

***

## 成果 TASKINFO-SLOTS-2 (2026-10-26) · 审查缺陷修复:A 槽取证、双写、断行与状态同步

对 TASKINFO-SLOTS 的自审发现 1 个严重回归 + 1 个语义缺陷 + 若干日志/状态问题。
本轮全部修复并补回归;手册无页项继续按 `fixed(no-manual)`。

### 1. A 槽取证(手册无页)

- `TASK` 处理体 `0x450bbf..0x450bd1`:A 参数存在时写 `task+0x15`
  (`mov [ebx+0x15], cl`);E 参数 `0x450bb1..0x450bbc` 写 `task+0x14`。
- 调度器 `0x4043dc..0x4043e6`:`movsx E; movsx A; test A, E; je 跳过`
  ⇒ **A 与 E 按位与非零**才步进。
- 任务构造器 `0x465630/0x465633` 对新任务同时写 E=A=1;
  `TASKINFO[A]` `0x4527b0` 用 `movsx` 读回。
- 内核新增 `TaskSeed/TaskNode/TaskView.active`、调度闸门
  `enabled && active`、`TASK` 的 A 参数、`Effect::SetActive`;
  `TASK[E/A]` 改为**只在参数存在时写**(修掉旧实现
  `TASK[ID=X Z=…]` 无条件 `SetEnabled(true)` 把已禁用任务重新打开的 bug);
  E/A 变更与 pending 任务都同步到 `task_view`。
- **A1 回归修复**:旧 `ORDER` 含 `"A"` 但 `match` 无分支,`TASKINFO[A]`
  会命中 `unreachable!` panic;现按引擎实现,不再崩溃。

### 2. 其余修复

- **TASK TEXTPOS**:上一轮台账写「官方无分支/n 内核 no-op」,但代码实际
  仍走 `other => Unimplemented`;本轮加 `Some("TEXTPOS")` 求值后丢弃,
  与 0.495 `0x4506f4` 同型(无槽 36 分支)。
- **FILENO0→SCRIPTPOS fallthrough**:引擎 `0x4525fa/0x4528fa` 写完 FILENO0
  后 `jmp 0x452628` 继续检查 SCRIPTPOS;两者同时请求时 SCRIPTPOS 二次写入。
  内核原先只取第一个 query;现补第二次写入(类型闸门各自生效)。
- **ReturnCode.line**:改用完整 u16 窗口 id(`kind | flag<<8`),不再被
  u8 `kind` 截断(单脚本文本行 >255);`text_line` 仍保存
  `id | type<<16` 的引擎原值。
- **TASK 重配置**:引擎复用任务记录且不清 `task+0xc/+0x10`;
  内核重建 `ExecState` 时继承 `text_line/next_voice`。
- **诊断**:LET 类型不符按 `query+目标类型` 只报一次(消息带 query 名);
  ID 未命中 + LET 未声明保留 `write_target` 的 Fatal,不再静默。

### 3. 回归(原始结果)

```text
cargo test -p yu-rt --test fixture            92 passed / 0 failed
  (新增 8 条:A 读写/E-A 同泵/双查询 fallthrough/完整 u16 行号/
   重配置继承/类型诊断去重/未声明目标/TASK TEXTPOS)

YURIS_SAMPLE_ERIS_YSBIN=… cargo test -p yu-rt
  unit 70 / consistency 4 / corpus 11 / corpus_golden 4 /
  fixture 92 / golden 3  = 全部 ok / 0 failed

cargo check --workspace --all-targets         Finished(仅既有 yu-gfx 警告)
```

### 4. 残留(在册)

- A 按 ysc checker 3(0/1)建模为 bool;引擎闸门是字节按位与,若脚本绕过
  checker 传入 >1 的 A/E,内核与引擎可能分歧(手册/检查器均限 0/1)。
- `NEXTVOICE` 非零值仍无 voiced 语料;实现按引擎原样读 `length`。
- ~~`SCRIPTPOS` 仍是内核内部 8 字节 blob~~ —— 已由后续成果
  `SCRIPTPOS-ENGINE` 替换为 0.495 引擎逐字节格式(旧 8 字节 blobs 不兼容)。

***

## 成果 SAVE-TIME (2026-10-26) · 存档时间错位:宿主本地时钟注入(`_TIME`)

### 1. 根因

- 存档菜单脚本 `es_SaveLoad.yst:178-266`(官方解包源)在保存时用
  `@_TIME(0,0..6)` 取「現実の日付」写入 `$mDATE(1)`,经 DNO=0004
  `$mSAVESTR()` 落盘;读档列表直接显示该字符串。
- 内核 `_TIME` 的旧口径(DIV-RT-009)是纯注入时钟:桌面/serve 从未给
  `BootConfig::start_ms` 注入真实值(恒 0),于是存档时间 = 1970-01-01 +
  游戏逻辑流逝,与实际日期完全错位。
- 这不是「命令未实现」,而是**宿主注入面缺失**;roadmap §B10 已挂
  `_OS_TIME(0)` 余项、DIV-RT-009 记录时区/纪元差异。

### 2. 修复

- `yu-rt`:`BootConfig::local_time: Option<HostLocalTime>`(年月日时分秒
  毫秒,构造函数校验范围);`@_TIME(1,*)` 返回注入基准,`@_TIME(0,*)`
  在基准上叠加 `now_ms - start_ms`,跨日/月/年进位;`None` 时保持
  1970 确定性注入时钟(用例/trace/golden)。
- `yu-engine`:`EngineOptions::local_time` 透传 `BootConfig`。
- `yu-cli`:新增 `clock::local_now()`(`time::OffsetDateTime::now_local`,
  安全 API,无 unsafe);`run`/`serve`/`desktop` 启动时注入一次;
  `case`/`bench`/测试默认 `None`。
- trace 回放固定 `local_time: None`(`yu-trace/src/record.rs`),不引入墙钟。

### 3. 回归(原始结果)

```text
cargo test -p yu-rt --lib
  78 passed / 0 failed(含 civil_days_round_trip、local_time_fields_and_midnight_carry)

cargo test -p yu-rt --test fixture time_sysvar_uses_injected_local_clock
  1 passed / 0 failed(@_TIME(0,0)=2026,@_TIME(1,4)=13)

cargo test -p yu-cli --lib clock
  1 passed / 0 failed(Windows 本地时间读取可用)

cargo check --workspace --all-targets
  Finished(仅既有 yu-gfx parity_fixes unused_mut 警告)

TEMP=<workbench>/tmp cargo run -q -p yu-cli --bin yu-cli -- case <project-eris> --case save-load-loop
  case save-load-loop: PASS
```

### 4. 残留

- `_OS_TIME(0)`(OS uptime)仍无宿主注入 ⇒ `Unimplemented`(roadmap 余项)。
- `_TIME(0,*)` 用逻辑流逝近似墙钟:长会话跨 DST/手动改系统时间不会重新
  取基准(存档时间戳场景可忽略);需要逐次取实时时钟时应扩宿主回调。
- trace golden `synthetic_trace_matches_golden` 的不一致来自本轮之前
  `yu-rt/tests/support/mod.rs` 的 TASKINFO 参数表改动(YCD sha 变化),
  与本项无关,未改 golden。

***

## 成果 SCRIPTPOS-ENGINE (2026-10-26) · `SCRIPTPOS` 换成 0.495 引擎逐字节格式

`TASKINFO[SCRIPTPOS]` 原先只产内核内部 8 字节 `[script_id][pc]` blob,
`TASK[SCRIPTPOS]` 也只恢复 script+pc,丢掉 IF/GOSUB/LOOP 三栈;ERIS
`s20/s21` 的 `es.LOG.TMPSAVE` + `TASK[ID,SCRIPTPOS]` 会在块内位置丢失
控制流上下文。本轮按 0.495 序列化器/装载器逐字节重写。

### 1. 引擎证据(REA 0.495 `yu-ris.exe` sha256 `4f5eefa3…`)

- 序列化器 `0x452a54`(`TASKINFO[SCRIPTPOS]` `0x452633` 调用):
  - 8 字节魔数 `0x5b4090` = `01..08`;
  - 2 字符 IF 深度;每帧 18 字符:`+0` loop 深度、`+4` entry pc、`+8` end pc;
  - 2 字符 GOSUB 深度;每项 16 字符:`+0` IF 深度、`+1` loop 深度、
    `+4` 返回 pc、`+8` 返回脚本号(u16);
  - 2 字符 LOOP 深度;每项 34 字符:`+0` IF 深度、`+4` body pc、
    `+8` end pc、`+0x10` counter 低 32、`+0x18` limit 低 32;
  - 8 字符 `task+0xc`(text_line)、8 字符 `task+0x20`(pc)、
    8 字符脚本名长度 + 原始脚本名字节。
  - 数值字节均为 `'a'+nibble` 双字符,多字节整数高字节在前(big-endian)。
- 装载器 `0x450fe0`(`TASK` `0x450bec` 调用):按同一布局写回
  `task+0x3e/+0x140/+0x244`、三个帧数组、`task+0xc/+0x20`,
  并用脚本名在 `0x801850` 名字表 strcmp 找回 `task+0x3c`;
  脚本名长度上限 `0x106`(`0x451749`)。
- 帧字段来源:`IF 0x4462d0`(`+0`=task+0x244 loop 深度、`+4`=当前 pc、
  `+8`=第三窗口 end)、`LOOP 0x447ef0`(`+4`=pc+1、`+0`=task+0x3e、
  `+8`=end、`+0x10`=counter、`+0x18`=limit)、
  `GOSUB 0x4459c4`(`+0`=task+0x3e、`+1`=task+0x244、`+4`=pc+1、
  `+8`=task+0x3c)。
- `TASK` 缺目标任务时 `0x4507b4 test ebx; je` 报错。

### 2. 实现

- 新增 `crates/yu-rt/src/scriptpos.rs`:`ScriptPos`/`IfEntry`/`GosubEntry`/
  `LoopEntry` + 引擎字节编解码;6 条单元测试含**手工拼装引擎串反解**。
- `IfFrame` 增加 `loop_depth`(op_if 压帧时取 `st.loops.len()`),
  用于序列化引擎 IF 帧 `+0`。
- `TASKINFO[SCRIPTPOS]`(含 FILENO0→SCRIPTPOS fallthrough)产引擎串;
  `TASK[SCRIPTPOS]` 解码后完整写回 `script_id/pc/base_a/text_line/
  if_stack/loops/frames`,脚本名→脚本号走 `yst_list` 精确匹配;
  保留 `next_voice`、TINT/TFLT/TSTR、E/A/Z(引擎装载器不写这些);
  目标任务不存在时 Fatal(对齐 `0x4507b4`)。
- 旧的 8 字节内核 blob 不再接受(会报 `rt.scriptpos` 魔数错误);
  需要兼容旧内核存档时必须重新生成。

### 3. 回归(原始结果)

```text
cargo test -p yu-rt --lib scriptpos       6 passed / 0 failed
cargo test -p yu-rt                       unit 79 / consistency 4 /
  corpus 11 / corpus_golden 4 / fixture 94 / golden 3 = 全绿
cargo check --workspace --all-targets     Finished(仅既有 yu-gfx 警告)
```

- 新增夹具:`fixture::task_scrip_pos_restores_full_exec_state`
  (合成 IF×2+GOSUB×1+LOOP×1 快照 → `TASK[SCRIPTPOS]` → TASKINFO 三嵌套数
  与 TEXTLINE 全对,且再次 TASKINFO[SCRIPTPOS] 还原同一 `ScriptPos`)。
- `taskinfo_fileno0_falls_through_to_scrip_pos` 改为解码引擎串,断言
  GOSUB 帧(返回 pc=1/script=0)、pc=1、脚本名 `fixture0001.yst`。
- ERIS `text_scenario` 600 帧:无 `rt.scriptpos` 诊断/错误;
  `s20/s21` 的 TASKINFO SCRIPTPOS 编码路径实际执行。

### 4. 残留(在册)

- 引擎不序列化 GOSUB 帧的 PINT/PFLT/PSTR 参数与返回值;内核恢复时新建
  帧的 args/ret 为空(与引擎串字段集一致,引擎复用内存时的陈旧值不可依赖)。
- LOOP counter/limit 在引擎格式里只有低 32 位;超出 i32 会截断/符号扩展
  (内核同型)。
- IF 帧的 base A 不在引擎串内;内核按帧索引(0 基)推导,恢复后 `IFBREAK`
  的 `task+0x3e` 口径一致。
- 损坏串:内核 Fatal(`rt.scriptpos`);引擎装载器打印错误后返回 0
  (TASK 处理体只判负),内核选择显式失败。

***

## 成果 SCRIPTPOS-XVER (2026-10-26) · SCRIPTPOS 跨五版核对与 GOSUB 线上宽度勘误

起因:审查提出实现证据过度集中在 0.495,要求用其他官方引擎交叉求证。
本轮把序列化器/装载器在 0.292 / 0.466 / 0.479 / 0.495 / v494 五版逐一比对,
发现并修正一处线格式误读:上一成果把 GOSUB 记录写成 16 字符,实际五版
引擎的序列化与装载推进量都是 **20 字符**。

### 1. 跨版本证据(序列化器 + 装载器,均为官方 exe)

| 版本 | 序列化器 | 装载器 | IF | GOSUB | LOOP |
|---|---|---|---|---|---|
| 0.292 | `0x438b2c` | `0x437548` | `0x12` | `0x14` | `0x22` |
| 0.466 | `0x4438d0` | `0x441ed4` | `0x12` | `0x14` | `0x22` |
| 0.479 | `0x44b718` | `0x449d7c` | `0x12` | `0x14` | `0x22` |
| 0.495 | `0x452a54` | `0x450fe0` | `0x12` | `0x14` | `0x22` |
| v494 | `0x4528c8` | `0x450e74` | `0x12` | `0x14` | `0x22` |

- 五版魔数都是 `01..08`(0.495 常量 `0x5b4090`;装载器校验
  `add ecx,-0x30205 / add edx,-0x80705`)。
- GOSUB 第 4 字段:序列化器把运行期 **u16** 零扩展后按 32 位写出
  (0.292 `0x438eae`、0.466 `0x443c51`、0.479 `0x44ba9f`、
  0.495 `0x452dbb` 均为 `movzx ..., word ptr [frame+8]` 后写 8 字符);
  装载器消费 8 字符但只把低 16 位写回 `+8`(0.495 `0x451302`
  `mov word ptr [edx+8], di`)。因此线上是 20 字符,不是 16。
- IF 18 字符(loop 深度、entry pc、end pc)与 LOOP 34 字符
  (IF 深度、body pc、end pc、counter 低 32、limit 低 32)五版一致;
  尾部 `text_line`/`pc`/名字长度三个 u32 + 原始名字五版一致,
  0.495 装载器把超过 `0x106` 的名字长度截断(`0x451749`)。
- TASKINFO 分派面:0.466/0.479/0.495/v494 的存在标志检查顺序
  (`+1,+0,+2,+3,+4,+5,+6,+9,+8,+b,+c,+d,+10,+11,+12,+7,+a,+13`)
  与 FILENO0→SCRIPTPOS fallthrough 完全一致;各分支字段语义也同构,
  只是任务结构体偏移随版本不同:
  TEXTLINE/NEXTVOICE = 相邻两个 dword(0.466 `+0x7ffc/+0x8000`、
  0.479 `+0xa74/+0xa78`、0.495 `+0xc/+0x10`);TASKINFO 的 A/E 分支
  读相邻两字节且 A=E+1(0.466 `+0x8005/+0x8004`、0.479
  `+0xa7d/+0xa7c`、0.495 `+0x15/+0x14`);IFNEST 读 IF 计数
  (0.466/0.479 `+0xa`、0.495 `+0x3e`)。`A & E` 调度闸门另在
  0.495 `0x4043dc` 与 v494 `0x4043cc` 逐指令一致;0.292 更早,
  没有 FILENO0、SCRIPTPOS 也在别的槽位。内核按**参数名**解析
  (不查任何一版槽号表),这类跨版本槽位变化不会造成静默失效。

### 2. 修正

- `scriptpos.rs`:`GosubEntry` 线上宽度 16→20;`return_script` 编排放宽为
  零扩展 u32、解码只取低 16 位(与装载器 `mov word` 一致);
  解码时脚本名超长按装载器行为截到 `0x106`(编码端仍拒绝超长名)。
- 新增单测:`gosub_return_script_is_wire_u32_and_decode_keeps_low16`
  (含篡改高位仍取低 16 位的对齐断言)、
  `decode_clamps_oversized_name_length_like_loader`。
- 模块注释改为跨五版证据(不再以 0.495 单版作唯一依据);
  DIV-RT-025 正文同步勘误。

### 3. 回归(原始结果)

```text
cargo test -p yu-rt --lib scriptpos       9 passed / 0 failed
cargo test -p yu-rt                       unit 82 / consistency 4 /
  corpus 11 / corpus_golden 4 / fixture 94 / golden 3 = 全绿
cargo check --workspace --all-targets     Finished(仅既有 yu-gfx parity_fixes 警告)
```

### 4. 残留

- 0.292 的 TASKINFO 参数集比 0.466+ 少(FILENO0 等),但其 SCRIPTPOS 线格式
  与后续版本同构,内核可直接解码;参数集差异由名字解析吸收。
- GOSUB 返回脚本号的高 16 位引擎恒写 0;内核与装载器一样只保留低 16 位,
  不把高位当作错误。





## 成果 SPRITE-SNAP-ORDER (2026-10-27) · 立绘归位空白帧收口:快照表面尺寸按命令序定稿

起因:上一轮收口了立绘位置、移动阶梯与逐帧卡顿,残留「过渡结束归位那一下
闪一帧空白」。本轮把它定位到**规划阶段顺序倒置**,并按官方语义(COPY 命令
执行时建表面并把尺寸写回层)改成命令序定稿,一帧不再丢。

### 1. 症状(原始证据)

β4 演出1 过渡结束,`es_SPExec.yst` 发
`CGACT[COPY2=1 ID=SPLBUFa601 ID2=SPLBUF601]` → `SIZE2`。逐帧 PNG(wgpu,
精确读回):host 2866 有立绘、2867 **整张背景**、2868 恢复;像素采样同结论
(`(388,180)` 从 `152,121,90` 跳到 `176,172,173` 再回 `245,192,139`)。

### 2. 根因:同一帧「先 COPY 后 SIZE」时,尺寸只能从**还没建的 GPU 目标**取

- `passes::plan` 原先把「图像加工预执行」(步骤 1)排在「命令段 COPY
  规划/建纹理」(步骤 2)之前。`SIZE2` 在步骤 1 里向 `Targets::snapshot(id)`
  要快照尺寸,而该 GPU 目标要到步骤 2 的 `ensure_snapshot` 才创建
  ⇒ `gfx.imageop.source.snapshot_missing` ⇒ 该 op 被跳过 ⇒ 整层只剩
  `RECTPAINT3` 的清空结果 ⇒ 这一帧没有立绘。
- 快照**内容**没有错:把快照纹理读回来,创建帧与之后两帧的哈希完全一致
  (`c7a79fd81b1ac1e9`,`(147,110)=[245,192,139,255]`),原判断
  「首建即空、冻结后不重试」被推翻。
- 与后端无关:soft 逐帧连续(它按命令序执行,COPY 先落 `snapshots`,不存在
  这个空洞);也无 `SKIP` 以外的报错。

### 3. 修法(单一口径,不是兜底)

- `passes::plan` 新增**步骤 0**:按 `frame.passes` 命令序预扫描全部可执行
  `Op::Copy`,用 `copy_output_dims()` 定稿 `SnapshotDims`(官方
  `FUN_0042f809` 的 `local_2c=(X2-X)+1` / `local_28=(Y2-Y)+1` 写回层)。
- `image_source_dims()` 的 `Snapshot` 分支优先取定稿值,其次才是既有 GPU
  目标。后续加工因此在本帧就能拿到「已经存在」的表面尺寸。
- `plan_copy()` 建纹理与 `capture_copy_mirror()` 建 CPU 镜像**共用同一份**
  尺寸,三处不再各算一遍(此前是两份近似公式,存在错位风险)。
- 全部落在 `crates/yu-render/wgpu/src/passes.rs`;无环境变量开关。

### 4. 回归

```text
cargo test -p yu-backend-wgpu --test wgpu size_after_copy_in_same_frame_keeps_snapshot_dims
test result: ok. 1 passed; 0 failed
```

测试有效性已反向验证:把 `image_source_dims` 的定稿分支临时去掉后,同一用例
精确复现生产错误并失败:

```text
同一帧 COPY→SIZE2 不得报快照缺失,得到 ["gfx.imageop.source.snapshot_missing"]
```

场景级验证(逐帧像素,`probe_layers --px 388,180 --px 388,350`):

```text
2866  (388,180)=152,121,90  (388,350)=255,248,229
2867  (388,180)=152,121,90  (388,350)=255,248,229   ← 修复前这里是背景色
2868  (388,180)=245,192,139 (388,350)=255,247,228
2869..2880  diff=0(完全静止)
```

### 5. 卡顿复查:不是泄漏,是 CPU 合成

`state_audit` 拉到 6200 帧(每 200 帧一档,同一场景内):

| frame | layers | copies | images | passes | render |
|---|---|---|---|---|---|
| 3200 | 678 | 72 | 28 | 102 | 3.49 |
| 3800 | 678 | 72 | 29 | 103 | 3.38 |
| 4000 | 678 | 72 | 68 | 142 | 10.70 |
| 4400 | 680 | 72 | 70 | 144 | 10.28 |
| 5000 | 581 | 70 | 65 | 136 | 10.71 |
| 6200 | 480 | 70 | 60 | 131 | 11.11 |

- 没有单调爬升:同一场景段内持平,跨场景随内容(可见层/op 数)起伏。
- 3800→4000 的一次台阶来自**对话文本窗**上线:文本窗层每帧 40 条
  `MERGE2/MERGE/TRANSITION` 全走 CPU 逐像素合成。新增计时桶实测:

```text
[rt] frame=4065 plan_total=12.68ms snap=0.34 image=11.63
[rt]   image_paths sampled=9 paint=17(0.54ms) pixel=1(0.00ms) composite=40(10.87ms)
```

  ⇒ 10.9ms / 12.7ms 花在 composite 上。

### 5.1 收口:合成链记忆化(2026-10-27)

`MERGE` 只有「目标是快照」时走 GPU(`plan_paint` 的 `PaintKind::Texture`),
普通层仍走 CPU,而对话窗每帧 41 条(`2×RECTPAINT3 + 39×MERGE2`)。

**关键事实**:每帧层表面都从**基图**重新开始(`apply_image_to_override` 的
首条 op 取 `resolve_base_texture`,不读上一帧结果),所以「基图 + 动作序列 +
几何 + `SET` + 各源像素身份 + 源文本修订号」构成**纯函数输入**;逐项相同时
输出必然逐像素相同。缓存命中时直接复用上一帧的 CPU 表面,链上每条仍按
「已执行」计数(与 soft 逐条重算的 `ops_drawn` 口径一致)。

- 判定:`passes::plan` 步骤 0.5;只对「全部动作 ∈ {RECTPAINT 族, MERGE 族}」
  且基图是资产/1×1 黑的层生效(快照目标走 GPU 命令段,不参与)。
- 指纹:`chain_signature()` + `layer_token()`(覆盖 `text_revisions`、
  覆盖纹理哈希、UV 修饰、资产键、快照 `CopyId`+目标 id)。
- 缓存:`WgpuRenderer::composite_cache`,`reset()` 清空,逐帧按存活层剪枝。

**验证(逐帧像素对拍,开/关记忆化各跑一遍)**

```text
4100 帧:frames compared 4100  differing 0
6200 帧:frames compared 6200  differing 0
```

**收益(同一帧,`YU_RENDER_TIMING=1`)**

| | 修前 | 修后 |
|---|---|---|
| `plan_total` | 12.68ms | 3.28–3.82ms |
| `composite` | 40 op / 10.87ms | 1 op / 1.45–2.04ms |
| 剩余 composite | — | `sp.tmpB → SPLBUF602`(2 条链,低于记忆化阈值) |

**残留(可选)**:普通层的 `MERGE` 仍是 CPU;若将来出现「每帧输入都在变」
的合成链(记忆化必然 miss),再上「离屏覆盖目标」(`Override::Pass`)把整条
链搬到 GPU。

### 6. 取证工具

`probe_layers` 扩充(仍是只读探针):

- `--px x,y`(可重复):采样最终屏幕像素,并给相邻帧 `diff=/changed=`;
- `--backend soft|wgpu`:同场景换后端对拍;
- 打印 `COPY <id> <dest> <- <src> src_tex=… region=… status=…`;
- `IMG … status=…` 补上状态,`SKIP` 行带 `source`。

### 7. 勘误

`X+=`(参数种类 2)按**增量**处理,与 `DIV-GFX-064` §4 原结论相反,详见该
文档新增的勘误小节与 `MOTION-SPIKE-2` §4 附注。

### 8. 验证命令(原始结果)

```text
cargo build --workspace --all-targets                                   Finished
cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets 37 套件全绿
```

***

## 成果 SAVE-EVENT-ORDER (2026-10-27) · 存档域统一命令时刻同步,修删档晚于刷盘

菜单存档失败(目标 `.sd` 被删且不再生成、无 Fatal)定位为存档/文件域
事件的**应用时机**问题,按统一口径修复,不是补丁。

### 1. 根因(探针实测,已还原)

`es_SaveLoad.yst` 的存档顺序:

1. `pc315 FILEACT[FILE=$es.SAVEPATH+$SFNAME DELETE=1]` —— 删除**事件入队**;
2. `pc341 SAVE[FILE=$SFNAME]` —— `op_save` **同步** `host.flush` 写盘;
3. `Engine::tick` 帧末 `host.apply_events(&events)` —— 此时才执行删除;
4. `pc342 SAVEEND[]` —— 清预约。

结果:刚写出的存档被延迟的删除抹掉,最终无文件、无报错。同一帧探针输出:
`AUDIT FILEACT script=19 pc=315` → `AUDIT SAVE flush pc=341 file=save30002.sd`
→ `AUDIT FLUSH ok exists=true`(25602 字节)→ `AUDIT FILEACT DELETE`
→ `final exists=false`。把 Save/File 事件临时改为 `emit` 同步应用后删除
排到 flush 之前,文件保留;但引擎随后仍 drain 一次,事件被重复应用——
所以正确修法是「命令时刻同步 + 只消费一次」。

日志里的 VARINFO STRTYPE、`SAVE[ID]` 缺 SET/G_*、`SAVE[SET]` 系统变量
`@60` 都是既有 **Recoverable/Unimplemented**(`step_command` 记录后继续),
与本次失败无关。

### 2. 引入点

结构性问题自 SaveHost 接线(B1 `16cef47`,2026-10-24)起存在:
`Vm::emit` 自 `d66ed41` 起只同步 Graphics/Window/Sound,Save/File 一直
等到帧末 drain。不是近期 TASKINFO/TASK/SCRIPTPOS/VARINFO 改动。
修复前最后一次内核菜单存档 `save00001.sd`(2026-10-25)缺 DNO 3(缩略图)
与 DNO 90(ICON),与内核两个既有缺口一致,说明当时写过但时序未被覆盖。

### 3. 修法(统一)

- `Vm::emit`:对 `Domain::Save | Domain::File` 的 DomainCall 也**命令时刻**
  同步调用 `save_host.apply_event`(与 Graphics/Sound 同口径);
- `Engine::tick`:帧末只保留事件统计/trace,不再把同一批事件喂给
  `SaveHost`(避免重复应用);
- 更新 `SaveHost` trait / `yu-save` 宿主文档 / 处理器注释,契约从
  「drain 时调用」统一为「命令时刻同步且只消费一次」。

### 4. 回归(原始结果)

```text
cargo test -p yu-rt --test fixture fileact_is_applied_before_save_file_flush_in_same_frame
  1 passed
cargo test -p yu-rt
  unit 82 / consistency 4 / corpus 11 / corpus_golden 4 / fixture 95 / golden 3 = 全绿
cargo check --workspace --all-targets  Finished
```

桌面复现(临时 save 根,菜单快捷存档):`save30002.sd` **25608 字节保留**
(修复前被删除)。

### 5. 残留(在册,另行处理)

- `SAVE[SET=@_RINT(1)]`(DNO 90):系统变量无 `var_snapshot`,仍只告警
  不写块。
- `SAVE[ID]`(DNO 3 缩略图):图层快照未实现(H-P4-02)。
- `FILEACT[DELETE]` 成功仍挂 `sv.command.unimplemented`(INFO)日志码,
  属日志码误用,不影响行为。
- 旧 `save00002.sd` 在用户工程根已被删除;新存档需在游戏内重新生成。

## 成果 BACKEND-SPLIT (2026-10-27) · 双后端定位:GPU 模式不再由 CPU 兜底/代劳

起因:主人指出 `soft` 与 `wgpu` 是**两条独立实现**,GPU 模式理应由 GPU 完成,
不得接入 soft 的实现;并要求把这条写进文档、再审查一遍职责耦合。

### 1. 写入的原则(新文档)

`docs/architecture/backends.md`(并在 `docs/architecture.md` v1.4 补充里挂链):

- `yu-backend-soft` = **CPU 模式**(`--backend soft`)+ 语义参考/对拍基线;
- `yu-backend-wgpu` = **GPU 模式**;像素工作必须由 GPU 完成,
  **运行期不得调用** `apply_*`/`blend_transition`/`scale_region`/`blit`;
- 允许共享的只有**契约与平台设施**:`Surface`/`RenderReport`/`SkippedOp`/
  `AssetIssue`、`TextureStore`(资产解码)、`TextComposer`(字形栅格);
- 机械判定规则(grep 命令)+ 现状账本(每个动作 → 现状 → 目标)+ 迁移顺序。

两个 crate 的模块头注释同步声明了该定位(rag 检索与新人先看到的就是它)。

### 2. 审查发现并修掉:GPU 启动失败会静默降级到 CPU

`crates/yu-cli/src/desktop.rs` 原来在 wgpu 初始化失败时自动 `options.backend =
Backend::Soft` 重试,只打一条 Warn。用户会以为在看 GPU 结果、实际是 CPU,
所有「GPU 模式还是卡/画错」的定位都会失真。已删除该兜底,改为显式失败:

```text
wgpu 后端启动失败:<原因>
本内核不会自动回退到 CPU 模式;确需 CPU 渲染请显式加 --backend soft。
```

### 3. 审查结论(在册,未清)

| 项 | 现状 | 处置 |
|---|---|---|
| 普通层 `MERGE/MERGE2/RECTPAINT` 等 ~20 个图像动作 | wgpu 调 `yu_backend_soft::apply_*`(CPU) | 按账本迁 GPU;`Override::Pass` 优先 |
| `ImageCacheKey`/`ImageOpSig`/`ImageSource` | wgpu 的缓存键类型定义在 soft | 抽 `yu-render-core` |
| `TextureStore`/`TextComposer` | 共享设施寄居 soft crate | 同上迁出 |
| `reconcile` 对拍 | `Renderer::Wgpu` 内持一个 `SoftRenderer` | **唯一允许共存**,缺省关闭,只读同一份 frame |
| 依赖方向 | `yu-gfx` 不依赖后端;soft 不依赖 wgpu;wgpu 依赖 soft | 待上面两项迁移后断开 |

### 4. 后端选择(两个独立项目,同一入口)

```text
yu-cli run <project> --mode desktop  --backend wgpu   # GPU 模式(产品路径)
yu-cli run <project> --mode headless --backend soft   # CPU 模式(确定性/无 GPU)
yu-cli serve <project> --backend soft
```

缺省:`headless`/`serve` = soft,`desktop` = wgpu。桌面宿主在 soft 下直接报错
并提示切 `--backend wgpu`(`host-desktop` 需要 wgpu 设备做窗口呈现)。

### 5. 验证(原始结果)

```text
cargo build -p yu-cli -p yu-backend-wgpu -p yu-backend-soft     Finished
cargo build --workspace --all-targets                           Finished
```

### 6. 结构重构(2026-10-27):抽出 `yu-render-core`,契约不再寄居 CPU 后端

按「先重构区分/耦合,再推性能」的顺序落地:

- 新 crate `crates/yu-render/core`(`yu-render-core`)承接两后端共享的**契约与
  平台设施**:`Surface`/`blit`/`RenderReport`/`SkippedOp`、`Texture`/
  `TextureStore`/`AssetIssue`(资产解码)、`TextComposer`/`TextPaintStats`/
  `text_base_key`/`ComposedLayer`(字形栅格)、`ImageCacheKey`/`ImageOpSig`/
  `ImageSource`/`SizeKey`/`SizeSource`、`scale_region`、`png`。
- `yu-backend-soft` 保留同名再导出(既有路径不破),`apply_*`/`blend_transition`
  等**像素算法仍留在本 crate** —— 它们就是 CPU 模式的实现。
- `yu-backend-wgpu`/`yu-engine`/`yu-cli`/`host-desktop` 改为直接引用
  `yu_render_core::`。重构后 `grep yu_backend_soft crates/yu-render/wgpu/src`
  只剩在册的 `apply_*` + `blend_transition`。
- 顺带把 `scale_region` 从 `imageop` 移入 core(它是通用栅格原语,不是后端语义)。

**行为中性验证(原始结果)**

```text
6200 帧逐帧屏幕哈希 vs 重构前:frames compared 6200  differing 0
cargo test -p yu-render-core -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
  38 套件全绿
cargo build --workspace --all-targets   Finished
```

**剩余耦合(账本 §4)**:wgpu 仍在普通层调用 `apply_merge`/`apply_rect_paint`/
`blend_transition`/`apply_reverse`/… 这批动作需逐个 GPU 化;优先级最高的仍是
普通层 `MERGE/MERGE2`(立绘表情合成的 `sp.tmpB → SPLBUF602` 每帧约 2ms)。

### 7. 渲染尖峰归因(2026-10-27,立绘在场稳态)

重构后用逐 op 计时(`>0.02ms` 才打印)定位立绘场景的每帧耗时构成:

```text
f=2890  merge      :/W0/sp.tmpB    -> :/W0/SPLBUF602  2.15ms   ← 表情/身体合成
f=2890  transition :/W0/SPLBUF601  -> :/W0/SPLBUFb601 1.20ms   ← 换图过渡
f=2890  rectpaint3 :/W0/BT…MESW.TX -> 同层            0.25/0.28ms
f=2890  rectpaint3 :/W0/BT…NAMEW.TX -> 同层           0.04/0.05ms
```

稳态(2500–2900 帧,393 帧)`plan_total` 中位 **3.08ms**,构成:
`image 2.14` / `snap 0.25` / `text 0.02` / `rest 0.63`。image 里就是上面两条
普通层合成 —— 也就是「立绘一出现 ms 就 3ms」的来源。

两条都在账本 §4 的未清项里(普通层 `MERGE` 与 `TRANSITION` 仍调用 CPU 实现):
`apply_merge`/`blend_transition` 各要克隆两侧表面(295×580×4 ≈ 0.68MB×2)再
逐像素处理。**修复方向 = `Override::Pass` 离屏目标 + 现有 merge/过渡 pipeline**,
验收:这两条 op 的 `composite` 计时归零、soft 对应函数在 wgpu 构建里
`unreachable!()` 仍能出画面。

## 成果 SPRITE-TRANSITION (2026-10-27) · 立绘过渡查证:`TID` 配对层缺失导致过渡整条被跳过

用户报演出1 后半程:①过渡应为半透明、实际直接黑边;②立绘过渡时被压矮/竖条;
③个别字不显示;④偶发尖峰。本轮按脚本 → gfx → 后端逐层对照,已定位②③之外的
主链,并修掉一处错误判据。

### 1. 脚本原貌(证据源)

`project-479/data/script/UserScript/演出機能.txt:207-236`:

```text
\SP.TR(11)            // 选过渡图案 11
\SP.2TP(256,0,1000)   // T: 256 → 0,1000ms
\SP.GO(YUURI)
```

`es_Sprite.yst` 的实现链:

- `es.SP.LOADTR`(`:6294-6375`):`$tr=$S.FTR(@wn)` 非空时
  `CG[ID=$TRBUF1 … FILE=$loadfilename]`(图案图 = `cg/tr/ysr023`)
  + `CG[ID=$TR1 … SX=1 SY=1 FILE=""]`(mask 层);
- 每帧(`:8162-8186`):`@L.TRA>0` 时先
  `CGACT[ID=$TRBUF1 ID2=$TR1 SIZE=1 SX=@lsx1 SY=@lsy1]` 把 mask 对齐到精灵尺寸,
  再 `CG[ID=$L1 TID=$TR1 T=@TP TA=@L.TRA(@LNO)]`。

图案 `cg/tr/ysr023.png` 实测是**柔和的点阵网格**(不是硬条纹),所以官方的
`TR(11)` 是"按点阵渐隐"的半透明过渡。

### 2. 现场(更正版:过渡确实执行了,问题在 mask)

> ⚠️ 本节原写「mask 层没被创建、过渡整条被跳过」,那是把观察窗口
> (4090-4160) 截在 `\SP.TR(11)` 生效之前导致的误判。以下是 4160-4260 的复核。

`probe_layers`(新增 `TRANS` 行与层状态 `T=/TA=/TID=`)在引擎帧 4253 附近:

```text
[tid] f=4253 ID=:/W0/SPL601 TID=SPLTR601
TRANS :/W0/SPL601 from=SolidBlack1x1 mask=Some(SolidBlack1x1)
      fade=0.0932 t=47 ta=128 from_layer=SPL601 to_layer=SPLTR601 size=295x501
```

- 过渡 op **有产出**,`T` 按 256→0 推进,`fade` 随公式变化;
- `from`/`mask` 在数据层是 `SolidBlack1x1`(SC 层 `FILE=""` 的 1×1 黑),
  真实像素由后端的 `from_layer`/`to_layer` 取**加工后表面**;
- mask 层 `SPLTR601`/`SPLTRBUF601` 在 `\SP.TR(11)` 生效帧才创建
  (此前不存在,所以 4090-4160 窗口里看不到)。

**软/硬对拍(同帧、精确读回)**:4245 帧两边都是"半透明渐隐",**都没有竖条**;
4154 帧两边**都出现同样的竖条**。⇒ 竖条不是 GPU 采样差异,是**上游(mask 内容/
取样区间)**的问题,两个后端一致复现。

### 3. 顺带修正的判据:`TextureRef::None` 不等于"没有配对"

`transition_pair` 原先把 `FILE=""` 的配对层判成无配对。引擎的 `YS_Texture2`
取的是配对层**表面**(`FUN_00414ba4` 行 881-971),`FILE=""` 层照样有 1×1 表面
并被 `CGACT` 写入,所以判据应是"节点存在即成立",像素由后端的 `to_layer`
(图像加工覆盖纹理)解析。已改;6200 帧逐帧像素对拍 **differing 0**(该语料里
本层始终缺失,所以是"修对了但还没被触发")。

### 4. 未闭环

- **竖条(软硬同现)**:4154 帧两后端一致。mask 图案 `cg/tr/ysr023` 是柔和点阵,
  不是竖条,所以是 **mask 的取样区间/尺寸**被算错。下一步在后端把
  `TRANS` 的 `mask` 解析结果(纹理 id + dims + uv 矩形)打出来对拍。
- **压矮**:`SPL601 size=295x501`(常态 531),同一过渡段出现;需确认是脚本
  `SXYZ` 的 Z 缩放还是 `SIZE` 源区间错。
- **过渡的"黑边 vs 半透明"**:4245 帧已接近半透明,说明 `T` 模型方向对;
  但 `transition_coverage` 仍是 TA=128 校准模型(DIV-GFX-045,TA≠128 偏差 ~0.12),
  要按官方固定管线状态重推。
- **个别字空白**、**偶发尖峰**:4200 帧 summary 的 `skipped_total=0`、无 text 诊断,
  需要按用户时间点抓帧再定位。

### 5. 结论

过渡问题的第一块多米诺是 **mask 层缺失**(VM/脚本侧),不是渲染公式;
但即便 mask 到位,`transition_coverage` 也只是 TA=128 校准模型
(DIV-GFX-045 已记 TA≠128 偏差 ~0.12),届时需按官方固定管线状态重推。

### 6. 已修:立绘退出时被压矮(过渡绘制丢失层的有效源区间)

**现场**:`SP.TR(11)` 淡出期间,立绘明显变矮;同一层在过渡帧的 `size=295x501`,
而常规帧的源区间是 295×531(源纹理 295×580)。

**根因**:`passes::plan_transition` 用 `push_quad`(**uv 恒 [0,1]**)画过渡 quad。
层上其实挂着 `SIZE`/`SIZE2` 的 UV 修饰(`Override::Uv { uv, width, height }`,
有效源是 295×580 里的 295×501 子矩形),而过渡路径把 uv 丢掉 ⇒ 整张 295×580
被压进 295×501 的框,**高度压 0.864×**。

**修法**(`crates/yu-render/wgpu/src/passes.rs` + `shaders/transition_fixed.wgsl`):

- 固定管线路径改取层上的 `Override::Uv`:`draw_uv/draw_w/draw_h` 来自修饰,
  顶点改用 `push_quad_uv`;
- mask 是独立表面,仍按整层 [0,1] 取样,所以在 shader 里把顶点 UV
  **反归一化**回层内坐标(`mask_uv = (uv - uv_min) / (uv_max - uv_min)`);
  UV 矩形随 uniform 下发(`Fade` 结构扩到 32 字节,`_DXMODE≠0` 的 psC/psD
  仍用 16 字节,不动)。

**验证**:

- 4154 帧源行映射:修前 `screen_y = 100 + src_row*501/580`(压缩),
  修后 `screen_y = 100 + src_row`(1:1),与脚本 `SIZE2` 的源矩形一致;
- **6500 帧逐帧哈希收口到只剩立绘过渡窗口**:`[4145,4193]`、`[4196,4450]`
  两组共 304 帧变化,**其余 5896 帧逐字节不变**。

**越界 UV 的夹取(现场回归的教训)**:整面区域按闭区间换算会得到
`1.00125/1.001667`(801/800)这类**越界 UV**;开幕 logo 的过渡层正好是这种
"整面 + 一点点越界",第一版未夹取 ⇒ 开幕那 100 帧全部变化(用户报
「黑白分界线很明显」)。加 `clamp_uv_max`(上界夹到 1,真正的子矩形不变)后,
**开幕/标题的过渡恢复逐字节一致**,只有立绘那种真子矩形(`v1=0.8638`)才走新路径。
教训:改公共绘制路径时,先用 6200 帧哈希把变化范围压到目标窗口,再交付。

> 说明:**过渡里那层点阵/竖纹是 mask 图案**(`cg/tr/ysr023` 的点阵模板),
> 属官方转场效果,不是缺陷;本轮只修"源被压进框"的几何。

```
cargo build --workspace --all-targets             Finished
cargo test -p yu-render-core -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
  38 套件全绿
```

### 7. 过渡覆盖度模型(官方是顶点 α 渐变,不是阈值)—— 在册待实现

用户报「时钟/开幕的过渡是硬边黑幕,官方是平滑半透明」。REA 逐指令解出官方
固定管线的真实做法(详见 `docs/divergences/DIV-GFX-065-transition-coverage-model.md`):

- 两个纹理 stage 都是 `SELECTARG1`(颜色) + `ALPHAOP=MODULATE`(alpha =
  纹理 alpha × `D3DRS_TEXTUREFACTOR.a` = 层 A),**stage2 关闭**——
  没有任何 `T` 参与 stage 状态,也就没有阈值;
- `T` 进入**顶点色**:`0x415462-0x415532` 连续构造 4 个带灰度 D3DCOLOR 的顶点,
  即一块**四角 α 渐变**的 quad,随 `T` 扫过层矩形;
- 因此官方视觉是「渐变 × mask 乘子」,天然柔和;我们的
  `transition_coverage`(slope 30..100、threshold 1-2fade)是 TA=128 的校准拟合,
  斜率导致 1%–3% 的过渡带 ⇒ 硬边。

**结论**:这不是调参能修好的,必须把 `plan_transition` 改成逐角 alpha + mask 乘子;
公式还需继续解 `0x415462-0x415532` 的浮点链。台账状态 `open`。

### 8. 过渡配置真值(脚本表)与结论更新

- `\FOUT(1500,5)`(jp_0053.txt:249,「幕を下ろす」)展开为全屏黑幕 +
  `\SP.TR("5")` + `\SP.2TP(0,256,1500)`。
- `SP.TR(name)` 经 `es.TRDEF.CHECK` 查表(`UserDefine/jp_0011.txt`):
  **`5` → `ysr005`,TA=4**(回転/時計回り);**`11`/`12` → `ysr011/012`,TA=128**
  (シャッター);`0` → `ysr000`,TA=0(纯 fade)。
- 脚本原注释:「値が大きいと『くっきりした感じ』」——**TA 就是软硬旋钮**。
- 我们的 `transition_coverage` 只在 **TA=128** 标定过(残差 ≤8/255),TA≠128
  偏差 ~0.12 ⇒ 时钟(TA=4)必然走样。这就是「时钟硬边、立绘勉强」的根因,
  与「调参」无关。

**下一步**:按官方微场景重测 TA∈{0,4,64,128} 的原始 α 表 → 反解闭式 →
替换 `transition_coverage`,soft/wgpu 同步。台账见 DIV-GFX-065 §4/§5。

### 9. 官方过渡 α 原始表(从已有 micro 参考帧读出)

`j2-micro-trans8` 的参数网格(TA{2,64,128,192} × T{0,64,128,192,255} × mask{0,128})
参考帧已把 α 写在像素里,读表结果与四条硬事实见
`docs/divergences/DIV-GFX-065-transition-coverage-model.md` §6。要点:

- mask=128 在 T=128 恒 0.5、T=192 恒 1.0(与 TA 无关);
- mask=0 在 T<=192 恒 0(唯一例外 TA=2/T=192 = 0.502);
- mask=0 在 T=255 **精确等于 `(256 − TA/2)/256`**(TA=2/64/128/192 → 0.996/0.875/0.749/0.624)。

结论:**只有两个 mask 电平,不足以分离「阈值/斜率/分块顶点 alpha」三个自由度**,
现有 `transition_coverage` 的拟合在 TA=4(时钟)上必然错。闭环需要新造一个
**256 级渐变 mask + 密 T 网格**的官方微场景(工程与官方 exe 已在
`target/tmp/j2-micro-trans8`,缺的是抓帧驱动,按
`<repo-old>/docs/windows-handoff.md` 重写)。

### 10. 结论修正:官方过渡是「CPU 轮廓带 + 逐块顶点 α」,不是逐像素覆盖率公式

重读 `FUN_00414ba4` 伪代码(REA `ev_128a7aa3…`)后确认:
stage1 的 `ALPHAOP` 最终被设成 `D3DTOP_DISABLE`(0x415846)⇒ **mask 不参与 alpha**;
`alpha = tex0.a × TFACTOR.a`,而 TFACTOR 是**每次 DrawPrimitiveUP 前设一次的标量**;
循环按 `+0x130`(部件)/`+0xb0`(分块)两张表逐块构造 4 顶点画 triangle strip。

⇒ 官方在 CPU 上**沿 mask 等值线切块**,每条带子按 T 算 α 再画。
这解释了 §9 的 α 表为什么怎么拟合都对不上(单块单元只有一个 α 台阶),
也解释了 TA=精度=分块密度=软硬。

**待办(在册)**:实现「轮廓带 + 逐块顶点 α」路径,soft/wgpu 同步;
现有 `transition_coverage` 仅为临时近似,在 `docs/architecture/backends.md` 账本标 `open`。

### 11. 定稿:过渡软边来自几何(等值线带),alpha 是常量

把 `FUN_00414ba4` 的赋值链对完后确认:4 个顶点同色(灰度 A)、TFACTOR 每层设一次、
stage1 `ALPHAOP=DISABLE` ⇒ **最终 alpha = tex0.a × A,与 mask/T 无关**。
「哪里看得见」由**几何**决定:循环按 `+0x130`(部件)/`+0xb0`(分块)表把可见区域
沿 mask 等值线切成 triangle strip,块内用 16.16 定点插值边界;`T` 决定等值线位置,
`TA` 决定分块密度。

⇒ 结论:不存在逐像素覆盖率公式;软边 = 等值线几何的平滑。要 1:1 必须实现
「等值线 → strip」渲染,与现有「单 quad + 覆盖率」是两种实现。
台账:`docs/divergences/DIV-GFX-065-transition-coverage-model.md` §7/§8。

### 12. 过渡 oracle 测试落地(40 格官方 α 网格)

新增 `crates/yu-render/soft/tests/transition_alpha.rs`:把官方微场景
`j2-micro-trans8` 参考帧里读出的 40 格 α 作为真值,逐格渲染同样构型并比对
(容差 2/255)。当前 **9/40 格超差,且全部落在 TA 依赖上**;新硬事实:
**官方 T=128 的 α 与 TA 无关**(mask=128 → 0.498、mask=0 → 0.000,四个 TA 一致),
而我们的 `fade` 随 TA 变 ⇒ 系统性偏差。测试以 `#[ignore]`(注明原因)留作
轮廓带实现的验收门禁。

### 13. 撤回宽带覆盖率模型 + 官方整场 FOUT 幕对拍(IoU 0.85-0.87)

**撤回**:曾把 `transition_coverage` 改成宽带形(`c=1-t`,`w=1/6`),在官方 9 级
mask 扫描上 rms 0.0043 —— 但它把**开幕**的平滑梯度掩膜糊成灰阶(帧 40 与
回退前逐像素差 10 万+ 像素、max 129),而官方开幕是锐利生长的黑菱形格。
两种带宽都能过那张表(采样只落在连通域边界,钉不住带宽),所以带宽必须靠
梯度掩膜画面判别。已回退 `c=1-2f / K=30+70f` 窄带阈值(`yu-gfx/src/layer.rs`、
soft `render.rs`、wgpu `passes.rs` + `transition_fixed.wgsl` 的同式),
并加 `transition_coverage_stays_a_hard_threshold` 防再被改宽。
回退验证:重渲帧 40 与回退前基线**逐像素一致**(changed=0, max=0)。

**整场取证**:新增 `tools/official_trace/capture_run.py`(默认**完全被动**,
不发任何输入,窗口关闭即结束),人手动把官方 0.495 点到
`jp_0053.txt:249` 的 `\FOUT(1500,5)`,0.2s 截屏;内核侧同一段演出在宿主帧
4585 进入同一条幕。按覆盖率对齐后黑区 IoU:

```text
官方0116 cov 0.149 <-> 内核4596 cov 0.148 IoU 0.477(小区域,边界占比大)
官方0117 cov 0.364 <-> 内核4608 cov 0.367 IoU 0.850
官方0118 cov 0.469 <-> 内核4616 cov 0.468 IoU 0.873
官方0119 cov 0.643 <-> 内核4630 cov 0.606 IoU 0.839
```

覆盖区内官方与内核**都是纯 0** ⇒ 官方这条幕也是「不透明黑 + 柔和等值线边」,
不是半透明白;推进速度在 0.2s 采样误差内一致。
立绘退场(`ysr011/012`,TA=128)确认为竖百叶快门擦除,非瞬失。
台账:`docs/divergences/DIV-GFX-065-transition-coverage-model.md` §10/§11。

**验证命令与结果**:

```text
cargo test -p yu-gfx -p yu-render-core -p yu-backend-soft -p yu-backend-wgpu
  => passed 221 failed 0 ignored 1(trans8 40 格,等值线带实现的验收门)
scenario_probe(开幕帧 40,回退前后逐像素)      => changed=0 max=0
capture_run.py(被动,132 帧 / 31.7s)          => stop: process-exit
```

注:`cargo build --workspace --all-targets` 本轮被并行会话的
`crates/yu-engine/src/capture.rs`(未跟踪文件,格式串参数不匹配)挡住,与本次改动无关;
受影响 crate 全部单独通过。

### 14. 修好「演出 1 最后淡出」的硬边幕:覆盖率按 TA 分两档

**症状(用户)**:「标题的 logo 浮现是没问题的,是剧情中的过渡便锐利了」,
点名演出 1 的最后淡出;官方是宽的柔边,我们是硬边。

**根因**:覆盖率一律用窄带 `K=30+70*fade`(等效带宽 0.02..0.033 mask 单位)。
标题 `SP.TR` 的 `TA=128` 用这个正好是硬边(已验收),但 `\FOUT(1500,5)` 的
`TA=4` 在官方是宽渐变幕,同一个窄带把它画成硬边。

**改动**(`crates/yu-gfx/src/layer.rs` + soft/wgpu/shader 同式):

```text
c = 1-2*fade;  K = 30+70*fade;  wide = 0.16/TA
a = wide <= 1/K ? clamp(K*(m-c))      // 硬档:与旧实现逐位一致(标题/快门)
                : clamp((m-c)/wide)   // 软档:剧情宽渐变(TA=4 → 0.04 ≈ 40px)
```

**验证**:

```text
标题帧 22/28/40/52 与改动前逐像素   => changed=0 max=0(逐位不变)
演出1 FOUT 覆盖率(dark%)           => 4594:0.086 4600:0.223 4606:0.308
                                      4612:0.417 4618:0.474 4624:0.524 4650:0.970
                                      覆盖区内部仍是实黑,边界 ~40px 柔带
soft vs wgpu 同帧 dark%             => 4608:0.372/0.350 4630:0.605/0.586 4650:0.983/0.970
cargo test -p yu-gfx --lib          => 17 passed(新增两档回归测试)
cargo test -p yu-backend-soft/-wgpu => 37+15+1+9+21 全过(9 级扫描仍 8/255 内)
cargo build --workspace --all-targets => 通过
```

**取证设施**:`YU_TRANS_DEBUG`(转场实际参数)/`YU_TRANS_DUMP`(落 from/mask
纹理);`capture_run.py` 改为只取客户区(旧实现含标题栏且底部被裁,像素比对错位)。

台账:`docs/divergences/DIV-GFX-065-transition-coverage-model.md` §12。

**补充(统一公式)**:按反编译 `*0.25` 常数把两档合成一条
`a = clamp((m-c)/(1/K + 0.25/TA))`,无分支。标题帧 28/40 与已验收基线差
0.0055%(2.6万/480万像素,最大 Δ19);演出 1 淡出实黑占比
0.029→0.272→0.453→0.555→0.928→1.000,边界柔带 ~65px。
对照图:`target/tmp/fade_montage.png`(上排新版 4594/4606/4618/4630,
下排旧版 4606/4630)。
**仍存在的硬边**:过渡 quad 只画 `size` 区域 399×299(位于 400,300),
其矩形边界是几何硬边(与掩膜无关);官方由「格 × 掩膜」网格承担,需落地
`DIV-GFX-065 §12.3` 的 quad 网格才能消除。

**定稿(统一公式 `0.16/TA²`)**:`a = clamp((m-c)/(1/K + 0.16/TA²))`,无分支。

```text
标题帧 28/40 与已验收基线   => 1303 / 861 px 差,max Δ1/255(等同未变)
演出1 淡出(mean,dark,mid)  => 4594 (158.0, 0.086, 0.365)
                              4606 (138.8, 0.314, 0.150)
                              4618 (112.6, 0.479, 0.070)
                              4630 ( 84.1, 0.596, 0.061)
                              4650 (  1.3, 0.983, 0.016)
                              4660 (  0.0, 1.000, 0.000)
cargo test -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu => 215 passed / 0 failed / 1 ignored
```

`0.16/TA²` 的两锚点:官方 micro 网格(TA=128)要求 TA=128 的格带≈0(否则
`mask=128,T=128` 的 α=0.498 被推到 0.43);整场帧(TA=4)实测柔带≈40px
(掩膜梯度≈0.001/px ⇒ 0.04 mask 单位)。真正的 1:1 仍是 quad 网格(§12.3)。

## 待办(截至 2026-10-27,按优先级)

| # | 待办 | 现状 / 卡点 | 证据锚点 |
|---|---|---|---|
| 1 | **过渡按官方两形态重写**:带表层走「部件 × 分块」网格+逐顶点 α,不带表层走 4 顶点 quad | 静态已确认判据 `if (layer[+0xa7])`,层对象 0x640 字节;缺**真机层里的两张表**——需完整播到幕落(几次采集都在 5s 内被关窗,`hits=0`) | `DIV-GFX-065 §12.3/§12.5`;工具 `tools/official_trace/dump_layer.py` |
| 2 | 开幕 logo 浮现**不该有过渡**:官方开幕 `FUN_00414ba4` 一次未被调用,我们多挂了一条 `SP.TR` | 已定位现象,未查「谁在开幕挂了 TR」 | `DIV-GFX-065 §12.5`、本次采集日志(`hits=0`) |
| 3 | 过渡柔带目前是 `1/K + 0.16/TA²` 的**行为替身**,非官方机制 | 标题 max Δ1/255、演出1 淡出已成连续融合带;边界走向仍是掩膜折线 | `crates/yu-gfx/src/layer.rs` 注释 |
| 4 | 普通层 `MERGE`/`TRANSITION` 上 GPU(`Override::Pass` 离屏目标) | 每帧约 2.15ms + 1.2ms 的 CPU 合成 | `docs/architecture/backends.md` 账本 |
| 5 | 「有的字不显示空白」 | 缺具体时间点/台词,无法复现 | 用户报告 |
| 6 | 40 格 oracle(`transition_alpha_matches_official_grid`)解锁 | `#[ignore]`,等第 1 项 | `crates/yu-render/soft/tests/transition_alpha.rs` |

## 成果 SAVE-THUMB (2026-10-27) · 存档缩略图两端打通 + 读档"卡死"根因(参数 `-=` 被当 `+=`)

### 0. 症状与结论

- 症状 A(主人报告):点击读档、确认「はい」后**画面永远停在读档界面**,
  逻辑帧仍在推进(宿主诊断 `画面像素连续 N 帧相同(逻辑帧仍在推进)`)。
- 症状 B(主人报告):存档**没有缩略图**。
- 结论:
  - A 的根因不在存读档脚本链路 —— 点「はい」后 `ES.LOADCONF.BTN.YES.ON` →
    `es.CGTSS.2` → `es.S.RETURN` → `es.SS.RETURN` → `es.Load` →
    `es.S.RETURN.LBL` 全部执行,存档也被 LOAD。真正卡住画面的是
    `CG[ID=CGS999 A-=8]` 的**复合赋值算子被当成加法**:全屏截图幕布
    alpha 从 256 加到 512(越界不 clamp),永不淡出,盖住已恢复的游戏画面。
  - B 是 H-P4-02 的两个 `Unimplemented`(`SAVE[ID]`/`LOAD[ID]`),本轮按官方
    DNO3 块格式实现,存档写入 100×75 真实画面、读档界面正常显示。

### 1. 取证(真值层级 2/3)

```text
# 参数算子 = 窗口 slot 高字节:1 = += , 2 = -=
$ cargo run -p yu-rt --example dump_pc_cmds -- 19 0 4000 | grep 'L623'
pc368 L623 CG  ID(len6)=v($#2176) | A~2(len4)=i8        # es_SaveLoad.yst:623 A-=8
$ cargo run -p yu-rt --example dump_pc_cmds -- 66 0 20000 | grep 'X~1'
pc5659 L8173 CG ID(len6)=v($#4115) | X~1(len29)=...      # es_Sprite.yst:8173 X+=(…)

# DNO3 图像块(官方 0.495 save00001.sd,btype 1)
ulen=31608 = [u32 1600][u32 30000][A 1600B][B 30000B]
A+0x78 = 100, A+0x7c = 75; B = 100×75 RGBA
# 0.479 save30001.sd:A=1536、B=4,w/h 在 +0x68/+0x6c ⇒ 读取端按 lenB==w*h*4 自证
```

### 2. 修法

1. **`yu-rt`**:新增 `compose_window_arg(comp, value)`(`comp==2` 先取负,
   `offset = comp != 0`),`eval_arg_windows` 统一走它。
   单测 `window_ops_tests::plus_and_minus_operators_are_distinct`。
2. **缩略图两端**:
   - `yu-save::image_block`(DNO3 编解码)+ `SaveHost::reserve_image/load_image`;
   - `GraphicsHost::capture_layer/restore_layer`;
   - `yu-engine::capture::GfxCapture`:离屏软渲 + `observe`(出现新 COPY 的帧
     与主渲染器同刻冻结快照;`SAVE[ID]` 回放当前帧取目标层快照并按图层声明
     尺寸重采样;`LOAD[ID]` 写 PNG 进 `SharedMem` 合成资产叠层 + `set_layer_asset`)。
3. **沿途三条渲染语义修正**:
   - COPY 目标表面尺寸 = 源/区域尺寸,不看目标层显示尺寸(soft + wgpu)。
     否则窗口截图被裁成左上角 100×75(全黑)。
   - soft `MERGE/MERGE2` 先于区域解析(`X/Y` 是绘制偏移;原来
     `MERGE2 X=0 Y=0` 被判 `region.partial` 跳过,缩略图合不进存档盘)。
   - VM `resolve_asset_size` 候选序与渲染器一致(已带扩展名先按原样)。

### 3. 验证(原始结果)

```text
# 幕布 alpha 随帧衰减(soft,点击 はい 在 700)
YU_SAMPLE_STEP=20 probe_layers <project> 700 1150 CGS999 --click …
engine 702: A=256 / 722: A=112 / 742..1150: A=0

# 读档后画面回到游戏
probe_flow <project> 1150 "300,217,281;440,100,590;540,180,130;700,350,303"
end-state: frame=1152 layers=607 visible=324 loadconf=0 ops=121
截图 target/tmp/e2e/fixed-soft.png  => 对白「近づけてみました。」(游戏画面)

# 桌面宿主(wgpu + winit)同一输入
yu-cli run <project> --mode desktop --backend wgpu --frames 1150 \
  --script load-confirm.json --shot fixed-desktop.png
stuck="推进正常";截图 => 游戏画面(对白「遠ざけてみました。」)

# 存档缩略图
save00001.sd DNO3: btype=1 ulen=31608 w=100 h=75 非零字节 24956
读档界面截图:三块缩略图为真实游戏画面(headless soft / headless wgpu /
桌面 wgpu 一致)

# 测试
cargo test -p yu-save                     => 14 passed(含官方 save00001 DNO3→100×75)
cargo test -p yu-rt --test fixture        => 98 passed
cargo test -p yu-rt --lib window_ops      => 1 passed
cargo test -p yu-backend-soft / -p yu-gfx => 37 / 17 等全过
cargo build --workspace --all-targets     => 通过
```

### 4. 未闭环 / 说明

- `yu-trace::golden::synthetic_trace_matches_golden` 仍红:合成
  `YSCom.ycd` 的 sha256 与 golden 不一致(HEAD 上即如此,与本轮改动无关;
  复核时未改 golden)。
- `cases/save-load-loop` 仅 `save_state_sha256` 变化(存档里多了 DNO3 缩略图,
  预期变化);`ops_total=122 / skipped=0` 不变。其余用例的 png/display 哈希
  受并行会话渲染改动影响,基线待统一重生成(本轮未动 golden)。
- `SAVEINFO` 仍 `Unimplemented`:手册无页、REA 处理体未取证、语料 0 次,
  按"无证据不猜"保留过渡态。

台账:`crates/yu-rt/docs/divergences/README.md` §DIV-RT-058(参数算子)、
§DIV-RT-059(SAVE[ID]/LOAD[ID] 图像块)。

## 成果 PERF-SPIKE (2026-10-27) · 双游戏实测尖峰归因:内容指纹/整图 clone/抓帧器整帧回放

**症状(用户)**:两款游戏都要抓渲染尖峰做优化;kotoneiro 有整段 30fps/10fps,
eris 场景切换有 200ms 级停顿。

**取样(人工推进,非脚本)**:`yu-cli run … --mode desktop --frame-stats CSV
--spike-log PATH --spike-ms 20`。`--spike-log` 是本轮新增的长帧自动取证:
阶段耗时 + 本帧逐命令增量(`take_command_hit_deltas`,零字符串分配) +
脚本落点 + pass/op 构成 + 目标层 op 排行。渲染子阶段改由 `RenderPhases`
走 CSV(与参与对账的 `RenderReport` 分离),**不再依赖 `YU_LOG=trace`**
(那会把 wgpu 自身每帧数百行 trace 灌进日志,20s 37MB 并拖慢帧)。

**根因(实测归因,按收益)**:

1. `content_hash_rgba` 是逐字节 FNV-1a(串行乘法 ≈3 cycles/byte),且在
   **每条图像加工 op 后**对整张结果图算一次 ⇒ `MERGE` 单条 2.7ms 里一半以上
   是指纹;
2. 每次 COPY 的镜像捕获与每次资产解析都**整图 clone**(重帧 64 COPY +
   27 加工 ⇒ 数百 MB/帧)⇒ `snap_ms=33ms` 主体;
3. `blit` 逐像素 `Option` 调用;
4. `SAVE` 抓帧器在出现新 CopyId 时**整帧软后端回放**;kotoneiro 精灵/按钮
   场景每帧换 CopyId ⇒ 每帧 100–375ms(`capture_ms` 单独计时后才可见);
5. VM 解释器 0.21µs/命令(未改,另线)。

**改动**:哈希改 8 字节字混合+splitmix64;`TextureStore`/快照镜像改存
`Rc<Texture>` + `resolve_shared`;新增 `blit_rgba` 行级 memcpy;
`apply_merge` 行切片 + 跳过 `au=0` 像素(恒等变换);抓帧器改为渲染后取
主渲染器镜像,缺失才回放(另试过给 `:/W0/` 主组复制补后缓冲镜像,会改变
`SAVE[ID]` 的命令时刻语义且无 headless 覆盖 ⇒ **已回退**,见报告 §5.4)。

**验证命令与结果**:

```text
kotoneiro 基线(1357 帧/36.7s,三项渲染修复前):
  interval avg=26.800 p50=16.695 p95=100.053 p99=216.402 max=733.711
  render avg=8.159 p95=83.044   snap avg=0.259 max=37.465
  image_composite avg=5.501 p95=77.909 max=144.276
kotoneiro 修复后(5930 帧/108.5s):
  interval avg=18.236 p50=16.660 p95=17.093 p99=66.639
  render avg=2.294 p95=2.268    snap avg=0.084 max=20.473
  image_composite avg=0.361 p95=0.000 p99=21.554 max=73.181
  capture>20ms 仅 6 帧(修复前每帧 100–375ms)
project-eris 基线(4804 帧/84.9s):
  interval p50=16.667 p95=17.126; tick max=128.707; 宿主未计入 max=217.302
project-eris 修复后(2958 帧/56.1s):
  render avg 2.055→1.759ms; capture>20ms 5 帧(peak 199.2ms)
  p95=33.397 属流程差异(本轮多开菜单);稳态 p50=16.672
中间态证据(run4):459 个重帧全部 memo=chains:0
  ⇒ ES 精灵合成链从未进入记忆化判定(快照基图/链含非 paint 动作)

cargo test -p yu-render-core -p yu-backend-soft -p yu-backend-wgpu -p yu-engine
  => 37/15/9/21/9/6 全 passed(含 soft↔wgpu 逐计数对拍)
yu-cli case <project-eris> --all
  => 1 passed / 8 failed:全部是 display_json/ops 差异,来自并行会话在改的
     VM `A-=` 语义;golden 待那边落定后统一重生成(本轮未动 golden)
```

**未闭环 / 下一步**:合成链记忆化对快照基图无效(需镜像代际进签名或前缀
缓存);CPU MERGE 已到内存带宽级,再降只能 alpha bbox 裁剪或 GPU 合成
(`MERGE2` 的 `/ao` 除法 GPU 固定混合无法逐位复刻,属研究项);抓帧器残余
回放(非主组源/镜像被 GPU 加工失效)。

报告:`docs/perf-spike-report-2026-10-27.md`。证据产物在
`target/tmp/perf/{kotoneiro,eris}/`(不入库)。

## 成果 PERF-FADE (2026-10-27) · 角色出场卡顿拆分、MERGE 整数化与日志异步化

**症状(用户)**:「角色一出来就卡顿」「开日志会发现更卡」;要求用 `--debug-ui`
看尖峰图。

**取证**(`--debug-ui` + `--frame-stats` + `--spike-log 20`,kotoneiro,
7379 帧/130.3s)。角色出场那一帧拆成三块:

```text
host=2914 work=320.8ms = tick 263.4 + render 55.5 + draw 0.8
  render: decode 31.0ms(35.7MB) / image 38.9ms(其中 MERGE 16.3ms)
  uploads 39.7MB
```

1. 脚本突发 263ms = `LET 112960 + LOOPEND 25385 + IF 19753`(0.21µs/条);
2. 资产解码 31ms / 35.7MB(`TextureStore::load`,新增 `decode_us` 记账,
   它是 `image`/`rest` 的子集);
3. CPU 整面合成/混合 16–82ms/帧(淡入期间每帧)。

**改动**:

- **MERGE 整数化**(`yu-render/soft/src/imagefilters.rs`):
  `round(cl*(1-a/255)+cu*(a/255)) = (cl*(255-a)+cu*a+127)/255`。
  证明:`num/255` 距半整数 ≥1/510,f64 误差 1e-13 量级,不会跨舍入边界。
  实测同形态帧 `merge:2`:55.8 → **7.8ms**。`MERGE2` 的 `/ao` 可能精确落在
  .5,**保留 f64 原式**。
- **跨度跳过**:`blend_transition` 按行求非零跨度(输出是新零面,双零源
  结果必零);`apply_merge` 整行 alpha 全 0 直接跳过。
- **日志异步**:JSONL 改后台线程 + 256KB `BufWriter` + 65536 行有界队列
  (满则丢行计数,退出前落一行);stderr 改 64KB 块缓冲、仅 WARN+ 立即刷;
  退出/panic 同步点:桌面 `App::finish` → `ConsoleHandle::flush()`、
  `main` 兜底 `log::logger().flush()`。修了首版异步写者「队列空当关闭」
  的 bug(run6 日志只剩 1 行),`yu-log` 单测改为先 `flush()` 再读。
- 诊断字段:`RenderPhases` 增 `decode_us/decode_bytes`、
  `image_transition_ops/image_bitwise_ops`;CSV/spike 行同步。

**验证**:

```text
cargo test -p yu-log -p yu-backend-soft -p yu-backend-wgpu -p yu-render-core -p yu-engine
  => 29/39/15/9/21/9/6 全 passed
  新增逐位对拍:merge_span_optimization_is_bit_exact(64 组随机+整行透明)
                blend_transition_span_optimization_is_bit_exact(64 组随机+窄源)
kotoneiro 复测(7379 帧/130.3s,--debug-ui 常开):
  interval avg=17.616 p50=16.633 p95=17.092 p99=33.664
  render avg=1.627 p95=1.628 p99=19.662
  image_composite avg=0.385 p95=0.000 p99=16.938 max=81.476(>5ms 帧 2.6%)
  decode avg=0.038 max=34.649(单帧峰值 71.4MB)
  日志 12391 行,退出摘要完整
对照(MERGE=f64,1649 帧):interval p95=66.377,image_composite avg=5.605 max=57.990
```

**未闭环**:角色出场的 263ms 脚本突发(VM 吞吐线,与并行会话 `A-=` 同域);
全屏 CG 过渡仍是 CPU 全图混合(≈24ms/条,跨度跳过无效,需 GPU 双纹理
shader);首次解码 31ms 可做预取/后台解码(需脚本层「即将使用」信号)。

报告 `docs/perf-spike-report-2026-10-27.md` §7。

## 成果 PERF-GPU-TRANS (2026-10-27) · `CGACT[TRANSITION]` 上 GPU(整数 lerp 逐位对拍)+ CPU/GPU 分工审查

**症状(用户)**:角色场景 `render spike 43.9ms`,面板上周期性黄/红尖峰;
「合成用到了 CPU…重点是 cpu 可以 cpu,但 gpu 就是 gpu」。

**审查结论**(802 层/619 可见/99 pass,角色场景每帧):

```text
图层 draw / SIZE 采样        已是 GPU
MERGE 到快照目标             已是 GPU(paint pass)
CGACT[TRANSITION] 双源 lerp  CPU,40–60ms/帧  ← 本轮迁走
MERGE/MERGE2 到普通层        CPU,8–60ms/帧   ← 下一步
合成结果上传                  36–75MB/帧      ← 随合成迁走消失
资产解码 / 脚本               CPU(本职),31ms/一次性 + 263ms/一次性
```

**改动**:

- `shaders/lerp.wgsl` + `pipelines.rs`:双纹理整数 lerp(`(from*(256-f)+to*f)>>8`),
  `textureLoad` 同坐标取 texel;`LayerId` 级离屏目标(`TargetSet.composites`)。
- `try_gpu_transition`:两源均为稳定 GPU 纹理、目标层无 CPU 消费者、非快照
  目标 ⇒ GPU 合成 pass(`DrawKind::Lerp`);否则显式回退 CPU
  (`YU_TRANSITION_DEBUG=1` 打印原因)。
- `Override::Gpu` + `plan_layer` 直接画合成纹理、COPY 可直接取它(不再上传)。

**验证**:

```text
cargo test -p yu-backend-wgpu --test wgpu cgact_transition
  => ok(GPU 路径断言 layer_composites≥1;f=0/128/256 与 soft 逐字节一致)
cargo test -p yu-render-core -p yu-backend-soft -p yu-backend-wgpu -p yu-engine
  => 39/15/9/22/9 全 passed
kotoneiro 复测(3034 帧/58s,--debug-ui):
  interval p50=16.651 p95=17.221;render p95=2.095
  image_composite >5ms 的帧 72/3034,全部是 merge:2(21–44ms)
  剩余回退:4 条(2× from CPU-only、2× to CPU-only)
  诊断无新增合成/lerp 缺失码(gfx.layer.already_absent 10987 条被异步日志吸收)
```

**未闭环**:`MERGE/MERGE2` 到普通层仍 CPU(需 ping-pong 离屏 + 整数 shader;
`MERGE2` 的 `/ao` 精确 .5 情况下 f64 round 方向不定,需单独定标);
SAVE 抓帧器仍有 430–455ms 单帧回放;解码/脚本属 CPU 本职(预取 + 解释器吞吐)。

报告 `docs/perf-spike-report-2026-10-27.md` §8。

## 成果 SND-CP932 (2026-10-27) · 日文文件名双字节被逐字节折小写导致声音资产查不到,同类字节扫描收口三处

**症状(用户)**:`kotoneiro` 片头出现
`WARN [sound] snd.file.missing @ snd@frame240 pc0: SOUND 文件未找到: voice/\x83R\x83g\x83l\x83C\x83\x8d`。

**取证(先证伪"封包/查询不通用")**:

```text
YpfSource::open(kotoneiro/data.ypf, Cp932)
  version=494 entries=12212 closed=true hash=12212/12212 key=0xff
  voice\コトネイロ.ogg  type=6(ogg) stored 54226B offset=0x958a0
LookupKey::new(b"voice/コトネイロ.ogg", Cp932) → exists()==true
```

**复现(修复前)**:`yu-sound` + 真实包,loader 依次收到
`voice/\x83r\x83g\x83l\x83c\x83\x8d`、`…ogg`、`…wav`,全部未命中 →
`snd.file.missing`(诊断文本与用户日志逐字节一致)。

**根因**:`yu-sound::normalize_name` 逐字节折 ASCII 小写,没有按 CP932 字符
推进。双字节字符的尾字节可以落在 `0x41..=0x5A`:`コ`=`83 52`、`イ`=`83 43`,
被折成 `83 72`/`83 63`(`コトネイロ` → `ビトネツロ`),包里存在的资产永远查
不到。官方处理体不逐字节改写(该文件在包内且官方可装载即为证)。

**同类审查(逐字节扫 CP932 串,不含 examples/tests)**:

| 位置 | 结论 |
|---|---|
| `yu-sound/state.rs normalize_name` | **真缺陷**(本轮复现)→ 按字符跳步 |
| `yu-sound/state.rs probe_names` 整串 `to_ascii_lowercase` | 同型写法 → 改为尾部 ASCII 后缀比较 |
| `yu-render/core/texture.rs has_extension` | **真缺陷**:`rsplit(0x5C)` 把 `ソ`(`83 5C`)当分隔符,`cg/ソ.png` 被判"无扩展名",原 key 不参与候选 → `asset.missing` |
| `yu-render/core/texture.rs known_extension_format` | 整串折小写(只用于尾部比较,未致命)→ 改为尾部比较 |
| `yu-rt/vm/handlers.rs has_image_extension` | **真缺陷**(与渲染器同判、同一 `rsplit`)→ 按 `cp932_text::char_len_at` 跳步 |
| `yu-vfs/key.rs normalize_component` | 安全(多字节整对跳过;单测已覆盖 0x5C 尾字节) |
| `yu-gfx/path.rs` | 安全(只以 `/` 为分隔符,0x5C 按字符保留) |
| `yu-rt ascii_case/fullwidth_case` | 安全(2026-10-24 已按字符跳步,同类事故前案) |
| `yu-sound/decode.rs` | 安全(按魔数判容器,不看扩展名) |
| `yu-font/source.rs normalize_name` | 不同域(`&str` 字体族名归一,非 CP932 字节) |
| CLI/日志/HTTP/GPU 名的大小写折叠 | 不同域(纯 ASCII 配置/协议串) |
| `yu-vfs/dir.rs` | 安全(非 ASCII 组件经码表 encode 回字节键,已有 GBK 尾字节单测) |

**改动**:

- `yu-sound/state.rs`:`normalize_name` 按 `yu_rt::cp932_text::char_len_at`
  跳步(双字节原样拷贝,只有单字节做 `\`→`/` 与 A–Z 折小写);新增
  `ends_with_ascii_ci`,`probe_names` 不再整串折小写。
- `yu-render/core/texture.rs`:新增 `last_component(key, codec)`(按 `Codec`
  lead/trail 跳步)与 `ends_with_ascii_ci`;`has_extension`/`known_extension_format`
  改用之。
- `yu-rt/vm/handlers.rs`:`has_image_extension` 改按字符跳步取最后组件。
- 单测:`yu-sound/tests/state.rs` 新增 CP932 归一/端到端解析 2 例并扩充探测序
  断言;`texture.rs` 新增尾字节分隔符 + 精确 key 解析 2 例;`handlers.rs` 新增
  `image_extension_tests` 2 例。

**验证**:

```text
cargo test -p yu-rt -p yu-sound -p yu-render-core --lib
  => yu-render-core 8/8, yu-rt 85/85, yu-sound(lib) 1/1 全 passed
cargo test -p yu-sound
  => 集成 39/39 passed(含新增 cp932_file_name_resolves_through_probe_order)
端到端(真实 kotoneiro/data.ypf,修复后同一探针):
  loader.query raw="voice/コトネイロ" → 未命中(无扩展名)
  loader.query raw="voice/コトネイロ.ogg" → 命中
  state=Playing file_type=Ogg total=1842  diags=0
```

**未闭环**:`docs/sound-ledger.md` §1.1 已补勘误(官方小写按字符而非逐字节);
当前工作区 `yu-render/wgpu` 在改(签名不匹配),整仓 `--workspace` 构建待其
收口;本轮只跑受影响 crate 的 lib/集成测试。

**收尾复审(2026-10-27)**:

- **诊断可定位性**:`snd.file.missing` 现在附带“已尝试的候选名”(归一化后
  真正的查找键)。修复前日志只打印原始参数 —— `名字被改写` 与 `文件不存在`
  在日志里完全同形,这是本缺陷难排查的直接原因。样例:
  `SOUND 文件未找到: VOICE/\x83R…（已尝试: voice/\x83R…_nope, …_nope.ogg, …_nope.wav）`。
- **回归护栏**:三处修复各加全 lead 扫描用例(lead `0x81..=0x9F` /
  `0xE0..=0xFC` × 尾字节 `0x41..=0x5A`,以及尾字节 `0x5C`);任何回到
  逐字节处理的做法都会在这些用例上成片失败。
- **复审范围扩大**:`yu-platform/*`、`yu-emote`、`yu-gfx/path.rs`、
  `yu-vfs/dir.rs` 均无新增同类点(`yu-emote` 的大小写只作用于贴图格式串
  `&str`,不碰 CP932 资源名)。
- **复跑**(语料环境变量按 AGENTS §2):
  `cargo test -p yu-sound -p yu-rt -p yu-render-core -p yu-vfs -p yu-backend-soft`
  => 18 个测试套件全 passed(39/15/2+1 ignored/9/86/4/11/4/98/3/1/41/42/14/2/5/1)。

## 成果 PERF-GPU-MERGE (2026-10-27) · `CGACT[MERGE]` 上 GPU(普通层 ping-pong + 快照目标原地写回)

**取证**:角色场景 939 帧含 MERGE,重帧 `render 19.9ms` = 2 条 MERGE
(`comp≈17ms`);加日志后在 1884 次回退里**全部是 `target is snapshot`**
—— 立绘缓冲层持快照纹理,而 GPU 合成此前只对普通层开(且只做了过渡)。

**改动**:

- `shaders/merge.wgsl`:整数式 `out.rgb=(lower*(255-a)+upper*a+127)/255`、
  `out.a=lower.a`,与 soft `apply_merge(…,false)` 逐位一致;`MERGE2` 的
  `/ao` 在精确 .5 处 f64 round 方向不定,**保留 CPU**。
- 普通层:两槽 ping-pong 离屏(`composites[(layer, slot)]`),读上槽写下槽。
- **快照目标**(官方语义 = 原地写快照表面):先按 `CopyId` 在**执行期**把
  快照 blit 到临时槽(COPY 命令先执行),再用整数 shader 写回快照纹理;
  CPU 镜像随之失效。新增 `PlannedTarget::Snapshot` / `BlitCmd`。
- 消费者预扫描把后续 MERGE 也算 GPU 友好(链式继续走 GPU)。

**验证**:

```text
cargo test -p yu-backend-wgpu --test wgpu cgact_merge_gpu_matches_soft_exactly
  => ok(普通层链式 2 条 MERGE,逐字节 + layer_composites≥1)
cargo test -p yu-backend-wgpu --test wgpu merge_on_snapshot_gpu_is_bit_exact
  => ok(快照目标、半透明上下层,逐字节)
cargo test -p yu-render-core -p yu-backend-soft -p yu-backend-wgpu -p yu-engine
  => 39/15/9/24/9 全 passed
kotoneiro 复测(2801 帧):939 帧含 MERGE,仅 72 帧仍有 >5ms CPU 合成
  (回退原因 upper is CPU-only);render p95 13.6ms
```

**新发现(下一刀)**:差分切换的剩余尖峰是 **SAVE 抓帧器整帧软后端回放**:
`work≈415–459ms = tick 44–69 + render 8–97 + capture 307–362`;
`capture>100ms` 9 次/2801 帧。GPU MERGE 让快照镜像失效,增加了「镜像缺失
→ 回放」的概率(run13 2 次 → run16 9 次)。两条候选:软后端只回放 Texture
pass(跳过 619 层屏幕绘制)或 SAVE 时刻按需回读 GPU 快照(1–5ms)。

报告 `docs/perf-spike-report-2026-10-27.md` §9。

## 成果 PERF-OOM-FIX (2026-10-27) · GPU 合成目标剪枝,修 wgpu Out of Memory

**现象(用户)**:角色出场后 `PANIC phase=tick frame=1679 msg=wgpu error:
Out of Memory`(wgpu 默认错误即致命)。

**根因**:`TargetSet.composites`(本轮 GPU 过渡/MERGE 的离屏目标,键 =
`(LayerId, 槽位)`)每张整尺寸纹理**只增不删**;差分/切场景不断产生新
LayerId ⇒ 显存耗尽。组离屏 `offscreen` 同样是存量泄漏。

**修复**:`prune_composites`(图层本帧存活 + 32 张硬上限)、
`prune_offscreen`(组 pass/图层引用存活集合),每帧规划开头执行;合成目标
只服务当帧,剪掉后当帧重建。退出摘要新增
`gpu targets composites/offscreen/snapshots/textures` 便于核对。

**另**:`SOUND` 同步解码是当前最大单帧成本(逐命令 profile:6 次 BGM =
235ms,占该帧 tick 95.8%;205 次 SE = 158ms),已加按文件名的 PCM 解码
缓存(96MB 上限,`Arc<Pcm>`);首次装载仍需付解码。

```text
cargo test -p yu-render-core/-soft/-wgpu/-engine/-sound
  => 39/15/9/24/9/41 全 passed;cargo build --workspace --all-targets 通过
```

**补充(run26 实测)**:15335 帧 / 263.2s / **avg 58.3fps**,显存目标峰值
127MB 无 OOM;唯一中断是 `BLIT 源快照缺失`(该 COPY 本帧被跳过)——已改为
blit/合成 pass 目标缺失时**跳过该合成**(不 fatal)。渲染侧稳态已成立,
剩余尖峰为 SOUND 解码与脚本突发(见 §10 profile)。

**根因(2026-10-27 收口)**:`wgpu: Out of Memory` 的真因是 **CPU 覆盖纹理
每次内容变化都新建 GPU 纹理**,而 `TextureCache` 只增不删 ⇒ 立绘/差分场景
每帧泄漏数 MB,46–63 秒耗尽显存(与三次崩溃点吻合)。新增
`TextureCache::rewrite`(同尺寸 `queue.write_texture` 原地写回,id 不变),
`plan_layer` / `image_source_texture` 两个上传点改为「复用/原地重写/新建」
三态;上传失败按 op 跳过不 fatal。

**干净长跑(run28,2026-10-27)**:12267 帧 / 217.6s / avg 56.4fps,正常关窗
exit 0;**OOM/校验/fatal/panic 计数为 0**;`gpu targets` 全程 4–7MB;
interval p95 17.12ms、render p95 2.08ms;`work>60ms` 仅 49/12267 帧(0.4%),
全部是 tick 主导(SOUND 解码 + 场景脚本)。

## 成果 CLOCK-WALL (2026-10-28) · 实时逻辑时钟去截断:动画时长回到墙钟,serve 不再丢 2 帧

> 触发:主人实测片头淡出变短,追问「动画时间是固定的还是会被截断的」。

### 1. 事实链(先取证,后改码)

- **动画不是固定帧数**:淡出/补间由脚本按毫秒推进。`es.CGFADEEXEC`(s8 pc453-521)
  pc460 `@8 = @_OS_TIME(1)` → pc471 `@< = @_OS_TIME(1) - @8` →
  pc472 `@: = 256 * @< / @2` → pc502 `CG[ID=$1 A=@=]`,pc510 `WAIT[FRAME=1]`;
  内核 `_OS_TIME(1) = now_ms - start_ms`(`crates/yu-rt/src/sysvar.rs:185`)。
- **离屏固定步长实测**(kotoneiro,`probe_layers`):片头 `MAC.EV("サークル", PINT3=2500)`
  的 logo 淡入 157 帧 × 16ms ≈ 2512ms(与脚本 2500ms 差 0.5%);标题幕布
  `CGS100` 由同一 `es.CGFADEEXEC` 驱动,A 256→0 用 25 帧 = 400ms。**脚本时长
  没有被引擎改短**。
- **桌面实跑(debug 构建,140 帧)暴露真正的尖峰**:f78 `interval=1249.7ms /
  tick=1216.4ms`(场景构建,纯 VM 泵);f85 `render=150.9ms`(其中 `snap=138.5ms`、
  `decode=138.2ms`、上传 3.6MB,首张全屏 logo 解码)。淡出期间帧间隔 33–50ms。
  ⇒ 「淡出变短」= **长帧/可见帧少**,不是时长被改;旧 500ms / 2 帧上限还会把
  这段时间永久丢弃(2 帧实测丢 6.58s,见 H-P1-05)。

### 2. 改动

- `yu-engine`:新增 `RealtimeClock`(墙钟毫秒、单调、**默认不截断**);
  `tick_realtime` 不再把 delta 夹到 `max_step_ms`,并新增 `tick_realtime_at(now)`
  注入缝。`max_step_ms` 降级为**显式调试开关**(默认 `None`);启用时丢弃量逐帧
  计数,经 `FrameReport::lag_ms` 暴露,首 3 次与每 600 帧打 WARN,不静默。
- `FrameReport` 新增 `clock_ms`(脚本可见时钟)与 `lag_ms`(被显式上限丢弃的毫秒)。
- `restart`:时钟在 boot **之后**重新归零,避免重启首帧把 boot 时间算进脚本时钟。
- `host-desktop`:`resumed` 末尾(窗口/交换链就绪后)调 `reset_realtime_clock()`,
  首帧 delta 从 0 起算;新增 `--max-step-ms N` 调试旗标(默认关)。
  `--frame-stats` CSV 末尾追加 `clock_ms,lag_ms`(脚本时钟/本帧丢弃量),
  `--spike-log` 行内加 `clock=/lag=`;旧列序不变。
- `serve`:删除 `2 × frame_ms` 上限(`server.rs`),新增 `--max-step-ms` 调试旗标;
  帮助文本同步。
- 旧「500ms 是官方口径」注记全部改为「默认不截断;截断 = 显式调试」。

### 3. 验证(原始结果)

```text
$ cargo build --workspace --all-targets
Finished `dev` profile ... 59.87s
(仅 yu-gfx tests/parity_fixes.rs 的 unused_mut 既有警告)

$ cargo test -p yu-engine
running 14 tests ... test result: ok. 14 passed
  含 realtime_clock_tests:
  long_gap_advances_full_wall_time(700ms 间隔推进 700ms,0 丢弃)
  explicit_cap_is_counted_not_silent(500ms 上限:推进 500 / 丢弃 200 计数)
  clearing_cap_restores_lossless_clock(关上限后恢复零丢弃)
  pause_reset_discards_without_rewinding(暂停 5s 后时钟不回退,只丢暂停段)
  sixty_hz_microseconds_average_to_one_second(60×16.667ms 逐帧增量之和=1000ms)

$ cargo test --workspace
除 `yu-cli --test logging_guards` 外全绿。该失败来自工作区里他人未提交的
`crates/yu-cli/src/case.rs` / `crates/yu-sound/src/state.rs`(HEAD 版本无
println!/eprintln!),与本轮改动无关,未触碰。

$ ./target/debug/yu-cli.exe run <kotoneiro> --mode desktop --frames 140 --no-sound \
    --frame-stats target/tmp/ktn/frames.csv --spike-log target/tmp/ktn/spikes.log --spike-ms 5
exit=0;CSV 关键行:
  f78 interval=1249.710 work=1234.384 tick=1216.448 render=12.418
  f85 interval=67.558  work=200.639  tick=47.955   render=150.940
      (plan=139.594 snap=138.499 decode=138.240 decode_bytes=3630000)
  f86-f125 淡出窗口:interval 33–50ms(work 32–40ms)

$ 同命令复跑(100 帧,验证新列)
  f1  clock_ms=34  lag=0   (首帧从 resumed 重置后起算)
  f78 clock_ms=3149 lag=0  (tick=1241ms 全部计入脚本时钟,旧 500ms 上限会丢 741ms)
  f85 clock_ms=4649 lag=0
```

### 4. 剩余(第 2 层,另立任务)

- 「淡出可见帧少」的根因是**开局单帧内的 VM 场景构建**(debug 1216ms / release
  90–450ms,见 perf 报告)与**首张全屏纹理解码/上传**(debug 138ms)。方向:
  预热首 pass 管线/glyph atlas、VM 命令泵吞吐;预热不碰脚本时序,才是正确修法。
- `--max-step-ms` 一旦打开就会让动画慢于真实时间,只用于复现旧症状,不作产品路径。

## 成果 SPRITE-FLASH-GPU-MERGE:修复 wgpu 立绘换差分/眨眼时的整层消失(2026-10-28)

### 症状与复现

桌面 wgpu 下角色立绘在眨眼/换差分的过渡帧整层消失(持续约 8 帧),
下一帧又回来。`--backend soft` 未见该现象。

复现用例:`target/tmp/case-dir1real.json`(标题 → β4機能紹介 → 演出機能 → 演出１,
3000 帧),逐帧画面签名经 `YURIS_CASE_TRACE_FRAMES` 采集。消失窗口
(engine frame,与 host frame 差 ~39,用 display JSON 的 `frame` 对齐):

```text
              改前(wgpu)        改后(wgpu)       HEAD(dfa7b0b) wgpu
f=2651..2658  165.77 nz=479542   155.30 nz=479174   155.30 nz=479174
f=2894..2900  165.26 nz=479240   158.8x nz=479xxx   158.83 nz=479200
f=2982..2988  162.78 nz=479690   139.53 nz=479540   139.53 nz=479540
```

(数值 = 全帧 RGB 均值 / 非零像素数;立绘消失时均值升高约 10,因为背景露出。)

### 根因(有代码依据,非推测)

`es_SPExec.yst` 换差分的标准命令序是:

```text
CGACT[COPY2=1 ID=<src> ID2=SPLBUF602]        ← 冻结 src 像素到快照 350
CGACT[ID=SPLBUF602 RECTPAINT3=1 SET=0 X=0 Y=0 X2=294 Y2=579]  ← 表面清成 (0,0,0,0)
CGACT[ID="sp.tmpB" ID2=SPLBUF602 MERGE=1]     ← 把上层叠上去
```

本轮新增的 GPU 合成路径(`try_gpu_merge` 的快照分支,`passes.rs`)把
**执行期的当前快照纹理**当 MERGE 的「下层」。但该纹理在同一个命令序里刚被
`RECTPAINT3 SET=0` 清成 alpha=0,而 `merge.wgsl` 按设计只写 RGB
(`out.a = lower.a`)⇒ 写回去的快照 alpha 恒 0。

随后 `SPLBUF602` 上的 `SIZE`→`SPL602` 以该快照为源,整层在 `SRCALPHA` 混合里
被抹掉 ⇒ 立绘消失。逐帧取证确认过这一点:诊断读回快照 350 的 GPU 纹理得到
`alpha_nz=0, mean=104.40`(RGB 有内容、alpha 全 0),同一帧 CPU 镜像
`alpha_nz=122259`。

CPU 参考路径的「下层」是 `snapshot_pixels()`(COPY 时刻冻结的像素),所以同一
命令序在 CPU 上画得出立绘——这正是回归前(HEAD)的行为。

### 修法

`crates/yu-render/wgpu/src/passes.rs` 新增 `snapshot_modified_before(frame, copy_id, at)`:
本帧在该 op 之前是否已有图像加工改写过这个快照的表面。命中则
`try_gpu_merge` 回退 CPU(`bail("snapshot surface modified earlier in frame")`),
基面仍是 COPY 时刻像素,与回归前逐位一致。未被同帧改写的快照仍走 GPU 合成,
所以本轮的性能收益不受影响。

不猜官方语义:`MERGE` 在「表面已清空」时是否应写 alpha 属
`docs/divergences/DIV-GFX-063` §2.4 的未取证点(手册只说 MERGE 反映上层不透明度),
本修复只在该分歧实际发生处回到已验证路径,并在 `DIV-GFX-063` 记一条待取证。

### 回归门禁

`crates/yu-render/wgpu/tests/wgpu.rs::merge_after_rectpaint_clear_on_snapshot_keeps_pixels`
(COPY → RECTPAINT3 SET=0 → MERGE,上下层隐藏后只留快照可见)。
已验证该测试在移除守卫时失败(`[0,0,0,255]`)、带守卫时通过(`[200,0,0,255]`)。

### 验证命令与原始结果

```text
$ cargo test -p yu-backend-wgpu --test wgpu merge_after_rectpaint_clear_on_snapshot_keeps_pixels
test result: ok. 1 passed

$ YURIS_CASE_TRACE_FRAMES=1..3000 ./target/release/yu-cli.exe case <project-eris> \
    --case dir1-wgpu --cases target/tmp/dir1-cases
改前→改后仅 22 帧变化,全部落在上面三个窗口,且都与 HEAD 的 wgpu 值一致。

$ cargo test --workspace --no-fail-fast
唯一失败:yu-trace --test golden 的 synthetic_trace_matches_golden
(YSCom.ycd sha256 不一致,已知既有红灯,与本轮无关)。

$ cargo build --workspace --all-targets
Finished(仅 yu-rt 既有 `coerce_param` 未使用警告)
```

### 取证工具(本轮临时加、已全部移除)

`YU_SPRITE_DBG`/`YU_GPU_IMG_TRACE`/`YU_GPU_IMG_OFF` 三组临时环境开关与
逐 op 打印已从工作区删除;保留的 `YU_TRANSITION_DEBUG`(既有)只打 CPU 回退原因,
可用 `[merge] CPU fallback: snapshot surface modified earlier in frame` 观察本修复命中。

## 成果 PERF-VM-HOTPATH (2026-10-28) · 解释器热路径:每命令摘/插执行位、参数名整表克隆、GOSUB 参数分配

### 定位方法(先量再改)

`--profile-commands` 的逐命令计时改为 **1/32 抽样并折算**:逐条 `Instant::now()`
两个加法本身约 0.2µs,旧实现把采样器开销算进了命令成本。另加 `YURIS_CASE_BENCH=1`
(用例执行器逐帧 tick 分位数 + 命令成本表 + `[casespike]` 长帧构成)与
`YURIS_CASE_TRACE_FRAMES` / `YURIS_CASE_SHOT_FRAMES`(逐帧画面签名/定点截帧),
全部只写诊断、不参与判定。

基线(`project-eris`,3000 帧,headless wgpu,自动推进到 `es_scenario.yst`):

```text
tick p50=4.34ms p95=5.67  work>25ms 50%(含采样开销)
IF=271ns×1263万  LET=165ns×763万  INT=229ns×397万  GOSUB=782ns×118万
VARACT=1.05µs  CG=1.29µs  LOOPEND=35ns×666万  IFEND=30ns×1262万
启动帧(engine f=3)=297–330ms:LET 121ms / CG 56ms / IF 33ms / VARACT 28ms
```

### 改动(全部在本轮未提交工作区)

1. **执行位缓存**(`yu-rt/src/vm.rs`):`exec` 由 `FxHashMap<String, ExecState>`
   改 `…Box<ExecState>`,并新增 `exec_slot` —— 调度器对同一任务连跑多条命令时
   不再逐条 `remove_entry`/`insert`(旧实现每条命令两次字符串键哈希 + 两次
   700B 搬移)。`exec_of`(TASKINFO)先查缓存位,`task_removed` 命中缓存即丢弃。
2. **参数名表随命令缓存**(`Vm::fill_cmd` → `Cmd::param_names: Rc<[String]>`):
   `VARACT`/`TASK`/`TASKINFO`/`CLIPACT`/`CLIPINFO`/`ERROR` 原本**每次执行**克隆整张
   参数名表(`VARACT` 启动帧 2.8 万次 × ~14 个 `String`);现在每个 `(script,pc)`
   站点克隆一次,执行期只 bump 引用计数。
3. **GOSUB/RETURN**:`param_plan`(参数编号+类型,零分配)、`ParamTy::coerce`、
   `LabelTarget` 常量标签按调用点缓存(不再每次重解码窗口 + BTreeMap 按字节查表 +
   克隆 `Label` 的两个堆字符串)。
4. **LET/声明/写**:`op_let` 就地归一化(不再重建 `LValue` 并克隆下标表);
   `op_declare` 改 `reset_declared`(同型同维复用槽存储);`VariableStore::write`
   去掉调用方一律丢弃的返回值克隆。
5. `VariableStore::read`/`canonical_var` 标 `#[inline]`,读取路径的空间判别合并为一次。

### 结果(A/B,同机同口径;`min of 3`)

```text
                 改前        改后
tick p50        4.34ms      3.62–3.66ms   (-16%)
work p50        5.30ms      4.43ms        (-16%,桌面 2500 帧)
work>25ms 占比  2.5%        1.2%
IF              271ns       226–229ns     (-16%)
LET             165ns       133–134ns     (-19%)
INT             229ns       179–180ns     (-22%)
GOSUB           782ns       329–346ns     (-56%)
RETURN          249ns       147–157ns     (-40%)
VARACT(启动帧) 28.6ms      8.2–9.5ms     (-70%)
启动帧总耗时    297–330ms   241–261ms     (-20%)
桌面 tick p50   3.89ms      2.94ms        (-24%)
```

### 验证命令与原始结果

```text
$ cargo test --workspace --no-fail-fast        # 带语料环境变量
59 个 suite 全绿;唯一失败 yu-trace --test golden
(synthetic_trace_matches_golden:YSCom.ycd sha256 不一致,既有红灯,与本轮无关)

$ YURIS_CASE_BENCH=1 ./target/release/yu-cli.exe case <project-eris> \
    --case perf-eris --cases target/tmp
[casebench] frames=3000 p50=3.63 p95=4.30 p99=4.88 max=260.9
[casebench] cmd IF avg=226ns  LET=133ns  INT=180ns  GOSUB=346ns  VARACT=1022ns

$ ./target/release/yu-cli.exe run <project-eris> --mode desktop --frames 2500 \
    --script target/tmp/perf-vm-bench.json --frame-stats target/tmp/desk-opt2.csv
exit=0  56.2fps  tick p50=2.94 p95=7.61 p99=22.08 max=314.0
work p50=4.43 p95=10.82 p99=26.00;>25ms 30 帧(1.2%)
```

### 未做(需主人决策,不是技术障碍)

场景进入仍有一次 **~95ms 的 BGM 解码**:`bgm/bgm01.ogg` 4,004,356 帧整段解码
(约 926× 实时),发生在 `SOUND` 命令内(官方同型 = 同步整段解码)。已确认这是
场景进入尖峰的主项(engine f=224:132ms 里 93ms 是它)。要做只能把解码挪出
tick 线程(`TOTALTIME` 改用 OGG granule/WAV 头算,播放等 PCM 就绪),属**语义分歧**,
需先定口径再动手;当前代码与官方一致,未擅自改。

> **勘误(2026-10-28,REA 逐行复核)**:上段「官方同型 = 同步整段解码」**不成立**。
> 0.495 `0x422ab0` 只做 `ov_open` + `ov_time_total`(`FUN_004806c5` ×1000 →
> `+0x120`),**没有 `ov_read`/整段解码**;它尾部调用的 `0x422c65` 只写格式字段并
> `malloc(bytes_per_sec/2 + 0x1000)` 后 `CreateSoundBuffer`(**0.5 秒 DirectSound 环**)。
> 93ms 是**本内核整段 `Vec<i16>` 解码**的实现偏差,不是官方行为。已按官方架构
> 重构为流式(见下一条成果);`TOTALTIME` 改为 `ov_time_total` 同口径(最后 page
> granule),不再需要「异步等待 PCM」这类语义分歧。

## 成果 PERF-READBACK-ASYNC (2026-10-28) · 屏幕读回不再每帧等 GPU

### 症状与定位

<game-b>(v494 商业包)桌面 wgpu 模式「角色出场后卡得要死」,偶发崩溃。逐帧阶段表
(`--spike-log`)显示:render 159ms 里 **readback=139ms**,而该帧只画了 31 个 draw。
读回实现是 `ReadbackCache::read` → `collect()` → `device.poll(wait_indefinitely)`:
只要 GPU 队列里还有活(立绘换差分帧有 71MB 纹理拷贝 + 数十 MB 上传),CPU 就被按在
帧里等整条队列跑完。决定性对照实验:把屏幕读回整段跳过 →
34.4fps → 50.9fps、work p95 60.5ms → 21.2ms、render max 181.5ms → 59.8ms。

### 改法

`crates/yu-render/wgpu/src/readback.rs` 主帧路径改为**非阻塞单缓冲**:

1. 每帧先 `poll(Poll)`(不等待)收上一帧;收不到就沿用上一次成功的表面;
2. 上一帧映射未完成(缓冲仍 mapped)⇒ **本帧干脆不提交读回**,只提交绘制命令
   (绝不往 mapped 缓冲写 —— 第一版双缓冲实现漏了这一条,触发
   `BufferStillMapped` 验证错误并让 tick 失败);
3. `read_now` 保持阻塞语义(首帧 / `--shot` / 用例 golden / 存档抓帧),
   `collect_blocking` 在未挂映射时自行 `map_async` 再等。

桌面呈现本来就靠 `screen_texture`,CPU 面只服务存档抓帧,允许滞后一两帧。

顺带把 `dump_texture`(另一会话新增的 `YU_TRANS_DUMP` 取证助手)恢复并接到
新的 `read_now` 上。

### 结果(<game-b>,1200 帧,同脚本同口径)

```text
            改前        改后
fps         34.4        45.2
work p50    6.63ms      5.25ms
work p95    60.49ms     30.68ms
work p99    168.97ms    93.02ms
render p50  2.01ms      1.78ms
render p95  32.39ms     16.92ms
render max  181.5ms     63.1ms
```

`cargo test -p yu-backend-wgpu` 25 个像素对拍 + 9 单测全绿(精确路径语义不变)。

### 仍然存在的**实现路线**问题(下一步,未做)

非阻塞读回只拆掉了「CPU 等 GPU」这一个同步点。逐帧账单仍然是:

- `plan` p95 16–58ms、`image` p95 10–35ms:**CPU 合成** 2048² 立绘缓冲;
- 上传 8.8MB/帧(重帧 54–115MB),快照 `copy_texture_to_texture` 107–186MB/重帧;
- 场景进入仍是 150–470ms 的 tick 突发(脚本初始化 + BGM 整段解码 95ms)。

官方引擎(REA 取证)每层有**常驻表面**,`CGACT` 就地改它,每帧只把可见图层 blit 到
后缓冲 —— 不存在「每帧重新合成 + 整张上传 + 整面拷贝」。我们每帧重放 op 流并用 CPU
重新合成,才是真正的瓶颈;要收口需把图层表面做成常驻 GPU 纹理、CGACT 链变成对该纹理
的 GPU pass,快照改成纹理句柄而不是整面复制。这是渲染契约级改动,待主人确认后再动。

## 成果 PERF-SOUND-STREAM (2026-10-28) · 声音装载改为官方流式架构,93ms BGM 解码离开 tick

### 定位与勘误(先纠正前一条成果的错误结论)

前一条成果把场景进入的 ~95ms 归因为「官方同型 = SOUND 命令内同步整段解码」。
2026-10-28 用 REA 对 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)逐行复核:

- `0x422ab0`(OGG 装载)只做 `FUN_0047f9a2`(ov_open 族)→ `FUN_00480654` →
  `FUN_0048064c` → `FUN_004815c8` → `FUN_004806c5`(**`ov_time_total`**,
  返回秒 ×1000 写 `+0x120/+0x134`),**没有任何 `ov_read` 或整段解码**;
- 它尾部调的 `0x422c65` 只写格式字段(`+0x1ba/+0x1bc/+0x1c6`,`+0x1b4/+0x198`
  = `channels*rate` 字节/秒),随后 `malloc(0x1000)` + `malloc(bytes_per_sec/2 + 0x1000)`
  并 `CreateSoundBuffer` —— **0.5 秒 DirectSound 环形缓冲**,解码是播放期按需填充。

所以 93ms 是**本内核整段 `Vec<i16>` 解码**的实现偏差,不是官方行为;也不需要
「异步解码/等 PCM 就绪」这类语义分歧。

### 改法

- 新增 `crates/yu-sound/src/meta.rs`:容器头部探测。OGG 读 ident header +
  首个逻辑流最后一个 page 的 granule;WAVE 读 `fmt `/`data`。`TOTALTIME`
  与 `ov_time_total` 同口径,装载 O(1)。
- 新增 `crates/yu-sound/src/stream.rs`:官方同构的流式音源。
  - OGG 装载只 `OggStreamReader::new`(3 个 header packet),不碰音频;
    `sample()`/`prefetch()` 按需 `read_dec_packet_itl`,窗口 0.5s(`LEAD_MS`,
    对齐官方环大小);`request_seek` 懒定位(PLAY/START/RESTART/循环回绕);
  - WAVE PCM / ACM 保持整段样本(官方 mmio 直读 / 一次 ACM 转换,
    REA `0x422e04`),逐样本与旧路径一致;语料仅 1 个 WAV(445KB)。
- `Layer.pcm: Option<Arc<Pcm>>` → `stream: RefCell<Option<Box<Stream>>>`;
  `mixer::render` 从窗口取样本(正常播放时 tick 已预取,回调只拷贝);
  `SOUNDINFO[DATALEFT/DATARIGHT]` 改为在播放光标按需取帧(官方 Lock 一帧同构)。
- 删除整段 PCM 缓存对 OGG 的使用(官方不缓存整段);WAVE/ACM 仍缓存 `Arc`。
- 帧账单新增 `sound_decode_ms`(tick 线程:装载头部+预取+查询按需取帧)、
  `sound_stream_ms`(含音频回调兜底)、`sound_stream_frames`、
  `sound_underruns`、`sound_seeks`、`sound_slow_seeks`;spike 行同步输出。
  解码错误经 `drain_stream_errors` 回到 `Diag` 并按装载失败同型停层。

### 证据

- REA `procedure_pseudo_code 0x422ab0 / 0x422c65 / 0x4806c5`(Ghidra 12.1.4,
  同哈希 `4f5eefa3…`);`0x4806c5` 的 `pcmlengths[i*2+1]/rate` 形状即
  vorbisfile `ov_time_total`。
- 语料审计 `cargo run --release -p yu-engine --example audio_duration_audit -- <root>`:
  `<game-b>` 6966 个音频(6965 OGG + 1 WAV)`header_drift=98`
  (BGM 3 / SE 34 / voice 61,`max_drift_ms=36`);`<project-eris>` 8 个 OGG 漂移 0。
  granule 与 lewton 末包裁剪的差异**以官方 granule 为准**:播放样本按总长截断、
  不足补静音,`TIME/END/LOOP` 自洽;这 98 个文件的 `TOTALTIME` 相比旧内核
  变化 ≤36ms(旧值来自 lewton 未按 granule 裁剪的帧数,不是官方值)。
- 单测:`gated_ogg_load_reads_header_only_then_streams_on_play`(装载零解码、
  `TOTALTIME` 来自 granule、重复 SOUND 仍零解码)、
  `gated_ogg_stream_render_matches_full_decode`(流式混音逐样本 == 整段
  `decode()`,含 `START=5000ms` 定位)、
  `gated_ogg_loop_wrap_back_to_zero_reopens_stream`(循环回绕必须重开 reader;
  修复前会把「窗口空且在末尾」误判为「reader 在 0」并欠载)、
  `wav_load_plays_immediately_and_keeps_totaltime`;`cargo test -p yu-sound`
  6 lib + 45 集成全绿。

### 结果(桌面 wgpu,`--no-sound`,同脚本同口径)

`<game-b>` 1200 帧:

```text
            改前(整段解码)      改后(流式,v4)
tick p50    3.03ms              1.88ms
tick p95    5.28ms              4.39ms
tick p99    29.48ms             6.53ms
tick max    295.06ms            226.44ms   ← f78 纯 VM 场景构建,非解码
render max  176.95ms            12.97ms
work p95    9.24ms              6.75ms
fps(稳态)   54.9                58.9
sound tick 解码 max  —          1.23ms(装载+0.5s 预取/定位)
```

解码主项的场景帧:f240 **238.0ms → 35.0ms**、f579 **295.1ms → 2.0ms**;
f78 是脚本场景构建(改前 255ms/改后 226ms,均无声音解码参与)。

`<project-eris>` 3000 帧:

```text
            改前      改后(v4)
tick p50    3.40ms    2.34ms
tick p95    5.15ms    3.54ms
tick p99    18.14ms   4.59ms
tick max    261.01ms  249.57ms(启动帧 f4;解码贡献 0)
f215        170.1ms   38.4ms   ← 场景进入(余量为 VM 脚本)
fps         59.9      59.9
```

启动帧 f4 的 6 次交替采样(head vs 本改动):中位数 256 vs 270、276 vs 265
(两次会话方向相反),声音解码贡献恒 0 —— 差异是脚本场景构建/机器负载噪声。

桌面 2500 帧(带 cpal 出声):exit 0、`fatal=None`、无 wgpu 验证错误;
`sound_decode_ms` max 1.56ms、`underruns=0`、`seeks=4`(循环回绕重开 reader)、
`slow_seeks=0`,首次非静音块正常(`[sound] first non-silent block`)。

### 其它编码路径(量化后不动)

- 纹理解码(`decode_ms`):p99 ≤1.0ms;<game-b> 最坏 10.7–13.7ms/4.1MB 在
  **render plan**(CG 切换首帧),不在 tick;渲染契约由另一条线改常驻表面,
  本任务不碰。
- 字形烘焙(`text_bake_ms`):p99 0,最大 5.14ms(<8ms)。
- 存档缩略图 PNG:`LOAD[ID]` 100×75 编码 69–75µs(`YURIS_CAPTION_TIMING=1`
  时 debug 行);SAVE 图像块本身是原始 RGBA(非 PNG),无需改。
- 「同文件重复 SOUND 的解码次数」:OGG 装载**恒 0 次整段解码**;
  `full_decodes` 只统计 WAVE/ACM 整段装载(本语料 0)。

### 门禁

- `cargo build --workspace --all-targets` 通过。
- `cargo test -p yu-sound`(6+45)、`-p yu-engine`(14)、`-p yu-rt`(86/4/11/4/98/3;语料测试需 `YURIS_SAMPLE_ERIS_YSBIN`,未设时 4 条显式失败)、
  `-p yu-backend-wgpu`(9/10/25)、`-p yu-backend-soft`(41/15/1/1)、
  `-p yu-cli --test logging_guards`(3)全部通过。
- `yu-cli case <project-eris> --all`:**既有红灯**,与本改动无关。
  `git archive HEAD` 重建的纯净二进制同样 0 passed / 9 failed,且 first-screen
  的 actual display sha 与本次构建逐字节相同(`f2d2fb…`);当前工作区另有
  `save-title-flow` 因存档目录状态偶发通过。用例期望值自 2026-10-26 未随最新
  gfx 改动重生成,待另一条线统一 `--update`。
- `cargo test -p yu-trace --test golden`:已知红灯(YSCom.ycd sha),同前。

### 已知限制(留档)

- 大 `START/RESTART`/循环回绕的懒定位是「重开 reader + 解码到目标帧」;
  语料均为 0 或小值(2500 帧带声运行 `seeks=4 slow_seeks=0`,单次 <2ms),
  若以后出现分钟级 START,需要改用 `seek_absgp_pg`(lewton 支持,页粒度)
  再做帧内丢弃。
- 窗口 0.5s 由 VM tick 预取;若游戏线程被外部阻塞 >0.5s,音频回调会按需解码
  (lewton 每 packet 一次小分配)。官方由独立声音线程填充环,本内核回调兜底,
  已用 `sound_stream_ms`/`underruns` 分开记账。

## 补充 PERF-STORY-SPIKE (2026-10-28) · 推进剧情/角色出场/差分高峰归因

### 复现

桌面 wgpu,`<game-b>`,脚本逐 5 帧点击 START/对话区(`target/tmp/story-robust2.json`),
`--no-sound --frame-stats --spike-log --spike-ms 20`。命中角色出场的帧:

```text
f1796  tick=44.7  render=131.8  plan=130.5  image=42.3  text=87.4
       text_cpu=86.4  text_bake=0.8  decode_bytes=71,416,512  upload_bytes=71,991,360
       text_draws=105 glyphs=110 layers/visible≈800/620
f1811  tick=39.6  render=31.3  image=26.4  decode≈24.6ms(35.7MB) upload=36.3MB text_cpu=0.9
f1846  tick=43.8  render=36.6  image=30.8  decode≈28.7ms(35.7MB) upload=36.3MB text_cpu=0.9
f1981  tick=36.4  render=29.2  image=24.6  decode≈21.9ms(53.6MB) upload=54.1MB text_cpu=1.0
```

### 归因(三类,互不混淆)

1. **渲染:新资产首帧解码 + 整张上传**
   `decode` 19–35ms、`upload_bytes` 18–72MB/帧;差分帧的 `image_ms` 基本就是
   `decode_ms`(`MERGE`/`TRANSITION` op 计数为 0)。这是 `TextureStore` 首次
   解析 PNG→RGBA 与 wgpu 首次上传的固有成本,缓存命中后为 0。
2. **渲染:文本层整张基面拿到 CPU(`content_cpu`)**
   角色出场帧 `text=87.4ms`,其中 **`text_cpu=86.4ms`**(新探针),而
   `bake/paint/hash/base` 合计 <1ms。`apply_text` 对每个文本目标层调用
   `content_cpu`(`crates/yu-render/wgpu/src/plan.rs:1593/1714`):
   - 层内容不在 `contents` 且纹理是资产 ⇒ `textures.resolve(key).cloned()`
     **整张 RGBA 拷贝**;
   - 否则 `read_back`(`read_texture`,**同步 GPU 读回**)。
   后续用 `--fixed-step` + 分段计数复现:`tcpu_rb=69.2ms / 2 次`,
   `tcpu_asset=0` —— **2 次全屏同步 GPU 读回**(约 35ms/次),不是 105 层
   逐层拷贝。每次读回的主要时间是在等同一帧的 72MB 上传/绘制队列
   (`read_texture` 的 `poll(wait_indefinitely)`)。这是「常驻图层表面 +
   GPU 文本合成」要收口的路径(另一条线),不是日志、也不是声音。
3. **tick:游戏脚本自身的场景/差分构建**
   `--profile-frames` 同帧采样:LET 24–29ms(105k–120k 次,0.21–0.24µs/次)、
   IF 4–9ms(12k–24k)、CG 1.1–1.6ms(700–1267 次)、CGACT/CGINFO;
   某一帧 `CGINFO` 85 次 × 49µs = 4.2ms(按层表查询,可缓存)。
   这是脚本执行量,不是解码;声音在本场景的 `sdecode` ≤2ms。

### 日志本身的影响(标题页稳定内容 A/B,300 帧)

```text
              tick p50/p95     render p50/p95    work p50/p95
quiet         2.81 / 4.79      1.53 / 2.36       5.06 / 7.38
info          3.10 / 5.18      2.27 / 2.94       5.94 / 8.86
debug         3.07 / 5.18      2.03 / 3.04       5.83 / 8.63
quiet+UI      2.88 / 4.88      1.87 / 2.44       5.75 / 8.46(draw p50 0.29→0.74)
```

info/debug 相对 quiet 只多 ~0.3ms tick / ~0.5ms render,debug-ui 多 ~0.45ms
draw;不会产生 30–120ms 尖峰。尖峰是上面的资产/文本/脚本成本,日志只是让它可见。

### 记账补充

`RenderPhases` 新增 `text_cpu_base_us`(仅 wgpu 填;soft 默认 0),spike 行输出
`text_cpu=`。`cargo build --workspace --all-targets`、wgpu 9/10/25、soft
41/15/1/1、logging_guards 3 全绿。

### 收口方向(需渲染线确认,本任务不动渲染契约)

- 文本合成不再逐层把整张基面搬到 CPU:层表面常驻 GPU,文本 pass 直接在 GPU
  上写;必须 CPU 合成时只取需要的区域/复用缓存。
- 差分/CG 在场景装载期预解码下几张候选(脚本按标签顺序加载),或把首帧解码
  分摊到非尖峰帧;首帧缺资产必须带 `asset.*` 诊断。
- 脚本侧:LET 循环是最大的 tick 项(0.21µs/次 × 十万级),`CGINFO` 可按层版本
  缓存(49µs/次 × 85),属 VM/域查询优化,不是编解码。

### 修复推进(2026-10-28)

已落地的安全修复(不改渲染契约、像素级回归):

1. **资产身份快路径**:层内容就是它的资产(`raw.hash == fnv(id, tex.id)`
   且采样窗恒等)时,直接复用 `TextureStore` 的资产 CPU 纹理,跳过 GPU 读回。
2. **文本基面 CPU 缓存**:键 = `(层, 基面稳定身份)`。稳定身份 =
   `layer base token + 已应用操作前缀`(`LayerSurfaces::raw_identity`),
   不混纹理池 id;同一基面换池/换文本仍命中。预算 64MB / 32 条,LRU。
   `YURIS_TEXT_BASE_CACHE=0` 可关闭做 A/B。
3. 记账/探针:`RenderPhases` 增 `text_cpu_base_us`、`text_cpu_readback_us/
   readbacks`、`text_cpu_asset_us/bytes`、`text_cpu_cached`、`text_cpu_stores`;
   spike 行输出 `text_cpu= / tcpu_rb= / tcpu_asset= / tcpu_cache= / tcpu_store=`。
   读回发生时还有 `RUST_LOG` 可控的 `[text-base] layer=... base_key=...` debug 行。
4. 宿主加 `--fixed-step`(固定 1/fps 逻辑时钟 + 确定性 `local_time`)供性能/
   像素回放;仍不能完全消除该作品的运行间差异(同一脚本可能落在不同剧情点),
   游戏级 A/B 以 spike 现场为准。

回归:`cgact_text_reuses_cached_cpu_base_across_text_changes`(wgpu)验证
「资产层首帧零读回 + 换文本零读回 + 与 soft 像素一致」;`base_cache_hits_by_
layer_and_hash_and_forgets`(core)验证缓存键/淘汰。`cargo test -p yu-render-core`
10、`-p yu-backend-wgpu` 26、`-p yu-backend-soft` 41/15/1/1、logging_guards 3 全绿。

**仍未解决(需常驻表面/GPU 文本,渲染线;文本部分已于 2026-10-29 收口,见下节)**:

- 游戏里这两层是 GPU 合成基面,且每帧基面身份变化/层重建,缓存与资产快路径
  都不命中:首帧仍 2 次读回 ≈73ms,后续文本帧仍各 2 次(队列空闲时 3.8ms/次)。
- 差分帧仍是首用差分图解码 19–35ms + 36–54MB 上传;并行预解码可把解码墙钟
  压到 1/4 左右,但上传/合成仍在,需与常驻表面一起做。


## 成果 PERF-TEXT-GPU (2026-10-29) · 文本字形直接写入 GPU 层表面,角色出场读回尖峰收口

### 结论

`CGACT[TEXT]` 的文本层不再把整张基面同步读回 CPU:字形仍由 CPU
(`TextComposer`/fontdue)栅格化成与 soft 完全相同的像素,但合成改为
**compute shader 直接把字形写进该层的常驻 GPU 表面**(storage
`rgba8unorm` read_write)。实测角色出场/换文本帧的 `text_cpu=0`、
`tcpu_rb=0`、`tgpu_fb=0`;365 个字形 = 949 次绘制(9 宫 SET 展开)时
`text≈0.9ms`。官方证据同前:`0x4056b8` 在层表面就地写字形,不存在整面
读回;此处对齐其数据流,不引入异步播放机制。

### 实现

- `yu-render-core/src/text.rs`:
  - `GlyphPlacement { x, y, image, blend }`(导出);
  - `TextComposer::collect_glyphs(&TextDraw)` 复用与原 CPU `paint_one`
    相同的字体、字号(含 `ascent_px`)、cp932 解码与
    `composite_offsets`(SET=0 → 1 遍;SET≠0 → 9 遍),按 CPU 绘制顺序
    返回位图与位置。
- `yu-render/wgpu/src/text_gpu.rs` + `shaders/text_glyph.wgsl`:
  - 基面 `raw.tex` 恒等采样时 `copy_texture_to_texture` 到目标表面;
    目标由 `LayerSurfaces::acquire` 分配,usage 增 `STORAGE_BINDING`;
  - shader 用 **整数运算**逐像素复刻 `Rgba::over` / `replace_with`
    (`yu-font/src/color.rs`),不做浮点混合;
  - **单个 workgroup 按 draw 顺序串行 + `workgroupBarrier()`**:CPU 的
    9 宫/多绘制项是有序叠加的,多个 workgroup 并行写重叠字形会让结果
    依赖调度(曾复现 `diff` 在 0/224/230 间跳变);串行后与 soft 逐像素
    一致;
  - 缓存键 `(text revision, 签名, 基面 hash, 基面稳定身份)`;预算
    128MB/32 层,LRU。同一帧重复渲染命中缓存,不重拷/不重 dispatch。
- 能力门禁:`Rgba8Unorm` 需 `STORAGE_READ_WRITE`,并启用
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`;不支持或管线创建失败时
  记 `text.gpu.unsupported` 诊断并回退 CPU 合成(`tgpu_fb` 计数),
  `YURIS_TEXT_GPU=0` 可强制关闭做 A/B。
- 记账:`RenderPhases.text_gpu_{layers,glyphs,draws,cached,fallback}`;
  spike 行 `tgpu=层/字形/draw tgpu_c= tgpu_fb=`;CSV 末尾追加 5 列
  (旧列序不变)。
- 未覆盖(仍走 CPU 文本合成,已计数 `tgpu_fb`):
  `text_layout`(屏幕级自动文本)与缩放/非恒等采样基面。

### 回归与测试

- 新增/加强 `crates/yu-render/wgpu/tests/surfaces.rs`:
  `text_layer_is_stable_across_frames` 覆盖首帧、同帧重复、换文本三态,
  要求与 soft 逐像素一致且不残留旧字形(重叠 A/B 绘制项即上述调度回归)。
- `cgact_text_layer_frames_match_soft_exactly`(1 字形)、
  `cgact_text_reuses_cached_cpu_base_across_text_changes`(换文本零读回;
  有 GPU 路径时断言 `tgpu_draws>=1`,无 GPU 时断言 CPU 基面缓存命中)。
- `cargo build --workspace --all-targets` 通过;
  `cargo test -p yu-sound`(6+45)、`-p yu-engine`(14)、
  `-p yu-rt`(设 `YURIS_SAMPLE_ERIS_YSBIN` 后 4/11/4/98/3)、
  `-p yu-render-core`(10)、`-p yu-backend-soft`(41/15/1/1)、
  `-p yu-backend-wgpu`(9/10/26 + doc 1)、
  `-p yu-cli --test logging_guards`(3)全绿。
- `cargo fmt --all` 会重排历史未格式化文件,本轮未把这类无关改动带入;
  改动文件按语义 diff 审阅。

### 游戏实测(<game-b>,桌面 wgpu,`--no-sound --fixed-step`)

复现命令(角色出场脚本,逐 5 帧点击 START/对话区;`--fixed-step` 固定
1/fps 逻辑时钟):

```bash
./target/release/yu-cli.exe run <game-b> --mode desktop --frames 1400   --no-sound --fixed-step --script target/tmp/story-robust2.json   --frame-stats target/tmp/gputext/frames3.csv   --spike-log target/tmp/gputext/spikes3.log --spike-ms 20 --log-level quiet
```

- 1400 帧,93 个 work≥20ms 的尖峰,其中 **82 个走 GPU 文本**;
  最大 365 字形/949 draws,`text=0.9ms`,`text_cpu=0`,`tcpu_rb=0`,
  `tgpu_fb=0`;文本帧的 `render` p95 3.1ms。
- 同口径历史基线(2026-10-28)f1796:`text=87.4ms`,其中
  `text_cpu=86.4ms`、`tcpu_rb=69.2ms/2 次`;本改动后该类帧的两次全屏
  同步读回消失。
- 干净存档标题页 1200 帧:`avg_fps=57.8`,`work p50/p95=3.7/5.8ms`,
  `tick p50=1.65ms`,`render p95=1.86ms`;首帧启动 170ms(资产装载/
  捕获),`fatal=None`,无 wgpu 验证错误,exit 0。
- 未覆盖项:自动文本 `text_layout` 仍 CPU(该作品场景首帧约 10ms);
  差分纹理首用解码仍在 render plan(实测 19–36ms + 36–57MB 上传),
  属常驻表面/预解码线的下一步,不在本轮文本路径内。

### 门禁备注

- `yu-cli case <project-eris> --all`:既有红灯(用例期望值未随最新 gfx
  重生成),与本轮无关。
- `cargo test -p yu-trace --test golden`:已知红灯(YSCom.ycd sha),同前。


## 成果 PERF-VM-TICK (2026-10-29) · 控制流事件与变量热路径收口,场景构建 tick 降约 30%

### 画像

同一 120 帧固定步长启动(`<game-b> --no-sound --fixed-step`),e80 是场景/按钮
构建帧:**811,375 条命令**。改前 `--profile-frames`(1/32 采样,含采样器
开销)分项:`LET 66.2ms / 310k`、`LOOPEND 40.3ms / 208k`、`IF 23.3ms / 99k`、
`CG 24.3ms / 21k`、`VARACT 6.0ms / 19.5k`,采样总时长 174ms;尖峰行
`tick=149.7ms`。非 profile 基线 e80 tick 137–157ms。最终 profile:
`LET 34.7ms`、`IF 11.4ms`、`CG 18.5ms`、`LOOPEND 5.8ms`,总采样
**87.1ms**(-50%);非 profile e80 tick **92–95ms**。

### 改动(全部保持脚本/画面语义)

1. **控制流事件门控**:`record_events=false` 时 `Jump`(IF/ELSE/LOOP/
   LOOPEND)不再入事件队列。引擎随后只统计队列长度并丢弃;场景构建帧的
   208k 次 LOOPEND 回跳是纯诊断开销。`LOOPEND` 40.3→6.2ms。
2. **图形/窗口域事件门控**:Graphics/Window `DomainCall` 已由同步图形宿主
   在命令时刻消费;关事件流时不再入队(CG 22.6→22.1ms,主要消除 21k 条
   事件/参数在队列里的驻留)。Text/FrameStart/End/SAVE/SOUND/File 等仍
   照常入队;`record_events=true` 的 golden 事件流逐字节不变。
   勘误(2026-10-29 复核):门控只在**确实注入了图形宿主**时生效;没有
   宿主的测试/探针(如 `eris_first_screen` 用 `drain_events` 驱动
   `GfxState`)仍照常入队,否则首屏 272 层会退化成只有根层。
3. **单变量窗口快路径**:`0x48` 取值窗口直接读变量域/系统变量;
   `LET` 左值单变量直接构造 `LValue`,不再走 Evaluator 栈机。门控后
   采样总时长 137.7→133.3ms;`eval_window` 只对 `0x48` 生效,避免改变
   Command 事件的参数记录(golden 锁定)。
4. **数组偏移错误名惰性化**:`linear_offset_for` 原来每次成功读写都
   `format!` 变量名,只为越界/负下标错误文案。改为错误分支内才格式化。
   `LET` 64.6→41.1ms、`IF` 23.3→15.3ms,采样总时长 133.3→102.6ms。
5. **数组取值栈去堆分配**:表达式 `0x29`(数组取值)原来每次 `Vec::new()`
   收集下标,改为 `SmallVec<[i64;4]>`;域参数 `args` 按窗口数预分配。
   采样总时长 102.6→87.1ms,`LET` 41.1→34.7ms、`IF` 15.3→11.4ms。
6. **诊断命令名惰性化**:去掉每条命令第二次 `Rc<Cmd>` 克隆,错误定位时按
   `(cur_script, cur_pc)` 从取指缓存取。

### 数据(同一脚本/固定步长)

- e80 tick:基线 137.4 / 156.6 / 159.8ms → 最终 91.8 / 95.3ms
  (约 -35%~-40%)。
- 1400 帧剧情脚本(剧情分支导致运行间差异):改前 93 / 92 个
  `work≥20ms` 尖峰、`tick p95` 19.6 / 19.1ms;改后多轮为 36–81 个尖峰、
  `tick p95` 6.0–17.6ms、`work p95` 10.7–23.2ms;同帧 e80 稳定在
  92–95ms,tick 最大值从 159.8ms 降到约 95ms。
- 干净标题页 1200 帧:`avg_fps≈58`,`work p95=5.37ms`,
  `tick p50=1.37ms`,`render p95=2.06ms`,最大启动帧 109.5ms。
- `<project-eris>` 600 帧:`avg_fps≈57`,`work p95=5.14ms`,
  `tick p50=1.90ms`,`render p95=2.15ms`,启动帧 167.4ms
  (此前同口径 272.6ms,含运行间波动)。

### 语义与回归

- `record_events=false` 只影响**内部事件队列**:Jump 与 Graphics/Window
  DomainCall 不入队;图形宿主仍在命令时刻同步应用,存档/声音/文件域事件、
  Text/ReturnCode/Diag 仍保留。`record_events=true` 时完整事件流不变。
- 新增 `jump_events_are_suppressed_when_record_events_off`(fixture):
  关事件流时无 Jump 且分支结果不变。
- `cargo test -p yu-rt`:golden 3、corpus 11、fixture 99、consistency 4、
  其它 86/3 全绿;`cargo build --workspace --all-targets` 通过;
  `logging_guards` 3 通过(移除 `linear_offset_for` 的常驻
  `unwrap_or_default` 后,`value.rs` 白名单 4→3)。
- 未改动:脚本命令数/顺序、变量值、渲染契约、声音流式路径。

### 剩余

e80 仍有 ~92ms:811k 条命令的**解释器吞吐**本身是主成本(LET ~35ms、
CG ~18.5ms、IF ~11.4ms、LOOPEND ~6ms)。下一步可做:
lvalue 下标表达式的预编译计划、CG/域参数路径的零分配、
`CGINFO` 按层版本缓存;按帧切分/跨帧续跑属显式宿主分歧
(DIV-HOST-002),不作为默认优化。


## 成果 PERF-ASSET-PRELOAD (2026-10-29) · 按官方 CG 命令时刻装载,消除角色出场解码/抓帧尖峰

### 官方证据(v494/0.495,REA + Ghidra 强制建函数)

- v494 `CG` 处理体入口 `0x42a498`(文档旧称 `0x42a4a8`;0.495 对应
  `0x42a514`,sha256 `4f5eefa3…`)。
- `FILE` 分支在处理体内**同步**调用 `0x42c2a8`;该函数:
  - 空文件名 → `0x40c5f8`(1×1 黑图);
  - 非空 → `0x40c7c9` → `0x40c9b4` 按 `.png/.bmp/.jpg/.gif` 探测并解码
    (`0x40d6f9` 等),返回后立即更新图层 `+0x90` 像素。
- 因此官方在 `CG[FILE]` **命令所在帧**完成解码,与图层当时是否参与合成无关。

### 官方线程普查(REA,同 0.495 镜像)

- 只导入 `KERNEL32.DLL::CreateThread`;无 `QueueUserWorkItem` /
  `CreateThreadpool` / `_beginthread`。调用点只有 CRT 包装
  `0x46bb30` / `0x46bdd1`。
- 包装的实际调用方:
  - `0x463f50`(WinMain `0x46164c` 内):按 CPU 数建**软件合成线程池**,
    每线程 `SetThreadIdealProcessor`;任务体(`0x4265cc`/`0x425e08`/
    `0x425a08`/`0x425680`/`0x4255ec`…)全是逐行 blit、alpha/色键混合、
    缩放与通道混合。
  - `0x45f3f4 → 0x422268`:先 `CoInitializeEx` 再进影片/COM 路径。
  - `0x4640c4 → 0x42543c`:由开关 `DAT_008015fc` 控制。
- 结论:官方**没有并行资产解码**;并行只用于软件合成与影片播放。
  本内核的并行 PNG 解码属实现层分歧,登记为 `DIV-GFX-067`
  (线程数 = `available_parallelism - 1`,钳制 `1..=4`,留一核给
  主线程/音频;结果按输入序写回,像素与串行一致)。

### 我们的分歧与修复

- 旧路径把解码延迟到 render plan 首次真正使用该层:角色部件在
  frame 834/852/… 已收到 `CG[FILE]`(keys `cg/stand/...`),但若层在组内/
  隐藏,直到入口帧 e1014 才解码/上传 → **36ms 解码 + 76ms capture**。
- `GfxState::apply_cg` 在命令时刻登记 `CG[FILE]` 资产 key;引擎在同一 tick
  末尾调用 `Renderer::preload_assets`(官方同帧语义):
  - `TextureStore::preload_parallel`:先顺序读 ypf 字节,再按资产并行
    解码(1..=4 线程,仍在本帧内完成,输入顺序写回缓存);
  - wgpu 侧随后在主线程 `ensure_asset` 上传;soft 侧只解码入缓存。
- 幂等:同一 key 在单批内去重、成功缓存只解码一次;失败缓存并在
  `ensure_asset`/`resolve` 处报 `asset.*` + `[asset] preload failed`。
- 额外收口:`GfxCapture` 不再对每个新 `CopyId` 无条件同步回读。
  - wgpu:`SAVE[ID]` 命令时刻用 `SnapshotSource` 按需读**上一帧已冻结
    快照**;没有 SAVE 不读;
  - soft:快照本来就是 CPU 表面,保留渲染后 eager 镜像(旧语义、无 GPU
    等待);
  - `observe` 只补“本帧 SAVE 已登记、但当帧尚未渲染”的 `wanted`。
  e754 的 163ms capture 消失;晚一帧的 SAVE 不再回归(新增
  `snapshot_source_reads_on_demand_and_caches`),save_state 与旧实现一致
  (旧/新 A/B 都是 `e264…`;expected `bbd6…` 是 DNO3 缩略漂移)。

### 实测(`<game-b>` 1200 帧固定步长剧情脚本)

- 入口帧 e1014:`work 167.8→51.7ms`,`render 46.1→4.6ms`,
  `decode 36.3→0`,`capture 76.1→0`;剩余 51.7ms 是 tick 46.3ms
  (208k 条脚本命令,VM 线)。
- 角色建立帧 e754:`work 241.1→4.7ms`(capture 163.1→0)。
- 1200 帧全量:`decode` 最大 **0ms**(全部在命令时刻预热),`render p95=3.39ms`。
- 并行解码:同批 4 张 1332×3351(17.85MB/张)从串行 57.5ms 降到
  **21–24ms**;VFS 读包只占 ~0.6ms,瓶颈是 PNG 解码。

### 回归

- `cargo build --workspace --all-targets` 通过;
  `yu-render-core` 11(新增 `parallel_preload_matches_sequential_and_caches`)、
  `yu-backend-wgpu` 9/10/26、`yu-backend-soft` 41/15/1/1、`yu-engine` 16
  (新增 `capture_snapshot`:晚一帧 SAVE 端到端 / 缺像素显式计数)、
  `yu-sound` 6/45、`yu-save` 14(设 `YURIS_SAMPLE_ERIS_ROOT`/`SDK0495`)、
  `yu-rt` 86/4/11/99/3、`logging_guards` 3 全绿;`yu-gfx`
  `--test preload` 2(仅有效图像层登记、取走不重发)。
- 像素未变:预加载只改变解码/上传时机;`decode` 降为 0 后入口帧 render
  由 46.1ms 降到 4.6ms。
- `cases/save-load-loop` 的 `save_state_sha256` 旧/新实现相同,仍是
  DNO3 缩略图预期漂移;`record-events-off` 的 `ops=85` 不变。

### 仍未解决

- e1014 剩余 `tick 46.3ms` 是 208k 条脚本命令(VM 解释器),不是纹理;
  下一步继续 VM 快路径/查询缓存。
- 同批并行预热仍有 21–24ms(单张最慢 PNG 决定);若要再降,需要更快的
  PNG 解码器或把命令批次拆到更早的隐藏装载帧(后者需官方证据)。
- 资产缓存:CPU 侧已有字节预算(`TextureStore` 默认 512MiB,
  `YURIS_ASSET_CACHE_MB=0` 关闭;只回收 `Rc::strong_count==1` 的条目,
  仍在图层/快照里的纹理不回收)。预算在**帧边界**执行,本帧新插入的
  条目豁免,避免 `preload_parallel` 批次在渲染前自我驱逐;帧内允许
  暂时超额。GPU 侧 `TextureCache.asset_ids` 仍随资产数增长(官方
  `CASH`/`REMALLOC` 策略未分类,暂不做未经证据的显存 LRU)。
- 同 tick `SAVE[ID]` 指向刚 `CG[FILE]` 的**资产层**时,`GfxCapture` 的独立
  `TextureStore` 可能重复解码一次(存档缩略图实际走快照链,本轮未触发);
  彻底消除需要主渲染器与抓帧器共享解码缓存。
- 同帧 `COPY/COPY2` 后立刻 `SAVE[ID]`:已按命令时刻补齐(注册表 →
  源链裁剪 → 整帧软回放;见 `DIV-GFX-068`)。源与目标都是**同帧新建**时
  仍需整帧回放(正确,约 0.4s/次),属于低频存档成本。

## 成果 THREAD-AUDIT (2026-10-29) · 官方线程普查:无并行资产解码

回答「并行是不是官方实现」:不是。REA 0.495(sha256 `4f5eefa3…`)普查结果:

- 只导入 `KERNEL32.DLL::CreateThread`;无 `QueueUserWorkItem` /
  `CreateThreadpool` / `_beginthread`。唯一调用点 `0x46bb30` / `0x46bdd1`
  是 CRT 风格线程包装(分配 0x74 上下文、挂起创建、`ResumeThread`)。
- 实际调用方三处:
  - `0x463f50`:按 CPU 数建**软件合成线程池**,每线程
    `SetThreadIdealProcessor`;任务体 `0x4265cc`/`0x425e08`/`0x425a08`/
    `0x425680`/`0x4255ec` 等全是逐行 blit、alpha/色键混合(SSE
    `pavgb`/`paddusw`)、缩放/通道混合。
  - `0x45f3f4` → `0x422268`:`CoInitializeEx` 后进影片/COM 路径。
  - `0x4640c4` → `0x42543c`:由开关 `DAT_008015fc` 控制。
- `CG[FILE]` 解码仍在命令处理体内同步完成(`0x42a498`/`0x42a514` →
  `0x42c2a8` → `0x40c7c9` → `0x40c9b4`),不投递线程池。

处置:

- 新增 `docs/divergences/DIV-GFX-067-asset-preload-parallel.md`:并行 PNG
  解码登记为实现层分歧;线程数改为 `available_parallelism - 1`、钳制
  `1..=4`(留一核给主线程/音频),结果按输入序写回,像素与串行一致。
  本次 4 张同批的实测样本原本就是 4 线程,数字不变。
- `GfxState::apply_cg` 的预热登记移到「确认该层会采用 FILE」之后:
  图层组/类型冲突/空 FILE 不再预热未使用资产(新增 `yu-gfx --test preload`)。
- 新增强化回归:并行与串行逐字节对比(不同颜色的 3 张 fixture);
  晚一帧 `SAVE[ID]` 端到端(`crates/yu-engine/tests/capture_snapshot.rs`)。

验证(原始结果):

```text
cargo build --workspace --all-targets            → Finished (50.40s)
cargo test -p yu-render-core                     → 11 passed; 0 failed
cargo test -p yu-gfx                             → 17/5/10/4/1/4/7/11/7/11/27/2/2/10/6/6 全 ok
cargo test -p yu-engine                          → 15 passed + capture_snapshot 2 passed
cargo test -p yu-cli --test logging_guards       → 3 passed
cargo test -p yu-backend-soft/-wgpu/-save/-rt    → 41/15/1/1、9/10/26/1、14、86/4/11/99/3 全 ok
```

未复测:当前环境无 `<game-b>` 样本路径,无法重跑 1200 帧游戏口径;
4 张同批的并行线程数未变(仍为 4),e1014/e754 数据沿用上一轮实测。

## 成果 SAVE-THUMB-FIX (2026-10-29) · 同 tick `COPY2→SAVE[ID]` 恢复命令时刻抓帧

### 症状与现场(用户存档实测)

`<game-b>` 手动新存档的缩略图为空。解析用户 14:20 的两份 `.sd`:

```text
save30001.sd / save30002.sd  DNO3 btype=1 ulen=1612  A=1600 B=4     (1×1)
官方/旧存档 save00001.sd     DNO3 btype=1 ulen=31608 A=1600 B=30000 (100×75)
```

### 根因(脚本 + 帧契约)

`es_saveload.yst`(sid 19)**同一 tick**:

```text
pc312 CG ID="SLTEMP2" Z=1 A=0 SX=1 SY=1 FILE=""   # 1×1 占位层
pc313 CGACT COPY2=1 ID="SLTEMP" ID2="SLTEMP2"
pc319 SAVE DNO=3 ID="SLTEMP2"
pc341 SAVE FILE=...   pc343 CGEND SLTEMP2
```

官方 `COPY2` 在命令处理器内立即写表面,`SAVE[ID]` 读同一块表面;
帧契约(66)后主渲染器在帧末才写快照,`GfxCapture` 只有“上一帧注册表”，
pc319 时新 `CopyId` 不存在 ⇒ 返回 `None`,目标层还是 `CG FILE=""` 的 1×1
占位 ⇒ 写出 1×1 缩略图。

### 修法

`GfxCapture::capture` 对快照三级取像素(见 `DIV-GFX-068`):

1. 注册表/镜像(上一帧快照,GPU 按需读回一次并缓存);
2. **源链裁剪**:沿 `CopyOp.source_texture` 回溯源快照/资产,按
   `CopyRegion` 裁剪(存档链 `SLTEMP→SLTEMP2` 走这条,**零回放**);
3. 命令时刻**整帧软回放**(组/后缓冲/带文本源),`last_screen` 由引擎
   每帧渲染后注入。
4. `SAVE[ID]` 落到 `SolidBlack1x1` 时显式 WARN(`COPY missing?`),不再
   静默写 1×1。

顺带收口资源上限:`TextureStore` 增加 CPU 字节预算(默认 512MiB,
`YURIS_ASSET_CACHE_MB` 覆盖;只回收 `Rc::strong_count==1` 的条目,
图层/快照在用的纹理不回收),新增 `resident_bytes()/evicted_count()`。

### 验证(原始结果)

```text
# 实机脚本(kotoneiro):START(750,415)→SAVE(380,812)→槽 1(350,200)
# 修复前: [caption] snapshot 336 pixels not materialized yet; deferred
#          DNO3 ulen=1612 / B=4
# 修复后(soft,同脚本):DNO3 btype=1 ulen=31608 A=1600 B=30000
# 首次整帧回放 436ms;加入源链裁剪后无 replay 行,存档界面看到缩略图
# 桌面 wgpu(--mode desktop --backend wgpu --frames 640,同脚本):
[caption] LOAD[ID] PNG 100x75 12241B encode=548us
DNO3 btype=1 ulen=31608 A=1600 B=30000

cargo test -p yu-engine --test capture_snapshot → 4 passed
  (上一帧 SAVE / 同 tick 资产源裁剪 / 同 tick 组源回放 / 缺像素计数)
cargo test -p yu-render-core              → 13 passed(新增缓存预算/预热批次豁免用例)
cargo test -p yu-gfx --test preload       → 2 passed
```

### 残余

- 组/后缓冲源仍整帧软回放(~0.4s/1100×825/188 层),低频存档可接受;
  同帧源+目标都新建时也走这条(正确但慢)。
- `GfxCapture` 的独立 `TextureStore` 仍与主渲染器分开;若 SAVE 直接指向
  刚 `CG[FILE]` 的资产层会重复解码一次(存档缩略图走快照链,未触发)。

---

## 成果 CG-AFFINE(2026-10-29) · CG 仿射/滤镜参数绑定 + 参数分类收口

把 `CG` 的仿射/滤镜族(`FX FY FBX FBY FRX FRY FRZ FCX FCY FEX FEY FEZ
FSD FMD FQU F`)从「原样归档 + 每帧挂号」改成**按引擎偏移逐项落地**,
并把 CG/CGACT 里剩下的未分类参数一次定案。

### 取证(REA 0.495,sha256 `4f5eefa3…`)

1. 参数取值是**声明下标**驱动:`FUN_00429fd1` 每命令 `memset(&0x684160,100)`
   后填 `[0x684160+name_id]=slot`(`0x42a078`),处理体再按
   `[0x684160+下标] → slot → [slot*8+0x683740]` 读值。
2. 属性块逐项写入点(下标 4/15 = X/FX 同写 `+0x1b0`;5/16 → `+0x1b8`;
   17/18 → `+0x1c0/1c8`;19/20/21 → f32 `+0x1f8/1fc/200`;22/23 →
   `+0x1d0/1d8`;24/25/26 → `+0x1e0/1e8/1f0`;27 → `+0x204`;28 → `+0x208`;
   29 → 字节 `+0x20c`;31 → 字 `+0x214`)。
3. 层构造默认值 `FUN_00465730`:`+0x1c0/1c8 = 10000`、`+0x204 = 0x80`、
   `+0x208 = 0x808080`。
4. `X` 与 `FX` 是同一物理量的两种单位:`CG[X=n]` → `+0x68=n`/`+0x1b0=n*100`;
   `CG[FX=n]` → `+0x1b0=n`/`+0x68=n/100`(`0x42aa8b..0x42aa9c` /
   `0x42ab4b..0x42ab52`)。
5. 属性算子 `0x42bd3c`/`0x42bdbc`:0 赋值 / 1 加 / 2 减 / 3 乘 / 4 除;
   `FQU` 是唯一直接赋值的例外(`0x42b592..0x42b5ac`)。
6. 其余参数(CG 处理体 `0x42a514`、CGACT 处理体 `0x42c4a4`,含
   `mov reg,0x6841xx` 的间接读取形态全量扫描):`TSX/TSY/MFILE/CASH`
   (下标 9/10/36/42)在 CG **无读取点**;`DXDRAW`(14)有读取点但只受
   全局 `DAT_008012e4` 门控(强制重绘);`MIPMAP` 值减 1 但引擎不生成
   mip 链;`DXBUF` 写**全局** `DAT_00801508`;`RID` 是资源图取图路径
   (`0x42bb94`:归一化 → 资源树 → 建层),`FILE` 与 `RID` 都缺才报错;
   CGACT 侧 `Z2`(8)/`ROTATION2`(30)无读取点。
7. `IDNO` 是 `__itoa(基数 10)` **直接追加**(`0x42c542..0x42c54c`),
   旧实现的 `ID + "." + 十进制` 多一个点。
8. `CGACT` 的 `RASTER1` **不在** 0.495 参数表(只有 RASTER/RASTER2/RASTER3)。

### 改动

- `crates/yu-gfx/src/layer.rs`:`FilterParam`/`FilterTransform` 退役,
  新增 `Affine`(字段/默认值与引擎逐项对应,文档内含写入点表);
  `Transform.filter` → `Transform.affine`。
- `crates/yu-gfx/src/state.rs`:`apply_layer_params` 逐参数落地;
  `FX/FY` 写 `transform.x/y`(= `+0x68` 口径);`TSX/TSY/MFILE/CASH`/
  `DXDRAW`/`MIPMAP`/`DXBUF` 只留档不再报「未分类」;`RID` 显式
  `gfx.image.rid.unsupported`;`RASTER1` 删除;`Z2` 静默接受;
  诊断码 `gfx.filter.unsupported` → `gfx.affine.unverified`(只在命令
  真的给了仿射参数时发一条)。
- `crates/yu-gfx/src/path.rs`:`IDNO` 去掉点。
- `crates/yu-gfx/src/json.rs`:`transform.affine` 结构化输出。

### 验证(原始结果)

```text
cargo test -p yu-gfx                         → 全部通过(27+9+… 项)
cargo test -p yu-gfx --test transform        → 9 passed
  (fx_fy_are_the_same_field_as_x_y_in_hundredths / affine_params_land_on_engine_fields
   / affine_defaults_match_engine_layer_constructor / fqu_is_plain_assignment_like_engine)
cargo build --workspace --all-targets        → ok
cargo test --workspace                       → 仅 yu-trace synthetic_trace_matches_golden 失败
  (既有问题:YSCom.ycd sha 997b5b63… ≠ golden 39ba92ef…,与本轮无关)
cargo test -p yu-cli --test logging_guards   → 3 passed
```

语料首屏 golden 复核(`YURIS_SAMPLE_ERIS_YSBIN=<project>/data/ysbin`,
`UPDATE_GFX_GOLDEN=1` 后与 HEAD 逐节点 diff):

```text
ADD/DEL 仅 transform.affine ↔ transform.filter 的形态变化
total value diffs: 0
```

900 帧实跑(`YURIS_SAMPLE_ERIS_ROOT=<project-eris>`,标题点击进本篇)
`CG[FX]` 事件 0 条 —— 首屏/开场无立绘,`FX` 路径未触发,故本轮无像素风险;
绑定由单测钉住。

### 残余

- 仿射字段的**绘制侧合成公式**仍未对拍(DIV-GFX-070):字段已存,
  后端只消费 `x/y`;非默认值图层标 `gfx.affine.unverified`。
- `RID` 资源图链路缺 provider(PSB/E-mote 资产头),保持显式挂号。

---

## 成果 DATA-ENTRY-KEY (2026-10-29) · 数据入口支持 pac 多包,脚本密钥自动解析

### 问题

商业发行版把封包放在 `pac/` 下并按内容命名(`bn.ypf`/`cg.ypf`/…),不是单个
`data.ypf`;`Project::discover` 只认 `data.ypf`/`data/`,遇到该商业包直接报
「找不到数据入口」。同时脚本密钥是每作可变的(`CRC32(字符串)`;本作 =
`CRC32("ClockupDl")`),运行核只认默认值 `CRC32("Yu-ris")`,换钥作品停在
`fmt.ybn.key`。

### 改动

- `crates/yu-engine/src/project.rs`:按官方发现规则(`FUN_0045eda0` /
  `FUN_0045e3b8`)扫 `<根>/*.ypf` + `<根>/pac/*.ypf`(开发工程再并入
  `<根>/data`);`update1..9.ypf` 为增量层、其余为基础包(按文件名排序,
  确定性);非 `YPF\0` 魔数的文件跳过并计数(实测 `pac/ed01..04.ypf` 是
  MPEG-PS)。`DataEntry` 改为 `PackSet{base,updates,skipped}`,
  `describe()` 带 `packs/updates/skipped`。
- `crates/yu-engine/src/keys.rs`(新):按开源实现的两条静态路线取钥 ——
  RxYuris `YSTB_Coder::GetXorKey`(读 `0x20+s1+8`)与 YU-RIS-Script-Editor
  `_guess_v5_key`(12 字节窗口投票);候选一律过结构自检
  (`Σ argc×12 == s2`、`opcode < 命令数`)。exe 字面量 CRC32 只作最后兜底
  (已标注:工具未用此路线)。
- `crates/yu-engine/src/engine.rs`:`BootConfig.script_key` 接解析结果,
  启动日志打印密钥与来源。
- `crates/yu-cli/src/{run,desktop,server}.rs`:数据摘要统一走
  `DatasetInfo::summary()`;`run` 新增 `--script-key <8 位十六进制>`。
- `docs/formats/ypf.md` §4.1:记录读取器的扫描/排序/跳过规则。

### 验证(原始结果)

```text
cargo build --workspace --all-targets          → ok(无 warning)
cargo test -p yu-engine -p yu-cli -p yu-vfs    → 全过
cargo test -p yu-engine --test project_layout  → 1 passed(真实商业包布局)
yu-cli run <0.481 商业包根>                           → script key: 43 2F 06 B0 (source=rx-yuris:header)
                                                 data=packed:...pac\bn.ypf (1099291835 bytes, packs=7 updates=2 skipped=4)
                                                 frame: 1 viewport: 1152x720(首帧可绘制)
yu-cli run <project-eris>                      → script key: D3 6F AC 96 (source=rx-yuris:header),packs=1,正常出帧
```

### 残余

- 存档仍被 `YSSD == 0x1E0` 等值门拒(本作 0x1E1),读档不可用。
- `YSTD == 495` 等值门未改(本作 481;本作 boot 路径未触发该表,留待统一处理)。
- 包内嵌 exe overlay、WZ 加密 YPF 未实现(缺样本 / 仅工具级证据)。

## 成果 YSV-STR-INIT-EVAL (2026-10-30) · ysv 串初值按官方语义求值,消除字节码拼进资源路径

### 问题

0.481 商业包引导期出现 5 条 `WARN [vfs] vfs.path @ V $w B &, B )…+0x0:
NUL byte in lookup key`,前缀是一段 YSTB 表达式字节码,后缀是
`cg/face/face_back_*.png`;声音侧同源(`se/` + 字节码 + `sys001`)。

定位:`ysv.ybn` 里 `slot=1412/1413`(`kind=2`,脚本 0 局部串)的初值不是
字面量,而是表达式 `$1143[38,21]` / `$1143[38,6]`;内核只认「单条 `0x4d`
字面量」,其余形态原样存字节,于是**字节码本身成了变量值**,被脚本
(`es_button.yst` s0 pc1124/1125 `LET $1351[$1382][i] = $1412 + $55[i]`)
拼进资源名。

### 证据(官方实现为准)

0.481 引擎 `FUN_00456e50`(`0x456e50`,调用点 `0x456f61`)应用 ysv 记录时:
数值初值走 `FUN_004658e0`/`FUN_00465800` 直写,串初值走
`(*DAT_008097e0)(buf, bytes, len)` 再 `FUN_00465654(slot, buf, len)` ——
**串初值过窗口求值器**(该指针静态镜像为 0,运行期赋值;全模块 9 处引用
都是 `CALL dword ptr [0x008097e0]`,参数形态统一为「缓冲/参数字节/长度」,
与命令参数同一入口)。`S_STR` 补不上这个缺口:0.495 分发表把它映射到
`0x4634d8`(`xor eax,eax; ret`)。

### 改动

- `crates/yu-rt/src/tables.rs`:`VarRecord::str_init_needs_eval()` ——
  空串初值 / 单条 `0x4d` / **解不成合法表达式**的字节都不求值(后者维持
  既有「原样字节」口径,不猜);其余走求值。
- `crates/yu-rt/src/vm.rs`:`ysv_init_value()` + `eval_init_expr()`,
  `apply_global_initial_values` / `build_globals_restore` /
  `apply_script_locals` 三处统一;求值失败留痕后按空串建槽,不再传播
  字节码。结果按 `ParamTy::Str.coerce` 归一(与参数槽同口径)。
- `crates/yu-rt/tests/fixture.rs`:新增回归
  `ysv_str_init_expression_is_evaluated_not_stored_raw`(全局/局部/数组元素三路)。
- `docs/divergences/DIV-RT-055-ysv-str-init-expression.md`:完整取证档案。

### 验证(原始结果)

```text
cargo build --workspace --all-targets                              → ok
cargo test --release -p yu-rt --test fixture ysv_str_init_expression
  → test ysv_str_init_expression_is_evaluated_not_stored_raw ... ok
cargo test --release -p yu-rt --test fixture                       → 100 passed
cargo test --release -p yu-rt -p yu-format -p yu-engine            → 全过

yu-cli run <0.481 商业包根> --frames 600 --log-level info
  修复前:vfs.path ×5(NUL byte in lookup key)
  修复后:vfs.path ×0,诊断汇总无新增条目
  probe_asset se/sys001.ogg → HIT 5097 bytes(声音路径纠正为 se/sys001)
  帧像素 sha256 未变(那些 CG 名在本包里本就不存在,属合法缺失)
```

### 残余

- `crates/yu-trace/tests/golden/synthetic_frame.jsonl` 过期(夹具 ysc/ycd
  字节自 `a117541` 之后被 `dfa7b0b` 改过,只有这两个夹具产物的 sha256/长度
  不一致,事件流一致)。重录 golden 属独立 `test` 改动,待主人确认。
- 官方 ysv 的**可见性掩码**(`in_EDX`/`in_ECX`/栈参,按 `flag`(恢复组)
  × 值类型过滤)内核仍按「全部装载」处理;本作无观测差异,留档待取证。

## 成果 SYSVAR-WINDOW-EXEC-MIN (2026-10-30) · `_WINDOW_EXEC_MIN` 按官方实现,消除每帧未实现挂号

### 问题

0.481 商业包运行期持续刷
`UNIMP [vm] rt.sysvar.unimplemented @ es_scenario.yst pc246: _WINDOW_EXEC_MIN`,
内核按「工程 ycd 声明但手册无页」一律挂号(DIV-RT-007 旧门禁)。
按 2026-10-24 政策(AU5+):**手册缺页 ≠ 不实现**,REA 证据足够即落地。

### 证据(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`)

- int getter `FUN_00427350`(跳表基址 `0x58c8a0`,548 项)case 117/118/119/120
  分别指向 `MOVSX EBP,byte ptr [0x801918]` / `[0x80191c]` / `[0x801920]` /
  `[0x801924]` —— MAX / **MIN** / CLOSE / CAPTION 是**连续四字节全局**,
  四条都是 1 字节有符号读。
- 写分派 case 118 → `0x446fa4` `MOV byte ptr [0x80191c],DL`(脚本可赋值)。
- 窗口过程 `FUN_0045acdc`:`WM_SYSCOMMAND & 0xFFF0` 的
  `SC_MINIMIZE(0xF020)` → `DAT_0080191c = (char)*ecx + 1`(槽号+1);
  `SC_MAXIMIZE(0xF030)` / `SC_CLOSE(0xF060)` 同族写另两个。
- 结论:MIN = 最小化ボタン押下状態,与 MAX 同型同宽。

### 改动

- `crates/yu-rt/src/sysvar.rs`:`_WINDOW_EXEC_MIN` 并入读分支(证据锚点写进注释)。
- `crates/yu-rt/src/vm/handlers.rs`:写分派并入(官方 `MOV byte` 存在)。
- `crates/yu-rt/src/vm.rs`:新增 `window_exec_min` 字段与 `MIN` 分支;
  四个状态统一 `i8` 截断 + 符号扩展(官方存储宽度 = 1 字节);新增
  `Vm::request_window_minimize`。
- `crates/yu-engine/src/engine.rs`:`Engine::request_window_minimize` 透出。
- `crates/yu-rt/tests/fixture.rs`:回归
  `window_exec_min_is_a_host_setter_round_trip`(宿主置 1 → 脚本读 1 → 写回 0)。
- 台账:`crates/yu-rt/docs/command-provenance.md` 新增 MIN 行;
  `crates/yu-rt/docs/divergences/README.md` DIV-RT-007 增记 2026-10-30。

### 验证(原始结果)

```text
cargo build --workspace --all-targets                    → ok,零 warning
cargo test --release -p yu-rt --test fixture window_exec_min
  → window_exec_min_is_a_host_setter_round_trip ... ok
cargo test --release -p yu-rt -p yu-engine               → 全过(102 + …)

yu-cli run <0.481 商业包根> --frames 1800 --log-level info
  修复前:rt.sysvar.unimplemented ×5(含 _WINDOW_EXEC_MIN 每帧刷)
  修复后:×4 —— 仅 _SAVE_PATH_TYPE×3(es_first.yst pc6/9/12)与 _RET×1
```

### 残余(取证已推进,待实现)

- `_SAVE_PATH_TYPE`(id 112):全局 `[0x801324]`;写侧 `FUN_00460c80` 从
  **`yssfs.dat`**(`FUN_00469df2` 键控记录文件,4 字节 key、0x20 dword
  记录,取 `puVar2[0x15]` = 偏移 0x54,值 0..3)读出后组路径:默认 `save\`;
  1/2/3 前置 `%APPDATA%` 类前缀 + `\<name>_SaveData\<name>\save\`,
  3 再插 `\My Games`。缺 `yssfs.dat` 容器格式 ⇒ 仍 `Unimplemented`
  (现行口径 = 宿主注入 `$_SAVE_PATH`)。
- `_RET`(id 49):getter 命中 `MOV EDX,2` / `MOV EBP,0x3e7` 常量块,
  **不读任何全局** ⇒ 不是状态变量,待语料语义定案后再实现。

## 成果 CGINFO-COLOR (2026-10-30) · `CGINFO[COLOR]` 按官方语义实现,按钮层尺寸回退补齐

### 问题

0.481 商业包引导期刷 7 条
`UNIMP [gfx] gfx.info.unsupported … CGINFO[COLOR] 需要像素/未分类语义,未取证`
(`es_button.yst` s0 pc2023/2087/2143/2187/2243/2273)。

### 证据

- 语料自证(`es_button.yst` s0 pc2024):`CG[ID]` 建层后依次
  `CGINFO[SX]/[SY]/[COLOR SET=0 SET2=0]`,再判
  `@SX==1 && @SY==1 && @color == 0x00808080` → `CGACT[RECTPAINT3]`。
  `0x00808080` = `128|128<<8|128<<16`(50% 灰)⇒ COLOR 返回
  `r | g<<8 | b<<16`,与 `ONMOUSE` 同打包。
- 引擎(0.495 `yu-ris.exe` sha256 `4f5eefa3…`):像素采样体
  `0x43ddc0..0x43de86` 的坐标对由全局 `[0x68417d]` 在「鼠标」与
  「显式 `SET/SET2`」之间切换,显式分支缺省 (0,0),越界 `-1`。

### 改动

- `crates/yu-gfx/src/state.rs`:`CGINFO` 增 `COLOR` 分支(复用
  `query_onmouse`,坐标取 `SET/SET2`,缺省 0,0);`query_onmouse` 的
  尺寸优先级补「VM 回填的资产尺寸」,并在无表面时按 0×0 返回 `-1`
  而不是报错(官方装载失败后矩形保持 0)。
- `crates/yu-rt/src/vm/handlers.rs`:`CGINFO` 资产元数据回退从
  `SX/SY` 扩到 `SX/SY/COLOR/ONMOUSE*`;像素查询在尺寸回填后重试原查询;
  资产**不存在**时按 `0×0` 回填。
- `crates/yu-rt/src/vm/handlers.rs` / `crates/yu-gfx/src/state.rs`:
  资产尺寸诊断补上具体键名(如 `cg/face/face_back`),便于定位。
- `crates/yu-save/src/state.rs`:存档写失败诊断补**解析后的绝对路径**。
- `crates/yu-gfx/tests/info_query.rs`:回归
  `cginfo_color_samples_pixel_at_set_coordinates`。

### 验证(原始结果)

```text
cargo build --workspace --all-targets   → ok,零 warning
cargo test -p yu-gfx --test info_query  → 8 passed
cargo test -p yu-cli --test logging_guards → 3 passed

yu-cli run <0.481 商业包根> --frames 1800 --log-level info
  gfx.info.unsupported : 51 → 0
  gfx.info.asset_size  : 15 → 0
  帧像素 sha256        : ad3cff2b…(未变)
  其余诊断:sv.file.write×4 / rt.sysvar.unimplemented×4 /
           snd.file.missing×1 / gfx.window.unsupported×1 /
           gfx.texture.missing×1
```

### 附带结论(非内核缺陷)

- **`sv.file.write` 拒绝访问(os error 5)**:不是内核问题,也不是游戏目录的
  权限问题 —— 是**按 exe 所在路径生效的文件系统沙箱**。对照矩阵(同一份
  二进制字节、同一令牌、同一目标 `D:\Download`):

  | exe 位置 | 写 `D:\Download` | 写 `C:\Users\…\Temp` | 写 `D:\Art\ai\projects\…` |
  |---|---|---|---|
  | `<repo>/target/release/yu-cli.exe` | 拒绝 | 拒绝 | 成功 |
  | `D:\Art\ai\projects\YurisDroid\…\wp.exe` | 拒绝 | — | 成功 |
  | `D:\Art\wp3.exe` | 成功 | — | — |
  | `D:\wp4.exe` | 成功 | — | — |
  | `C:\Users\…\Temp\wp2.exe` | 成功 | — | — |

  即:**exe 位于 `D:\Art\ai\projects\` 之下时,进程只能写该子树之内**;
  写到外面任何位置都是 ACCESS_DENIED。同一时刻 `cmd.exe`/PowerShell/Python
  都能写 `D:\Download`(ACL 为 `Authenticated Users:(M)`,令牌也带该组),
  所以目标目录本身可写。验证:把 `yu-cli.exe` 复制到
  `C:\Users\…\Temp\` 后跑同一命令,4 条 `sv.file.write` 全部消失,
  `save/config.sd`(7054B)/`extra.sd`(6542B)正常落盘;`--save-root` 指到
  工程子树内同样 5 个 `.sd` 全写成功 ⇒ 写入链路正确。规避方式:从普通终端
  运行、或把 exe 放到项目树外、或 `--save-root` 指向可写目录。
- **`snd.file.missing voice/GLO_M01_0458`**:该资产在本安装的任何包里都
  不存在。全包 5852 条索引里没有任何 `*_M01_*` 语音;语音命名是
  `voice\mit_n01_*.ogg` / `voice\sin_n01_*.ogg` / `voice\mob_n01_*.ogg`
  (同号 `0458` 存在为 `mit_n01_0458`/`sin_n01_0458`)。查找链路本身正确
  (小写归一 + `.ogg/.wav` 探测,REA `0x40c9b4`),属数据侧缺文件。

## 成果 SYSVAR-SAVE-PATH-RET (2026-10-30) · `_SAVE_PATH_TYPE` 与 `_RET` 收口,系统变量未实现挂号归零

### 问题

商业包 1,800 帧运行剩 4 条 `rt.sysvar.unimplemented`:`_SAVE_PATH_TYPE` ×3
(`es_first.yst` pc6/9/12)+ `_RET` ×1(`puserdesign` pc4)。两者都因「工程 ycd
声明、手册无页」被旧门禁一律挂号。按 2026-10-24 政策(手册缺页 ≠ 不实现),
取证后落地。

### 证据(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`)

- **`_SAVE_PATH_TYPE`(id 112)**:getter `FUN_00427350` case 112 =
  `MOVSX EBP, byte ptr [0x801324]`。写侧 `FUN_00460c80` 打开
  `<app>\yssfs.dat`(`FUN_00469df2` → `FUN_00469dc1(path, mode, 0x40)`),
  用 `FUN_00469ebe(rec, 4, 0x20, h)` 读 0x20 个 u32;该函数 = 加锁 +
  `FUN_00469eed`(**标准 CRT 缓冲 `fread`**:`FUN_0046950e` = `_read`、
  `FUN_004705f9` = `_filbuf`)+ 解锁 ⇒ **明文,无解密**。取 `rec[0x15]` =
  字节偏移 0x54,只有 0..=3 写全局,其余保持初值 0。
- **`_RET`(id 49)**:getter 表项 `0x4279dd`,实际体 `0x4279de` =
  `MOV EBP, 0x3E7` ⇒ 恒 999,不读任何全局。

### 改动

- `crates/yu-format/src/yssfs.rs`(新):`yssfs.dat` 定长记录(0x80 字节 /
  0x20 个 u32)、`SAVE_PATH_TYPE_OFFSET = 0x54`、`save_path_type()`
  (只接受 0..=3,缺失/超域 ⇒ 0);两条单测。
- `crates/yu-rt/src/vm.rs`:`BootConfig::save_path_type` + `Vm` 字段与访问器。
- `crates/yu-rt/src/sysvar.rs`:`_SAVE_PATH_TYPE` 读注入值;`_RET` 返回 999
  (证据与「哨兵」推断写进注释,不按语料猜语义)。
- `crates/yu-engine/src/engine.rs`:启动读工程根/辅助根的 `yssfs.dat` 注入。
- `crates/yu-trace/src/record.rs`:回放侧显式写 0 并标注「未进 trace 头」。
- `crates/yu-rt/tests/fixture.rs`:回归
  `save_path_type_and_ret_are_injected_not_unimplemented`。
- 台账:`docs/game-settings-ledger.md` 新增 §3.1 `yssfs.dat`;
  `crates/yu-rt/docs/command-provenance.md` 补两行;DIV-RT-007 增记 2026-10-30。

### 验证(原始结果)

```text
cargo build --workspace --all-targets  → 0 error / 0 warning
cargo test --release -p yu-format --lib yssfs   → 2 passed
cargo test --release -p yu-rt --test fixture save_path_type_and_ret → ok

yu-cli run <0.481 商业包根> --frames 1800 --log-level info
  rt.sysvar.unimplemented : 4 → 0
  帧像素 sha256           : ad3cff2b…(未变;yssfs.dat 全零 ⇒ type 0,
                            与修复前「三条件全假」等价)
yu-cli run <project-eris> --frames 200
  rt.sysvar.unimplemented : 0 命中(仅剩 EMOTE 的 deferred(master) 与
                            FILEACT[DELETE] 的 INFO)
```

### 残余(取证已定边界)

- getter 跳表 548 项只有约 68 个不同目标:约 480 个 id 共享 4 个**族块**
  (`0x42755f`×129 / `0x4274b0`×128 / `0x42740d`×128 / `0x427c8a`×95),
  即 `_SERIAL_*`(16..47)、`_;M*`(71..102)这类**有真实行为**(族块内做
  `[0x68996d]` 索引 + 相邻双数组取值),不是「官方不支持」。按「无证据不猜」
  维持 `Unimplemented`,但台账已不再把它们归类为「官方也不支持」。
- `yssfs.dat` 非零配置的实机样本未取得(需要跑 32 位 `セーブファイル設定.exe`),
  路径阶梯的前缀常量(`0x681c60`/`0x6825c0`)字面值也未取。

## 成果 YSSD-SNAPPY-ENCODER (2026-10-30) · 存档写回按引擎代号选压缩层,新增 Snappy 编码器

### 问题

内核的存档写入端一直用 zlib level 1(0.495 SDK 形态),而这一代商业引擎
(0.481,随包带 `YSSnp.dll`)的存档块是 **Google Snappy** —— 我们写出的
`.sd` 与官方同代产物不同层。同时普查发现玩家存档的 `DNO=1024` 是**嵌套
容器槽**,通用解压路径会报 `fmt.yssd.codec`。

### 证据

- 压缩层:24 个商业包存档 / 3568 块逐块解压,单块与多子块全是 Snappy
  (块首 varint32 = 块头 `ulen`,如 `94 e4 32` → 832020);
  `YSSnp.dll` 导出 `YSSnp_Compress`/`YSSnp_Uncompress`/
  `YSSnp_CompressedLength`/`YSSnp_UncompressedLength`。
- 嵌套槽:`DNO=1024` 载荷 = `u32 inner_len | u32 len_b | zlib(inner_len)`,
  zlib 输出长度逐文件等于 `inner_len`(55376..57369),内容以 `YSSD` 开头、
  版本 `0x1E1`、195 块 —— 完整二级容器(18/18 文件一致)。
  其后的第二段负载不是 zlib,`len_b` 语义未定性。

### 改动

- `crates/yu-format/src/snappy.rs`:新增 `compress()`(varint32 + 字面量 /
  三类复制标签;偏移按大小选 1/2/4 字节形态;字面量按 60 字节切块)。
  不追求逐字节复刻 Google 的启发式 —— Snappy 是格式,不是规范化编码。
- `crates/yu-format/src/yssd.rs`:`BlockCodec{Zlib,Snappy}` + `detect()`
  (按 RFC1950 头判定)+ `encode_block_body_with` / `encode_multi_body_with`;
  原 `encode_block_body`/`encode_multi_body` 保留为 zlib 包装(0.495 逐字节
  回归不受影响)。模块文档补三种块形态与压缩层选择。
- `crates/yu-save/src/state.rs`:从盘上存档学习压缩层 + `set_block_codec()`
  显式覆盖;`flush` 按学习/显式值编码。
- `crates/yu-engine/src/engine.rs`:`detect_block_codec()` —— 引擎根有
  `YSSnp.dll` ⇒ Snappy,否则 zlib(引擎代号信号,优先于盘上学习)。
- `crates/yu-format/tests/yssd_roundtrip.rs`:新增
  `commercial_save_blocks_recompress_as_snappy`(三种块形态全覆盖)。
- `docs/divergences/DIV-RT-056-save-nested-dno1024.md`:嵌套槽档案。

### 验证(原始结果)

```text
cargo build --workspace --all-targets   → 0 error / 0 warning
cargo test -p yu-format --lib snappy    → 5 passed(含压缩器往返)
cargo test -p yu-format --test yssd_roundtrip commercial_save_blocks
  → 24 文件 / 单块 3511(Snappy 3476 + zlib 35)/ 多块 57 / 嵌套 18 全部往返一致

yu-cli run <0.481 商业包根> --frames 1800 --save-root <可写目录>
  写出的 config.sd  DNO=1: clen=24985 ulen=524308 head=94 80 20(旧: 78 01)
  写出的 kidoku.sd  DNO=1: clen=39025 ulen=832020 head=94 e4 32
                          (官方同块 clen=39327 —— 同格式、压缩率略优)
  写出的 voice.sd   8 块(此前该文件因写失败不存在)
```

### 残余

- `DNO=1024` 的第二段负载(`len_b` 指向的字节段)语义未定性;疑为存档槽
  缩略图/元数据,需要官方读档界面对该槽的消费路径才能定案(DIV-RT-056)。
- 我们的 Snappy 输出与官方**不是逐字节一致**(压缩率略优 0.8%):格式合法、
  任何合规解码器可读;若要逐字节复刻需照搬 Google 的哈希/跳步启发式,
  目前无互操作必要。

## 成果 CG-MODE-NOCASE (2026-10-30) · MODE 4/8/10..18 定案:官方固定管线无 case,按通常路径对齐

### 问题

DIV-GFX-002 长期挂 `unimplemented`:手册 `cg/cg.html` 列了 MODE
4(除算)/8(差分)/10..18(Overlay..PinLight),但内核找不到对应的着色器
本体,于是后端把这 11 种模式的图层**整层跳过**。跳过意味着画面缺层 ——
比"用错的公式画"更糟。

### 证据(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`,逐条反汇编)

- **状态分派链**(`FUN_00414ba4`,`0x4155dd` 起):
  `MOV EAX,[EBP+0x18]`(MODE)→ `TEST EAX,EAX; JZ`(MODE 0 默认块)→
  `CMP EAX,1/2/3/5/6/7/9` 依次 `JE` 到各模式块;链尾
  `JMP 0x4158c8` 附近的通用路径 —— **4/8/10..18 没有任何 case**,
  且该跳转**跳过 MODE 0 的状态块**(`0x41563d..0x41587d` 那段,
  写 SRCALPHA/INVSRCALPHA/ADD)。
- **TFACTOR 选择链**(`0x414d05` 起):`MOV EDX,[EBP+0x18]`;
  `TEST EDX,EDX; JNZ` → 比较链只认 `{1,2,3,5,6,7,9,100,98}`(命中则
  TFACTOR = A 灰度);其余值 `MOV EDX,0xFF000000`。
- 结论:手册里的 Photoshop 名只是**文档**;引擎既不实现公式,也不给这些
  值设状态 —— 它们实际继承上一层留下的 D3D 状态(顺序相关)。
- 语料侧:商业包与 ERIS 两套语料 `CG[MODE=]` 只出现 0/2/3/5(其余为
  表达式形态),零处使用 4/8/10..18 —— 这条修复不影响现有画面。

### 改动

- `crates/yu-gfx/src/layer.rs`:新增 `BlendMode::engine_effective()`
  (11 个无 case 的值 → `Normal`);`fixed_state()` 先归约,`confidence()`
  从 `Unimplemented` 改 `Likely`,`needs_shader()` 恒 `false`(那 4 个
  ps_2_0 blob 是转场/缩放 pass,不是混合模式)。
- `crates/yu-render/{soft,wgpu}`:`supported_blend()` 改为
  `Some(mode.engine_effective())`,不再有"整层跳过"分支;
  soft 的 `stage_reduce` / `blend_pixel_with_stage` 入口也归约,保证
  手写公式与状态表机械一致。
- 测试:`blend_table` 三条断言改为新不变量(19 种模式都有状态表;8 个
  原生 case + 11 个归约);`parity_fixes` 的 `mode_update_*` 改为断言
  MODE=4 仍可执行、层保留原始 MODE、渲染期归约。
- 台账:`docs/gfx-unimplemented-todo.md`、`crates/yu-gfx/docs/divergences/README.md`
  的 DIV-GFX-002 改 `fixed(no-manual)`;`docs/render-parity.md` §5.3/§5.4
  补完整反汇编依据。

### 验证(原始结果)

```text
cargo build --workspace --all-targets   → 0 error / 0 warning
cargo test -p yu-gfx --test blend_table → 10 passed
cargo test -p yu-gfx --test parity_fixes → 27 passed
cargo test -p yu-backend-soft --lib     → 41 passed
cargo test --workspace --release        → 仅 yu-trace 过期 golden(既有)
```

### 残余

- 官方对无 case 模式的**真实像素**取决于上一层遗留状态(顺序相关),
  不可复现;内核选择"通常路径"作为确定性对齐,已记入
  `docs/render-parity.md` §5.3。若将来出现使用这些模式的语料,需按官方
  逐帧截图确认该近似是否可接受。

## 成果 WINDOW-MINMAX (2026-10-30) · `WINDOW[MAXMODE/MINMODE]` 归类落地,商业包 1800 帧 UNIMP 归零

### 问题

0.481 商业包 1800 帧只剩 1 条未实现诊断:

```text
UNIMP [gfx] gfx.window.unsupported @ s9 pc99: WINDOW: 窗口参数未分类: MINMODE
```

脚本侧对应 `es_first.yst:169` `WINDOW[NO=0 MINMODE=0]`(首屏关掉最小化
按钮),`es_first.yst:206` 同族 `WINDOW[NO=0 CLOSEMODE=…]`。

### 取证

1. **参数表(工程自证,真值层级 3)**:用新增探针
   `cargo run -p yu-engine --example probe_cmd_params -- <root> WINDOW WINDOWINFO`
   从两个样本工程的 `ysc.ybn` 直接读出声明序 —— 两者一致:
   - `WINDOW`:… 14 `MINIMIZE` / 15 `MAXIMIZE` / **16 `MAXMODE` / 17 `MINMODE`**
     / 18 `CLOSEMODE` / 19 `FLASH` / 20 `FLASHTIME` / 25 `RESIZE`;
   - `WINDOWINFO`:… 17 `MINIMIZE` / **18 `MAXMODE` / 19 `MINMODE`** / 20 `CLOSEMODE`。
   读写成对 ⇒ 每窗口**持久状态**,不是瞬时动作。
2. **官方落点(REA 0.495 `FUN_00457d8c`,sha256 `4f5eefa3…`)**:
   创建窗口时按状态算 `dwStyle` —— 基样式 `0x02CB0000`
   (= `WS_CLIPCHILDREN|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX|WS_MAXIMIZEBOX`),
   非主窗口改 `0x02C80000`(去掉 `0x00010000` = `WS_MAXIMIZEBOX`);
   `DAT_008015e4`(RESIZE)额外加 `0x00040000` = `WS_THICKFRAME`。
   即 MINMODE/MAXMODE 与 RESIZE 同族,是**窗口镶边开关**,由窗口创建
   路径消费,不是渲染状态。

### 改动

- `crates/yu-gfx/src/frame.rs`:`WindowState` 增 `max_mode`/`min_mode`
  (缺省 1 = 主窗口创建样式里两个按钮位都在)。
- `crates/yu-gfx/src/state.rs`:`apply_window` 接受 `MAXMODE`/`MINMODE`
  并写入窗口状态(不再进"未分类");`query_windowinfo` 增两条读回。
- `crates/yu-gfx/tests/support/mod.rs`:测试用 `WINDOW`/`WINDOWINFO` 参数表
  改为**真实工程槽序**(补 `WSX/WSY/MAXMODE/MINMODE/FLASH/FLASHTIME/RESIZE`,
  `WINDOWINFO` 的 `FULLSCREENBUTTON` 更正为工程实际名 `FULLSCREEN2`)。
- 新用例 `window_min_max_mode_round_trip_through_windowinfo`:缺省读 1 →
  `WINDOW[MINMODE=0 MAXMODE=0]` → 读回 0,且不产生"窗口参数未分类"。
- 台账:`docs/gfx-unimplemented-todo.md` 与
  `crates/yu-gfx/docs/divergences/README.md` 的 DIV-GFX-037 改
  `partial(no-manual)`,写明读写成对证据与创建样式锚点。

### 验证(原始结果)

```text
cargo build --workspace --all-targets          → 0 error / 0 warning
cargo test -p yu-gfx --test info_query         → 9 passed
cargo test --workspace                         → 仅 yu-trace 过期 golden(既有,夹具字节在 dfa7b0b 改过)
yu-cli run <0.481 商业包根> --frames 1800      → UNIMP 1 → 0
yu-cli run <project-eris> --frames 200         → UNIMP 1(仅 EMOTE,deferred(master))
```

### 残余

- `FLASH`/`FLASHTIME`(槽 19/20)仍未归类:语义应是任务栏闪烁
  (`FlashWindowEx` 的 `dwFlags`/`dwTimeout`),但两个语料零使用、无 REA
  读取点 ⇒ 维持不实现,等出现语料再取证。
- 写侧仍是 host 边界:内核不持有 Win32 窗口,`MINMODE=0` 只改状态,
  不实际移除桌面窗口的最小化按钮(DIV-GFX-037)。

## 成果 WINDOW-MANUAL (2026-10-30) · WINDOW 三字段按手册落地 + 语料普查工具 + DIV-GFX-013 台账校正

### 问题

上一轮把 `MAXMODE/MINMODE` 归类后,`WINDOW` 里仍有 5 个参数被静默丢弃
(`MINIMIZE/CLOSEMODE/ACTIVE/WSX/WSY`)。翻 0.495 手册才发现其中三个
**本来就有页**,不该按"未取证"处理。

### 取证(手册原文,`マニュアル/YU-RIS/html/window/`)

`window.html` 关键词表只列 18 个:`NO/CAPTION/A/E/X/Y/Z/SX/SY/ACTIVE/
FULLSCREEN/FULLSCREENBUTTON/MINIMIZE/CLOSEMODE/CGID/MENUID/REGIONID/DRAGID`
—— **`WSX/WSY/MAXIMIZE/MAXMODE/MINMODE/FLASH/FLASHTIME/RESIZE` 都不在页内**。

- `MINIMIZE = 値(0/1)ウィンドウの最小化(=1)／通常化(=0)を切り替えます`
  ⇒ **切换动作**,不是按钮开关(开关是 `MINMODE`)。
- `CLOSEMODE = 値(0～2)閉じるボタンを押したときの挙動を設定`
- `ACTIVE = 値(0/1)ウィンドウをアクティブにする場合に１を設定`,
  例 4 注:`ACTIVE=0` としても非アクテイブには出来ません` ⇒ 0 是无副作用值。
- `FULLSCREENBUTTON = 値(0/1)最大化ボタンの有効(=1)／無効(=0)を設定`
  —— 手册用这个名字,0.495 工程 `ysc.ybn` 的 `WINDOWINFO` 叫 `FULLSCREEN2`。

`windowinfo.html` 读回页同样列 `MINIMIZE`(是否最小化中)与 `CLOSEMODE`
(关闭按钮行为),`WSX/WSY/MAXMODE/MINMODE/DISPLAYNO/Z/A` 无页。

### 语料普查(新探针,静态口径)

新增 `crates/yu-engine/examples/census_params.rs`
(`census_params <root> <命令名[:参数名]>…`):遍历工程全部脚本,按
**参数声明槽名**统计命令的参数使用次数并打印 `s/pc/line` 位置。这是
"某个无页参数到底有没有人用"的第一手依据。

```text
商业包 222 脚本 / ERIS 141 脚本
WINDOW(ERIS):  NO 31, CAPTION 6, FULLSCREEN 5, X/Y/Z 5, CLOSEMODE 4,
               CGID 3, SX/SY 3, WSX/WSY 3, ACTIVE 2, MAXIMIZE 2,
               MINIMIZE 2, RESIZE 2, E 1
CGACT(商业包 491 组 / ERIS 274 组):见 DIV-GFX-013 行
```

`MINIMIZE/MAXIMIZE` 的调用点形如 `WINDOW[NO=0 MINIMIZE=…]`(单参数、无
伴随状态)⇒ 动作语义,与手册一致;`s12 pc629` 先 `WSX/WSY` 再 `MAXIMIZE`。

### 改动

- `crates/yu-gfx/src/frame.rs`:`WindowState` 增 `close_mode`(0..2);
  `minimized` 文档改为"写侧 = `WINDOW[MINIMIZE]` 手册切换动作"。
- `crates/yu-gfx/src/state.rs`:
  - `apply_window`:`MINIMIZE` → `minimized`、`CLOSEMODE` → `close_mode`、
    `ACTIVE` → 文档化无副作用、`WSX/WSY` → `outer_size`(成对校验,只给
    一个仍报"部分更新未取证")。
  - `query_windowinfo`:增 `CLOSEMODE`;`FULLSCREENBUTTON` 与
    `FULLSCREEN2` 互为别名。
- 新用例 `window_minimize_close_mode_follow_manual`(手册三条)+
  `window_wsx_wsy_sets_outer_size_read_back_by_windowinfo`。
- 台账:`DIV-GFX-071`(新,`fixed(手册有页)`)记 MINIMIZE/CLOSEMODE/ACTIVE/
  WSX-WSY 的读写配对;`DIV-GFX-013` 按普查重写(soft 全量实现、wgpu 缺
  `BLUR*/DIFFUSION/WHIRLPOOL/RASTER*`、`TEXCOPY` 手册无页且 ERIS 三处
  全为 `ID==ID2` 自复制)。

### 验证(原始结果)

```text
cargo build --workspace --all-targets        → 0 error / 0 warning
cargo test -p yu-gfx --test info_query       → 11 passed
cargo test --workspace                       → 仅 yu-trace 过期 golden(既有)
yu-cli run <0.481 商业包根> --frames 1800    → UNIMP 0(WARN/ERR 0)
yu-cli run <project-eris> --frames 200       → UNIMP 1(仅 EMOTE,deferred(master))
soft vs wgpu 300 帧商业包 --shot            → PNG sha256 完全一致
```

### 残余

- `MAXIMIZE`(ERIS 2 处)是"立刻最大化"的窗口动作,手册无页;内核只按
  状态记录,`ShowWindow(SW_MAXIMIZE)` 交宿主(DIV-GFX-037)。
- wgpu 的滤镜族(`BLUR*`/`DIFFUSION`/`WHIRLPOOL`/`RASTER*`)仍无 GPU 路径,
  按 `docs/architecture/backends.md` §5.4 应补 `gpu.cpu_fallback.<action>`
  标注或 shader;1800 帧内未被触达,两端截图目前逐字节一致。
- `TEXCOPY/TEXSSCOPY` 手册无页、无 REA 读取点定位(0.481 侧锚点在本构建
  不落在已分析函数内),维持 `gfx.copy.texcopy.unsupported`。

## 勘误 MODE-CLASS (2026-10-30) · DIV-GFX-002 分类由 `fixed(no-manual)` 更正为 `unsupported-by-official`

前一条成果(CG-MODE-NOCASE)把 MODE 4/8/10..18 记成 `fixed(no-manual)`。
按 AGENTS.md §0 的五种终态,这是错的:**手册 `cg/cg.html` 有页**(列了
`MODE=値(0～18)` 与 19 个名字),所以不属于"手册缺页";官方引擎又没有实现,
所以也不是 `fixed`。正确分类 = `unsupported-by-official`。

同时把证据收紧(读全了 `FUN_00414ba4` 的函数体,不再只依赖早期分派链):

- 该函数按 MODE(第 5 个栈参数)走 if/else 链,分支只有 `0/1/2/3/5/6/7/9`;
  其余 11 个值落进最后的 `else`,**只拷局部变量,一条
  `SetTextureStageState`/`SetRenderState` 都不设**。之前写的"跳过 MODE 0
  的状态块、落进通用路径"是错的 —— 什么都不设。
- 手册的 19 个名字属**脚本语言参数域**(编译器接受该值),同页自带注释
  `※覆い焼きリニア＝加算モード` 说明名字之间存在同义关系。
- exe 里的 ps_2_0 blob 按 D3DX 编译器签名数有 **5 处**(此前台账写 4 个),
  已命名 3 个:`YS_ShaderTest1`/`YS_TexDiffU`/`YS_TexSize`,均为转场/缩放/
  测试 pass,没有一个对应这 11 种混合模式。

配套代码改动(不改可见行为):

- `BlendConfidence` 新增 `Unsupported` 变体(与 `Likely` 区分:后者是"照官方
  状态画、差像素对拍",这里是"官方没有状态可照");11 个模式改标它,
  `state.rs` 不挂诊断、层仍可执行。
- 删掉 `needs_shader()` 里的 `let _ = self;` 假实现与 `_unused_shader_modes`
  死函数(只为兼容 `json.rs` 的 `"shader"/"fixed"` 字段保留方法本体)。
- 修正 `engine_effective()` / `fixed_state()` 的文档错位(`fixed_state` 的
  旧文档被顶到 `engine_effective` 上)。

验证:`cargo build --workspace --all-targets` 0 warning;
`cargo test -p yu-gfx` 全绿(blend_table 10 / parity_fixes 27 / info_query 11);
商业包 1800 帧 UNIMP 0;ERIS 200 帧仅 EMOTE(deferred);
soft vs wgpu 300 帧 PNG sha256 与改动前完全一致(`6b2fd875…`)。
