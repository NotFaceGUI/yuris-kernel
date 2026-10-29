# 性能取证 · 双游戏实测与尖峰收口(2026-10-27)

> 范围:桌面路径(`yu-cli run … --mode desktop`,wgpu 后端)的两份真实语料
> —— `<game-b>`(v494,`data.ypf` 3.3GB)与 `<project-eris>`
> (官方 0.495 编译器全量产物)。两份都由**人手动推进**(真实点击/键盘),
> 关闭窗口即结束;不是脚本回放。
>
> 结论先行:尖峰主因不是 VM,也不是 GPU,而是
> **① 内容指纹的逐字节 FNV、② 每次 COPY/MERGE 整图 clone、③ `SAVE` 抓帧器
> 每帧整帧软后端回放**。三项收口后 <game-b> 的帧间隔
> p95 从 **100.05ms(约 10fps)降到 17.09ms(60fps)**、`image_composite`
> p95 从 **77.91ms 降到 0.000ms**(峰值 144.3→73.2ms)、`snap` 均值
> 0.259→0.084ms;eris 稳态保持 60fps,渲染均值 2.06→1.76ms。
> 剩余固定成本是 VM 解释器吞吐(0.21µs/命令,场景装载 90–450ms 单帧)。

## 0. 口径与取证设施

```bash
# 逐帧 CSV + 长帧自动取证(阈值 20ms 工作耗时)
yu-cli run <project-root> --mode desktop --color off \
  --log-file   target/tmp/perf/<game>/run.jsonl \
  --frame-stats target/tmp/perf/<game>/frames.csv \
  --spike-log  target/tmp/perf/<game>/spikes.log --spike-ms 20
```

- `--frame-stats`(本轮扩展):`interval/work/tick/render/draw` +
  渲染子阶段 `plan/snap/image/text/copy_text/rest/encode/readback/tail` +
  细项 `text_bake/text_paint/text_hash/text_base/image_paint/image_pixel/
  image_composite` + 计数 `uploads/upload_bytes/draw_calls/readback_bytes/
  merge_ops/merge2_ops/paint_ops/pixel_ops/memo_chains/memo_hits`。
  子阶段由 `RenderPhases`(`yu-render-core`)承载,**与参与 soft/wgpu 逐计数
  对账的 `RenderReport` 分离**(后者要求 `Eq` 且逐帧可比较)。
- `--spike-log`:工作耗时 ≥ 阈值的帧落一行(不逐帧格式化):阶段耗时 +
  本帧命令构成(逐帧 `command_hits` 增量,零字符串分配) + 脚本落点
  (`es_scenario.yst@247 task=es.IDSubTask2`) + pass/op 构成 +
  目标层 op 数排行榜。默认关闭,不产生文件。
- 注意:`YU_LOG=trace` 会把 **wgpu 自身**每帧的 trace(数百行)灌进日志,
  20 秒 37MB 并明显拖慢帧 —— 性能采集不要开;渲染子阶段已走结构化 CSV。

## 1. 基线(修复前)

### 1.1 <game-b>(1357 帧 / 36.7s)

```text
interval  avg=26.800 p50=16.695 p95=100.053 p99=216.402 max=733.711
work      avg=15.829 p95=88.573 p99=203.411
tick      avg= 4.419 p95= 6.901 p99= 38.542 max=212.104
render    avg= 8.159 p95=83.044 p99= 97.228 max=225.392
snap      avg= 0.259 p99=2.526 max=37.465
image_composite avg=5.501 p95=77.909 p99=88.065 max=144.276
upload_bytes 单帧峰值 = 75.6MB
```

- 重帧形态(告警行):`layers=800 visible=619 passes=99 opmix=copy:64 image:27`
  → `plan=152ms snap=32ms image=110ms`;`uploads=4..6, upload_bytes=36–75MB`。
- 交替出现 30fps 段(`interval=33.4ms`,每帧 `image_composite≈19–20ms`)与
  10fps 段(`interval≈100ms`)。
- 场景/菜单装载单帧 `tick=200–450ms`(LET/LOOPEND/IF 各十万级)。

### 1.2 project-eris(4804 帧 / 84.9s)

```text
interval  avg=17.538 p50=16.667 p95=17.126 max=333.799  → 57fps
work      avg= 7.316 p95= 8.646
tick      avg= 4.572 p95= 5.331 max=128.707
render    avg= 2.055 p95= 3.042 max=  6.564
image_composite avg=0.505 p95=1.227 max=3.702
宿主未计入(work−tick−render−draw) max=217.302ms
```

- 稳态健康;尖峰全部是**一次性**:VM 菜单/场景构造 `tick=62–129ms`
  (`opmix=layer:26 …`、`ops=122k–455k`),以及场景切换帧上
  `宿主未计入=217ms`。

## 2. 根因(按实测归因)

1. **内容指纹太贵**:`content_hash_rgba` 是逐字节 FNV-1a(乘法是串行依赖,
   ≈3 cycles/byte)。它在**每条图像加工 op 结束后**对整张结果图算一次
   (`Override::new`),1100×825 一帧仅指纹就十几毫秒 —— 实测
   `MERGE` 单条 2.7ms 里一半以上是它。
2. **整图 clone**:`capture_copy_mirror` 每次 COPY 先 `source.rgba.clone()`
   造临时 `Surface`;`resolve_cpu_source_with`/`resolve_base_texture` 每次
   解析资产又整图 clone。重帧 64 条 COPY + 27 条加工 ⇒ 每帧数百 MB 无谓拷贝,
   这是 `snap_ms` 33ms 的主体。
3. **`blit` 逐像素**:`pixel()/set_pixel()` 带边界检查 + `Option`,
   同一份区域拷贝比 memcpy 慢一个数量级。
4. **`SAVE` 抓帧器每帧整帧回放**:`GfxCapture::observe` 在出现**新 CopyId**
   时让离屏软后端整帧渲染一次(为把快照冻结在创建帧)。<game-b> 的精灵/
   按钮场景**每帧都换 CopyId** ⇒ 每帧多做一次全量软渲染:
   `capture=100–375ms/帧`(取证后才可见,此前被记成「宿主开销」)。
5. **VM 解释器固定开销**(未改):0.21µs/命令;菜单构造 45 万条 ⇒ 95ms,
   场景装载 12 万条 ⇒ 30–130ms。

## 3. 改动(全部保持像素语义)

| 位置 | 改动 |
|---|---|
| `yu-render-core/src/text.rs` | `content_hash_rgba`/`texture_content_hash` 改为 8 字节字混合 + splitmix64 终结(确定性、内容敏感,仅运行时缓存键) |
| `yu-render-core/src/texture.rs` | 缓存改存 `Rc<Texture>`;新增 `resolve_shared`(O(1) 共享) |
| `yu-render-core/src/surface.rs` | 新增 `blit_rgba(w,h,&[u8],dst,…)`;`blit` 行级 `copy_from_slice`(memcpy) |
| `yu-render/wgpu/src/textures.rs` | 快照 CPU 镜像存 `Rc<Texture>`,`snapshot_cpu_shared` |
| `yu-render/wgpu/src/passes.rs` | 镜像/资产/快照源全部走 `Rc`;`apply_merge` 行切片 + 跳过全透明像素(公式恒等);合成链/动作计数进 `RenderPhases` |
| `yu-render/soft/src/imagefilters.rs` | `apply_merge` 去逐像素边界检查(同式浮点,结果逐位一致) |
| `yu-engine/src/capture.rs` + `engine.rs` | 抓帧器改为**渲染后**取主渲染器镜像(`Rc` 转移),镜像缺失才回放;`capture_ms` 单独计时 |
| `yu-engine` / `yu-rt` | 新增 `frame_op_summary`、`current_position`、`take_command_hit_deltas`(诊断只读) |
| `host-desktop` / `yu-cli` | `--spike-log`/`--spike-ms`;CSV 增加渲染子阶段列 |

`apply_merge` 的 `au == 0` 跳过是**恒等变换**:`MERGE` 为 `cl*(1-0)+cu*0=cl`,
`MERGE2` 为 `ao=al`、rgb 化简回 `cl`、alpha 写回 `al`,浮点 round 后逐位相同。

## 4. 复测

### 4.1 <game-b>(5930 帧 / 108.5s,全部修复后)

```text
interval  avg=18.236 p50=16.660 p95=17.093 p99=66.639 max=766.274
work      avg= 7.109 p95= 7.028 p99=55.292
tick      avg= 4.009 p95= 4.274 p99=33.965 max=444.806
render    avg= 2.294 p95= 2.268 p99=28.426 max=130.685
snap      avg= 0.084 p95= 0.001 p99= 0.007 max=20.473
image_composite avg=0.361 p95=0.000 p99=21.554 max=73.181
capture > 20ms 的帧:6
```

- **p95 = 17.09ms ≈ 60fps**:此前的 30fps/10fps 段消失。
- 剩余重帧都是**单次**场景事件:`host=1268 work=761.9ms = tick 51.4 +
  render 130.7 + draw 12.2 + capture 566.9`(一条拿不到镜像的复制仍走回放)。
- 中间态(run4,只做了渲染三项、未改抓帧器)证据:
  `memo=chains:0 hits:0` 出现于全部 459 个重帧 ⇒ ES 精灵合成链**从未进入**
  记忆化判定(基图是快照 / 链内含非 paint 动作),不是「签名抖动导致 miss」。

### 4.2 project-eris(2958 帧 / 56.1s,全部修复后)

```text
interval  avg=18.749 p50=16.672 p95=33.397 p99=50.010 max=250.082
work      avg= 8.309 p95=29.070
tick      avg= 5.860 p95=23.517 max=132.727
render    avg= 1.759 p95= 4.721 max=8.454
snap      avg= 0.039 p95=0.066 max=1.257
image_composite avg=0.557 p95=2.218 max=5.409
capture > 20ms 的帧:5(peak 199.2ms)
```

- 与基线(84.9s 流程)相比:渲染均值 2.06→1.76ms、`snap` 峰值 0.69→1.26(量级未变)、
  抓帧回放从「每次场景切换 ~217ms」降到 5 帧、峰值 199ms。
- `p95=33.4ms` 高于基线的 17.1ms 是**流程差异**(本轮开了更多菜单/场景切换):
  尖峰 260 帧里 163 帧以 VM 为主;稳态 p50 仍是 16.67ms。

## 5. 剩余瓶颈与下一步(按收益排序)

1. **合成链记忆化对 ES 精灵链无效**(`chains:0`):允许「快照基图 + 镜像代际」
   进入签名,或做**前缀缓存**(缓存链上不变前缀的结果纹理)。重帧里
   `image=36–74ms` 仍是 CPU 像素合成,`uploads=36–75MB/帧` 是同一批结果重传。
2. **CPU MERGE 已接近内存带宽上限**(0.7–2.7ms/条,大缓冲);
   再降只有两条路:alpha bbox 裁剪(精灵部件通常只占大缓冲一小块,
   `au=0` 区域已被证明可跳过)或 **GPU 合成 `Override::Pass`**(官方用
   硬件 blit;CPU 公式 `MERGE2` 带 `/ao` 除法,GPU 固定混合不能逐位复刻,
   需要 shader/往返,属研究项)。
3. **VM 突发**:0.21µs/命令仍是场景/菜单装载主项(eris 132ms、
   <game-b> 445ms 单帧)。属解释器吞吐线(与并行会话的 `A-=` 修复同域)。
4. **抓帧器残余回放**:非主组源 / 镜像被 GPU 加工失效的快照仍会回放
   (<game-b> 1 帧 567ms、<project-eris> 5 帧 ≤199ms)。曾尝试给 `:/W0/` 主组复制补
   CPU 镜像(后缓冲区域),但那会改变 `SAVE[ID]` 读到「命令时刻层表面」的
   时序(软后端回放会在同一帧继续应用 `CGACT`),且无 headless 用例覆盖
   (用例走 soft 后端),故**回退未验证的改动**;正确做法是把 copy 的
   `source_texture`/`texts` 冻结配方交给抓帧器重放,而不是直接读后缓冲。
5. **图片上传**:`upload_bytes` 36–75MB/帧只发生在合成结果变化的帧,
   随第 1/2 项一起消失。

## 6. 回归状态

```text
cargo test -p yu-render-core -p yu-backend-soft -p yu-backend-wgpu -p yu-engine
  => 37 / 15 / 9 / 21 / 9 / 6 … 全 passed(含 soft↔wgpu 逐计数对拍)
yu-cli case <project-eris> --all
  => 1 passed / 8 failed:失败项全部是 display_json/ops 计数差异,
     来自并行会话正在改的 VM `A-=` 语义(handlers.rs),不是本次渲染改动;
     golden 基线需在该改动落定后统一重生成。
```

证据产物(不入库):`target/tmp/perf/{<game-b>,eris}/{run*.jsonl,frames*.csv,spikes*.log}`。
分析脚本:`target/tmp/perf/analyze.py`。

## 7. 追加(同日) · 角色出场卡顿、日志阻塞与 MERGE/过渡热路径

### 7.1 症状与拆分(阈值为实测,非推测)

用户报告「角色一出来就卡顿」,且「开日志更卡」。带 `--debug-ui` 取样
(<game-b>,7379 帧 / 130.3s,面板常开)。角色出场那一帧(tick 为单位:ms):

```text
host=2914 work=320.8 = tick 263.4 + render 55.5 + draw 0.8
        render 明细:decode 31.0(35.7MB 像素) / image 38.9(其中 MERGE 合成 16.3)
        uploads 4 → 39.7MB
```

三块成本各自独立:
1. **脚本突发**(263ms):`LET=112960 LOOPEND=25385 IF=19753` —— 场景/立绘
   初始化的 20 万条命令,0.21µs/条(解释器吞吐线,本轮未动);
2. **资产解码**(31ms / 35.7MB):`TextureStore::load` = VFS 读 + 图像解码;
3. **CPU 合成/混合**(16–82ms/帧,淡入期间每帧):`MERGE` 与
   `CGACT[TRANSITION]` 的整面像素运算。

`decode` 是新增记账(`RenderPhases.decode_us/decode_bytes`),它是
`image`/`rest` 的**子集**(解码发生在 `resolve_shared` 里),两者不能相加。

### 7.2 MERGE 整数化(主项,7× 提速,逐位不变)

旧实现每像素 6 次 `u8→f64` + 3 次浮点乘 + `round` + `clamp`;在 2048²
合成缓冲上这是**计算瓶颈**(实测单条 MERGE ≈ 28ms,2 条 = 55.8ms/帧)。
改成整数式:

```text
round(cl*(1-a/255) + cu*(a/255)) = (cl*(255-a) + cu*a + 127) / 255
```

正确性:`num/255` 距任何半整数 ≥ 1/510 ≈ 0.00196,f64 误差在 1e-13 量级,
不可能跨过舍入边界;正数 `round`(half-away)等于 `(num+127)/255`。
`MERGE2` 带 `/ao` 除法(精确值可能正好落在 .5 上),**保留 f64 原式**。

实测(同形态帧,`imgactions=merge:2 merge2:0`):
`image_composite` 55.8 → **7.8ms**(单条 ≈ 3.9ms)。
新增逐位对拍测试 `merge_span_optimization_is_bit_exact`(64 组随机 + 整行透明)。

### 7.3 过渡/合成按跨度跳过(局部内容时才省)

- `blend_transition`:输出是新零面,两个源都为 0 的像素结果必为 0 ⇒ 按行
  求非零跨度,只写该段(`row_span_rgba`)。
- `apply_merge`:整行 alpha 全 0 是恒等 ⇒ 整行跳过(`alpha_span`)。
- 逐位对拍:`blend_transition_span_optimization_is_bit_exact`(64 组随机,
  含窄源/异尺寸/整行为零)。

注意:**全屏 CG 交叉淡入时两个源整行都有内容,跨度优化没有收益**
(实测该形态仍 3 条 transition ≈ 73ms),这是下一步 GPU 化的目标。

### 7.4 日志不再阻塞帧线程

旧链路每条日志:`stderr.write + \n + flush()`(桌面逐行刷)+ JSONL 两次
`write_all` 到无缓冲 `File`;诊断洪水时帧线程被同步 I/O 拖住(用户实测
「开日志更卡」)。现在:

- `JsonlFile` = 后台线程 + `BufWriter(256KB)` + 有界队列(65536 行);
  帧线程只做一次 `try_send`,队列满丢行并计数(退出前落一行说明),不静默;
- stderr 改 64KB 块缓冲,**只有 WARN+ 立即 flush**;退出/panic 前的同步点:
  桌面 `App::finish` → `ConsoleHandle::flush()`,`main` 兜底 `log::logger().flush()`;
- 顺带修了异步写者的首版 bug(排空循环把「队列为空」当关闭,写完第一行
  就退出线程;run6 的 JSONL 只有 1 行);`yu-log` 单测改为先 `flush()` 再读。

实测:同一会话 12391 行日志、退出摘要完整、帧时间不再随日志量抖动。

### 7.5 复测(全量修复后)

```text
<game-b>(7379 帧 / 130.3s,--debug-ui 常开):
  interval  avg=17.616 p50=16.633 p95=17.092 p99=33.664 max=416.893
  render    avg= 1.627 p95=1.628 p99=19.662
  image_composite avg=0.385 p95=0.000 p99=16.938 max=81.476(>5ms 的帧 2.6%)
  decode    avg= 0.038 p99=0.000 max=34.649(单帧峰值 71.4MB)
  日志 12391 行,退出摘要完整
对照(未整数化,MERGE=f64,1649 帧):
  interval p95=66.377   image_composite avg=5.605 p95=19.176 max=57.990
```

### 7.6 仍未闭环

- **脚本突发 263ms**:角色出场/菜单构造的解释器固定成本;属 VM 吞吐线
  (与并行会话的 `A-=` 修复同域,本轮未动)。
- **全屏 CG 过渡**:`TRANSITION` 在整屏内容上仍是 CPU 全图混合(≈24ms/条),
  跨度跳过无效;要再降只能 GPU 双纹理 shader(需与官方逐位对齐论证)。
- **首次解码 31ms/35MB**:可做预取(下一句台词/下一张立绘)或后台解码,
  需要脚本层的「即将使用」信号。

## 8. 追加 · CPU/GPU 分工审查与 `CGACT[TRANSITION]` 上 GPU

### 8.1 角色场景每帧的 CPU/GPU 归属(802 层 / 619 可见 / 99 pass)

| 工作 | 实际在哪 | 每帧成本 | 结论 |
|---|---|---|---|
| 图层 draw(619 可见层 → 25 draw) | GPU | — | ✓ 已是 GPU |
| `SIZE`/`SIZE2`/镜像 | GPU(UV 采样) | — | ✓ 已是 GPU |
| `MERGE` 到**快照**目标 | GPU(paint pass) | — | ✓ 已是 GPU |
| `CGACT[TRANSITION]`(双源 lerp) | **CPU**(本轮前) | **40–60ms/帧** | → 本轮迁到 GPU |
| `MERGE/MERGE2` 到**普通层** | CPU(整数化后 ≈4ms/条) | 8–60ms | 下一步 GPU |
| `RECTPAINT` 普通层 | CPU | ~0.4ms | 可 GPU(管线已存在) |
| 合成结果上传 | CPU→GPU | 36–75MB/帧 | 随合成迁走消失 |
| 资产解码(VFS+PNG) | CPU | 31ms/35MB(一次性) | 保持 CPU,需预取/后台 |
| 脚本(≈20 万条命令) | CPU | 263ms(一次性) | 保持 CPU(解释器吞吐) |

即:**像素合成(GPU 本职)此前有一半在 CPU;解码/脚本(CPU 本职)保留**。

### 8.2 GPU 整数 lerp(本轮实现)

- 新增 `lerp.wgsl`:全屏三角形 + `textureLoad` 两源同坐标取 texel,
  `out = (from*(256-f) + to*f) >> 8`(~整数,含 alpha),`from` 越界按 0。
  与 soft `blend_transition` **逐位一致**(UNORM→f32→round 精确还原 u8)。
- 规划期(`try_gpu_transition`):两源可解析为稳定 GPU 纹理(资产/1×1/
  快照/更早的 GPU 合成)、目标层无 CPU 像素消费者、非快照目标 ⇒ 建
  `LayerId` 级离屏目标(`TargetSet.composites`)+ lerp pass;否则显式回退
  CPU(`YU_TRANSITION_DEBUG=1` 打印原因)。
- 绘制期:`Override::Gpu` 直接作为图层纹理(不再上传),COPY 也可直接取它。
- 对拍门禁:`cgact_transition_gpu_matches_soft_exactly`(f=0/128/256,
  半透明源,逐字节 + RenderReport 逐字段 + 断言 `layer_composites≥1`
  证明真的走了 GPU 路径)。

### 8.3 未闭环(下一步按收益排序)

1. **`MERGE`/`MERGE2` 到普通层**仍是 CPU(整数化后 8–60ms/帧);可复用同一
   离屏 + 两纹理 shader(`MERGE` 整数式已可逐位复刻,`MERGE2` 的 `/ao`
   需在 shader 里做整数除法)。
2. **全屏 CG 过渡**现在走 GPU,但若源是 CPU 覆盖(`override` 已加工)仍回退;
   实测回退原因 `to is CPU-only` 少量。
3. **SAVE 抓帧器**仍有 430–455ms 的单帧回放(镜像缺失:非主组源/被 GPU
   加工失效的快照)。
4. 解码 31ms/35MB 与脚本 263ms 属 CPU 本职,需预取与解释器吞吐两条线。

## 9. 追加 · `MERGE` 上 GPU(含快照目标 ping-pong)与「差分切换」剩余尖峰

### 9.1 `CGACT[MERGE]` 上 GPU

- 新增 `merge.wgsl`:`out.rgb = (lower*(255-a) + upper*a + 127)/255`,
  `out.a = lower.a`;与 soft `apply_merge(…, false)` **逐字节一致**
  (`MERGE2` 的 `/ao` 在精确 .5 处 f64 round 方向不定,仍走 CPU)。
- 普通层目标:两槽 ping-pong(`composites[(layer, slot)]`),读上一槽、写下一槽。
- **快照目标**(实测占绝大多数,1884/1884 次回退原因是 `target is snapshot`):
  按官方「原地写快照表面」语义 —— 先把快照整面 blit 到临时槽(执行期按
  `CopyId` 解析,COPY 命令先执行),再用整数 shader 写回快照纹理,并让
  CPU 镜像失效。
- 对拍:`cgact_merge_gpu_matches_soft_exactly`(普通层链式 2 条)、
  `merge_on_snapshot_gpu_is_bit_exact`(快照目标、半透明上下层,逐字节)。

实测(<game-b>,2801 帧):939 帧含 MERGE,其中只有 72 帧仍有 >5ms 的 CPU
合成(回退原因 `upper is CPU-only`),其余已在 GPU;`render p95 13.6ms`。

### 9.2 「差分切换」剩余尖峰 = SAVE 抓帧器整帧软后端回放

run16 的重帧构成(3 个样本):

```text
host=1871 work=458.8ms = tick 51.7 + render 97.4 + draw 0.8 + capture 307.6
host=1960 work=419.1ms = tick 68.7 + render 24.0 + draw 0.7 + capture 325.1
host=1793 work=414.8ms = tick 43.6 + render  7.7 + draw 0.8 + capture 362.2
```

`capture>100ms` 出现 9 次/2801 帧(约 1 分钟)。原因:`GfxCapture::observe`
在出现**新 CopyId** 且主渲染器**没有 CPU 镜像**时,让离屏软后端整帧回放
(95 个 Texture pass + 619 个可见层的屏幕绘制)。差分/立绘重建正好每帧产生
新 CopyId ⇒ 命中该回退。本轮 GPU MERGE 让快照镜像失效,进一步增加了
「镜像缺失 → 回放」的概率(run13 2 次 → run16 9 次)。

**下一步(二选一,都需要单独定标)**:

1. **软后端只回放 TEXTURE pass**:快照只由 Texture pass 产生;跳过 Screen/
   Offscreen 的图层绘制(619 层)可把 300ms 砍到几十 ms。风险:组源 COPY
   依赖 Offscreen 合成,需保留组 pass;
2. **SAVE 时刻按需回读 GPU 快照**:`capture()` 里对该 `CopyId` 做一次阻塞
   读回(1–5ms),替代整帧软渲染。需要把 `GpuContext`/`TargetSet` 的只读
   句柄传进抓帧器,并保证与 VM tick 的借用不冲突。

## 10. 追加 · 渲染已清干净后,重帧归因转移到 `tick`(逐命令 profile)

### 10.1 结论

GPU 侧(过渡/MERGE/上传/读回)已不再是瓶颈:

```text
<game-b> run18(3241 帧,差分/立绘场景):
  interval p50=16.68 p95=50.26 p99=83.77
  render   p50= 0.89 p95=13.95 p99=24.74
  snap     p50= 0.00 p95= 7.69
  image_composite p50=0.00 p95=9.22        ← MERGE 绝大多数已在 GPU
  work>60ms 的帧 89/3241,全部是 tick 主导
```

### 10.2 逐命令 profile(`--profile-commands`)

`tick` 内的真实构成(命中率来自 VM 采样,单位 µs/op):

```text
host=785 tick=250.8ms   SOUND 235.3ms(95.8%) hits=6   avg=39225µs/op   ← BGM 解码
host=240 tick=190.3ms   SOUND 158.2ms(87.3%) hits=205 avg=  772µs/op   ← 场景 SE 批量装载
host=78  tick=226.4ms   LET 62.9 + CG 27.4 + IF 21.1 + VARACT 19.2
                        + LOOPEND 12.2 = 160.3ms(hits 31 万/2.1 万/9.9 万…)
host=785 [site] IF/LOOPEND/IFEND es_cgmode.yst pc575/579/578 各 1.15 万次
```

⇒ 「角色/差分出来卡一下」= **BGM/SE 同步解码**(单条 39ms) + **场景脚本突发**
(0.2µs/条,30 万条级),不是渲染。

> **勘误(2026-10-28)**:「BGM/SE 同步解码」是**当时旧实现**的行为。REA 复核
> 官方 `0x422ab0` 只读头部(`ov_time_total`),`0x422c65` 建 0.5s 环;本内核
> 已改流式,整段解码不再发生在 tick。角色出场/差分剩下的尖峰来自首用纹理
> 解码/上传与文本层 GPU 读回,归因见 `PROGRESS.md` 的 `PERF-STORY-SPIKE`。

### 10.3 已做:解码 PCM 缓存

`SoundState` 增加按解析文件名的解码缓存(`Arc<Pcm>`,96MB 上限):同一文件
再次 `SOUND` 只复制样本,不再重解码。场景初始化那 205 次 SE 装载、切场景回
同一 BGM 都会命中。首次装载仍需付解码成本。

### 10.4 下一步

1. **BGM 首次解码(39ms/条)**:异步解码/预解码;需要先确认官方 SOUND 的
   加载时机(同步 or 延迟),否则会与 `SOUNDINFO[TOTALTIME]` 时序对不上。
2. **脚本突发**:`LET/IF` 0.2µs、`CG` 1.3µs、`VARACT` 1.0–1.5µs;CG/VARACT
   是下一步最值得看的两个(每帧 2 万/2 万次)。
3. 渲染侧遗留:`upper is CPU-only` 的少数 MERGE、`snap`(镜像)7.7ms/帧、
   SAVE 快照读回 67ms(已从 300–360ms 降下来)。

## 11. 回归修复 · GPU 合成目标只增不删导致 `wgpu: Out of Memory`

- **现象**:角色出场/切差分后 `PANIC phase=tick frame=1679 msg=wgpu error:
  Out of Memory`(wgpu 默认把错误当致命)。
- **根因**:本轮新增的图层级 GPU 合成目标 `TargetSet.composites`(键 =
  `(LayerId, 槽位)`)每张都是**整尺寸纹理**,只增不删;差分/切场景不断产生
  新 LayerId ⇒ 累积数百张 ⇒ 显存耗尽。组离屏 `offscreen` 也同样是只增不删
  (存量问题)。
- **修复**:`prune_composites`(按「图层本帧是否存活」剪枝 + 32 张硬上限)
  与 `prune_offscreen`(按组 pass/图层引用存活集合)在每帧规划开头执行。
  合成目标只服务当帧,剪掉后当帧会 `ensure_composite` 重建。
- **可见性**:退出摘要新增
  `gpu targets composites=N offscreen=N snapshots=N textures=N`,可直接确认
  不随场景累积。

### 11.1 同批回归的另外两处(均已修)

1. **Validation 逃逸成 panic**:给 `create_target`/资产上传/读回缓冲加 OOM
   作用域时,把原来的 Validation 作用域替掉了 ⇒ 同一次创建里的校验错误
   走 wgpu 默认处理器 panic(`frame 1319 Validation Error`)。改为
   **外层 OOM + 内层 Validation** 双作用域,两类都转 `Diag` 并按 op 跳过;
   设备级兜底再装一个 `on_uncaptured_error` 只记录 `{error:?}`(带资源
   细节),不再让 GPU 错误冻结游戏。
2. **`BLIT 源快照缺失`**:快照目标 MERGE 的 blit 源在「本帧无 COPY 且
   GPU 纹理不存在」时执行期取不到 ⇒ `tick 失败`致命。加计划期预检
   (本帧有对应 COPY 或已冻结纹理才走 GPU),自合成(ID 与 ID2 同一快照)
   也回退 CPU —— GPU 不能对同一纹理读写。

### 11.2 长会话实测与最后两处软失败

```text
<game-b> run26: 15335 帧 / 263.2s / avg 58.3fps(用户实际游玩 4 分 23 秒)
  gpu targets 58MB(中途峰值 127MB),无 OOM
  结束于一次罕见 `BLIT 源快照缺失`(该 COPY 本帧被跳过 ⇒ 快照未落地)
```

修复:blit 源快照缺失、快照目标 pass 目标缺失都改为**跳过该合成**
(两条同时跳过,目标与源是同一 `CopyId`),不再 `tick 失败`;日志留
`[blit]/[merge] ... skipped` 便于计数。

这一条也说明:**渲染侧 58.3fps 的稳态已成立**;剩下的「飙红」是
`tick` 里的 SOUND 解码与脚本突发(§10)。

### 11.3 真正的 OOM 根因:CPU 覆盖纹理每次新建、纹理缓存只增不删

防御措施(剪枝/预算/作用域)只是止崩,真正的泄漏在这里:

- `plan_layer` / `image_source_texture` 对**帧内 CPU 覆盖**(立绘缓冲合成
  结果、文本覆盖等)每次内容指纹变化都调 `TextureCache::upload` **新建**
  一张纹理;`TextureCache` 从不回收 ⇒ 立绘/差分场景每帧泄漏数 MB,
  **46–63 秒**耗光显存(与三次崩溃的时间点完全吻合,崩溃时我们自己的
  目标只占 7–127MB,资产缓存 ~90 张)。
- 修复:`TextureCache::rewrite` —— 同尺寸内容变化时 `queue.write_texture`
  **原地写回同一张纹理**,纹理 id 不变(bind group 缓存继续命中)。
  `plan_layer` 与 `image_source_texture` 两个上传点都改为「内容没变⇒复用;
  同尺寸内容变化⇒原地重写;否则才新建」。上传/重写失败按 op 跳过,
  不再 fatal。

### 11.4 泄漏修复后的干净长跑(结论)

```text
<game-b> run28(用户正常关窗,exit 0):
  12267 帧 / 217.6s(3m38s)/ avg 56.4fps
  interval p50=16.66 p95=17.12 p99=49.68
  render   p50= 0.76 p95= 2.08 p99=23.11
  snap     p95= 0.00   image_composite p95=0.83
  gpu targets 全程 4–7MB;textures=98
  错误计数(OOM/校验/fatal/panic)= 0
  work>60ms 的帧 49/12267(0.4%),全部 tick 主导(最大 478.5ms)
```

⇒ 显存泄漏收口,渲染侧稳定(3 分半 56fps、无崩溃);剩余 0.4% 长帧是
`tick` 里的 SOUND 解码与脚本突发(§10),属下一步。

---

## 12. 后续(2026-10-29):文本层读回已由 GPU 文本 pass 收口

本报告 §1 记录的角色出场 `text_cpu` 尖峰(整张基面同步读回)已在后续
落地中消除:`CGACT[TEXT]` 的字形仍由 CPU 栅格化(与 soft 同一份像素),
但合成改为 compute shader 直接写入层的常驻 GPU 表面,不再
`read_back`。实测尖峰帧 `text_cpu=0`、`tcpu_rb=0`,365 字形/949 draws
`text≈0.9ms`;像素一致性由 wgpu/soft 逐像素测试锁定。实现与完整数据见
`PROGRESS.md` 的「成果 PERF-TEXT-GPU (2026-10-29)」。

仍未覆盖:`text_layout`(屏幕级自动文本)与缩放/非恒等采样基面仍走
CPU;差分纹理首次解码/上传仍在 render plan。

## 15. 后续(2026-10-29):同 tick `COPY2→SAVE[ID]` 与资产缓存预算

`es_saveload.yst`(sid 19)在**同一 tick**执行
`CG[FILE=""] → COPY2(SLTEMP→SLTEMP2) → SAVE[ID=SLTEMP2]`;官方 COPY 在
命令处理器内立即写表面,内核的写表面在帧末,导致存档 DNO3 退化为 1×1。
`GfxCapture` 现在按「上一帧注册表 → 源链裁剪 → 命令时刻整帧软回放」取
像素,实测存档恢复 100×75(`ulen=31608`),桌面 wgpu 与 soft 一致;完整
取证见 `DIV-GFX-068` 与 `PROGRESS.md`「成果 SAVE-THUMB-FIX」。

`TextureStore` 增加 CPU 缓存字节预算(默认 512MiB,
`YURIS_ASSET_CACHE_MB` 覆盖;只回收无外部引用的条目),长会话不再无限
累积解码位图;GPU 资产纹理仍随资产数增长,待官方 `CASH`/`REMALLOC`
语义取证后再定策略。
