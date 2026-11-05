# DIV-GFX-007 · `TEXCOPY`/`TEXSSCOPY` 不是纹理 blit(PASTESPRITE/GETSS)

- 状态:**layer-loss fixed;像素语义 open(evidence)**
- 关联:DIV-GFX-004(多部件)、DIV-GFX-013(图像加工)、DIV-GFX-022
- 取证对象:官方 0.495 `yu-ris.exe`,
  sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`
- 日期:2026-11-05

## 1. 现象

旧内核把 `CGACT[TEXCOPY/TEXSSCOPY]` 当成“纹理句柄复制”:

1. 无条件把目标层纹理改成 `TextureRef::Snapshot(new CopyId)`;
2. 把目标层 `texture_status` 置 `Unimplemented("gfx.copy.texcopy.unsupported")`;
3. 后端因此**永久跳过目标层**。

ERIS 语料有 3 处自复制(`es_Sprite.yst:5604/8884/8956`,
`ID==ID2`),另有 1 处 `TEXSSCOPY` 在注释里(`:8028`)。自复制本应不改变
可见结果,但旧实现会让该精灵层消失。

## 2. REA:两个参数走两个完全不同的官方处理体

CGACT 处理体 `0x42c4a4`(opcode 2)尾部按参数标志分派:

```text
0x42e661  movzx eax, [0x6841a2]   ; slot 66?
0x42e668  test  eax,eax / jne 0x42f2ac
0x42e670  movzx eax, [0x6841a3]   ; slot 67?
0x42e677  test  eax,eax / jne 0x42f1e3
0x42e67f  movzx eax, [0x6841a4]   ; TEXCOPY (slot 68)
0x42e686  test  eax,eax / jne 0x42e6b9
0x42e68a  movzx eax, [0x6841a5]   ; TEXSSCOPY (slot 69)
0x42e691  test  eax,eax / je  0x42f1ce
0x42e699  mov eax,[esp+0x8c] / mov edx,[esp+0x84]
0x42e6a7  call 0x43cd6c            ; GETSS
0x42e6ac  xor eax,eax / ... ret
0x42e6b9  mov eax,[esp+0x8c] / mov edx,[esp+0x84]
0x42e6c7  call 0x43ccd8            ; PASTESPRITE
0x42e6cc  mov byte [0x80130c],1
0x42e6d3  xor eax,eax / ... ret
```

### 2.1 `TEXCOPY` = `FUN_0043ccd8`(PASTESPRITE)

`FUN_0043ccd8`(137 B):

```c
DAT_00801dec = A[+0x34];                 // 源层对象
DAT_00801df0 = B[+0x34];                 // 目标层对象
DAT_00801dd0 = B[+0x78];                 // 目标宽
DAT_00801dd4 = B[+0x7c];                 // 目标高
_DAT_00801dc8 = (float)B[+0x1d0];        // FCX
_DAT_00801dcc = (float)B[+0x1d8];        // FCY
_DAT_00801dd8 = B[+0xa5];
DAT_00801ddc  = B[+0xa8];
DAT_00801de0  = B[+0xac];
FUN_0040af1c();                           // 失败串 "YS_PASTESPRITE Error."
```

`FUN_0040af1c`(5490 B)= 按目标层 `+0xa6/+0xa7` 网格、`+0xb0/+0x130`
每格宽高与源层 `+0x21c` 部件纹理做 sprite 粘贴,并在过程中重建目标层的
tile 表。**这不是像素 blit**:它消费多部件布局(`DIV-GFX-004`)。

### 2.2 `TEXSSCOPY` = `FUN_0043cd6c`(GETSS)

`FUN_0043cd6c`(1436 B)先检查 D3D device / BeginScene / sampler 过滤,
再 `FUN_00414ba4()` + `FUN_0043d308()`,从全局 `[0x8019c0+0xc/+0x10]`
取宽高、`malloc` 目标表面并写 `[目标+0x90]`、`+0x78/+0x7c`;失败串
`GETSS SetRenderTarget Error 1.`。语义 = **抓取当前表面/后缓冲到目标层**,
不是源层 blit。

## 3. 内核处置(2026-11-05)

- `cgact_copy` 对 `TexCopy/TexSsCopy`:
  - **不改写**目标层 `texture`,**不**把层标 `Unimplemented`;
  - 仍写 `CopyOp` 审计记录(status 保持 `Unimplemented`,不发射 pass);
  - 发 **recoverable** 诊断 `gfx.copy.unsupported`,文案点明
    `PASTESPRITE(FUN_0043ccd8→FUN_0040af1c)` / `GETSS(FUN_0043cd6c)`;
  - 不清理旧 copy/image 日志、不抓屏幕快照(避免空快照副作用)。
- `frame()` 跳过 `TexCopy/TexSsCopy` 项,不生成空快照 pass。
- 结果:语料自复制不再让精灵层消失;“粘贴/抓屏”像素仍未实现,台账
  `open(evidence)`,不冒充 `fixed`。

## 4. 仍未闭(证据任务)

1. `TEXCOPY` 的单部件/多部件粘贴像素:需要 PSB/多部件资产样本 +
   官方微场景(`CG[源] → CGACT[ID=源 ID2=目标 TEXCOPY=1] → CGEND[源]`);
2. `TEXSSCOPY` 的“当前表面”是后缓冲还是当前 RT、与 `COPY2 :/W0/`
   的差异;
3. 自复制(语料唯一形态)在官方是否也重建 tile 表、像素是否逐位不变。

## 5. 回归

```text
cargo test -p yu-gfx --test group
  texcopy_keeps_destination_texture_and_layer_drawable ... ok
  texsscopy_keeps_destination_texture_and_layer_drawable ... ok
  texcopy_self_copy_keeps_layer_drawable ... ok
```
