# 任务书 B14 · YMV 影片图层(路线图 B14,媒体宿主)

> 状态:**完成(2026-11-03)**。`YMV`/`YMVINFO`/`YMVEND` 落入 `fixed(no-manual)`
> (手册无 YMV 页;证据 = 0.495 官方处理体 + 官方 SDK 样例 + ERIS 演示脚本)。
> 台账已划:coverage-matrix 三行、DIV-RT-045、domain-event-contracts §9、
> DIV-GFX-027 勘误、新增 `docs/divergences/DIV-GFX-073-ymv-movie-layer.md`、
> todo-inventory §3.2、game-settings-ledger 110 行、roadmap B14、PROGRESS 成果。
>
> 验证(原始结果):
> `cargo build --workspace --all-targets` 0 warning;
> `yu-format::ymv` 6 通过(含官方三份样例)、`yu-gfx --test ymv_movie` 7 通过、
> `yu-rt --test fixture ymv` 1 通过、`yu-render-core --lib movie` 2 通过、
> `yu-backend-soft --test ymv_movie` 1 通过(真实 `sample2.ymv` 两帧像素差异
> >1000)、`yu-cli --test logging_guards` 3 通过(顺带修复存量未标注
> `unwrap_or` 8 处)。全量 106 个测试二进制通过;两条既有红与本次改动无关:
> wgpu `diffusion_permutes…` 偶发(GPU 读回,单跑通过)、
> `yu-host-android runs_real_corpus…`(本机无音频端点,HEAD worktree 同样失败)。
>
> 剩余:`MOVIE`(MPEG/AVI 全屏宿主,`0x44b134`)不在本批范围。
>
> 仓库:`<repo>`。动手前读:`AGENTS.md`、本文件、`docs/tasks/roadmap-batches.md`
> §B14、`crates/yu-rt/docs/domain-event-contracts.md` §9、
> `crates/yu-rt/docs/divergences/README.md` DIV-RT-045。

## 0. 结论先行(取证推翻旧口径)

旧口径:「`YMV` = 宿主媒体(動画)对象生命周期,`YMVINFO` 需宿主媒体元数据」。
**取证结论:YMV 不是全屏视频播放器,而是「把一串帧动画挂到普通图层上」**:

- ERIS 演示脚本把 YMV 当**图层**用(`es_Sprite.yst` 的 `L.FTYPE==6` = YMV 层:
  `CG[ID=… FILE=""]` 建空层 → `YMV[ID=… FILE=…]` 挂影片 →
  `YMVINFO[NUM/SX/SY/FPS]` 取元数据 → `CGACT[SIZE=1]` 套尺寸 →
  `YMV[ID=… FRAME=n]` 换帧 → 释放时 `YMVEND[ID]`)。
- 官方 SDK 说明(`UserScript/演出機能.txt` 的 BETA4_MOVIE 页):「映像のみ /
  音声なし / 最高 30fps まで」「普通のスプライトとして利用することが出来る」
  「雨の動画を作成すれば雨の演出に使える」⇒ **无音轨、由脚本推进帧**。
- 因此本批**不实现播放时钟/音频**,只实现:容器解码、图层帧切换、元数据查询。

## 1. 证据锚点

二进制:`<side-repo>/target/sdk/0.495/yu-ris_0495_032/システム/yu-ris.exe`
(sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`)。

| 事实 | 锚点 |
|---|---|
| 命令号:`YMV`=107、`YMVEND`=108、`YMVINFO`=109;参数序 `YMV(ID,IDNO,FILE,FRAME)` / `YMVEND(ID,IDNO)` / `YMVINFO(ID,IDNO,LET,NUM,SX,SY,FPS)` | `<side-repo>/workbench/analysis/ycd-commands.json`(0.495 `new`) |
| 分发表:`0x7acfcc→0x458ee0`、`0x7acfd0→0x459a64`、`0x7acfd4→0x459b98` | `FUN_0045cf90` 装载器 |
| `YMV` 处理体 `0x458ee0`:键 = `ID [+ "." + itoa10(IDNO)]`;`FILE` → `0x4141e4` 读包 → `0x414559` 取层 → `0x459094` 解析挂接;`FRAME` → `0x45934c` | 线性反汇编(capstone) |
| `YSMV` 解析/挂接 `0x459094`:头部 `{ver@4, frames@8, alpha@0xc, w@0x10, h@0x14, fps@0x18, 未用@0x1c}`,三张 `u32[frames]` 表起于 `0x20`(帧偏移 / 颜色段长 / alpha 段长),两段各自按 `buf[i] ^= (i & 0x0f) + 0x10` 解密 | 同上(循环在 `0x4591d8..0x45923b`) |
| 帧选择 `0x45934c`:帧号 <0 → 0、> `frames-1` → `frames-1`;`[layer+0x86] = (alpha标志 == 1)`;颜色段 → `0x4593f4`(D3D 表面解码)、alpha 段 → `0x459964`;末尾 `0x408724` 标脏 | 同上 |
| `YMVINFO` `0x459b98`:字段 `NUM=[+0] / SX=[+8] / SY=[+0xc] / FPS=[+0x10]`;**层或影片不存时向 LET 写 0**(int 0 / float 0.0,`0x459d63`/`0x459de2`) | 同上 |
| `YMVEND` `0x459a64`:按同一键取层 → `0x43d634` 释放影片结构(与 `YMV[FILE]` 重挂前的释放同一入口) | 同上 |
| 样例产物 `data/pac/{rain,sample2,sample3}.ymv`:YSMV v1;`rain` 800×600 24fps 20 帧、`sample2/3` 400×300 30fps 306 帧;每帧 = 2 段 XOR 混淆的**基线 JPEG**(颜色 RGB + alpha 灰度 L) | `<side-repo>/workbench/tmp/sdk-test/data/pac/*.ymv`,解密后 PIL 可解 |
| 演示脚本用法 | `<side-repo>/workbench/project-eris/save2/data/script/ERIS/Sprite/es_Sprite.yst`(`#=es.L.YMV.INIT/MAIN`、`CGEND → YMVEND[ID]`);`.../UserScript/演出機能.txt` `#=BETA4_MOVIE` |

### 1.1 勘误:DIV-GFX-027(IDNO 键含点)

`YMV` 键拼接块 `0x458f51..0x458f96`:`edi=0x5e3b80`(ID 已拷入) →
`edx=0x808354`(**字符串 `"."`**,字节 `2e 00 00 00`)→ 追加 → `0x46a7c0`
(`__ultoa` 基数 10)追加十进制。CG(`0x42c533..0x42c56e`)与 CGACT
(`0x42c735..0x42c77c`)是**同型块**,同一常量。⇒ IDNO 键真形式 = `<ID>.<n>`,
DIV-GFX-027 当年把「`.`追加循环」误读成 itoa 追加,**已修正**(旧实现反而对)。

## 2. 实现设计(分层,严守各 crate 铁律)

### 2.1 `yu-format::ymv`(纯字节进出,唯一容器真值)

```text
0x00 "YSMV"
0x04 u32 version        (官方解码器不校验;保留字段)
0x08 u32 frames
0x0c u32 alpha_flag     1 = 第二段存在且为 alpha
0x10 u32 width
0x14 u32 height
0x18 u32 fps
0x1c u32 (未用,样例恒 0)
0x20 u32[frames] 帧偏移(相对文件首)
+0x20 u32[frames] 颜色段长
+0x20 u32[frames] alpha 段长
然后逐帧 [颜色段][alpha 段],各段独立 XOR 混淆:byteᵢ ^= (i & 0x0f) + 0x10
```

API:`Ymv::parse(bytes)`(校验 magic/表边界/段区间)、`info()`、
`frame_planes(i)`(原始混淆切片)、`decode_plane(plane)`、`frame_jpegs(i)`。
安全边界(表越界/段越界)返回**类型化错误**,不 panic、不静默截断。

### 2.2 `yu-gfx`(图层语义的唯一真值)

- `TextureRef::Movie { asset: Vec<u8>, frame: u32 }`:图层当前帧的声明式引用;
  像素由渲染端的资产管线物化(与 `Asset` 同层,不在 gfx 里解码)。
- 图层节点新增 `movie: Option<MovieState>`(`asset` / 元数据 / 当前帧)。
- `YMV[FILE]`:取层(不存在 → 可恢复诊断,不建层;官方 `0x45905e` 是错误弹窗
  路径),用 VM 推入的元数据挂接;换片时按 `CG[FILE]` 同型作废旧快照/加工。
- `YMV[FRAME]`:帧号钳位后只改 `frame` 与纹理引用。
- `YMVEND`:清 `movie` 与纹理引用(= 官方释放影片结构),保留图层本身。
- `YMVINFO`:走既有 `op_info` → `query_info_with`;层/影片缺失一律回 `Int(0)`
  (与官方写 0 对齐),多字段同时置位按既有 `gfx.info.multi` 规则。
- 元数据来源:`GraphicsHost::note_movie(asset, MovieInfo)`(与
  `note_asset_size` 同型:VM 持数据源,解析后回填)。

### 2.3 `yu-rt`(命令与 VFS)

- `YMV` → 新 `HandlerKind::Ymv`:求值参数后按 `FILE` 探测(`键` 与 `键+".ymv"`)
  读包 → `yu_format::ymv::Ymv::parse` → `host.note_movie(原始FILE键, 元数据)`
  → 照常发 `Domain::Movie` 事件(事件契约不变,仍带脚本原始参数)。
- `YMVINFO` → 新 `HandlerKind::Ymvinfo` → 复用 `op_info`(LET 写回由 query 结果)。
- `YMVEND` 维持 `Domain::Movie`(无脚可见返回)。

### 2.4 `yu-render-core`(像素)

- `TextureStore::resolve_ref(TextureRef::Movie)`:按 `(资产, 帧)` 合成缓存键,
  读包(精确键 → `+".ymv"`)→ 解析 → 解密该帧两段 → `image` 解 JPEG →
  RGBA8(颜色 RGB;alpha = 灰度段亮度;`alpha_flag==0` 时 alpha=255)→ 入同一
  LRU 预算。尺寸以头部 `width/height` 为准,平面尺寸不符时按平面对齐并记诊断。

### 2.5 `yu-engine` / 其它

- `capture.rs` 的 `TextureRef` 穷举匹配补 `Movie`(与 `Asset` 同路径解析)。
- `yu-gfx::json` 的显示视图补 `Movie`(稳定 JSON:`{"kind":"movie",…}`)。

## 3. 台账划改清单(本批强制)

1. `crates/yu-rt/docs/coverage-matrix.md`:`YMV`/`YMVEND`/`YMVINFO` 三行 →
   `fixed(no-manual)`,证据列写 0.495 地址 + 样例。
2. `crates/yu-rt/docs/divergences/README.md` DIV-RT-045 媒体侧:改为已实现并链接
   `docs/divergences/DIV-GFX-073-ymv-movie-layer.md`。
3. `crates/yu-rt/docs/domain-event-contracts.md` §9:YMV/YMVINFO 行改为实现的
   契约(`note_movie` 推入 + `YMVINFO` query 回 0 语义)。
4. `crates/yu-gfx/docs/divergences/README.md`:DIV-GFX-027 更正为「键含 `.`」;
   新增 DIV-GFX-073(YMV 图层语义)。
5. `docs/todo-inventory.md` §3.2 与 `docs/game-settings-ledger.md` 110 行:
   去掉「YMV 未实现」口径,改记 12/13 已落地。
6. `docs/tasks/roadmap-batches.md` §B14:标记完成 + 终态。
7. `PROGRESS.md`:追加「成果 B14」。

## 4. 验收

- `cargo test --workspace --all-targets` 全绿(新增:format 容器单测 + 官方样例
  往返、gfx 帧切/查询/销毁、rt 命令分类与元数据推送)。
- 语料:`rain.ymv` 解码首帧 → PNG 目视(颜色 = 8 段 JPEG,alpha = 雨丝灰度);
  `sample2.ymv` 第 0/153/305 帧可解且尺寸 400×300。
- 实机:ERIS 演示 `演出機能` →「ムービー機能」→ 水族箱影片按 30fps 播放,
  可与背景/文本层共存、可缩放(F5 截图比对旧线/官方观感)。
- 全仓 `grep -c "YMV"` 的 `Unimplemented` 挂账点归零(B15 复核口径)。
