# DIV-GFX 系列 · yu-gfx 与官方引擎的已知差异与挂账

> 制度:`docs/architecture.md` §8、`docs/render-parity.md`。
> 每条含:现象、证据、处置、复核方式。状态:`open` / `fixed` /
> `unimplemented`(已知语义未实装,后端必须跳过)/ `blocked`(缺样本/工具)。

| 编号 | 主题 | 状态 | 影响 |
|---|---|---|---|
| DIV-GFX-001 | MODE 3/5/6/7/9 的固定管线状态已取证,但辅助纹理内容/运算本体未取证 | unimplemented | 这些模式图层标 `LayerStatus::Unimplemented`,后端跳过 |
| DIV-GFX-002 | MODE 4/8/10..18 的着色器本体未定位 | unimplemented | 同上 |
| DIV-GFX-003 | `FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/FQU/FID/F` 的槽位→结构体偏移未逐项绑定 | unimplemented | 参数原样进 `Transform.filter`;图层标 Unimplemented |
| DIV-GFX-004 | 多部件层(`+0xa6/+0xa7/+0xb0/+0x130`)几何语义未取证 | unimplemented | 契约保留 `Part` 位置,首屏未出现 |
| DIV-GFX-005 | 外部掩码 `MID` 的取值通道(α/亮度)/尺寸要求未取证 | unimplemented | 带 `MID` 图层标 Unimplemented |
| DIV-GFX-006 | 过渡 `TID` 配对层缺失时的第二纹理来源、`T` 的推进者(`+0x82` vs `+0x1e8`)未取证 | open | `TransitionDraw.to` 可为 `None`;公式已按 REA 实现 |
| DIV-GFX-007 | `CGACT[COPY/COPY2]` 的时序=`上一帧已渲染目标`(旧线实测),区域 `X/Y/X2/Y2` 语义未逐项取证 | unimplemented | 复制目标层纹理=`Snapshot`,`Op::Copy` 标 Unimplemented |
| DIV-GFX-008 | 图层组未给 `SX/SY` 时的离屏尺寸/清屏未取证 | open | 组 `size=None`;后端暂用视口/透明黑(Likely) |
| DIV-GFX-009 | 颜色空间:引擎未设 `D3DRS_SRGBWRITEENABLE`,按 gamma 空间混合;未用官方截图逐像素验证 | open | 帧输出 `gamma_encoded_srgb8` |
| DIV-GFX-010 | 帧入口无显式 `Clear`;可见背景黑来自 D3D9 默认/设备初始化 | open | `Clear::default_black()`(Likely) |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/RID/MFILE/CASH/...` 的视觉语义未分类 | unimplemented | 原样进 `Layer.extra`,每参数一条诊断 |
| DIV-GFX-012 | 文本字形栅格化/行距/描边/窗口定位未实现 | unimplemented | 只输出 `TextRun`/`TextStyle` 模型 |
| DIV-GFX-013 | `CGACT` 图像加工(`RECTPAINT*`/`SIZE`/`ROTATION`/`MONO`/...)的像素算法未实现 | unimplemented | `Op::Image` 记录原值,后端跳过 |
| DIV-GFX-014 | 同 Z 顺序:手册称「不確定」;内核取创建序 | fixed(decision) | `(z, seq)` 稳定排序 |
| DIV-GFX-015 | 首屏逻辑帧号由 VM 事件决定;`frame 0` 是初始化,首个可见画面在后续逻辑帧 | note | golden 取“首个有可见层的逻辑帧” |
| DIV-GFX-016 | `CG` 无 `FILE` 时按引擎 get-or-create 建无纹理节点;手册例 4 称“图层不存在=错误” | open | 无纹理节点不产生绘制,只保留状态 |
| DIV-GFX-017 | `COPY` 的属性复制范围(是否含 Z)未逐项取证 | open | 当前按手册“图像+属性”复制 A/E/MODE/T/TA/坐标/滤镜/Z;REA 块未逐项确认 Z |
| DIV-GFX-018 | 子窗口(`WINDOW[NO=1..3] CGID`)的叠放/位置/可见性不在 gfx | unimplemented | Screen pass 只合成主窗口 `:/W0`;子窗口组只产出 Offscreen pass,交 window/host 会话 |
