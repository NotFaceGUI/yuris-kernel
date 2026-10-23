# E-mote 运行时台账(第三方插件,不属于引擎 shader 集)

> 结论:E-mote 不是 `yu-ris.exe` 内置的 effect/pass,而是**可选插件运行时**。
> 引擎用 `LoadLibraryA` 动态加载 `YSEmote.dll`,后者再依赖 `emotedriver.dll`
> (M2 E-mote SDK);模型是 `.psb`,渲染器自带 58 个 D3D9 shader。
> 引擎主体自身的 shader 集仍只有 4 个 ps_2_0(`docs/shader-parity.md`)。
>
> 本台账记录证据、现状与支持计划;未实现部分显式挂账,不伪造。

## 0. 证据源

| 项 | 值 |
|---|---|
| 引擎 | SDK 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`;见 `docs/shader-parity.md` §0) |
| 插件 | `system/forE-mote/YSEmote.dll`(2014-10-03,256 个无名导出)、`emotedriver.dll`(2014-07-28,10 个导出) |
| 数据 | `<project-eris>/data/cg/stand/emote_test.psb`(`PSB\0` v2,8,832,512 B) |
| 手册 | `<sdk495>/マニュアル/E-moteはじめに.txt`、`E-moteERIS命令マニュアル.txt`(CP932) |
| 语料 | `project-eris`;120 帧 census(`target/tmp/j4-emote/census120.txt`) |
| 反汇编 | REA/Ghidra:`FUN_004661ec`(加载器)、`FUN_00460924`(配置);`fxc /dumpbin`(58 个 blob) |
| 实现仓库 | <https://github.com/NotFaceGUI/yu-emote>(公开;git submodule 挂在 `yu-emote/`;MIT OR Apache-2.0;当前 M1 完成、M2/M3 core) |

## 1. 引擎侧接入(已取证)

### 1.1 动态加载

`FUN_004661ec`(`0x4661ec`)逐字:

```c
hLibModule = LoadLibraryA("emotedriver.dll");
if (hLibModule != NULL) {
    FreeLibrary(hLibModule);                 // 只探测存在性
    DAT_00801380 = LoadLibraryA("YSEmote.dll");
    if (DAT_00801380 != NULL) {
        // GetProcAddress(YSEmote.dll, ordinal 0x1/0x2/0x3)
        // GetProcAddress(YSEmote.dll, ordinal 0x65 .. 0x100)
        DAT_00801750 = 1;                    // "E-mote 可用" 标志
    }
}
```

- 引擎字符串:`E-mote`、`E-mote `、`EmotePlayer`、`YSEmote.dll`、`emotedriver.dll`。
- `YSEmote.dll` 自己导入 `emotedriver.dll` 的 `EmoteCreate` 与
  `EmoteFilterTexture`;`emotedriver.dll` 导入 `d3dx9_43.dll`
  (`D3DXMatrixTranspose/Multiply`、`D3DXVec3Project`、`D3DXMatrixLookAtLH`)。
- **结论**:E-mote 是可选组件;DLL 不在时引擎仍可启动,只是
  `DAT_00801750=0`、E-mote 相关调用不生效。它不是引擎 FXO 的一部分。

### 1.2 命令面

| 层 | 命令 | 证据 |
|---|---|---|
| YSCM(引擎) | `EMOTE`(opcode 处理体 `0x4405a4`)、`EMOTEINFO`(`0x441014`) | `crates/yu-rt/docs/coverage-matrix.md` |
| ERIS 脚本 | `\SP.ST(file, fade)` 加载 `.psb`;`\SP.EM.VAR/PLAY/DPLAY/STOP/DSTOP/VO` | `E-moteERIS命令マニュアル.txt` |

`SP.EM.*` 是 ERIS 脚本层的子程序(YSCM 之上);内核当前只把 `EMOTE`/
`EMOTEINFO` 作为 `Domain::Emote` 域事件发出,不做子系统语义。

### 1.3 PSB body key 的来源(已取证)

`PEmotePlayer` 构造器 `0x10001890` 在堆上建 `PSBFilter`(`0x100017f0`,
0x48 B),并把 6 词流状态写进栈:

```text
0x018D9: [esp+0x18] = 0x1007185C          ; PSBFilter vftable
0x018E1: [esp+0x1C] = 0x075BCD15
0x018E9: [esp+0x20] = 0x159A55E5
0x018F1: [esp+0x24] = 0x1F123BB5
0x018F9: [esp+0x28] = eax                 ; key
0x018FD: [esp+0x2C] = 0
0x01901: [esp+0x30] = 0
```

`eax` 来自 `0x1005B907(0x10071350)` → `0x1005B8F1` → `strtol(str, 0, 10)`。
`0x10071350` 是 ASCII `"391141577"`;`391141577 = 0x175058C9`,正是
`emote_test.psb` 实测的 body key。

结论:**key 是每份 `emotedriver.dll` 内嵌的十进制字符串**,不是 PSB 文件
属性,也不是引擎参数/文件名 hash。游戏换 DLL 即换 key;`yu-emote` 只从
调用方(`YURIS_EMOTE_KEY`/配置)取 key,不硬编码。

### 1.4 `YSEmote.dll` ABI(已取证)

`YSEmote.dll` 只有 80 个导出,全部是跳板:

| 序号 | 目标 | 说明 |
|---|---|---|
| 1 | `return 1000` | 版本/ABI 号 |
| 2 | `emotedriver!EmoteCreate(const InitParam&)` | 返回 `IEmoteDevice*` |
| 3 | `emotedriver!EmoteFilterTexture(buf,len,callback)` | 纹理过滤/回调 |
| 101..121 | device vtable `+0x00..+0x50` | `PEmoteDevice` 22 槽 |
| 201..256 | player vtable `+0x00..+0xdc` | `PEmotePlayer` 56 槽 |

引擎加载器 `0x4661EC` 把序号 101..121 存到 `0x801384..0x8013D4`、
201..256 存到 `0x8013D8..0x8014B4`;序号 1/2/3 存在
`0x8014C0/0x8014BC/0x8014B8`。

`InitParam` 的实际访问(构造器 `0x10001110`):

```c
[0]  = 带 vtable 的对象指针;调用 (*(*p)[0]+4)(p)  // 设备初始化
[1]  = malloc 覆盖(可选)
[2]  = free   覆盖(可选)
```

`PEmoteDevice`(0x10 B, vftable `0x100716CC`)只做设备/屏幕管理:引用计数、
`+0x228/+0x22C/+0x270/+0x274/+0x2A0/+0x2A4/+0x2A8/+0x3B4/+0x3B8` 状态、
`+0x44 SetScreenRect(int,int)`、`+0x48 CreatePlayer(out**,int,int)`(构造
`0x18` B 的 `PEmotePlayer`,vftable `0x1007172C`)、`+0x4C Reset`。

`PEmotePlayer` 的 56 个槽中与运行时直接相关的:

| 槽 | 地址 | 事实 |
|---|---|---|
| 3 | `0x10001BD0` | `(player_impl+0x10)` 交给 `0x1000D190` |
| 9/10 | `0x10001C50/0x10001C90` | 往返 `player_impl+0x358`(float) |
| 11/12 | `0x10001CA0/0x10001CB0` | 往返 `player_impl+0x30`(byte) |
| 13..18 | `0x10001CC0..0x10001D10` | 往返 `+0x24/+0x28/+0x2C`(float) |
| 19 | `0x10001D20` | 4×float → `0x10018770` |
| 24..26 | `0x10001E10..0x10001E80` | 读 `player_impl+8` 的 `+0xF4/+0xF8/+0x150` |
| 40/41/42 | `0x10002190/0x100021A0/0x100021F0` | 变量数量/按索引取变量(`0x1001A010`) |
| 43..50 | `0x10002210..0x10002780` | 载入/查询/替换 motion(转发 `0x1001A130..0x1001A8A0`) |
| 51 | `0x1000E040` | 更新 |
| 53 | `0x1000FCD0` | 按 float 推进(内部依次走 0x1001D4B0/0x1001C8F0/0x10014FC0/0x10016820/0x10017420/0x10017710/0x10018700/0x100189C0/0x1000B730) |
| 54 | `0x100316D0` → `0x10031640` | 渲染入口 |
| 55 | `0x100028D0` | motion 列表/结尾处理 |

### 1.5 `EMOTE` / `EMOTEINFO` 处理体(反编译事实)

`EMOTE`(`0x4405A4`)在 `DAT_00801750 != 0` 时:

1. 用 `DAT_00684160`(角色名槽)+ `"."` + 参数值字符串(`FUN_0046A7C0`,
   基数 10)拼出对象名,经 `FUN_004141E4/FUN_00414559` 解析,取
   `obj+0x61C` 作为 E-mote player;
2. 按参数类型分派:
   - type 2(字符串):`FUN_00465FF0(player, name, value)`;
   - type 1(标志):`FUN_00466074(player, name)`;
   - type 0:`FUN_00465FD8(player)`;
3. 可见性/启用:`(*DAT_00801404)(player, 0|1)`;`0x801404` 对应
   YSEmote 序号 212 → player 槽 11(`+0x2C`,`player_impl+0x30` byte);
4. 批量数值变量:`(*DAT_00801458)(player, id, value, time*0.06, 0)`;
   `0x801458` 对应序号 233 → player 槽 32(`+0x80`,`0x10001FB0`)。
   **0.06 是脚本时间到播放器时间的换算系数**(证据:该乘法紧邻调用)。

`EMOTEINFO`(`0x441014`)走 `(*DAT_0080148C)`;`0x80148C` 对应序号 246 →
player 槽 45(`+0xB4`,`0x10002380`),返回值与 `1` 比较。字段语义仍未见
完整证据,保持"未取证字段显式拒绝"。

### 1.6 纹理格式(已取证)

`source/tex#N/texture/type` 在样本中是 `RGBA4444`。`emotedriver.dll`
`0x1005543B..0x10055712` 把它映射到 `D3DFMT_A4R4G4B4`(`0x1A`,16 bpp),
转换器 `0x10054BA0` 逐像素:

```text
word = little-endian u16
a = word >> 12; r = (word >> 8) & 0xF; g = (word >> 4) & 0xF; b = word & 0xF
out = (a*17)<<24 | (r*17)<<16 | (g*17)<<8 | (b*17)
```

用该顺序解码官方 atlas 得到正确肤色/发色;反向 nibble 顺序会把肤色与
发色交换(已用官方图集人工比对)。

## 2. E-mote 自带 shader 集(58 个 unique)

从 `emotedriver.dll` 的 D3D9 version token 扫出,按内容去重:

| 模型 | unique | 参数/变体 |
|---|---:|---|
| `ps_2_0` | 27 | 13 × `cTex`;13 × `cTex+cAlphaMaskTex`;1 × `cTex+cPixelateTex` |
| `ps_3_0` | 27 | 与 ps_2_0 一一对应;masked 组多 `float2 cScMul` |
| `vs_2_0` | 3 | 2 × `float4x4 cMat + float4 cScOffset`;1 × `cMat` |
| `vs_3_0` | 1 | `cMat` |

代表性反汇编(`fxc /dumpbin`):

```text
ps_2_0 (0x7ec00, 220 B):        texld r0,t0,s0; mov r1.xzw,r0; mov r1.y,r0.w; mov oC0,r1
vs_2_0 (0x80e10, 432 B):        cMat(c0..c3), cScOffset(c4);
                                 mad r0, v0.xyzx, (1,0,0.5,-0.5), ...
                                 dp4 oPos.{z,w,x,y}; mad oT1.xy, ... , c4
```

- 这些 shader 是 E-mote 网格渲染器(骨骼矩阵 `cMat`、屏幕偏移
  `cScOffset`、纹理 `cTex`、Atlas 遮罩 `cAlphaMaskTex`、像素化
  `cPixelateTex`)的私有机型,**不是**引擎的 4 个 effect pass。
- 因此“引擎内置 shader 是否实现完”与“E-mote 是否支持”是两件事。

### 2.1 重新提取与逐变体归类(2026-10-22)

`target/tmp/j4-emote/unique` 已被清理,本次从 `emotedriver.dll` 的指针表
`0x100866C0..0x100867AC` 重新提取。D3D9 token 规则(用自编译 cso 校准):
指令长度在 bits 24..27(`1 + length` 个 DWORD),注释 token `0xFFFE` 的长度
在 bits 16..30;末尾 `0x0000FFFF` 后跟 4 B 实际长度并 16 B 对齐。

结果与旧台账一致:**58 unique**:

| 模型 | 数量 | 指针表区 |
|---|---:|---|
| `ps_2_0` | 27 | `0x100866C0..0x10086728` |
| `vs_2_0` | 3 | `0x1008672C..0x10086734` |
| `ps_3_0` | 27 | `0x10086738..0x100867A0` |
| `vs_3_0` | 1 | `0x100867A4..0x100867AC`(3 个指针指向同一 blob) |

27 个像素程序不是 27 种独立算法,而是以下特征的组合(索引与官方表一致):

| 组 | 索引 | 特征 |
|---|---|---|
| plain | 0,1,2,6,7,8,12,13,14,18,19,20,24 | `cTex`(+`v0`)、×1/×2、`1-c`/`1-2c`、RGB 预乘、纯顶点色、掩码写 G |
| masked | 3,4,5,9,10,11,15,16,17,21,22,23,25 | 上述组合 + `cAlphaMaskTex.g` 乘 alpha(25 把 `g` 也乘 mask) |
| pixelate | 26 | `cTex` + `cPixelateTex`,alpha 乘像素化采样 |

`ps_3_0` 的 masked 组额外带 `float2 cScMul`(屏幕空间缩放)。顶点程序:
`cMat`(basic)、`cMat+cScOffset`(屏幕 UV,两种投影写法);`vs_3_0`
等价于 basic。

实现映射见 `yu-emote/src/render.rs` 的 `PIXEL_PROGRAMS` / `VERTEX_PROGRAM_LIST`;
每个变体的 WGSL 已落成独立文件:`yu-emote/shaders/ps_2_0/00.wgsl`..
`26.wgsl`、`ps_3_0/00.wgsl`..`26.wgsl`、`vs_2_0/00.wgsl`..`02.wgsl`、
`vs_3_0/00.wgsl`(共 58 个),由 `cargo run --bin gen-shaders` 从特征表生成,
测试 `shader_files_match_generator` 防止漂移。

## 3. 语料现状(120 帧 census)

| 项 | 结果 |
|---|---|
| `EMOTE` | 静态 13 处;运行时 8 次:`BUST=128 ×2`、`HAIR=128 ×2`、`PARTS=128 ×2`、`SMOOTH=1 ×2` |
| `EMOTEINFO` | 静态 3 处;本窗口 0 次 |
| E-mote 模型 | `save/data/cg/stand/emote_test.psb` 存在(8.8 MB) |
| 首 120 帧是否加载模型 | **否**(`CG FILE` 样本中无 `emote_test`) |
| `SYSTEMINFO[EMOTE]` | 内核当前硬编码返回 **0**(`handlers.rs` 的 `want[3]` 分支)。官方返回 `DAT_00801750`(DLL 加载成功=1);在运行时完成前保持 0 是诚实的 capability 回答,否则脚本会走 E-mote 分支但无渲染 |
| 当前内核行为 | 只发 `Domain::Emote` 事件;不渲染 E-mote 角色;`.psb` 不在图像解码器能力内 |

> 结论:title/boot 窗口不依赖 E-mote;后续角色场景会依赖。语料确实使用
> E-mote 参数命令,所以“完全不支持”会在角色场景缺失。

### 3.1 当前实现(2026-10-22)

`yu-emote` 独立仓库已完成 M1,并把 M2/M3 推进到 core:

- PSB/MDF 解析(`emote-psb`,MIT);
- body-only PSB 解密(6 词流,key 为每游戏 DWORD)+ `psb-key-search`;
- `psb-info` 确定性摘要(version/资源数/top-level/节点计数/深度);
- YU-RIS 适配骨架:`EMOTE` 参数记录、`EMOTEINFO` 未取证字段显式拒绝、
  capability、`suspend/resume`、dirty 帧标记。

样本 `emote_test.psb`(body key `0x175058c9`)已解析:v2、2 个资源、
`metadata` 存在、5863 objects、max depth 36;与 `eluna` 的
`names=405/strings=388/resources=2` 一致。

2026-10-22 M1 完成(`psb-dump` 实测):

```text
tex#0 2048x2048 RGBA4444 icons=97  resource=1 (8,388,608 B, sha256 b73e697e…)
tex#1  512x256  RGBA4444 icons=13  resource=0 (   262,144 B, sha256 44819f5b…)
6 objects / 21 motions / 234 layers / 888 layer keyframes
24 variables / 11 timelines / 17 transition controllers
```

解码后的 RGBA8 逐字节哈希稳定(`tex#0` `55b0d647…`,`tex#1` `89e9dd77…`),
两次 `Model` JSON 序列化逐字节一致。

2026-10-22 继续完成 M2/M3 baseline:

- `yu-emote::pose`:frameList 选择、type 0/2/3 语义、`ti` 量化、角度最短路径、
  `inheritMask` 通道继承、`transformOrder` 四阶段、coordinate 平面(XY/XZ)、
  type-3 嵌套 motion(子时间 = parent - frame_start + timeOffset)、
  motion `priority` 发射顺序;
- `yu-emote::plan` + `soft`:atlas 矩形/枢轴、4×4 Bezier 网格、corner color、
  确定性 CPU 光栅与 PNG;
- 样本 time 0:229 个 pose 节点、145 个可见 drawable;跳过 115 个 blank
  proxy 后实际 30 个彩色 draw item、3960 三角形、2 个 composite mask
  source;`psb-render` 输出完整角色(头发/眼/眉/身体/裙/腿),人物在
  model 空间的 bbox 约 `x -643..429, y -971..767`,预览需宿主/工具做
  fit 变换(官方引擎由 CG 图层变换提供)。

2026-10-22 M2/M3 正确性修复(像素输出已验证):

- **绘制顺序以解析 Z 为准**:native `MMotionManager` 先按 `priority` 发射,
  再按 `DrawFrameInfo +36`(=`LayerInfo +620`,累积坐标 Z)升序 `std::sort`。
  样本 Z:後髪 -10、胴体 0、輪郭 3、前髪 4、瞳 5..6;仅用 priority 会让
  後髪盖住脸。`priority` 只作同 Z 的确定性 tie-break。
- **motion 级参数表**:`parameterize` 索引的是**所属 motion** 的
  `parameter` 数组,不是根表。`Motion` 现保存 `parameters` 与 motion 级
  `parameterize`;眼睛/嘴/眉/泪/頬 motion 由 `face_eye_open`/`face_talk`/
  `face_eyebrow`/`face_tears`/`face_cheek` 驱动其时间线。
- **type-0 保持**:终帧 type 0 必须回退到同 frameList 内最近一个有 content 的
  关键帧;嵌套眼部 motion 在 type-0 间隙后仍应可见。
- **corner color 字节序为 `0xRRGGBBAA`**(低字节 alpha);眼皮层
  `0x000000FF` 是"不透明黑",不是透明。
- **composite alpha mask**:type-12 且 `stencilType & 4` 且有
  `stencilCompositeMaskLayerList` 的 owner,其子孙按同 motion scope 内
  所列 source 图层的 alpha 并集裁剪;opacity 0 的 `add_mask` 不参与
  (与 native 可见 sprite 过滤一致)。CPU 路径按 coverage buffer 实现。
- **YSEmote 导出为 stdcall**(`RET imm`);按 cdecl 声明会在数次 Update 后
  破坏栈。ordinal 254 = `Update(seconds)`,255 = render,内部转发到
  `[impl+8]` 的 device render loop。
- 新增 `--only/--skip/--limit` 调试过滤与 `demo/make_demo.py`(多角色 ×
  4 表情 + 待機ループ,单文件 HTML;WebP 内嵌)。
- **时间线播放**:`psb-render --timeline NAME --seconds T [--loop]` 用
  `Animator` 推进 timeline 并把求值变量交给 pose;demo 用
  `待機ループ00` 生成 6 帧循环。`--view SCALE,X,Y` 固定相机,避免
  `head_slant` 等姿态因 auto-fit 改变大小(此前“歪脖子变小”的根因)。
- **sprite mesh 继承(2026-10-22 修复,对照 eluna 所引 native
  `sub_10353CF0`/`sub_1038B340`)**:`mesh.bp` 是归一化到 sprite icon box
  的 4×4 Bezier patch,**不是世界空间 warp**。native 仅把
  `meshTransform == 1 && (meshSyncChildMask & 8)` 的祖先 patch 折叠进
  子层;折叠方式是控制点位移逐点相加(`dst = next + (self - identity)`),
  不是 Bezier 复合。绘制时每个 sprite 用自己的 icon box 采样累积 patch
  (`left + p*width`,`top + p*height`)。此前把 `mesh.bp` 当世界 warp 会撕
  裂角色;现在眼部 `UD/LR/hitomi_sp/eye_pos`、头/身/口/眉的形变链都按此
  实现,笑颜/闭眼/歪头不再溢出或缩放。
- **composite mask source 解析(同期修复)**:`stencilCompositeMaskLayerList`
  的 label 在 owner 所在 motion player 内解析;type-0 source 贡献自身可见
  几何,type-3 source 是嵌套 player 代理,必须展开为其下所有实际绘制的
  sprite(`sub_10337560` + `sub_103390C0` second pass)。仅按 label+scope
  匹配会漏掉嵌套眼/泪的 source。
- **meshSyncChildMask 0x1/0x2/0x4(2026-10-22 补齐)**:坐标按
  `patch.domain` 映射;角度/缩放在 ±0.0001 差分上按 `sub_10335500` 的
  X/Y 轴平均角与三角面积开方计算,并只写入 `inheritMask` 选中的通道。
- **meshCombinator(2026-10-22 补齐)**:`combinatorList` 按变量在
  `rawMeshList` 资源的 `meshCount` 个 patch 间插值,`neutralIndex` 取单位
  patch;首个条目在 key 与层 `parameterize` 匹配(或无 parameterize)时作为
  self patch,其余作为 child patch(相对 identity 的位移)。
- **native 继承源(2026-10-22 补齐)**:`inheritMask & 0x400000` 的层对继承
  透明,后代继续使用其上方的 source;partial inherit 先减去 motion root
  通道、构建相对矩阵,再在外侧乘 root 矩阵(`sub_10331060`)。
- **blend/MODULATE2X(2026-10-22 补齐)**:CPU 路径实现 `sub_1041A190`
  的 0..5 blend 表;MODULATE2X 帧的中性 vertex color 是 `0x808080FF`,
  ×2 后为白。
- **PSB v4 `ems` 支持**:Nekopara 系列为 `spec: ems`、`source.tex` 单页
  4096² `RGBA8`、icon 键为四位字符串(`"0020"`),不是 v2 的
  `tex#0/tex#1` + `RGBA4444`。已用 `NekoWebShow` 镜像的
  chocola/vanilla/azuki/coconut maid 模型验证:
  87 icons、21 objects / 48 motions、base motion `タイムライン構造`;
  time 0 每个角色 34 个 draw item、中性帧 68 三角形(仅非单位 mesh 才
  细分;笑颜帧约 3.3k)、2 个 mask source。
  Coconut 有 32 个非默认 blend item,CPU 路径按普通 alpha 输出(待 M4 修正)。
  这些游戏资产只在本地测试,不入库、不随 demo 分发。

- 仍缺:type-7 clip rect、inner/outer stencil phase、type-9 anchor、
  type-10 feedback、particle、pixelate、wgpu 管线、物理控制器、官方像素对拍。

**未做**:物理控制器(bust/hair/parts)的数值求值、差分淡入曲线、参数化
deformer mesh 继承、wgpu 管线、官方像素对拍。

## 4. 支持分层(建议)

| 层 | 内容 | 是否本仓库内核 |
|---|---|---|
| A. 域事件契约 | `EMOTE`/`EMOTEINFO` 参数名、值域、帧号;宿主按名字消费 | **是**(已有事件,补契约) |
| B. E-mote 运行时 | `.psb` 解析、骨骼/参数/时间线求值、网格生成 | 新子系统(建议 `yu-emote`) |
| C. 渲染 | 58 个 shader 的 WGSL 移植、Atlas/遮罩/像素化、合成进 `CompositorFrame` | 新子系统 + wgpu 后端扩展 |
| D. 官方 DLL 旁路 | Windows 宿主加载 `YSEmote.dll` 取纹理 | 仅作对拍 oracle,不作为交付路径 |

### 4.1 里程碑(建议)

- **M0 契约(完成)**:`EMOTE`/`EMOTEINFO` 证据级契约已写入
  `crates/yu-rt/docs/domain-event-contracts.md` §7;`.psb` 表达裁决为
  **RGBA 纹理 + 图层变换**(不新增 `Op::Emote`,不把网格塞进 `yu-gfx`)。
- **M1 数据(完成,规范化待做)**:PSB v2 schema、Atlas、
  objects/motions/layers/frames、variables/timelines/controllers;
  `psb-dump` 确定性输出;`emote_test.psb` 逐字段校验(见 §3.1)。部件显隐/
  规范化规则仍待取证。
- **M2 动画(core 完成)**:`yu-emote::Animator` 实现按名变量、可选 fade、
  PLAY/DPLAY/STOP、循环、固定 tick;线性采样仅用于 `easing==0`。官方帧率
  与非零 easing 曲线未取证,通道由调用方提供/显式挂账。
- **M3 渲染(core 完成)**:58 个 shader 变体的 WGSL 特征表 + Bezier 网格
  细分(4×4 控制点、`meshDivision`)。wgpu 管线、运行时变体选择、骨骼/图层
  求值待做。
- **M4 对拍**:官方 `YSEmote.dll`(Windows/D3D9)截图 vs 内核,逐像素阈值;
  阈值先声明。

### 4.2 验收

- E-mote 角色的位置/缩放/alpha/遮挡与官方一致;
- `VAR/PLAY/DPLAY/STOP/DSTOP/VO` 的时间行为一致;
- DLL 缺失时显式诊断(不静默空白),且不影响非 E-mote 游戏;
- 语料角色场景不再出现“模型不显示”的静默失败。

## 5. wasm 与开源生态(2026-10-22 检索)

### 5.1 结论

- **官方 DLL 不能进 wasm**:`emotedriver.dll` 是 Win32 x86 + D3D9/D3DX9,
  浏览器没有同构运行时;Wine/D3D9 进 wasm 不现实。
- **当前浏览器架构不需要 wasm 跑内核**:`yu-host-wasm` 只是 presenter
  (native `yu-cli serve` 推 RGBA 帧);E-mote 在 native 侧实现即可,
  浏览器照常收帧。
- **若要做“内核整体进浏览器”**,干净实现的 Rust E-mote 可编译到
  `wasm32-unknown-unknown`;渲染用 wgpu/WebGPU(本仓库已有 wgpu 后端)。
  这需要另立 kernel-in-browser 任务(把 `yu-rt`/`yu-gfx`/`yu-backend-wgpu`
  的 wasm 构建跑通),不是 E-mote 单点能解决的。
- **没有找到成熟的“WebGPU/wasm E-mote 渲染器”开源项目**;现有浏览器
  渲染都是 FreeMote WebGL(JS/WebGL,非 wasm,非商业许可)。

### 5.2 项目对照

| 项目 | 语言/许可 | 能力 | wasm | 对本仓库 |
|---|---|---|---|---|
| [storycraft/emote-psb-rs](https://github.com/storycraft/emote-psb-rs) | Rust/MIT | PSB/MDF 读写、serde、资源流;**不渲染** | 依赖均可 wasm,未显式测试 | **M1 首选参考/依赖**(MIT) |
| [xmoezzz/eluna](https://github.com/xmoezzz/eluna) | Rust/未声明 | PSB 解析、motion/layer/timeline/controller/draw-frame 提取、wgpu 预览播放器 | 未提及 | 行为参考;**无许可,不复制代码** |
| [Mieluoxxx/amadeus-emote-loader](https://github.com/Mieluoxxx/amadeus-emote-loader) | Rust/Apache-2.0 | PSB/JSON 解析、烘焙给旧 runtime;**不渲染**;已归档 | **显式 wasm32**(约 351 KB) | wasm 证据;不采用(需闭源 runtime) |
| [UlyssesWu/FreeMote](https://github.com/UlyssesWu/FreeMote) | C#/CC BY-NC-SA 4.0 | PSB/EMT 全套工具 + 桌面 Viewer | 无 | **只作 oracle/事实参考** |
| FreeMote WebGL runtime(被 NekoWebShow / amadeus-next-emote / Emote_Widget 使用) | JS/WebGL,CC BY-NC-SA 4.0 | 浏览器 E-mote 渲染 | 无(JS) | 证明浏览器可行;非 1:1(“合成播放黑剪影”),不采用 |
| [Chocola-X/NekoWebShow](https://github.com/Chocola-X/NekoWebShow) | JS/AGPL-3.0 | 浏览器 E-mote 展示 | 无 | 行为参考;AGPL + 非商业 runtime |
| [703519523/Emote_Widget](https://github.com/703519523/Emote_Widget) | Python+JS/CC BY-NC-SA | Qt WebEngine 里的 E-mote | 无 | 行为参考 |
| [UserUnknownFactor/psbtool_py](https://github.com/UserUnknownFactor/psbtool_py) | Python/MIT | PSB 文本翻译 | — | 与渲染无关 |

### 5.3 建议

1. M1 用 `emote-psb-rs`(MIT)做解析基线,逐字段用 `emote_test.psb`
   校验;若引入依赖,文件头/README 注明来源(MIT 合规)。
2. M2/M3 自研;`eluna` 只读行为,不复制(无许可)。
3. M3 用 wgpu 写网格管线:同一份 Rust 代码 native + wasm32;浏览器用
   WebGPU(Chrome/Edge 可用),老浏览器需要 WebGL2 回退(额外工作)。
4. 不做“官方 DLL + Wine/wasm”;不把 FreeMote WebGL 作为交付依赖
   (许可 + 非 1:1)。
5. 若要把“内核整体进浏览器”,单独立项,不塞进 E1。

## 6. 非目标 / 风险

- **不在本次 shader 任务内**:psA/psB 属于引擎 FXO;E-mote 是插件运行时。
- PSB v2 是 M2 私有格式,需以官方数据/官方 DLL 行为为准;无证据字段不猜。
- `YSEmote.dll` 的 159 个导出是**序号**接口,语义需逐个反查;建议先用
  官方 DLL 做 oracle,再写干净实现。
- D3D9 网格/矩阵 → wgpu 的坐标/采样差异需单独对拍(vs 的 `cScOffset`
  与半像素问题)。
- 许可:E-mote 是商业中间件;干净实现只参考可观测行为,不复制其代码。
