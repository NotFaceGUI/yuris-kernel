# DIV-GFX-060 · `CG[FILE=""]` 的官方实现(REA 原始字节)

- 状态:**open(证据已锚定;手册与 REA 存在口径差异,需实测裁定)**
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
