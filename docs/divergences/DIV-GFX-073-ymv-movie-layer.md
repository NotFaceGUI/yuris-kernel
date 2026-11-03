# DIV-GFX-073 · `YMV` 影片图层(帧动画挂到普通图层)

> 状态:**fixed(no-manual)**(2026-11-03,B14)。手册无 `YMV` 页;证据 =
> 0.495 官方处理体 + 官方 SDK 样例产物 + ERIS 演示脚本 + SDK 使用说明。
> 任务书:`docs/tasks/task-b14-ymv.md`。

## 1. 结论

`YMV` **不是**全屏视频播放器,而是「把一串帧动画挂到**普通图层**上」:

```text
CG[ID=… FILE=""]                         建空层(精灵框架 FTYPE==6)
YMV[ID=… FILE=…]                         挂影片(帧 0)
YMVINFO[ID=… NUM/SX/SY/FPS LET=@…]       取帧数/尺寸/帧率(写 LET)
CGACT[ID=… SIZE=1 SX=… SY=…]             把层矩形设成影片尺寸
YMV[ID=… FRAME=n]                        换帧(脚本按 FPS 自己推进)
YMVEND[ID=…]                             释放影片(图层保留)
```

SDK 说明(`UserScript/演出機能.txt` 的 `BETA4_MOVIE` 页)明确:
「映像のみ・音声なし・最高 30fps まで」「普通のスプライトとして利用する
ことが出来る」⇒ **无音轨、无播放时钟**:帧推进由脚本做
(`es_Sprite.yst` 的 `es.L.YMV.MAIN` 用 `_OS_TIME` + `YMVINFO[FPS]`)。

## 2. 格式(`YSMV`)

官方解析器 `0x459094`(锚点见任务书 §1):

```text
0x00 "YSMV"
0x04 u32 版本(样例恒 1;官方解码器不校验)
0x08 u32 帧数
0x0C u32 alpha 标志(1 = 第二平面为 alpha;官方置层 +0x86)
0x10 u32 宽      0x14 u32 高      0x18 u32 fps      0x1C u32(未用)
0x20 u32[帧数] 帧偏移(文件相对)
      u32[帧数] 颜色段长
      u32[帧数] alpha 段长
      逐帧 [颜色段][alpha 段]
```

- 两段各自按 `byte[i] ^= (i & 0x0f) + 0x10` 就地解混淆(`0x4591d8..0x45923b`);
- 解后是**基线 JPEG**:颜色段 RGB、alpha 段灰度(样例 `rain` 0..70、
  `sample2/3` 恒 255)。这也解释了「同一文件两段头字节相同」的现象 ——
  XOR 密钥按段内偏移重置。

样例实测:`rain.ymv` 800×600/24fps/20 帧;`sample2/3.ymv`
400×300/30fps/306 帧(官方 SDK `data/pac/`)。

## 3. 内核分层

| 层 | 职责 |
|---|---|
| `yu-format::ymv` | 容器切分 + 解混淆(纯字节进出);表/段越界返回类型化诊断 |
| `yu-rt`(VM) | `YMV[FILE]` 时读包(原 key → `+".ymv"`)解析**头部**,`GraphicsHost::note_movie` 推 `MovieInfo`;`YMVINFO` 走 `op_info` 查询写 `LET`;`Domain::Movie` 事件契约不变 |
| `yu-gfx` | 图层 `movie` 状态 + `TextureRef::Movie{asset, frame}`;`FRAME` 钳位 `[0, 帧数-1]`;`YMVEND` 释放 |
| `yu-render-core` | `resolve_movie(asset, frame)`:解该帧两段 JPEG → 合成 RGBA8(alpha = 灰度亮度;标志 0 时 255),按 `(资产, 帧)` 缓存 |
| soft / wgpu | soft 直接采样;wgpu 每资产 4 个固定槽位纹理(`帧号 % 4`)就地覆写,绑定组 id 恒稳定 ⇒ GPU 纹理数与影片长度无关 |

## 4. 与官方的对齐点(逐条)

| 行为 | 官方锚点 | 内核 |
|---|---|---|
| 键 = `ID[.IDNO]`(**点分隔**,DIV-GFX-027 勘误) | `0x458f51..0x458f96`(`0x808354` = `"."` + `__ultoa`) | `path::normalize` |
| `FILE` 缺失/资产不可用 ⇒ 图层原状 + 错误路径 | `0x45905e`(错误弹窗) | `gfx.texture.missing` / `rt.movie.asset` 可恢复诊断,不改图层 |
| 目标层必须已存在(按 ID 查层表) | `0x4141e4`/`0x414559` | `gfx.layer.missing`,不代建层 |
| 换片 = 释放旧结构再挂新结构 | `0x43d634` → `malloc(0x24)` | 清旧快照/加工/文本,重设纹理 |
| `FRAME` 钳位 `<0→0`、`≥帧数→帧数-1` | `0x45936e..0x459380` | `clamp_frame` |
| `[层+0x86] = (alpha标志 == 1)` | `0x459389..0x45938e` | `layer.alpha_aware` |
| 颜色段 → 层表面、alpha 段 → 层 alpha | `0x4593f4` / `0x459964` | RGBA 合成纹理 |
| `YMVINFO` 四字段;缺层/缺片写 **0**;多字段同时置位 ⇒ **槽号最小者生效** | `0x459b98`(`0x459d63`/`0x459de2`;四字段是顺序 `if` + 各自 `ret`) | `query_ymvinfo`(固定 `NUM→SX→SY→FPS` 取首个) |
| `YMV[FRAME]` 在未挂影片时**静默**无操作 | `0x45934c` 开头 `test ebp,ebp; je ret` | 空操作,不刷诊断 |
| 每换一帧整体覆写层表面 ⇒ 旧绘制不可见 | `0x4593f4` 解整帧进同一表面 | `reset_movie_surface`(作废该层复制/加工/文本记录) |
| `YMVEND` 释放影片、图层保留,**alpha 位不重置** | `0x459a64`(只释放 `[层+0xa0]`) | `texture = None`,其余属性保留 |
| 影片层可作为**图像加工/复制的源**(框架 `CGACT[SIZE2]` 每帧缩放拷贝) | 官方 `0x45934c` 后层表面即为普通表面 | `TextureRef::Movie` 纳入全部源层白名单(gfx 6 处)+ 后端像素解析 |

## 5. 未取证 / 限制(显式)

- **`YMV[FILE=""]`**:官方只对 `CG` 定义过「FILE="" ⇒ 1×1 黑」;YMV 没有
  这条语义(空名会让装载失败),内核按「未给 FILE」处理,不猜文件名。
- **同帧多图层引用同一影片的不同帧**:CPU 侧每资产只保留**最近一帧**
  (换帧即摘掉上一帧;已被图层/快照持有的 `Rc` 不受影响),因此 306 帧的
  影片不会把 147MB 全留在缓存里;wgpu 端每资产 4 个槽位环形复用,
  超过 4 个并发帧时后写者覆盖(实际脚本不会出现,出现时取最后写入的帧)。
- **YMVEND 之后的 GPU 纹理**:wgpu 的影片槽位按资产保留(不随图层销毁
  立即释放),上限 = 4 × 会话内播放过的影片数。
- **图层矩形 ≠ 影片尺寸**:按图层矩形缩放绘制(官方把影片表面画进图层
  矩形,0x459530 分支只重建表面、不改矩形);脚本惯例用 `CGACT[SIZE]`
  设成影片尺寸。
- **alpha 段尺寸与颜色段不一致**:按「不透明」处理并 `log::warn!`
  (畸形容器不猜像素)。
- **fps 上限 30**:由制作工具(`YSYmv.exe`)约束,引擎原样返回;
  内核不钳位。
- 手册无 `YMV` 页(政策:按 REA/引擎证据实现并标注 no-manual)。

## 6. 成本与性能(实测)

官方把播放期成本压到「每帧两张 JPEG 解码」:容器在 `YMV[FILE]` 内一次读入
并**全平面解混淆**(`0x459094`),`YMV[FRAME]` 只切段(`0x45934c`)。内核按同
形态实现:每个影片资产常驻一份 [`DecryptedYmv`](已解混淆的整份字节,
`Rc` 共享),换帧只切段 + 解 JPEG。

探针 `cargo run -p yu-backend-soft --example probe_ymv_timing`
(官方样例 `sample2.ymv` 5.9MB / 306 帧 / 400×300):

| 成本项 | debug(默认) | release |
|---|---|---|
| 容器载入 + 全平面解混淆(每片一次) | 22–24 ms | **0.65 ms** |
| 单帧解码(换帧;两张 JPEG + RGBA 合成) | 30 ms → **5.3 ms**※ | **0.6 ms** |
| 同帧重复取(缓存命中) | 0.4 µs | 0.1 µs |
| 软后端整帧渲染(含 1:1 `SIZE2`) | 49 ms | 7.4 ms |

※ 已在 `Cargo.toml` 给 `zune-jpeg`/`image` 单独加 `opt-level = 3`
(与既有 `libz-sys debug=false` 同型):解码器是纯计算密集的第三方代码,
dev 档不优化会把 30fps 影片打到 30fps。上表 5.3ms 是该配置下的 debug 值,
其中约 4.7ms 是本仓库自身的逐像素合成循环(debug 未优化)。

结论:**播放帧率取决于构建档位,不是实现**。release 下换帧 0.6ms,占 60fps
预算的 3.6%;debug(加解码器优化后)5.3ms/换帧,对应每帧 ~2.7ms 均摊。

## 7. 复核

- `cargo test -p yu-format --lib ymv`(含官方样例往返)
- `cargo test -p yu-gfx --test ymv_movie`(挂接/钳位/查询/释放/域路由)
- `cargo test -p yu-rt --test fixture ymv`(解析→note_movie→LET 写回)
- `cargo test -p yu-render-core --lib movie`(双平面合成/越界/缺资产)
- `cargo test -p yu-backend-soft --test ymv_movie`(真实 `sample2.ymv`
  两帧渲染,像素差异 > 1000)
