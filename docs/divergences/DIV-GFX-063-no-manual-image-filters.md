# DIV-GFX-063 · 无手册页滤镜/合成族(`partial(no-manual)`)

> 状态:`partial(no-manual)`。按 AGENTS §0 的 2026-10-24 政策,手册缺页
> 不等于不实现:凡 `yscm` 参数表存在、且语料有实际调用的动作,一律给出
> 执行体并标注证据级别;语义未取证的部分在此登记,不伪装成已对齐。

## 1. 已落地(证据 = yscm 参数表 + 语料调用)

| 动作 | 参数 | 实现位置 | 证据级别 |
|---|---|---|---|
| `ROTATION` | `SET`(角度) | `imagefilters::apply_rotation` | 手册有页 |
| `ROTATION2` | `SET` | 同上 | `partial(no-manual)` |
| `SAIDO` | `SET`(0..256,128=原样) | `apply_saido` | **REA 已反编译**(见 §2.2) |
| `AKARUSA` | `SET`(0..256,128=原样) | `apply_akarusa` | **REA 已反编译**(见 §2.2) |
| `MONO` | `SET`(阈值) | `apply_mono` | 手册有页 |
| `MOSAIC` | `SET`(块边长) | `apply_mosaic` | 手册有页 |
| `BLUR` | `SET`(半径) | `apply_blur` | 手册有页 |
| `BLUR2` | `SET`(半径) | `apply_blur_h` | 手册无页;**REA 已逐个反编译**(见 §2.1) |
| `BLUR3` | `SET`(半径) | `apply_blur_v` | 同上 |
| `BLUR4` | `SET`(半径) | `apply_blur`(与 `BLUR` 同核) | 同上 |
| `BLUR5` | `SET`(半径) | `apply_blur_alpha_mask` | 同上(恒为 alpha-only 遮罩) |
| `BLUR6` | `SET`(半径) | `[+0x86]=1` → `apply_blur_alpha_mask`,否则 `apply_blur` | 同上(按层标志二选一) |
| `DIFFUSION` | `SET`(轮数×2) | `apply_diffusion` | **REA 已反编译**(见 §2.1.1) |
| `WHIRLPOOL` | `SET`(进度,度×10) | `apply_whirlpool` | **REA 已反编译**(见 §2.1.2) |
| `RASTER` | `SET`(幅度)+`SET2`(相位°) | `apply_raster` | **REA 已反编译**(见 §2.1.3) |
| `RASTER2` | 同上(只有垂直) | 同上 | 同上 |
| `RASTER3` | 同上(两向叠加) | 同上 | 同上 |
| `SEPIA` | 无 | `apply_sepia` | **REA 已反编译**(见 §2.2) |
| `XOR` / `AND` | `ID`(上层) | `apply_bitwise` | `partial(no-manual)`:RGB 逐通道,alpha 规则未取证 |
| `MERGE` | `ID`(上层) | `apply_merge(upper,false)` | `partial`:alpha 公式仅手册文字依据 |
| `MERGE2` | `ID`(上层) | `apply_merge(upper,true)` | `partial`:同上 |
| `RESIZE` | `X/Y/X2/Y2` | `imageop::apply_resize` | 几何已对 REA,滤波未取证 |
| `TRIM` | `SET` | `imageop::apply_trim` | 手册有页 |
| `MEIDO` | `SET` | `imageop::apply_meido` | `partial(no-manual)` |
| `VERTICAL` | 无 | `imageop::apply_vertical` | 手册例 10/11 + 语料 `:8233/8240` |
| `TRANSITION` | `SET`+`ID2`(第二输入)+`ID3`(输出) | `imageop::blend_transition` | 见 DIV-GFX-061 |

## 2. 明确未取证的点

1. **`BLUR2..BLUR6` 的算法(2026-10-30 逐个反编译收口)**:
   - 六个变体 = 六个不同执行体,action 码 `BLUR 0x02`/`BLUR2 0x03`/
     `BLUR3 0x04`/`BLUR4 0x05`/`BLUR5 0x06`/`BLUR6 0x07`(分派 `FUN_0042f809`);
   - **半径一律 `max(1, SET)`**(六者 prologue 一致,不是 `SET × 档位`);
   - `BLUR`(`FUN_00431d28`)/`BLUR4`(`FUN_00433718`)= H→V 两遍盒式
     (两种实现、像素结果相同);`BLUR2`(`FUN_0043265c`)= **只有水平遍**
     (临时缓冲只 malloc 不写);`BLUR3`(`FUN_00432e58`)= **只有垂直遍**;
   - `BLUR5`(`FUN_00434748`)= **alpha-only 两遍 + RGB 归零**(黑底阴影遮罩),
     **没有** flag 分支;`BLUR6`(`FUN_00434b7c`)的 `flag=1` 分支逐行同式,
     `flag=0` 才是普通模糊;
   - 通道口径由层 `[+0x86]` 选(分派 `0x430191` 把它作为第 9 个参数):
     `0` = 只平均 R/G/B、alpha 保留;`1` = 四通道一起平均;
   - 边界一律**窗口收缩**(`lo=max(0,i-r)`,`hi=min(n-1,i+r)`,除数 = 实际
     像素数),不是夹取重复边缘;
   - 门禁:`blur_two_pass_rgb_matches_soft`/`blur2_is_horizontal_only_and_matches_soft`/
     `blur5_alpha_mask_darkens_background_like_soft`/
     `blur6_alpha_aware_layer_takes_shadow_branch`(soft/wgpu 逐像素一致),
     soft 单测 `blur2_is_horizontal_only`/`blur3_is_vertical_only_and_keeps_source_alpha`/
     `blur4_is_two_pass_box`/`blur5_is_alpha_only_mask`/
     `alpha_aware_blur_averages_alpha_too`。
   - 已知边界:**本内核的文本合成推迟到绘制期**,所以 `BLUR6` 在「层里画过
     文字」时的官方语义(先烘字、再取 alpha 遮罩)尚未对齐 —— 与层 `[+0x86]`
     的文本置位一起待文本烘焙时序落地后再接;当前只由 `RECTPAINT>=2`/`RESIZE`
     置位(两者都真的改本层像素)。
2. **`DIFFUSION` / `WHIRLPOOL` / `RASTER*` 的算法(2026-10-30 逐个反编译收口)**:

   ### 2.1.1 `DIFFUSION`(action `0x0a` → `FUN_004358d4`)

   1. 先把整面 `memcpy` 到新缓冲(`0x435912`,MMX `memcpy`),再在新缓冲上就地
      置换 —— 与就地置换等价(同一遍内 2×2 块互不重叠);
   2. `SET/2`(C 的向零取整)轮,每轮两遍:
      - 正向:`y = y0, y0+2, … < y1`,`x = x0+it, x0+it+2, … < x1`;
      - 反向:`y = y1-1, y1-3, … >= y0`,`x = x1-1-it, x1-3-it, …`
        (次数 `ceil((x1-x0-it)/2)`,与正向同数);
   3. 每个 2×2 块取一次 `rand()%24`,按 `0x5b4780` 的 24×4 置换表重排四个像素,
      **只写 RGB**(汇编只写 `+0/+1/+2/+4/+5/+6`),alpha 原样保留;
   4. 随机源是 CRT `_rand()`(`0x46ac0c`):`seed = seed*0x343fd + 0x269ec3;
      return (seed>>16) & 0x7fff`,状态在 CRT 线程局部区、**默认种子 1**
      (全 exe 无 `srand`),且**全 exe 只有本函数两处调用它** ⇒ 这条流只被
      扩散消耗,从 1 开始即可逐次对齐。
   5. 实现口径:soft 用渲染器级 `seed` 顺序推进;wgpu 把「此前已消耗的块数」
      跨帧累计,着色器用**仿射复合的二进制幂**直接跳到第 k 个随机数
      (`transform.wgsl` 的 `lcg_jump`),因此不需要任何 CPU 逐像素参与。

   ### 2.1.2 `WHIRLPOOL`(action `0x27` → `FUN_00437368`)

   - 中心 `(w/2, h/2)`(整数除法);
   - 半径表长度 `maxr = round((hypot(w,h)+1)*0.5 + 1)`;
   - 角度表 `angle(r) = SET/10 · (1 - r/maxr)` 度,
     `cos_tab[r] = round(cos(angle)·256)`、`sin_tab[r] = round(sin(angle)·256)`;
     角度常量是 `0x58ed38` = `0.017453292519444445`,**不是** `π/180`
     (差在末位,已按位取用);
   - 采样:`sx = ((-(y-cy)·sin + (x-cx)·cos) >> 8) + cx`、
     `sy = (((x-cx)·sin + cos·(y-cy) >> 8)) + cy`(`>>` 为算术右移),
     再各自夹到 `[0, w-1]` / `[0, h-1]`;
   - 半径 `r = round(hypot(x-cx, y-cy))`;官方把它缓存在 `DAT_005d5c08`
     (键 = 宽高),缓存只加速、不改结果。

   ### 2.1.3 `RASTER` / `RASTER2` / `RASTER3`(action `0x15` / `0x16` / `0x17`)

   `0x15`/`0x16` 是分派体 `FUN_0042f809` 里的**内联**两支,`0x17` 走
   `FUN_00431658` 的 `variant=3`;三处像素公式一致:

   ```text
   行相位 shift_x = round(sin((y + SET2)° · π/180) · SET)     // 只有 RASTER/RASTER3
   列相位 shift_y = round(cos((x + SET2)° · π/180) · SET)     // 只有 RASTER2/RASTER3
   采样    src[clamp(y + shift_y, y0, y1)][clamp((x-x0) + shift_x, 0, x1-1)]
   ```

   - `SET` = **幅度**、`SET2` = **相位(度)**,不是「带宽/进度」;
   - 角度先用 **f32** 常量 `0.017453292`(`0x58effc` = `0x3C8EFA35`)乘、
     再交给 `sin`/`cos`(x86 的 `fsin`/`fcos`,见 `0x46ab34`/`0x46aa84`);
   - **取整是「向零截断」,不是四舍五入**:两处 `fistp` 前都把 FPU 控制字
     `or 0xC00`(RC=11)(`0x43174b`/`0x4317a3`;旋涡侧 `0x4373ed`/
     `0x437490`/`0x4374af`/`0x437592`)。后果之一:`SET2=90` 时
     `0.017453292f·90` 略小于 π/2,`sin` 得 `0.99999999…`,截断后位移是
     **0** 而不是 1 —— 实现与用例都按这个口径;
   - 水平下标是**区域相对**的 `(x-x0)+shift_x`,夹取上界却用绝对 `x1-1`
     —— 官方原样,整层调用时等价于「末列复用倒数第二列」;
   - 变体选择(`FUN_00431658` 的 `variant`):1 = 幅度只作用在水平、
     2 = 只作用在垂直、3 = 两者。

   ### 2.1.4 两个后端的落点与已知边界

   - soft:`imagefilters::apply_diffusion` / `apply_whirlpool` / `apply_raster`
     (像素参考实现,单测在册);
   - wgpu:`transform.wgsl` 的 **mode 16..20**(真 GPU pass;扩散是
     `2×(SET/2)` 次 fullscreen pass 的 ping-pong,每遍由 `u.n` 传
     `(方向, it, 随机数基数, 每行块数)`);
   - 对拍用例:`raster_shifts_rows_by_sin_on_gpu_and_matches_soft` /
     `raster2_shifts_columns_by_cos_on_gpu_and_matches_soft` /
     `raster3_combines_both_axes_on_gpu_and_matches_soft` /
     `whirlpool_rotates_by_radius_on_gpu_and_matches_soft` /
     `diffusion_permutes_blocks_on_gpu_and_matches_soft` /
     `diffusion_matches_soft_across_sizes`(8 组尺寸 × SET)。
   - **已知边界 1**:三角函数精度。官方用 x87 `fsin/fcos`(double 参数、
     80 位中间结果),soft 用 `f64`、wgpu 用 `f32`。由于取整是**截断**,
     只有当乘积落在**整数**边界 ±1e-9 内才可能差 1 像素;对拍用例刻意避开
     这些点(`SET2=60` 等)。未做逐位保证,登记为残余。
   - **已知边界 2**:区域参数(`X/Y/X2/Y2`)对这三族**未接线**(与
     `BLUR` 族同现状);官方执行体是按区域循环的。语料里三者都没有
     region 用法(`census_params` + 调用点逐条核对),故未实现。
   - **取整审查的扩展结论**(见 §2.2):`CGACT` 里**只有** `WHIRLPOOL` 与
     `RASTER` 族用浮点;其余滤镜执行体全是纯整数。因此 `SAIDO`/`AKARUSA`/
     `SEPIA` 原先的「f64 + 四舍五入」是**没有依据**的,已按执行体重写为整数。

3. **`SAIDO` / `AKARUSA` / `SEPIA` 的整数公式(2026-10-30 逐个反编译)**:

   ### 2.2.1 `SAIDO`(action `0x1d` → `FUN_00436bdc`)

   - 灰度 = **`(r+g+b)/3`**(整数截断;函数开头先建 766 项 `i/3` 查表,
     `i*0x55555556>>32` 形态),**不是** Rec.601 加权;
   - `SET < 0x80`:`k = 0x80 - SET`,`out = px + ((l - px)·k >> 7)`
     (`>>` 为算术右移);
   - `SET >= 0x80`:`k0 = SET - 0x80`,先按 `k0` 逐通道试算,再按
     溢出(`(0xff-px)*0x80/d`)与下溢(`px*-0x80/d`)收缩 k;前三条比较
     **原始 k0**、后三条比较**已收缩的 k**(官方原样,不是统一 min);
   - alpha 原样保留。

   ### 2.2.2 `AKARUSA`(action `0x01` → `FUN_00431b40`)

   - 官方是**加法**亮度偏移,不是乘法:`SET < 0x80` 分支
     `out = (px - 0x80) + SET` 并夹下界 0;`SET >= 0x80` 分支查
     `DAT_00689e60[SET + px]`;
   - 该表在 `.data` 的 BSS 区(运行时初始化,静态读不到),用内存探针读官方
     进程 400 项,**逐项等于 `clamp(i - 0x80, 0, 255)`**(零偏差);
   - 合并成 `out = clamp(px + SET - 128, 0, 255)`。

   ### 2.2.3 `SEPIA`(action `0x26` → `FUN_004371cc`)

   逐指令复刻(内存序 **BGRA**,所以权重 `117/601/306` 对应 B/G/R,
   合起来正是 Rec.601 的 0.114/0.587/0.299):

   ```text
   l10  = 117·B + 601·G + 306·R
   base = l10 << 10
   R' = clamp(trunc((base + 0x11E1400) / 2^20), 0, 255)
   G' = clamp(trunc((base - 0x46361)   / 2^20), 0, 255)
   B' = clamp(trunc((base - 0x2C33311) / 2^20), 0, 255)
   ```

   `trunc` 由 `(x>>19)>>12`(负数得 `0xFFFFF`)实现的**向零截断**,
   不是算术右移的地板除。三通道偏移分别是 +17.9 / -0.27 / -44.2。

   ### 2.2.4 取整审查的完整结论

   - 全 exe 的 `or ...,0xC00`(FPU 控制字 RC=11 = 向零截断)共 61 处,
     是 MSVC `(int)` 转换的通用惯例;
   - `CGACT` 的 17 个执行体里,**只有 `WHIRLPOOL`(4 处)与 `RASTER`/`RASTER3`
     (2 处)有浮点**,全部走该惯例;其余 15 个(含模糊族、`DIFFUSION`、
     `MONO`、`MEIDO`、`SAIDO`、`AKARUSA`、`SEPIA`、`REVERSE`)**一条浮点
     指令都没有**;
   - 因此两后端在这三族(以及 §2.2 的三个)上的整数公式是逐位可对齐的,
     跨后端对拍用例在册(`saido_akarusa_sepia_match_soft`)。
   - 仍属「模型」而非取证的部分:`ROTATION`(官方 `CGACT` 链里没有像素
     旋转执行体,应是绘制期变换)、`MERGE`/`MERGE2`(官方是 D3D9 绘制)、
     `TRANSITION`(D3D 混合)、`SIZE/SIZE2/RESIZE`(D3D StretchRect)。

4. **`XOR`/`AND` 的 alpha 通道**:当前只作用于 RGB,alpha 保留下层。
   引擎侧的混合公式未反出。
5. **`MERGE`/`MERGE2` 的 alpha 公式**:目前依据手册文字(`MERGE` 按
   上层 alpha 插值,`MERGE2` 直接覆盖)。缺 REA 级验证。
   - **2026-10-28 收口(REA 证据)**:官方合并执行体 `FUN_004405a4`
     (0.495 `yu-ris.exe`,sha256 `4f5eefa3…81138f`,Ghidra 12.1.4)的合并
     分支(`DAT_00684174` / `DAT_00684175` 两块图层表)是:
     ```text
     list[i].ptr = layer[upper]->+0x38[i]      // 上层的每部件纹理
     list[i].val = layer[lower]->+0x34[i]      // 下层的每部件位置
     for each: dev->Draw(ptr, val, ratio*0.06, 0)   // 画到当前渲染目标
     ```
     `dev` = `[iVar9+0x61c]`,渲染目标由命令处理器在此前选定(即**下层
     表面**)。⇒ **`MERGE` 只把上层“画”进下层表面,不读下层的旧像素**;
     不透明度由 D3D9 固定管线的混合状态给出。D3D9 未启用
     `SEPARATEALPHABLENDENABLE` ⇒ alpha 与 RGB 同因子,即
     `a' = a² + lower_a·(1−a)`(与 `quad.wgsl` 的 Normal 同式)。
   - 由此解释并修掉实机问题:立绘换差分的命令序
     `COPY2 → RECTPAINT3 SET=0 → MERGE`,清空后下层 alpha=0,但 MERGE 的
     alpha 是**写回**的(不是保留旧值)⇒ 结果 alpha = 上层 alpha,
     整层可见。旧实现「MERGE 保留目标 alpha」会让整层 alpha=0,经 SRCALPHA
     混合整层不可见 —— 实机症状正是眨眼/换差分闪屏(**skipped=0、RGB 正常,
     只有 alpha 全零**)。
   - 实现:wgpu `merge.wgsl` 的 `out.a = (a·a + la·(255−a) + 127)/255`;
     soft `apply_merge(…, false)` 同一整数式。两后端仍逐位一致。
   - 门禁:`merge_after_rectpaint_clear_on_snapshot_writes_alpha`(跨后端
     一致 + 清空后仍可见)、soft 单测
     `merge_writes_alpha_so_cleared_target_stays_visible` /
     `merge_alpha_uses_same_factor_blend`。
   - 残留:`MERGE2` 的 `/ao` 归一化仍按手册 + 既有实现(语料未在快照清空
     形态下用到);`ratio*0.06` 的每部件缩放因子语义未取证(与本内核
     X/Y=0 的用法无关)。

6. **`RESIZE` 的缩放滤波**:`SIZE` 族用最近邻/双线性区分,`RESIZE` 走
   双线性;官方是否另有滤波选择未取证。

## 3. 不做的部分(显式排除)

- `CGINFO[TEX]`(E-mote 判定)仍属 `deferred(master)`。
- `TEXCOPY`/`TEXSSCOPY` 属纹理拷贝族,不在此档案内。

## 4. 验证

```text
cargo test -p yu-backend-soft --lib          # 53 passed(含 20 条滤镜像素单测)
cargo test -p yu-backend-wgpu --test wgpu    # 35 passed(含 4 条模糊 + 6 条滤镜对拍)
```

单测锚点(`crates/yu-render/soft/src/imagefilters.rs`):
`saido_64_is_half_saturation`、`akarusa_192_is_1_5x`、
`mono_threshold_splits_black_white`、`mosaic_fills_blocks_uniformly`、
`blur_preserves_uniform_color`、`zero_params_are_identity`、
`merge_transparent_upper_keeps_lower`、`merge2_opaque_upper_overwrites`、
`bitwise_applies_to_rgb_only`、`blur2_is_horizontal_only`、
`blur3_is_vertical_only_and_keeps_source_alpha`、`blur4_is_two_pass_box`、
`blur5_is_alpha_only_mask`、`alpha_aware_blur_averages_alpha_too`、
`diffusion_applies_two_pass_permutation_with_msvc_rand`、
`diffusion_below_two_is_identity`、`whirlpool_rotates_by_radius_table`、
`whirlpool_zero_progress_is_identity`、`raster_horizontal_shifts_row_by_sin`、
`raster_vertical_shifts_column_by_cos`、`raster3_combines_both_axes`。

跨后端对拍(`crates/yu-render/wgpu/tests/wgpu.rs`):
`blur_two_pass_rgb_matches_soft`、`blur2_is_horizontal_only_and_matches_soft`、
`blur5_alpha_mask_darkens_background_like_soft`、
`blur6_alpha_aware_layer_takes_shadow_branch`、
`raster_shifts_rows_by_sin_on_gpu_and_matches_soft`、
`raster2_shifts_columns_by_cos_on_gpu_and_matches_soft`、
`raster3_combines_both_axes_on_gpu_and_matches_soft`、
`whirlpool_rotates_by_radius_on_gpu_and_matches_soft`、
`diffusion_permutes_blocks_on_gpu_and_matches_soft`、
`diffusion_matches_soft_across_sizes`。
