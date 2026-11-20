# 补丁字体(CP932 载体字 + 字形重映射)

> 适用:汉化补丁在游戏根放 `hook.ini`(`[FONT] FONTFILENAME=…`)与一个
> ttf,正文不换编码,而是用**合法 CP932 的载体字**承载中文,由随包字体
> 把载体码位映射到目标字形。实现见 `crates/yu-engine/src/patch.rs`
> (`PatchFont` / `detect_patch_font`)与 `crates/yu-engine/src/engine.rs`
> (`apply_patch_font`)。

## 1. 事实与取证(2026-11-20,样本 `42802_…`)

样本:8 个 `ystNNNNN.ybn`(YSTB v466)+ `yscfg.ybn`,标准异或密钥
`D3 6F AC 96`(`CRC32("Yu-ris")`),无换钥、无二次加密。

| 观察 | 结果 |
|---|---|
| 正文解码 | 严格 CP932 全部可解;严格 GBK 在 700+ 段失败 |
| 载体字 | 解出的 Unicode 是“载体”而非目标字 |
| 字体 | `qiandong.ttf`,family = `qiandong`,Unicode cmap 28523 项 |
| 重映射 | 2997 个汉字码位与其他码位**共用同一 glyph** |

载体 → 目标(同一 glyph,`fontTools` 实测):

```text
髮 U+9AEE ─┐
咚 U+549A ─┴ uni549A
哢 U+54E2 ─┐
咔 U+5494 ─┴ uni5494
釿 U+91FF ─┐
嗒 U+55D2 ─┴ uni55D2
鮠 U+9BA0 ─┐
啪 U+556A ─┴ uni556A
```

同一串按 CP932 解出的载体字:

```text
髮髮髮髮髮髮。哢釿，鮠釿。滋滋~。
```

用 `qiandong.ttf` 渲染 = 补丁的目标文本:

```text
咚咚咚咚咚咚。咔嗒，啪嗒。滋滋~。
```

用普通中文字体(如微软雅黑)渲染则回到载体字原形,即用户看到的
“一部分乱码、一部分汉化”:载体恰好等于目标字的段落正常,其余段落错形。

## 2. 检测规则

`detect_patch_font(project)`:

1. 在游戏根、开发布局的 `<root>/data/` 找 `hook.ini`(文件名大小写不敏感)。
2. 解析 `[FONT]` 段(段名/键名大小写不敏感,支持 `#`/`;` 注释、BOM、CRLF):
   - `FONTFILENAME` = 字体文件名(必填;空 = 不是字体补丁);
   - `FONTNAME` = 声明的 family(只进诊断;真实 family 以字体字节为准)。
3. 按 `hook.ini` 同目录 → 同目录 `fonts/` → 游戏根 → 游戏根 `fonts/`
   的顺序找文件(路径里的 `\` 统一成 `/`)。
4. **声明了字体却找不到 = 显式报错**;空文件/超过 64 MiB 同样报错
   (不静默降级,否则症状仍是“一部分乱码”)。

`hook.ini` 只做窗口改名/文本替换、没有 `[FONT]` 声明时返回 `None`,不影响
普通工程。

## 3. 引擎语义(FONTCHANGER 同型)

补丁把字体名在 GDI 层替换成 `FONTNAME`,所以引擎也必须**替换全部字体**,
只设“宿主默认字体”不够——脚本显式点名其它 family 或按 `FONT NO` 选字体
的文本会绕过默认字体,继续用错字形。

`Engine::new` 在渲染器创建前调用 `apply_patch_font`:

| 宿主字体状态 | 行为 |
|---|---|
| 未指定(`FontChoice::Script`) | 注入补丁字体,`FontChoice::HostFont`,**强制全部 FONT 请求** |
| 已选同一 family | 补注入字体字节(若缺),并把选择提升为强制 |
| 选了别的 family,未锁 | 补丁字体接管(未锁的默认/家庭选择不足以解码载体字),`warn` 记录 |
| 选了别的 family 且 `--font-lock` | 尊重宿主显式强制,`warn` 记录(载体字可能错形) |

`font_lock` 同时覆盖 `NAME` / `NO` / `BNO` 三条解析路径(见
`yu-render/core/src/text.rs::resolve_face` 的 forced 分支)。

## 4. 使用与限制

- 把补丁的 `hook.ini`、ttf 与 `ysbin/` 一起放进游戏根即可,无需再传
  `--font`;字体自动加载并强制。
- `--font <ttf>` 不带 `--font-lock` 时只算“默认字体”,补丁字体仍会接管;
  确实想用别的字体(诊断/偏好)时写 `--font <ttf> --font-lock`。
- 字体文件必须在**游戏根/其 `fonts/`/`hook.ini` 同目录**;只复制 `ysbin/`
  不会触发检测。
- 补丁字体与编码路线正交:`--patch original`(CP932)是这类补丁的正确
  编码;`--patch gbk` 仍会把正文解坏。
- 字体度量与官方 FONTCHANGER 一样会改变行宽/居中基准(补丁自带代价)。

## 5. 验收

```bash
# 单元测试(无样本也跑;真实样本用环境变量显式开启)
cargo test -p yu-engine --lib patch_font

# 真实补丁样本(指向含 hook.ini + ttf 的补丁根)
export YURIS_SAMPLE_FONT_PATCH="<补丁根>"
cargo test -p yu-engine --lib real_patch_font_is_detected_and_forced -- --nocapture

# 端到端(合成工程启动链:hook.ini → 注入 → forced_font)
cargo test -p yu-engine --test patch_font_boot -- --nocapture
# 期望: [patch-font-boot] 样本字体 family = qiandong … ok

# 识别结果(CLI)
cargo run -p yu-engine --example patch_probe -- <游戏根> auto
# 期望 evidence 含: hook.ini: FONTFILENAME=… FONTNAME=…
```
