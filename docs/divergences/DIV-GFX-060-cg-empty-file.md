# DIV-GFX-060 · `CG[FILE=""]` 的官方实现(REA 原始字节)

- 状态:**fixed**(2026-11-15;消费口径见 §6 / DIV-GFX-085)
- 取证对象:官方 `0.495` `yu-ris.exe`(sha256 `4f5eefa3…`),`CG` 处理体 `0x42a514`

## 1. 结论(两层)

**① `FILE` 槽未给 ⇒ 像素完全不动。** 这是确定的。

**② `FILE=""`(给了空串)⇒ 走独立的"生成 1×1 像素"路径**,与"未给"是两条不同分支。
该路径把层尺寸写成 1×1 并填充一个新分配的 1×1 像素缓冲,四个字节全写 0。

## 2. 证据链

槽位布局(实测,两处引用交叉确认):
- `0x684160 + slot` = 该参数在本次调用里的压缩序号(0 = 未指定);
- `0x683fc0 + idx*4` = 该参数的字符串指针;
- `0x683740 + idx*8` = 该参数的整数对。

`FILE` = `CG` 槽 33 ⇒ 存在标志 `0x684181`。

```text
;; CG 主路径
0x42a7a9  mov edx, 0x684181        ; FILE 标志
0x42a7ae  movzx esi, byte[edx]
0x42a7b1  test esi, esi
0x42a7b3  je  0x42a801             ; ← 未给 FILE:跳到下一槽,像素不动
0x42a7c4  mov ebx, [esi*4+0x683fc0]; FILE 字符串
0x42a7cf  call 0x408724
0x42a7dd  call 0x42c2a8            ; ← 装载入口
0x42a7e2  test eax, eax
0x42a7e4  jne 0x42a7ed             ; 失败 ⇒ [0x80186c]=1、return -1
```

装载入口按 FILE 是否为空分叉:

```text
;; 0x42c2a8
0x42c2ac  mov ebx, [eax+0x218]     ; 层对象文件名缓冲
0x42c2b2  mov byte[ebx], 0         ; 先清空文件名
0x42c2b5  movsx ebx, byte[edx]     ; FILE[0]
0x42c2b8  test ebx, ebx
0x42c2ba  je  0x42c2c9             ; ← 空串 ⇒ 0x40c5f8
0x42c2bf  call 0x40c7c9            ; 非空 ⇒ 解码装载真图
```

空串分支(`0x40c5f8`):

```text
0x40c5fe  mov byte[esi+0x86], 0
0x40c605  mov ecx, 1
0x40c60a  mov [esi+0x78], ecx      ; 宽 = 1
0x40c60d  mov [esi+0x7c], ecx      ; 高 = 1
0x40c618  mov eax, [esi+0x90]      ; 旧像素
0x40c61e  mov edx, [esi+0x94]
0x40c629  call 0x40c680            ; 重新分配 1×1
0x40c62e  mov [esi+0x90], eax      ; 换缓冲
0x40c634  test eax, eax
0x40c636  je  0x40c66a             ; 失败 ⇒ return -1
0x40c638  xor edx, edx             ; ← edx = 0
0x40c63a  mov byte[eax+0], dl      ; 0
0x40c642  mov byte[ecx+1], dl      ; 0
0x40c64b  mov byte[ecx+2], dl      ; 0
0x40c654  mov byte[ecx+3], dl      ; 0
0x40c663  xor eax, eax             ; return 0 成功
```

## 3. 与手册的口径差异(**不擅自站队**)

- 手册 `cg/cg.html` 例 3:「`FILE=""` で**1x1サイズの黒画像**生成」——措辞是"黑"。
- REA:四字节**全 0** ⇒ 32 位 ARGB 下为 `0x00000000`。

两者在"是否带 alpha"上不同:手册强调颜色为黑,REA 显示 alpha 也是 0。
**在拿到官方截图逐像素对比前,不改内核行为**(当前 `SolidBlack1x1` =
`[0,0,0,255]`,与手册措辞一致)。

## 4. 对 `<repo-old>` 旧线结论的订正

旧线 `crates/yuris-stage/src/lib.rs`(2026-10-17 勘误)称:

> 「`FILE=""` 只对"新建层"是 1×1 纯色;对已有纹理的层是空操作」

REA 否定"空操作"这部分:**`FILE` 给了空串就一定会替换像素**。
"未给 FILE 才保持像素"才是真正的分界线。旧线把两者混成了一个判断。

## 5. 复查入口

- `read_bytes 0x42a514`(CG 主体)、`0x42c2a8`(装载分叉)、`0x40c5f8`(空串分支);
- 槽位偏移由 `0x684160 + slot` 与 `0x683fc0`/`0x683740` 三处引用确认。

## 6. 手册 vs REA 的裁定(2026-11-15)

**两者都对,只是消费口径不同**:

- **像素运算**(`CGACT[SIZE/SIZE2/COPY/MERGE`] 的源、文本基面)读的是层表面
  原始字节 → 四字节全 0 = **透明**;
- **直接绘制 / 双纹理过渡取样**按手册呈现为「1×1 の黒画像」→ **不透明黑**。

裁定依据(v474 商业样本,逐帧捕获):

1. 转场起点黑帧的最后一个 op 是 `SIZE BUF→LNO_FB`,源 BUF 是 `FILE=""`
   空白面;若按黑读,黑 1×1 被放大成 1281×721 全屏不透明黑板,z=8,900,000
   盖住整屏(`content=1281x721` 指向 1×1 纹理);
2. 官方同脚本不闪;REA 空串分支写入四个零字节 ⇒ 官方 `SIZE` 读到透明;
3. 把 1×1 纹理整体改成透明会让黑幕/转场消失(用户实机 A/B)⇒ 直接绘制
   必须保持黑。

实现:`resolve_ref` 增加 `blank` 参数,像素 op 源传 `SOLID_EMPTY`、过渡取样
传 `SOLID_BLACK_PX`;soft 的 `solid_fill_texture` 与文本基面同步。
完整根因、逐帧数据与验证见 **DIV-GFX-085**。

**已裁定(2026-11-17,REA 0.486 `FUN_0042b144`)**:
`CG[FILE]` 的装入统一走 `FUN_0042b144(EAX=层, EDX=文件名)`:
`*EDX==0` → `FUN_0040c5f4`(无条件写 `+0x78/+0x7c=1`,表面重分 1×1);
非空 → `FUN_0040c7c5`(按新图重建表面)。
因此**已存在层**上的 `CG[FILE=""]` 也把矩形重置为 1×1,
`CGINFO[SX/SY]` 随后返回 1(SDK `es_Sprite.yst:4064` 的透明守卫依赖它)。
内核已在 `apply_cg_inner` 的换图分支同步空串 1×1、资产换图按新图尺寸重置;
回归 `empty_file_on_existing_layer_resets_rect_to_1x1`。
