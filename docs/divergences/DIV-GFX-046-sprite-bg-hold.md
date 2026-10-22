# DIV-GFX-046 · ERIS sprite 系统的 boot logo 保持/淡出(BG 提升)

- 状态: **fixed(J3)**
- 优先级: 高(title boot 动画与时间锚点)
- 发现日期 / 方式: 2026-10-21 / 官方 boot 逐帧截图 vs 内核逐 tick 渲染
- 相关: EVID-J3-02(`docs/title-motion-ledger.md` §9.6)、DIV-GFX-006/045

## 现象

官方启动 0.2–2.0 s 显示 Firstia logo(`cg/bg/logo`)的菱形 wipe,
随后保持 1.2 s、用 800 ms 淡出,再淡入 title。内核在修复 EVID-J3-02
后能渲染 wipe(tick 13–59),但 tick 63 起 logo 被
`CGEND SPLBUF401/SPL401/SPLTR401` 删除,屏幕全黑到 tick 224,
title 首次出现约 3.7 s(官方约 2.2–2.5 s)。

## 证据

- 官方 boot:`target/tmp/j3-official-boot/frame_0001..0020.png`
  (`capture_screen.py --warmup 0 --frames 20 --interval 0.1`);
  frame_0008 为完整 Firstia logo。
- 脚本:`ERIS/UserScript/SCENARIO_START.txt`:
  `\BG(logo, 200, 800, 23, , , 1)` → `\WA(1200,,1,1)` → `\FOUT(800, 23)`
  → `\TITLE(@L(4001), -1)`。
- sprite 缓冲约定:`es_Sprite.yst` 的 `$LID="SPL"`、`$LIDBUF="SPLBUF"`、
  `$LIDTR="SPLTR"`、`$LIDTRBUF="SPLTRBUF"`。
- 内核状态(tick 62):`SPLBUF401=cg/bg/logo`、`SPLTRBUF401=cg/tr/ysr023`、
  `SPL401` 可见 T=256;tick 63 三者被 CGEND 删除。
- 官方/内核 wipe 完成帧对齐:tick 61 vs frame_0008,4×4 采样平均通道差 0.13。

## 根因(2026-10-22 追加,EVID-J3-03/04)

不是缺 sprite 渲染器。官方 sprite 系统本身就是脚本
(`ERIS/Sprite/es_Sprite.yst`),每帧翻译成 `CG`/`CGACT` 命令;内核缺的是这些
命令的低层语义:

1. `MATH3D`(REA `FUN_00448900`,`0x448900`)未实现 ⇒ `es.SP.XYZCALC` 返回的
   缩放 `zx/zy` 恒为 0;
2. `CGINFO[SX/SY]` 对 `Asset` 纹理返回 `gfx.info.asset_size`(无资产元数据
   provider)⇒ `es.CGACT_SIZE` 读到的 `SX/SY` 为 0;
3. 于是 `SIZE<1` ⇒ `CG[ID=$dst X=999999999 Y=999999999]`(屏幕外哨兵)
   ⇒ 提升出来的 BG 层永远不可见,tick 63 后再被 `CGEND` 删除 = 黑屏。
4. `CGACT[SIZE/SIZE2]` 本身也未实现(此前标 Unimplemented)。

实装后 tick 13–59 菱形 wipe、tick 63–139 保持、tick 139–190 反向 wipe 淡出全部
由脚本自身驱动,无需新增契约。

## 结论与处置

- **已修**:可见 wipe(EVID-J3-02);保持/淡出(EVID-J3-03/04:
  MATH3D + `CGINFO[SX/SY]` 资产头 + `CGACT[SIZE]`)。
- **不需要**“过渡完成后提升为 BG”的新契约:脚本用
  `SP.BGCOPY`(`CGACT[COPY2]`)+ `CGACT[SIZE]` + `CGACT[RECTPAINT3]`
  自己完成 BG 提升,官方也没有额外机制。
- **已撤回的固定实现**:中途曾在 `yu-gfx` 硬编码 `SPL/SPLBUF/SPLTR/SPLTRBUF`
  做缓冲层配对(EVID-J3-02)。那是脚本命名,不是引擎语义(EVID-T-10);
  `CGACT[SIZE]` 落地后由脚本自己写像素,特判已成多余。现为纯状态驱动,
  600 帧对拍与 boot 时间线不变。
- 附加修复(EVID-J3-05/06/07):
  - 过渡取样必须用**图层当前像素**(脚本先对配对层做 `CGACT[REVERSE]`,
    读资产原图会方向镜像);
  - `COPY ID=:/W0/` 读**上一帧后缓冲**,不是本帧组内容;
  - wgpu 快照必须**冻结**(命令段每帧重放 `CopyOp`);
  - 主组直绘(DIV-GFX-008):主窗口组不再经离屏 RT,否则淡入淡出被再乘一次
    组 alpha(实测 100→77)。

## 复核

```bash
# 官方 boot
python tools/official_trace/capture_screen.py --exe <project-eris>/yu-ris.exe \
  --cwd <project-eris> --out target/tmp/j3-official-boot \
  --frames 20 --interval 0.1 --warmup 0 --no-send-input
# 内核 boot(soft)
YURIS_FIRST_FRAME_TICK=61 cargo run -p yu-backend-soft --example first_frame
```
