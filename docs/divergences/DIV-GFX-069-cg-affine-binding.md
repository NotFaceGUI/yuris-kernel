# DIV-GFX-069 · `CG` 仿射/滤镜参数的槽位绑定与绘制缺口

- 状态:**fixed(绑定)**(绘制侧合成公式仍 open,见 §5)
- 关联:DIV-GFX-003(绑定)、DIV-GFX-011(参数分类)、DIV-GFX-027(IDNO)、
  DIV-GFX-031/039(CGACT 参数分类)
- 证据等级:REA 0.495 静态反汇编 + 运行时结构体默认值(`shipped-artifact`)

## 1. 现象

`CG[ID=… FX=… FY=… FBX=… FRX=… …]` 是 ERIS 立绘/特效的常规写法
(`es_Sprite.yst` 每帧一条,同时给出 `FX FY FBX FBY FRX FRY FRZ FCX FCY
FSD FMD FID F`)。旧内核把这一族**原样塞进** `Transform.filter`,并给图层
打 `gfx.filter.unsupported`,于是:

- `CG[FX=…]` 完全不移动图层(引擎里 `FX` 就是图层 X 位置);
- 立绘的缩放/旋转/中心参数无处落地,`CGINFO` 也无法回读;
- 每帧一条「槽位绑定未取证」诊断,把真正缺的「绘制公式」问题淹没。

## 2. 官方行为(0.495,sha256 `4f5eefa3…`)

### 2.1 参数是按「声明下标」取的,不是按固定槽号

命令执行前 `FUN_00429fd1` 做一次参数预扫:

- `memset(&DAT_00684160, 100)`;
- 对第 `i` 个参数:`name_id = *item`、`type = item[2]`、`op = item[3]`,
  然后 `(&DAT_00684160)[name_id] = i`(`0x42a078`);
- 处理体再读 `[0x684160 + 声明下标] → slot → [slot*8 + 0x683740]`。

所以「声明下标」就是 `ysc.ybn` 里该命令参数表的顺序。CG 的下标表
(`0x42a514..0x42bcb0` 全量读取点还原)与 `probe_param_bytes` 输出一致。

### 2.2 逐项绑定(写入点 = CG 处理体 `0x42a514` 的属性块)

| 参数 | 下标 | 读取 | 写入 | 字段 |
|---|---|---|---|---|
| `X` | 4 | `0x42aa2e` | `0x42aa9c` | `+0x1b0` = `X*100`(同时 `+0x68` = X) |
| `FX` | 15 | `0x42aae5` | `0x42ab3b` | `+0x1b0` = FX(同时 `+0x68` = FX/100) |
| `Y` | 5 | `0x42ab82` | `0x42abf0` | `+0x1b8` |
| `FY` | 16 | `0x42ac2c` | `0x42ac82` | `+0x1b8` |
| `FBX` | 17 | `0x42b0ed` | `0x42b10f` | `+0x1c0` |
| `FBY` | 18 | `0x42b14d` | `0x42b16f` | `+0x1c8` |
| `FRX` | 19 | `0x42b1d1` | `0x42b1ff` | `+0x1f8`(f32) |
| `FRY` | 20 | `0x42b268` | `0x42b296` | `+0x1fc`(f32) |
| `FRZ` | 21 | `0x42b2ff` | `0x42b32d` | `+0x200`(f32) |
| `FCX` | 22 | `0x42b372` | `0x42b394` | `+0x1d0` |
| `FCY` | 23 | `0x42b3d2` | `0x42b3f4` | `+0x1d8` |
| `FEX` | 24 | `0x42b432` | `0x42b454` | `+0x1e0` |
| `FEY` | 25 | `0x42b492` | `0x42b4b4` | `+0x1e8` |
| `FEZ` | 26 | `0x42b4f2` | `0x42b514` | `+0x1f0` |
| `FSD` | 27 | `0x42a760` | `0x42a798` | `+0x204`(i32) |
| `FMD` | 28 | `0x42b54b` | `0x42b583` | `+0x208`(i32) |
| `FQU` | 29 | `0x42b5a4` | `0x42b5ac` | `+0x20c`(i8,**直接赋值**) |
| `F` | 31 | `0x42b5ff` | `0x42b63a` | `+0x214`(i16) |
| `FID` | 30 | `0x42b5bb` | `0x42b5c6..0x42b5d8` | 字符串拷入 `+0x210` 缓冲 |

### 2.2.1 「从不读取」的精确清单

CG 处理体入口 `0x42a514`(`mov eax,1; call 0x429fd1`),体长到
`0x42bcb0`;全量扫描该区间内所有 `[0x684160+下标]` 与
`mov reg,0x6841xx`(间接形态),**没有**读取点的下标:

| 命令 | 未读下标 | 参数 |
|---|---|---|
| `CG` | 9 / 10 / 36 / 42 | `TSX` / `TSY` / `MFILE` / `CASH` |
| `CGACT`(入口 `0x42c4a4`) | 8 / 30 | `Z2` / `ROTATION2` |

`DXDRAW`(CG 下标 14)**有**读取点 `0x42b6b7`,但只看全局开关
`DAT_008012e4`(非零才强制重绘),不是图层属性。

### 2.3 默认值(层构造 `FUN_00465730`,分配 0x640 字节)

```
+0x1c0 = 0x2710 (10000)  FBX   ← 绘制侧 ÷100 = 100%
+0x1c8 = 0x2710 (10000)  FBY
+0x204 = 0x80            FSD
+0x208 = 0x808080        FMD
+0x1b0/+0x1b8/+0x1d0/+0x1d8/+0x1e0/+0x1e8/+0x1f0/+0x1f8/… = 0
```

### 2.4 绘制侧

`FUN_00414ba4` 行 117-125:`fild qword [esi+0x1b0/1b8/1c0/1c8/1d0/1d8]`、
`fld dword [esi+0x1f8/1fc/200]`,随后统一 `/100.0`;`FEX/FEY` 在
`0x414031/0x41403b` 另按 i64 读取。属性复制块(`0x4307ad..0x430845`)
把 `FEX/FEY/FEZ/FSD/FMD/FQU/F` 一并复制 ⇒ `COPY` 携带这些字段。

### 2.5 属性算子

所有仿射字段走 `0x42bd3c`(i64)或 `0x42bdbc`(带回绕):算子码取自参数
`op` 字节 —— 0 赋值 / 1 加 / 2 减 / 3 乘 / 4 除。`FQU` 是唯一例外
(`0x42b592..0x42b5ac` 直接赋值)。

## 3. 内核行为(修复前)

`FilterParam` 只做名字归档 → `Transform.filter`;不写任何位置/缩放字段;
`CG[FX]` 对画面零影响。

## 4. 处置

- `Transform.filter: FilterTransform` → `Transform.affine: Affine`,
  字段与 §2.2 一一对应(名字、类型宽度、默认值见 `Affine::engine_default`)。
- `FX/FY` 写入 `transform.x/y`(= 引擎 `+0x68`,像素口径,`value/100`);
  `X/FX` 是同一物理量的两种单位,后写覆盖先写(与引擎同序)。
- 其余字段存**引擎原值**(百分位/角度×100/位掩码),绘制侧统一 ÷100。
- 诊断:`gfx.filter.unsupported`(绑定未取证)退役,改为
  `gfx.affine.unverified`(**绘制合成公式**未对拍),且只在命令真的给了
  仿射参数时发一条(无参更新不再刷屏)。
- `FQU` 按引擎语义直接赋值(不参与算子)。

## 5. 仍未闭

### 5.1 绘制侧合成公式(DIV-GFX-070)

后端目前只用 `transform.x/y` 做轴对齐矩形绘制,尚未应用
`FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY`。缺口是**合成公式**
(缩放/旋转/中心的组合顺序、16.16 定点与半像素偏移的精确位置),
不是绑定 —— 需要在 `FUN_00414ba4` 上做逐行还原 + 官方微场景像素对拍。
在此之前图层标 `gfx.affine.unverified`,后端不得声称已对齐。

### 5.2 `+0x68` 与 `+0x1b0` 是两个字段(COPY 后查询口径差异)

引擎把位置存两份:`+0x68`(像素,`CGINFO[X]` 读它,`0x43d816`)与
`+0x1b0`(百分位,绘制读它,`0x414bed`)。每次 `CG[X]`/`CG[FX]` 写入都会
同时更新两份,所以平时等价;但**属性复制块 `0x430716..0x430845` 只复制
`+0x1b0..+0x214`(含 FBX/FBY/FCX/FCY/FRX/FRY/FRZ/FEX/FEY/FEZ/FSD/FMD/
FQU/F),不复制 `+0x68/+0x6c`**。

内核只有一个 `transform.x/y`(像素口径),`copy_attributes` 会把它一起复制
⇒ 绘制位置与引擎一致,但 `CGACT[COPY]` 之后 `CGINFO[X/Y]` 会报**源层**的
位置,引擎报目标层的旧值。

影响面:语料 127 条 `CGINFO` 全部只查 `SX/SY`(无 `X/Y/FX/FY` 查询),
当前零命中;等实现 `CGINFO[X/Y]` 时若要对齐,需把位置拆成
`+0x68`/`+0x1b0` 两个字段(或让 COPY 不回填像素视图)。

## 6. 复核

```bash
cargo test -p yu-gfx transform            # 绑定/默认值/FQU 语义
cargo test -p yu-gfx --test display_list_golden   # 序列化形态
YURIS_SAMPLE_ERIS_YSBIN=<project>/data/ysbin \
  cargo test -p yu-gfx --test eris_first_screen   # 语料首屏零值差异
```

首屏 golden 复核结论:唯一差异是 `transform.filter:[]` → `transform.affine:{…}`,
**数值差异 0 处**(首屏不含立绘,`FX` 路径未触发)。

取证工具(只读,可复跑):

```bash
# 参数槽位读取点扫描(把 [0x6841xx] → 声明下标 → 参数名)
python tools/rea/scan_cg_slots.py <project>/yu-ris.exe 0x42a4a0 0x42b800
# 运行期命令参数表(声明序/种类/检查器)
YURIS_SAMPLE_ERIS_ROOT=<project> python tools/rea/param_table_probe.py 8 1 2
cargo run -p yu-gfx --example probe_param_bytes -- <project>
```
