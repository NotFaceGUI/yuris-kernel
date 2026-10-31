# 任务书 I · 渲染后端(`yu-assets` + soft + wgpu)

> 目标: 把 `yu_gfx::CompositorFrame` 真正画成第一帧 PNG,再用 wgpu 复现同一帧。
> 软后端是**确定性参考**;wgpu 是主后端。字体/文本由 G-FONT 会话并行提供,
> 本会话先不渲染文本(遇到 TextRun 跳过并诊断)。

## 0. 边界

当前仓库里 **没有** `yu-assets`/`yu-backend-*`;这是从零新建,不是补现有模块。
`yu-gfx` 会话 F 正在改语义层,`yu-font` 会话 G-FONT 正在并行;本会话:

- 新建 `crates/yu-assets`、`crates/yu-backend-soft`、`crates/yu-backend-wgpu`;
- **不修改** `yu-gfx`/`yu-rt`/`yu-font`;只消费公开类型;
- 若 gfx 类型正在变,用最小适配层/本地输入结构,发现契约缺口先写文档,不擅自改 gfx;
- 第一帧输出一份 `target/tmp/first_frame.display.json`(CompositorFrame 的规范化
  display JSON)供 H-TRACE ③ 录制;通过文件协作,不共享代码;
- wasm host 不在本会话;先把桌面 soft + wgpu 跑通。

## 1. `yu-assets` · 纹理管线

- 输入:`TextureRef::Asset(key)`、`SolidBlack1x1`;`Offscreen`/`Snapshot` 先返回
  Unimplemented(COPY 未实现)。
- 数据源:`yu-vfs` 的 `YpfSource`/`DirSource`;按 key 取字节。
- 扩展名探测:按 `CompositorFrame.image_probe_order`;`Psb`/未知码显式 Unimplemented。
- 解码:`image` crate(PNG/BMP/JPG/GIF)→ RGBA8;失败/缺失 → `asset.*` Diag。
- 缓存:`BTreeMap<key, Texture>`;重复引用不重复解码;记录缺失资产清单。
- 大图/多图:懒加载;不要一次性解全部 data.ypf。
- wasm 预备:`YpfSource` 需要 `from_bytes`/内存源;本会话可加**加法式** API 到
  `yu-vfs`(不改解析语义),或先用 `DirSource`/`MemSource` 跑桌面。

## 2. `yu-backend-soft` · 确定性参考

- 用 `tiny-skia`(或等价纯 Rust)按 `CompositorFrame.passes` 顺序合成:
  - `PassTarget::Texture`:copy/image;status 非 Executable 的 op 跳过 + 记录;
  - `PassTarget::Offscreen`:组先子后父;组 alpha/blend 作用于合成结果;
  - `PassTarget::Screen`:主窗口组 → 800×600(实际用 frame.viewport)RGBA8。
- Alpha:固定管线 `min(A,255)/255`;`MODE 0/1/2` 先实现,其它按 `status` 跳过。
- Filter/clear/color_space 按 CompositorFrame;不要自行改语义。
- 快照:RGBA8 → PNG;同时输出 `frame<N>.json` 摘要(层数/pass/跳过项/缺失资产)。
- 确定性:同一 FrameDescription 两次 PNG 逐字节一致。

## 3. headless 第一帧

- 一个 example/CLI(可在 `yu-backend-soft/examples`):
  1. 打开 `data.ypf`;
  2. `Vm::boot` + 循环 tick 到首个可见层(设上限,不写死 frame 3);
  3. 把 `DomainCall/Text` 喂给 `GfxState`;
  4. `gfx.frame()`;
  5. `yu-assets` 解析纹理;
  6. soft 后端渲染 → `target/tmp/first_frame.png` + 摘要;
  7. 打印 `events_dropped()`、被跳过 op 数、缺失资产。
- 与官方 0.495 ERIS 同分辨率首帧截图并排;差异归因到资产/alpha/blend/跳过项。

## 4. `yu-backend-wgpu` · 主后端

- `wgpu` 24(或与工作区兼容版本);离屏 RGBA8 渲染,再呈现/读回。
- 严格按 CompositorFrame:
  - blend 用 gfx 提供的状态/语义;不要用 wgpu 默认混合;
  - filter/clear/alpha/color_space 显式设置;不启用隐式 sRGB 转换;
  - Offscreen/Screen pass、组 alpha/blend 与 soft 一致;
  - 纹理上传只接受 RGBA8;缓存纹理,不每帧重传。
- 先做桌面 headless(`--snapshot` 读回 PNG),再做窗口 surface;wasm 留给后续。
- 与 soft 对同一帧逐像素比较:报告完全不同像素、最大通道差、差异区域;
  差异必须能定位到 blend/filter/alpha/几何/纹理。

## 5. 验收

- `first_frame.png` 能画出 ERIS 首个可见帧的背景/CG/组/黑幕;
  按钮快照/文本等 Unimplemented 项必须在摘要中列出,不能假装完整。
- soft 与 wgpu 同帧对比;差异有归因。
- `cargo test -p yu-assets -p yu-backend-soft -p yu-backend-wgpu` 全绿;
- `cargo check --target wasm32-unknown-unknown` 对纯 Rust 部分通过(若依赖阻塞,记录);
- `cargo clippy` 无新增 warning;无 println!/eprintln!/dbg!;
- 不改 gfx/rt/font;无 git 写操作;游戏数据/截图不入库。

## 6. 环境

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; TMP="$TEMP"
export YURIS_TEST_TMP="D:/Art/ai/projects/yuris-kernel/target/test-tmp"
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"   # 数据入口:官方挂载规则
export YURIS_SAMPLE_ERIS_YPF="D:/Art/ai/projects/YurisDroid/workbench/project-eris/data.ypf"
export YURIS_SAMPLE_SDK0495="D:/Art/ai/projects/YurisDroid/target/sdk/0.495/yu-ris_0495_032"
```
