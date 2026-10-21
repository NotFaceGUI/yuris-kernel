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
| DIV-GFX-006 | 过渡 `TID` 配对层缺失时的第二纹理来源、`T` 的推进者(`+0x82` vs `+0x1e8`)未取证 | open | 空 `TID` 已确认=无配对(普通层,会话 F 修复);非空但查不到配对层仍 Unimplemented;公式已按 REA 实现 |
| DIV-GFX-007 | `CGACT[COPY/COPY2]`:旧线“上一帧已渲染目标”被 EVID-J-11 微场景 Refuted(至少 group 源 = 命令执行时当前状态);区域 `X/Y/X2/Y2` 与 layer 源时序未测 | open | 复制目标层纹理=`Snapshot`,`Op::Copy` 恒标 Unimplemented;契约变更需主人确认 |
| DIV-GFX-008 | 图层组离屏尺寸/清屏/RT alpha 语义:主组 `W0` 官方直接画到屏幕,子组离屏 alpha 不参与父级合成(EVID-J-08) | open | 组 `size=None` 时后端暂用视口/透明黑;内核当前把 `W0` 当 Offscreen,半透明层因此与官方不同;修正需主人确认契约 |
| DIV-GFX-009 | 颜色空间:引擎未设 `D3DRS_SRGBWRITEENABLE`,按 gamma 空间混合;未用官方截图逐像素验证 | open | 帧输出 `gamma_encoded_srgb8` |
| DIV-GFX-010 | 帧入口无显式 `Clear`;可见背景黑来自 D3D9 默认/设备初始化 | open | `Clear::default_black()`(Likely) |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/RID/MFILE/CASH/...` 的视觉语义未分类 | unimplemented | 原样进 `Layer.extra`,每参数一条诊断 |
| DIV-GFX-012 | 文本字形栅格化/行距/描边/窗口定位未实现 | unimplemented | 只输出 `TextRun`/`TextStyle` 模型 |
| DIV-GFX-013 | `CGACT` 图像加工: `RECTPAINT/2/3` 已按 EVID-J-02..05 实现;`SIZE`/`ROTATION`/`MONO`/`BLUR`/... 像素算法仍未实现 | partial | `Op::Image` 对 `RECTPAINT/2/3` 可在原地、整层或完整矩形、Asset/1×1 黑图时标 Executable;ID2/快照源/其余动作仍 Unimplemented,后端跳过 |
| DIV-GFX-014 | 同 Z 顺序:手册称「不確定」;内核取创建序 | fixed(decision) | `(z, seq)` 稳定排序 |
| DIV-GFX-015 | 首屏逻辑帧号由 VM 事件决定;`frame 0` 是初始化,首个可见画面在后续逻辑帧 | note | golden 取“首个有可见层的逻辑帧” |
| DIV-GFX-016 | `CG` 无 `FILE` 时按引擎 get-or-create 建无纹理节点;手册例 4 称“图层不存在=错误” | open | 无纹理节点不产生绘制,只保留状态;会话 F 未能重新锚定 CG 处理体,保持现状 |
| DIV-GFX-017 | `COPY` 属性复制范围:`FUN_0042f809` 行 365-447 明确复制 A/E/MODE/T/TA/X/Y/尺寸/TID/MID/滤镜;该块**未见 Z**,当前 gfx `copy_attributes` 复制 Z | open | Z 未在块中出现 ⇒ 当前复制 Z 是 Likely 偏差;等 `FUN_0042f809` 调用点/Z 字段偏移确认后再改 |
| DIV-GFX-018 | 子窗口(`WINDOW[NO=1..3] CGID`)的叠放/位置/可见性不在 gfx | unimplemented | gfx 输出全部窗口状态;Screen 只合成主窗口(CGID/E/A),子窗口组只产出 Offscreen pass,交 window/host 会话 |
| DIV-GFX-019 | 顶点 DIFFUSE RGB 未建模(引擎顶点色构造与反编译局部变量重叠,未能可靠还原) | open | 可执行模式 0/1 的 DIFFUSE 只进 alpha stage;MODE 2/3/5/6/7/9 已 Unverified,后端不得自行假定顶点色 |
| DIV-GFX-020 | `CGINFO/FONTINFO/WINDOWINFO` 的 `LET` 查询需要脚本执行期同步读图形状态 | partial | `yu-rt::GraphicsHost` + `yu_gfx::GfxHost` 已实现同步查询与 LET 写回;`EXIST/E/A/X/Y/Z/MODE/T/TA/TID`、组/黑图尺寸、FONTINFO 当前样式、WINDOWINFO 主窗口几何可用 |
| DIV-GFX-021 | `TEXT[ID]` 事件字段是 UTF-8 `String`;CP932 非 UTF-8 ID 无法无损承载 | unimplemented | 非 UTF-8 ID 清空并显式报告,不做有损替换;ERIS 未使用 TEXT[ID] |
| DIV-GFX-022 | 资源图 + `SX/SY` 的缩放/裁剪语义未取证(仅 `FILE=""` 黑图有真实用法) | unimplemented | `SolidBlack1x1` 按尺寸可执行;资源图带尺寸标 `gfx.image.size.unsupported` |
| DIV-GFX-023 | 复制/图像日志上限 16,384 条,超限丢弃最旧一半 | note | 记录 `dropped_ops()` + `gfx.log.truncated`;正常首屏为 0 |
| DIV-GFX-024 | `CGINFO[SX/SY]` 对资源图返回真实纹理尺寸需要资产元数据(图像文件头/解码器);gfx 不读文件 | blocked | 有显式 `SX/SY` 的层、组、`SolidBlack1x1` 可答;资源图无显式尺寸时 Unimplemented,不猜 0 |
| DIV-GFX-025 | INFO 多字段同时置位时 `LET` 写回顺序未取证 | unimplemented | `gfx.info.multi`;VM 不写 LET,避免猜顺序 |
| DIV-GFX-026 | INFO 的 `COLOR/ONMOUSE*/CAPTION/ACTIVE/FULLSCREEN/...` 需要像素/鼠标/窗口 host | unimplemented | `gfx.info.unsupported`;不伪造 |
| DIV-GFX-027 | `IDNO` 的 `ID + "." + 十进制` 拼接未重新锚定(旧地址失效) | open | 路径按现规则;若引擎不同则需改 |
| DIV-GFX-028 | 图像层下挂子层的目标 RT(同一目标 vs 组 RT)只有遍历汇编证据,无真实样本 | open | 当前按后序扁平绘制到父目标;需官方合成脚本确认 |
| DIV-GFX-029 | pass 的 copy/image 分组顺序与脚本命令交错序不同 | open | 复制/加工全 Unimplemented;交错序取证后改 |
| DIV-GFX-030 | `CG[TIDNO]` 未处理(引擎可能按 `TID + "." + N` 解析) | open | 解析为 `TID` 的 IDNO 变体未取证;当前忽略该参数并只留档 |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层运算、`Z2`/`BNO` 修饰参数未取证 | unimplemented | ID3/ID4 已诊断;Z2/BNO 原样留存,不执行 |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 的精确对应仅 Likely | open | 当前 SWAP=图像+坐标+可见属性,SWAP2=图像+坐标;需 `FUN_0042f57d` 调用点确认 |
| DIV-GFX-033 | `TEXT` 的 `OUTPUTMODE/WAIT/RETURN/PAGE/CLICK/AUTO*/…BUTTON` 显示节奏未实现 | unimplemented | VM 只记录命令事实;点击/跳过按钮属 host |
| DIV-GFX-034 | 文本跨帧持久:gfx 每帧清空 `text`,`TEXT[CLEAR]` 未传播到 gfx | unimplemented | 当前 `TextRun` 只保留一帧;引擎文本窗口状态机未建模 |
| DIV-GFX-035 | `FONT[NO/BNO]` 未解析到真实字体;`GRADATION/ROTATION/PITCH/LANG/X/Y` 未分类;默认字体/颜色/AA 未取证 | unimplemented | 保存原值;字体解析归 `yu-font` |
| DIV-GFX-036 | `FONTINFO NUM/LANG/GRADATION/ROTATION/PITCH/X/Y` 需要字体列表;未设置的 COLOR2/NAME 默认值未取证 | unimplemented | 返回 `gfx.info.unsupported`,不写 LET |
| DIV-GFX-037 | `WINDOW` 的 CAPTION/ACTIVE/FULLSCREEN/MINIMIZE/CLOSEMODE/MENUID/REGIONID/DRAGID/Z 归 host;主窗口 `A` 透明度(β4 仅显隐)/`SX/SY` 是否改视口未取证 | open | gfx 记录 X/Y/SX/SY/Z;Screen 只消费 E/A 的显隐与 CGID |
| DIV-GFX-038 | INFO 对“缺失图层/窗口”的非 EXIST 字段、以及 `MFILE/TEX/FX/TRIM/...` 未取证 | unimplemented | 返回 `gfx.info.missing/unsupported`;VM 不写 LET |
| DIV-GFX-039 | `CGACT` 未分类参数:`RASTER1`(手册有、ERIS 表无)、`FSX/FSY/FX2/FY2`、`Z2` | unimplemented | `RASTER1` 已识别为图像动作;其余原样留存 |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态查询未接线(`text/textinfo.html`) | unimplemented | VM 目前归 Unknown;ERIS 未使用;需与 TEXT 状态机一起做 |
| DIV-GFX-041 | 主组/子组 RT 的结构与 alpha 格式:主组 `W0` 官方直绘到屏幕;子组 RT alpha 不参与合成(行为已由 EVID-J-08 微场景确认),生产路径的 `D3DFMT`/alpha 写掩码未定位 | open | 内核当前 `Offscreen(:/W0)` + RGBA8 直通 alpha,半透明层与官方不同;**契约改动需主人确认** |
| DIV-GFX-042 | D3D9 半像素/quad 采样相位:官方矩形顶行与背景混合(1px 边缘),内核整数矩形全填充 | open | `RECTPAINT` 区域内部已逐通道一致;1px 边缘差异单独挂账,不纳入 Phase B 门禁 |
