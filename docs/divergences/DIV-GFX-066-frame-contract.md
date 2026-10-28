# DIV-GFX-066 · 帧契约:图层表面常驻,帧不再逐 op 重放

> 状态:`fixed`(2026-10-28)。改动范围:`yu-backend-wgpu` 的帧规划/执行 +
> `yu-engine` 的存档抓帧 + `yu-render-core::RenderPhases` 新字段。
> soft 后端**未改**(它保留为逐像素 oracle)。

## 1. 官方模型(证据)

| 事实 | 锚点 |
|---|---|
| `CGACT` 在命令处理器内**就地**改目标表面 | `crates/yu-gfx/src/state.rs:3440`(`FUN_0042f809` 尾部 `*(iVar4+0x90) = local_68`) |
| `COPY` 直接改纹理,不读“上一帧”缓冲 | `DIV-GFX-051` §官方行为 |
| 图层有常驻像素表面;存档直接读它 | `crates/yu-gfx/src/host.rs:37`(读取端 `0x4477c0`,首 u32 = `0x640` 位图头) |
| `CGACT[TEXT]` 即时写层表面 | `DIV-GFX-055` |
| `MERGE`/`MERGE2` 的 alpha 公式未 REA 级验证 | `DIV-GFX-063` §2.4 |

旧 wgpu 实现每帧从图层表 + 命令日志**重建** `CompositorFrame`,把整条
`CGACT` 链在 CPU 上合成成覆盖纹理再上传(实测 `<game-b>` 1200 帧:
`image` p95 15–35ms、上传平均 8.8MB/帧/重帧 115MB、快照整面拷贝
2×35MB)。那与官方“命令时刻就地改表面”的模型不同,重活全部来自重算+搬运。

## 2. 新契约

`crates/yu-render/wgpu/src/surfaces.rs`(`LayerSurfaces`)+ `plan.rs`:

1. **每层一张常驻表面**:`Content { tex, uv, width, height, display, bilinear, hash }`。
   `uv` 是采样窗(`SIZE`/镜像只改它,不重采样),`display` 是 `SIZE` 的
   浮点目标尺寸(16.16 定点口径,见 `docs/render-parity.md` §7)。
2. **按 `Op.seq` 命令序就地应用**:`COPY`/`RECTPAINT*`/`MERGE`/`MERGE2`/
   `XOR`/`AND`/`TRANSITION`/`REVERSE`/`MEIDO`/`SAIDO`/`AKARUSA`/`MONO`/
   `SEPIA`/`MOSAIC`/`ROTATION`/`RESIZE`/`TRIM` 都是对该表面纹理的一次
   GPU pass。新增着色器:`composite.wgsl`(MERGE2/XOR/AND)、
   `transform.wgsl`(原地滤镜族)、`resample.wgsl`(`scale_region` 的 GPU 等价)。
3. **写入永不原地**:每次像素写入申请一张新纹理(池内复用),本帧已登记的
   读取者仍看到命令时刻的内容(EVID-T-15)。写入目标 = `PlannedTarget::Surface(id)`。
4. **操作前缀复用**:每层保存 `(seq, 签名)` 已应用前缀。签名 = 动作 + 参数 +
   各源**当前内容身份**(自引用时用基面身份)。前缀相同 ⇒ 该 op 本帧零 pass
   (官方命令只执行一次,本内核每帧重发的是命令日志)。
   帧首核对同层 `seq` 前缀:日志被折叠(`supersede_*`)或 `CG[FILE]` 重建 ⇒
   回到基面整条重放(官方就地执行,重放幂等)。
5. **快照三态**(`apply_copy`):
   - 整幅 + 未采样 ⇒ **别名**源内容句柄(零拷贝;内容纹理不再原地改写,
     冻结语义天然成立);
   - 带区域/带采样窗 ⇒ 物化后 `copy_texture_to_texture` 只拷区域;
   - CPU 像素按需读回(存档抓帧;每张快照一次,不逐帧)。
6. **文本**:`CGACT[TEXT]`/`TextRun` 以「修订号 + 内容签名 + 基面身份」为键
   合成一次,上传一次,之后随表面常驻(`TextComposer` 仍是 CPU 栅格化)。
7. **帧 = blit**:屏幕 pass 按 `(z, seq)` 对每个可见层画一个 quad(带
   alpha/blend/UV),不再有“图层自合成 + 覆盖纹理上传”。

## 3. 快照目标不只读;`MERGE` 写 alpha(与旧 wgpu 的差异)

快照表在本实现里只读:所有像素写入都落在**层表面内容**上(写入永不原地),
所以 COPY 冻结的像素不会被同帧 `RECTPAINT` 改写。

`MERGE` 系列的 alpha 口径按官方执行体定案(REA 0.495 `0x4405a4`,详见
`DIV-GFX-063` §2.4):官方只把**上层**的每部件纹理按**下层**的每部件位置
`dev->Draw(...)` 画到当前渲染目标,不读下层旧像素;alpha 由 D3D9 固定管线
同因子混合写回(`a² + la·(1−a)`)。因此:

- `COPY2 → RECTPAINT3 SET=0 → MERGE` 清空后仍然可见(alpha 被写回),
  立绘换差分/眨眼不再整层消失;
- 旧 wgpu「MERGE 保留目标 alpha」是错的,已删;
- 两后端同一公式(`merge.wgsl` / soft `apply_merge`),逐位一致。

## 4. 实测(`<game-b>` v494,1200 帧,桌面 wgpu,`--no-sound`)

| 口径 | 改前 | 改后 |
|---|---|---|
| render p95 | 24.5ms | **2.1–3.7ms** |
| plan p95 | 22.2ms | **0.3–2.4ms** |
| work p95 | 49.6ms | **7.3ms**(轻场景)/ 24.2ms(含美术首次解码帧) |
| fps | 38.5 | **50–58**(2500 帧长稳 54.2) |
| 上传(合成,不含资产首次解码) | 平均 8.6MB / 单帧 79MB | **平均 0.23MB / 单帧 3.6MB**(后缓冲) |
| 上传(含资产首次解码) | 平均 8.9MB / 单帧 115MB | 平均 0.5MB / 单帧 54MB(解码帧) |
| 快照拷贝 | 重帧 2×35MB | 别名命中 106 次;剩余区域拷贝单帧 ≤8MB(p95 0) |
| GPU 目标字节 | 127MB | 4–217MB(池上限 64MB;大表面不复用) |

资产首次解码尖峰(`decode_ms` 5–30ms / 18–54MB)在两版里都存在(改前同一
场景 `decode` 71MB / 38ms),是加载新美术的一次性成本,不是每帧渲染预算;
`RenderPhases::upload_asset_bytes` 把两者分开记账。

### 4.1 跨后端逐帧对拍(`<game-b>` 2500 帧,728 个输入)

```text
common frames 2498 · 计数/跳过差异 0 · 像素差异 188 帧(全部是 §3.2 的
SIZE UV 采样/淡入舍入;平场与文字帧 0)
```

## 5. 回归

```text
cargo test -p yu-backend-wgpu        25 条逐像素对拍 + 7 条 surfaces 契约 + 9 单测
cargo test -p yu-backend-soft / yu-render-core / yu-engine ...
cargo test -p yu-cli --test logging_guards
```

`case <project-eris> --all` 在本机是**既有红灯**:display JSON golden 在
HEAD(未改动的 pristine 导出构建)上同样全红(0 passed / 9 failed);本改动
使 `save-load-loop`(PNG 部分)与 `save-title-flow` 转为通过。逐项对比见
`PROGRESS.md` 成果 SURFACE-RESIDENT。

## 6. 残留

- `BLUR*/DIFFUSION/WHIRLPOOL/RASTER*`:没有 GPU shader;遇到时显式跳过
  (计数 + `gpu.imageop.unsupported.*`),不静默降级。两份语料 1200/3000 帧
  未出现。
- `TRIM` 需要 alpha 包围盒:读回一次(低频;`cpu_fallbacks` 计数)。
- `YURIS_LEGACY_FRAME_REPLAY` 回退开关未保留:旧路径与“快照 CPU 镜像”
  (`GfxCapture.mirrors` 的每帧预捕获)在同一改动里删除,保留一份死代码
  与三分支状态会与 W8(删旧路径)冲突;完整旧实现留
  `target/tmp/passes.orig.rs`(仅本地)。
