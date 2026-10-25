# DIV-RT-054 · 日志告警收口:1306 条诊断里哪些是误报、哪些是归属

> 状态:`fixed`。触发口径:主人「接下来你需要修补的是 大量的 warn 和 UNIMP」。
> 取证方式:700 帧 headless 真实语料(`data.ypf`)抓 JSONL 诊断,按
> (severity, code) 聚合后逐条查证据 —— 手册、`es_Sprite.yst` 源码、
> `yst00066.ybn` 编译产物、REA 0x495 `yu-ris.exe`。

## 1. 修复前后对照

| count | severity | code | 处置 |
|---|---|---|---|
| 1301 | Recoverable | `gfx.layer.missing` | → Info `gfx.layer.already_absent` |
| 626 | Unimplemented | `sv.command.unimplemented` | 删除(纯误报) |
| 979 | Unimplemented | `gfx.info.unsupported` | → 实现 `CGINFO[TEX]` |
| 300 | Unimplemented | `gfx.info.missing` | → 缺层 `TEX` 回 0 |
| 3 | Unimplemented | `gfx.param.nonvisual` | → 合法非视觉参数,留档不报 |
| 3 | Unimplemented | `rt.cmd.unimplemented` ×3 | 1 条真 bug + 1 条归属 + 1 条 deferred |
| 2 | Recoverable | `sv.path.escape` | 绝对路径误判 |
| 2 | Recoverable | `sv.file.missing` | → Info(幂等删除) |

**收口后(700 帧)**:非 Info 诊断 **1 条**,即 E-mote(主人明令暂缓)。

## 2. 逐条证据与处置

### 2.1 `FILEINFO` ×626 —— 宿主侧无条件误报(**已删除**)

`crates/yu-save/src/state.rs::apply_event` 对**每条** `FILEINFO` 事件都
报「SIZE/RELATION 未实现」。而内核 `op_fileinfo` 早已实现三条查询:

| 参数 | 引擎锚点 | 实现 |
|---|---|---|
| `EXIST` | 0x44286c → 0x44316c(包内/散盘四策略) | 是 |
| `SIZE` | 0x443572(`FindFirstFileA` → `nFileSizeLow`) | 是 |
| `RELATION` | 0x4429e6 → 0x442a20(`FindExecutableA`) | 是 |

宿主那条诊断与事实相反,直接删除。真正未实现的支(`FOLDER`)由内核
自己带位置上报。

### 2.2 `CGEND` 删不存在的层 ×1301 —— 幂等空操作(**降 Info**)

证据链(三层交叉):

1. **脚本源码**:`es_Sprite.yst` 的清理助手 `es.SP.DEL2.SET` 对每个精灵槽
   **无条件**发 `CGEND`,不先查 `CGINFO[EXIST]`;
2. **编译产物**:`yst00066.ybn` pc2008.. 展开确认为同一模式
   (`es.LNO.GET` → `CGEND $LIDBUF+槽号`),且被 `LOOP[SET=(@LMAX+1)]` 100 槽调用;
3. **语义**:删除的后置条件「该层不存在」在删除前已成立 ⇒ 与成功等价。

按 Recoverable 报会在演出期刷 1301 条 WARN。降为 Info `gfx.layer.already_absent`。
**不是隐藏错误**:之后对该层的 `CGACT`/`CGINFO` 仍按 `LAYER_MISSING`/
`INFO_MISSING` 报,真故障点照常暴露。

### 2.3 `CGINFO[TEX]` ×1279 —— 缺失语义(**已实现,`fixed(no-manual)`**)

手册 `cg/cginfo.html` 的字段表**没有** `TEX`(只有 ID/LET/SET/SET2/
EXIST/E/A/TID/T/TA/X/Y/Z/SX/SY/MODE/COLOR/ONMOUSE*)—— 属手册缺页。

证据来自脚本源码(`es_Sprite.yst`,四处完整闭环):

```text
:4091  CG[ID=$LB1 TEX=@_DXMODE]              ← 建层时写入
:4144  CG[ID=$LIDBUF+$(1+@_LC+@No*100) TEX=@_DXMODE]
:3454  CGINFO[ID=$LIDBUF+$(1+@mLC*100) TEX=1 LET=@texchk]   ← 读回
:3455  IF[@texchk==2]  // 分支注释:「E-moteレイヤなら」
```

`:4156`、`:5849`、`:7771` 三处同型 ⇒ **2 = E-mote 层**。

实现:
- `Layer` 新增 `tex: i64`(零初始化 = 非 E-mote);
- `CG[TEX=]` 写入真实字段(此前落 `extra` 并被标 nonvisual);
- `CGINFO[TEX]` 读回;
- **层不存在时回 0**(类型探测语义):这四处 ID 由 `$LIDBUF+槽号` 拼接,
  未装载的槽自然不存在,中间没有 `EXIST` 前置检查。

语料中没有任何脚本把 `TEX` 设成 2(E-mote 由引擎装载时置位),本内核
E-mote 属 `deferred(master)`,故该值只会是脚本显式写入的。

### 2.4 `LINT` / `REMALLOC` ×3 —— 合法非视觉参数(**不再报**)

手册 `cg/cg.html` 无这两项的说明(缺页)。它们不进像素、不进 `CGINFO`
返回值,原样留档即完整语义。此前走 `other` 兜底被标「未分类」,现单列
为归档分支。同批归档的还有 `LINT2`/`TSX`/`TSY`。

### 2.5 `FILEACT` ×1 —— **真 bug**(VM 判定漏算)

`SaveHost` 契约明确消费 `Domain::Save | Domain::File`(`event.rs` trait
注释),但 VM 的 `handled` 判定只算了 Graphics/Window/Sound ⇒
`es_first.yst:SYSTEM@2` 的 `FILEACT` 被误报未接线。

修复:`handled` 增加 `save_host.is_some() && matches!(domain, Save | File)`。
注意 `SaveHost::apply_event` 在**事件 drain 时**调用,不在命令点同步发生,
所以判据是「宿主已安装」而非「本次已处理」。

### 2.6 `SYSTEM` ×1 —— 宿主归属(**降 aligned-stub**)

`SYSTEM` 的 8 个参数(`SCREENSAVER`/`BACKGROUNDEXEC`/`SX`/`SY`/`TIMECOUNT*`)
全部写进进程级状态供 WndProc 读,**没有脚本可见返回**。域事件契约 §9 已把
`TRAY` 等 System 域命令定为宿主边界,`SYSTEM` 同属此类。

取证限制(如实记录):
- 手册 `system/` 目录下**没有** `system.html`(只有 fps/input/mouse);
- `BACKGROUNDEXEC` 全手册 0 命中;
- 引擎侧逐参数处理体**尚未定位** —— `0x41b42c`(经 `YU-RISScreenSaver`
  字符串 xref 找到)是**屏保窗口创建**,不是命令处理体。

因此本条只声明**归属**,不声称逐参数已对齐。参数值照原样进事件载荷,
平台层可作为。

### 2.7 `EMOTE` ×1 —— `deferred(master)`

路线图 B9,主人明令暂缓。保持 Unimplemented 但文案标明「已决定暂缓,
不是待查项」,避免下游误当「做了但没效果」。

## 3. 附带修掉的路径判定 bug

`sv.path.escape` ×2:脚本 `es_First.yst:28` 写
`$es.SAVEPATH = $_SAVE_PATH`(宿主注入的 save 目录**绝对路径**),
`:30` 直接 `FILEACT[CREATE=1 FOLDER=$es.SAVEPATH]`。旧 `confined_path`
见到绝对路径即拒,把合法调用误判成越界。

修复:绝对路径也接受,但**仍强制**「必须在 save 根内、不得含 `..`」
(已存在的路径做 canonicalize 前缀校验;不存在的新建路径做词法前缀校验)。
安全边界未放松。

`sv.file.missing` ×2 同批降 Info:`es_SaveLoad.yst:417` 注释是
「上書き保存でなく新規保存にする」—— 每次保存前无条件删旧槽,首次
保存时目标本就不存在,是正常路径。

## 4. 验证

```text
700 帧 headless 真实语料(修复后)
  Info          gfx.layer.already_absent   1301
  Info          sv.file.missing               2
  Info          rt.cmd.aligned_stub           1
  Info          gfx.texture.missing           1
  Unimplemented rt.cmd.unimplemented          1   ← E-mote deferred(master)
  非 Info 合计:1

cargo test --workspace                        80 套件全绿
像素回归(wgpu 演出1 f900)  sha256 9584b2d6… 与修复前逐位相同
```

新增测试(`crates/yu-gfx/tests/parity_fixes.rs`):
- `cgend_on_absent_layer_is_idempotent_info`
- `cginfo_tex_reads_back_and_absent_is_zero`
- `nonvisual_cg_params_are_archived_without_unimplemented`

更新测试:`crates/yu-rt/tests/fixture.rs::tray_is_a_domain_event`
(断言从「System 域进挂号表」改为「不进」,与 §9 归属一致)。

## 5. 残留(在册,不为消告警而改)

- `SYSTEM` 逐参数处理体未定位:归属已定,参数语义待取证;
- `EMOTE` 全族:`deferred(master)`;
- `gfx.texture.missing`(1 条 Info):`CG` 未给 `FILE` 时按引擎
  get-or-create 建无纹理层,是正常路径,保持 Info。
