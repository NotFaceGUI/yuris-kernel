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
| `MERGE` | `ID`(上层) | `apply_merge_layers(upper,false)` | **REA 已手解内核**(§2.5 已定案) |
| `MERGE2` | `ID`(上层) | `apply_merge_layers(upper,true)` | **REA 已手解内核**(§2.5 已定案) |
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
     旋转执行体,应是绘制期变换)、`TRANSITION`(D3D 混合)、
     `SIZE/SIZE2/RESIZE`(D3D StretchRect)。
   - `MERGE`/`MERGE2` **不再是模型**:两者都定位到逐行 CPU 循环执行体
     (`0x43b33c` / `0x43b8ec`)并逐字节手解了混合内核,公式已定案
     (见 §2.5)。这也是本节旧版「官方是 D3D9 绘制」判断的勘误。

4. **`XOR`/`AND` 的 alpha 通道**:当前只作用于 RGB,alpha 保留下层。
   引擎侧的混合公式未反出。
5. **`MERGE`/`MERGE2` 的 alpha 公式与执行体**:**已定案(RE 级)**。
   两个执行体 `FUN_0043b33c` / `FUN_0043b8ec` 与两套混合内核都已逐字节
   手解,公式见本节末尾「已定案公式」。手册 `cg/cgact.html` 例 23/24
   (`MERGE` 只反映上层不透明度、`MERGE2` 两者都反映)与内核实际的参数
   传递一致。

   #### 勘误(2026-10-31):`0x4405a4` 不是 MERGE

   本节此前把「官方合并执行体」定为 `FUN_004405a4`(0.495 `0x4405a4`),
   并据此把 alpha 写成 `a² + la·(1−a)`。**这条归因是错的**:

   - `FUN_004661ec`(PE 0.495,`0x4661ec`)是运行时库绑定,它做
     `DAT_00801380 = LoadLibraryA("YSEmote.dll")`,`DAT_00801458 =
     GetProcAddress(DAT_00801380, (LPCSTR)0xe9)`。
   - `FUN_004405a4` 入口第一句就检查 `DAT_00801750`(YSEmote.dll 加载
     成功标志),为假时直接 `return 0`;它的合并分支调用的
     `DAT_00801458` 正是上面绑定的 **E-mote 绘制函数**。
   - 仓库里的域事件契约也早记对了:`crates/yu-rt/docs/domain-event-contracts.md`
     §「`EMOTE` 处理体 `0x4405a4`」。⇒ `0x4405a4` 是 **`EMOTE` 域**的
     opcode 处理体,**不是 CGACT 的 `MERGE`**。
   - CGACT 分派表在 `0x7ace28`(`MOV [0x7ace28+i*4], <handler>`,初始化代码
     `0x45d1e0` 起)。表里 `0x4404f0` / `0x4405a4` 只是恰好相邻的两个槽位,
     两者都在 E-mote 分支后面(同样以 `DAT_00801750` 为门)。
   - **结论:CGACT 合并族的真实执行体尚未定位。** 现有实现没有任何 RE 锚点。

   #### 当前实现的依据(只有两条,且都不是 RE)

   1. **像素实测**(主因)。实测 kotoneiro 立绘链路
      (`es_SPExec.yst` 的 `es.SP.PRO.LOADCG`):下层立绘缓冲 `SPLBUF702`
      在眼睛区域有 26773 个像素 `alpha==0` 但 **RGB 非零**(均值 159,117,122;
      素材 PNG 在透明区带残留色),上层差分图 `sp.tmpB` 在同区域有 15426 个
      抗锯齿边像素(alpha 1..254)。若 RGB 按 `cl·(1−a)` 全权重混下层,这些
      「看不见的脏色」会在 a≈128 处按 50% 被拉进可见区 —— 观感正是眨眼时
      眼睛周围那圈不规则缝隙/眼罩。官方的 **lerp 形式**(见下 `co = L.c +
      (U.c−L.c)·强度`)在 `L.a=0` 时强度恒为 0 ⇒ `co = L.c`,脏色进不了
      可见区;这条性质是后来从内核汇编直接读出来的,与像素实测互相印证。
   2. **两条硬约束**同时成立才唯一确定式子:
      - 下层不透明(la=255)⇒ `ao = 255`,与 2b53786 之前的 `out.a = lower.a`
        逐位相同;
      - `RECTPAINT3 SET=0` 清空后(la=0)⇒ `ao = a`,差分不透明处仍可见
        (41ca861 要修的症状)。
      两者只有**归一化 over** 同时满足:
      ```text
      ao = a + la·(1−a)
      co = (cu·a + cl·la·(1−a)) / ao
      ```

   #### 实现与门禁(2026-10-31 按内核汇编重写后)

   - wgpu:`merge.wgsl`(`MERGE`)/ `composite.wgsl` mode 0(`MERGE2`),
     两族各按自己内核的整数式(见下「已定案公式」),不再共用一套。
   - soft:`apply_merge_at_layers` 同一组式子;`MergeTables`(`OnceLock`
     惰性构造)复刻官方两张查找表,热循环里只剩乘法。
   - 门禁:wgpu `merge_does_not_leak_hidden_lower_rgb_at_antialiased_edges`
     (下层透明带残色 + 上层抗锯齿边,断言不泄色且与 soft 逐字节一致)、
     `merge2_matches_soft`、`layer_alpha_semantics_match_soft`、
     `merge_and_merge2_differ_by_at_most_one_lsb`;
     soft `merge_ignores_hidden_lower_rgb_where_lower_is_transparent` /
     `merge_over_opaque_lower_is_plain_interpolation` /
     `merge_writes_alpha_so_cleared_target_stays_visible` /
     `layer_alpha_is_the_source_blend_strength` /
     `merge2_opaque_upper_keeps_official_one_lsb_darkening`。

   #### MERGE / MERGE2 执行体取证(2026-10-31,已定位)

   分派链全部打通(0.495 `yu-ris.exe`,Ghidra 12.1.4):

   ```text
   CGACT 处理体          FUN 0x42c4a4   ([0x7ace28], opcode 2)
     MERGE  参数标志      0x684182 (CGACT 参数下标 34)
       → CALL 0x43b33c   (MERGE 执行体)
     MERGE2 参数标志      0x684183 (CGACT 参数下标 35)
       → CALL 0x43b8ec   (MERGE2 执行体)
   ```

   两个执行体都**不是** GPU draw,而是**逐行 CPU 循环**(与 `FUN_0043b518`
   预处理后按跨度循环),混合内核经函数指针间接调用:

   | | MERGE `FUN_0043b33c` | MERGE2 `FUN_0043b8ec` |
   |---|---|---|
   | 门禁 | `lower.pixels != 0 && upper.alpha != 0` | `lower.pixels != 0`(不看上层 alpha) |
   | 全不透明快路径 (`upper.alpha == 0x100`) | `DAT_005d1618` = `LAB_004028f8`(纯拷贝) | `DAT_005d1620` = `LAB_00402758`(纯拷贝) |
   | 一般路径 | `DAT_005d161c(dst, src, **srcLayerAlpha**, span)` | `DAT_005d1624(dst, src, **dstLayerAlpha**, **srcLayerAlpha**, span)` |

   **唯一实质差异:MERGE 只传上层的层 alpha,MERGE2 连下层的层 alpha 一起传**
   —— 与手册 `cg/cgact.html` 例 23/24 完全一致。

   MERGE 混合内核(`LAB_00402828`)的 alpha 递推(手解 `0x402828` 汇编):

   ```text
   dst.a' = dst.a + ((src.a − T[dst.a][src.a]) * layerAlpha) >> 8
   RGB    = dst.c + ((src.c − dst.c) * effAlpha) >> 16     // 高精度中间量
   ```

   其中 `T` / 第二张表是运行时构造的 256×512 查找表(*.data 的 BSS 区,
   `0x62f4de` / `0x78b6e0`,静态不可读)。代入 `T[d][s] = s·d/255` 后,
   两条实测硬约束**同时成立**:

   | 场景 | 递推结果 | 期望 |
   |---|---|---|
   | 下层清空(d=0)、上层不透明(s=255) | 255 | 255 ✓(41ca861 的症状) |
   | 下层不透明(d=255)、上层 alpha=128 | 255 | 255 ✓(2b 的眼罩症状) |

   且代数上 `dst + src·(1−dst) ≡ src + dst·(1−src)`(两者都是
   `al + au − al·au`),即**标准归一化 over**。⇒ 本仓库 2026-10-31 的修复
   (归一化 over)与官方语义一致;2b53786 的 `a² + la·(1−a)` 被证伪
   (a=128 时 191,而官方给 255)。

   #### 已定案公式(2026-10-31,逐字节手解两套内核)

   `CGACT` 处理体 `0x42c4a4` 分派后,两个执行体各自把**图层不透明度**
   `[层+0x80]`(`CG[A=n]`,0..256)作为独立参数压给混合内核。参数位置已从
   入口序列回溯(`0x43b979 movsx edi,word[ebx+0x80]` 中 `ebx` = 上层、
   `0x43b998 movsx edx,word[ebp+0x80]` 中 `ebp` = 下层),记作:

   | 记号 | 含义 |
   |---|---|
   | `srcA` | **上层**(`ID`)的 `[层+0x80]`,0..256 |
   | `dstA` | **下层**(`ID2`)的 `[层+0x80]`,0..256 |

   > 两个内核的**形参位置不同**,不要用「第 3 参」这种说法跨族套用:
   > `MERGE` 是 `blend(dst, src, srcA, span)`(3 参);
   > `MERGE2` 是 `blend(dst, src, dstA, srcA, span)`(4 参)——
   > 同一个位置在 `MERGE2` 里装的是**下层**的 alpha。本节统一用 `srcA`/
   > `dstA` 指代**角色**,与实现里的 `image.source_alpha` / `image.target_alpha`
   > 一一对应。

   ##### MERGE —— 内核 `LAB_00402828`(`DAT_005d161c`)

   ```text
   T   = L.a·U.a/255                  ; 查表 0x62f4de(字表,行距 0x400)
   w   = (U.a − T)·srcA >> 8          ; >>8 是 sar 截断
   a'  = L.a + w
   U'  = U.a·256/a'                   ; 查表 0x78b6e0,a'=0 的行全 0
   co  = L.c + ((U.c − L.c)·U'·srcA) >> 16
   ```

   两张表由 `FUN_0045c04c` 在启动时构造:

   - `T`:同一算式 `la·ua/255` 的字节版在 `0x45c60a` 生成(magic
     `0x80808081 >> 7` = 除以 255),字表 `0x62f4de` 与之同源;
   - `U'`:`0x45c58f` 循环 `MOV EAX,ESI; CDQ; IDIV EBX`,即 `j*256/a`,
     `ESI` 按 0x100 步进、`EBX` = 行号 1..255。

   三处必须照抄的细节:

   1. **RGB 是 lerp 而非加权平均**。这条天然满足「下层不可见处不泄色」:
      `L.a=0 ⇒ w=U.a ⇒ a'=U.a ⇒ U'=256 ⇒ co=U.c`。
   2. **`srcA` 是源混合强度**:256 = 全混合、0 = 完全不动。`dstA`
      **完全不参与 MERGE**(执行体不传这个参数)。
   3. **`U'` 用整数除法截断**。所以 `U.a=128` 且下层不透明时强度是
      `128/256` 而不是 `128/255` —— 与「按 255 归一化」的直觉差一点点,
      但这是官方的值。

   两个直接推论(已写成门禁 `layer_alpha_is_the_source_blend_strength`):

   - 两层都不透明 ⇒ `w=0` ⇒ `a'=255`、`U'=U.a` ⇒ 输出 = 上层原值;
   - `srcA=0` ⇒ `co=L.c` 且 `a'=L.a` ⇒ 下层完全原样。

   ##### MERGE2 —— 内核 `LAB_00402654`(`DAT_005d1624`)

   ```text
   a1 = L.a·dstA >> 8                 ; **下层**的层 alpha 乘下层像素 alpha
   t  = (U.a + 1)·a1 >> 8             ; `+1` 在乘法左操作数上
   w  = (U.a − t)·srcA >> 8           ; **上层**的层 alpha 乘下层露出量
   a' = a1 + w
   co = L.c + ((U.c − L.c)·srcA >> 8)·U.a / (a'+1)    ; idiv,向零截断
   ```

   与 MERGE 是**三处不同的算术**,不是同一内核的变体:

   | | MERGE `0x402828` | MERGE2 `0x402654` |
   |---|---|---|
   | 下层的层 alpha(`dstA`) | 不使用 | 乘进**下层自己的**像素 alpha(`a1`) |
   | RGB 分母 | 查表 `U' = U.a·256/a'`,再 `>>16` | 内联 `idiv`,分母 `(a'+1)` |
   | 除法方式 | 表值 + 算术右移 | `idiv`(向零截断) |

   ⇒ **层 alpha 全 256 时两者也不是逐位相同**:全不透明上层 + 不透明下层
   时 MERGE 给 `U.c` 原值,MERGE2 给 `L.c + (U.c−L.c)·255/256`,差 1 LSB。
   这是官方自带的量化(分母 `(a'+1)` 防除零的副作用),回归按 ≤1 放宽
   (`merge_and_merge2_differ_by_at_most_one_lsb`)。

   「下层不可见处不泄色」同样成立:`L.a=0 ⇒ a1=0 ⇒ t=0 ⇒ w=0 ⇒ a'=0 ⇒
   den=1 ⇒ co=L.c`;叠 `dstA` 也不会把残色放大。

   ##### 为什么早先的「归一化 over」也能过像素实测

   标准 over(`a + la(1−a)`,`(cu·a + cl·la(1−a))/ao`)与官方的
   `L + (U−L)·U.a/a'` 在 `srcA=256`、`U.a` 与 `a'` 同源时**代数等价**,
   只差 1 LSB 级量化。所以早期靠像素实测(眨眼不泄色、清空后仍可见)
   反推出来的式子能对,但推导出的「参数乘在哪一项」是错的 —— 那版把层
   alpha 折进**像素 alpha**,会同时改变上层权重,与官方的「只缩小下层露出」
   方向不同。现已改成内核原式。

   ##### 接入方式

   `ImageOp` 新增 `source_alpha` / `target_alpha`,在**命令时刻**快照
   (`CG[A=…]` 可在合并之后改写层,回放期读会错)。映射:

   - `source_alpha` = `srcA`(上层),`target_alpha` = `dstA`(下层);
   - 宿主对 `MERGE` 只传 `srcA`(`dstA` 槽填 256,内核不读);
     `MERGE2` 两个都传真值。

   **同时修掉一个真实缺陷**:`base_token` 与 `image_signature` 原先都不含
   层 alpha ⇒ `CG[A=…]` 只改不透明度、不改纹理时,`sync_base` 判「基面未变」
   而跳过快照,合并结果停在旧 alpha 下的值(实测:同场景改下层 A 后画面不
   更新)。两处都已纳入。

   #### 残留(明确未完成)

   - **`CG[Z]` 是否也参与**:`Z` 是图层序,合并结果写回 `ID2` 后层级不变,
     暂按不参与处理。无锚点。
   - `X/Y` 是否参与对齐、`ratio*0.06` 的每部件缩放语义:现按「整面合成、
     忽略 X/Y」处理(2b 之前的既有行为),**无 RE 锚点**。
   - `XOR`/`AND` 的 alpha 通道:当前只作用 RGB、alpha 保留下层,公式未反出。

6. **`RESIZE` 的缩放滤波**:`SIZE` 族用最近邻/双线性区分,`RESIZE` 走
   双线性;官方是否另有滤波选择未取证。

## 3. 不做的部分(显式排除)

- `CGINFO[TEX]`(E-mote 判定)仍属 `deferred(master)`。
- `TEXCOPY`/`TEXSSCOPY` 属纹理拷贝族,不在此档案内。

## 4. 验证

```text
cargo test -p yu-backend-soft --lib          # 59 passed(含 22 条滤镜/合并像素单测)
cargo test -p yu-backend-wgpu --test wgpu    # 34 passed(含 4 条模糊 + 6 条滤镜对拍)
cargo test -p yu-backend-wgpu --test surfaces # 14 passed(含 3 条合并族门禁)
```

单测锚点(`crates/yu-render/soft/src/imagefilters.rs`):
`saido_64_is_half_saturation`、`akarusa_192_is_1_5x`、
`mono_threshold_splits_black_white`、`mosaic_fills_blocks_uniformly`、
`blur_preserves_uniform_color`、`zero_params_are_identity`、
`merge_transparent_upper_keeps_lower`、
`merge2_opaque_upper_keeps_official_one_lsb_darkening`、
`layer_alpha_is_the_source_blend_strength`、
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
