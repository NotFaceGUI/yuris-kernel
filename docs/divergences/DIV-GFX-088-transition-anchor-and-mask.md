# DIV-GFX-088 · `TRANSITION` 的 X/Y 落点与 `TRANSITION2` 三源遮罩过渡

- 状态:**fixed(no-manual)**(2026-11-15)
- 取证对象:官方 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`):
  `0x43bad0`(TRANSITION)、`0x43bca0`(TRANSITION2)、
  `FUN_0043b518`(落点/裁剪)、`FUN_0043c110`(权重表)、
  `FUN_0043c008`(逐像素混合)
- 相关:DIV-GFX-061(TRANSITION 基本语义)、DIV-GFX-086(合并落点)

## 1. 官方语义(REA 逐指令)

### 几何(两族共用)

`0x43bad0` / `0x43bca0` 入口都按 `src.width+X`、`src.height+Y`、`X<0`、
`Y<0` 计算四边剪切位(8/2/4/1),再调 `FUN_0043b518`:

```text
目标起点 = (X, Y) 夹到 [0, dst)
源起点   = (0, 0)(越界的源侧由剪切位跳过)
跨度/行数 = 源 A 尺寸 ∩ 目标表面
```

目标矩形外**不写像素**(保留目标旧像素)。`X2/Y2` 不参与过渡。

### TRANSITION(`0x43bad0`)

`ID`=源 A、`ID2`=源 B、`ID3`=目标;`SET`=进度(权重 `SET+1`,1..256)。
无 `X/Y` 时等价于整层 lerp(既有实现,不动)。

### TRANSITION2(`0x43bca0`)

参数表顺序(0.495 ysc):

```text
ID  ID2  ID3  ID4  … SET SET2 …
```

dispatcher 映射(0x42e3b7..0x42e408):

```text
寄存器 EAX/EDX/ECX = ID/ID2/ID3;栈参 [esp+0x40] = ID4
SET = [esp+0x58], SET2 = [esp+0x54]
```

执行体角色:`ID`=源 A、`ID2`=源 B、`ID3`=遮罩、`ID4`=目标。

脚本侧(`es_sprite.yst` pc6092/6111)的 `SET` 与 `TRANSITION` 是**同一个
进度变量**(表达式 `56030040ee0b48030040890e…` 完全相同),`SET2` 是独立
变量(`56030040d40b…`)⇒ 映射与本实现一致。

权重表 `FUN_0043c110(k = 256−SET, SET2)`:

```text
SET2 == 256: table[m] = (m < SET) ? 0 : 256        // 硬阈值
否则: acc = m·SET2
      v = (acc>>1) + ((SET2>>1)·k) + SET2·(−128) + k
      table[m] = min(v,255) + 1                    // 1..=256
```

逐像素 `FUN_0043c008`(遮罩字节 = 内存序 BGRA 的 **B**,本仓库 RGBA 的蓝):

```text
SET = 0    → 原样拷贝源 A(含 alpha)
SET >= 255 → 原样拷贝源 B
其余: w = table[mask.B]
      w < 2 ⇒ 源 B
      否则 out[c] = B[c] − ((B[c]−A[c])·w >> 8)   // 8 位回绕,不 clamp
```

## 2. 本内核修法

- `ImageOp` 新增 `source3`(仅 T2 的遮罩;不进 display JSON);
- `cgact_transition` 统一处理两族:记录 `x/y`、`SET/SET2`、`source3`、
  目标 `ID3`/`ID4`;缺 ID/层/组目标按原诊断跳过;
- soft:`transition_into`(锚定 lerp,复用 `lerp_weighted`)/ 
  `transition2_into`(端点拷贝 + 权重表 + 官方整数式);
- wgpu:新增 `transition.wgsl`(base + from + to + mask + 参数,四纹理
  bind group)与 `AuxPipeline::Transition`;矩形外返回 base;
- 无 `X/Y` 的 `TRANSITION` 继续走原 `lerp.wgsl`/`blend_transition`
  (整层、画布 = to),避免回归;
- `image_signature` 纳入 `source3`;`reads` 集合纳入 `source2/source3`;
  `image_full_overwrite` 对锚定过渡/T2 返回 false(读目标旧像素);
- 锚定过渡/T2 仅当写矩形**整面覆盖**目标、且目标不在三源中时按纯写折叠;
  部分落点保留历史(矩形外读目标旧像素)。折叠的“后续读取”检查覆盖
  `source/source2/source3`:目标被后续项当第二源/遮罩读取时不折叠。

## 3. 语料用量

- `TRANSITION`:官方 ERIS 1 次、<0.474 样本> 1 次,**均不带 X/Y** → 旧路径不变;
- `TRANSITION2`:<0.474 样本> `es_sprite.yst` pc6111 1 次(SET/SET2 + 4 ID),
  官方 ERIS 0 次;此前被 `gfx.imageop.unsupported` 跳过,现在执行。

## 4. 回归

```text
cargo test -p yu-gfx --test parity_fixes cgact_transition
  → 3 passed(含 anchor x/y、T2 mask/dst/set2 登记、旧路径 x/y=None)
cargo test -p yu-gfx --test parity_fixes transition2_
  → 5 passed(含缺遮罩拒绝、整面覆盖折叠、部分落点保留历史、
    source2 读取阻断折叠)
cargo test -p yu-backend-soft transition
  → 10 passed(含锚定/裁剪、T2 端点/硬阈值/SET2=0、窄源 B/遮罩零填充)
cargo test -p yu-backend-wgpu --test surfaces transition
  → 7 passed(含同帧目标基底链,GPU 与 soft 逐字节一致)
cargo test -p yu-backend-soft -p yu-backend-wgpu -- --test-threads=1
  → 全绿(soft 68;wgpu surfaces 32)
cargo test -p yu-gfx / -p yu-engine
  → 全绿
```

## 5. 残余/边界

- 官方 `X/Y` 是 32 位整数;小数落点未取证,本实现 `round()`(两后端一致)。
- `SET2` 缺省按 0(官方 dispatcher 读 `[esp+0x54]` 时未写则保持槽初值;
  语料 T2 始终显式给 `SET2`);`SET2=0` 退化为均匀混合。
- T2 的三源按官方假设同尺寸;尺寸不同时本实现越界按零采样,官方是直接
  读各自缓冲(未定义),语料未出现。
- 官方 `FUN_0043c110` 硬阈值分支的零循环从下标 1 开始,`table[0]` 保留
  **上一次**表的残留值;本实现按 0 处理。只在 `SET2=256`、遮罩 B=0 且此前
  执行过非硬阈值 T2 时产生差异;语料未观察到混用。
- `SET2>256` 官方仍走渐变公式,本实现钳到 256(硬阈值);负 `SET` 官方
  进入 `k>256` 分支,本实现按 `SET=0` 拷贝源 A。脚本量纲取证为 0..256,
  越界值不承诺逐位一致。
- 目标层没有任何基图(`TextureRef::None`)时,官方执行体看到目标缓冲指针为
  空会整段跳过;本实现按「目标无基图」报错/跳过,不写入。语料 T2 目标均为
  带 `SX/SY` 的空表面(`$LB3`),不触发该分支。
- 整面覆盖目标的锚定过渡/T2 现按纯写折叠;只写部分矩形(矩形外读目标)
  仍不参与折叠,长时间逐帧部分落点会受 `MAX_OP_LOG` 上限约束。折叠前后
  整面覆盖的像素结果逐位相同。
