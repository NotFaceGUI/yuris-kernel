# DIV-RT-079 · 0.500 归入 555 商业族(TEXT/命令表/分发表对拍)

- 状态: **fixed(no-manual,2026-11-24;`<v500-relirium>` 取证)**
- 优先级: 高(0.500 启动期 `TEXT[FILE]` Fatal;PRODUCT/按钮查询未接线)
- 发现日期 / 方式: 2026-11-24 / 运行诊断 + REA 反编译对拍
- 关联: DIV-GFX-093(YDG)、DIV-RT-056/075/078(DNO=1024)、
  `docs/formats/v500-abi-census-2026-11-22.md`

## 1. 现象(改前)

`yu-cli run "<v500-relirium>" --frames 2`:

```text
FATAL rt.expr.malformed @ data\script\eris\scenario\es_scenario_load.yst pc4:
  求值结果是被引用的变量槽($2875),不是值
UNIMP rt.cmd.unimplemented @ data\script\eris\es_first.yst pc1: PRODUCT
gfx.info.unsupported x2822 / gfx.info.missing x1642 (CGINFO[LINT2])
```

主任务随后停在启动期 DIALOG(更新检查),标题/主界面不成立。

## 2. 为什么 0.500 归入 555 族(证据)

`ysc.ybn` 两者均 121 槽、11102B;除版本字段与 `GRADATION` kind 外逐字节
相同(2026-11-22 普查)。本轮对 0.500 `Relirium.exe`(ProductVersion
`0,500,0,0`)做处理体对拍:

| 命令 | 0.500 处理体 | 与 v555 对比 |
|---|---|---|
| `TEXT` | `0x482214`,837B | 归一化反编译**逐行一致**(FILE 小写化/`/`→`\`/追加 CRLF/按 CR/LF 切行写 STR 数组/CLEAR) |
| `CGINFO` | `0x46b398`,3552B | 一致(含 `+0x58/+0x60` 按钮号字段) |
| `RETURNCODE` | `0x47afbc`,78B | 一致(EndOnly) |
| `VARINFO` | `0x484a2c`,2775B | 一致 |
| `GOSUB`/`RETURN` | 901B/647B | 一致 |
| `LET`/`VARACT`/`SAVE`/`LOAD` | 3522/5290/4891/2806B | 一致或仅构建差异 |
| `MOVIE2`/`MOVIE2END`/`MOVIE2INFO`/`REGINFO` | `0x47ac94/…/0x47ad14`,各 29B | **0.500 是空桩**(仅 `FUN_0045688d(); return 0;`),v555 是真实处理体 |
| `PRODUCT` | `0x488b20`,4663B | **0.500 新增** COM=1/2/3、0x38/0x39、0x5dc1..0x5dc9;缺 0x21/0x24/0x25 |

0.500 分发表:初始化器 `0x492d90`,表基址 `0x7aac60`,121 槽,默认体
`0x48c0c0`;默认体/空操作体名单与 v555 清单逐名一致。

## 3. 实现(按版本/表能力门控,不按游戏名)

- `SemanticProfile::from_yscm_version`:500 与 555 共用
  `RecordDelimited`/`FileArray`/`Inert`/`EndOnly`/`FirstChar`/
  `LoadTime`/`SlotId`/`partial_index`;新增 `is_v555_family()`。
- `save_image` 仍是**精确 555** 才 `V555`(A=1680):0.500 官方存档
  (`<v500-relirium>/save/*.sd`,195 块)没有 DNO=3 缩略图块,写侧保持
  `ContainerProfile`,等有 0.500 缩略图样本再裁决。
- 引擎:555 族(500/555)安装独立运行面;`CGINFO[LINT/LINT2]` 回读
  对 555 族开启。
- 分发表:555 族用 555 默认/空操作清单;0.500 另把
  `MOVIE2`/`MOVIE2END`/`MOVIE2INFO`/`REGINFO` 覆盖为 `NoopEval`
  (官方 29B 空桩)。
- `PRODUCT[COM=1/2/3]` 按 0x488b20 落地:1=写 `0x1F6`(INT)/`502.0`
  (FLT),3=写 0,2=挂账(错误消息文本未取证)。语料只有
  `es_first.yst pc1` 的 COM=3。

## 4. 验证

- `cargo test -p yu-rt --lib semantics`(新增 500 断言);
- 改后启动:无 `rt.expr.malformed`;`textures: decoded=204`;
  `rt.text.file.lines`: `scenario\00_tr.txt` 163 行 → `$2875`;
- 启动期 DIALOG(更新检查)由 `probe_engine_dialogs --answer 2` 回答后,
  标题层 `ES.TITLE.*` 出现;第 900 帧截图 `title900b.png`(8,296,178 B)
  为正确的 1920×1080 标题背景(4 瓦片拼图)并显示「前回のつづきから」,
  不再是清屏黑;主任务进入 `es_eris` 按钮事件循环;
- `CGINFO[LINT2]` 不再报 `gfx.info.unsupported`,按钮状态层带
  `LINT2=18/999`。

## 5. 残余(均非 0.500 独有,按命令补)

- `SYSTEM[SX/SY]`:**VM 侧已实现**(`Vm::set_screen` + `apply_system_screen`,
  555 族门控;`_SCREEN_SIZE_X/Y` 跟随);官方同时重建后缓冲
  (`FUN_004430b4` 的显示模式/尺寸重算),宿主重绘端口尚未接线,
  见 `rt.system.trace`。
- `PRODUCT[COM=24001/24002/24009]`(YSWEBM 预载/播放/结束)与
  `COM=56/57`(更新检查文件时间):语料用到,当前按 no-op 挂账;
  需要 WebM 图层媒体后端再实现。
- `$4780[-1]` 负下标:0.500 启动链命中一次,当前按 DIV-RT-002 忽略;
  官方负下标语义未取证。
- 0.500 的 `MATH` 处理体比 v555 多一段(疑似新增函数),语料只用
  COS/SIN/SQRT/RND/POW/ATAN2,未阻塞。
