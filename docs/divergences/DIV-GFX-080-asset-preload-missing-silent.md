# DIV-GFX-080 · 通用 cgsys 资产在本作未发行时,预加载按 WARN 刷屏

- 状态: fixed
- 优先级: 低(仅日志噪声;像素/行为与官方一致)
- 发现日期 / 发现方式: 2026-11-07 / 三款商业包桌面实跑日志
  (`yu-render: [asset] preload failed key=…: asset.missing`)

## 复现

- 触发: 任意商业包启动。`es_Button`/`es_Config`/`es_Gsa` 的通用脚本
  按固定命名引用 cgsys 资产,标题/设置画面初始化即 `CG[FILE=…]`。
- 命中示例(全部核实为**包内确实没有**,不是查找路径问题):

| 资产 key | 引用来源 | 包内实际 |
|---|---|---|
| `cgsys/main/button/btn_click_bt3` | `main/button/btn_click_bt3`(s108/s109 GOSUB 字面量) | 无 `btn_click*`(SDK 0.495 cgsys 有) |
| `cgsys/main/tip_namewindow_back` | `main/tip_namewindow_back`(s108 pc38 等) | 只有 `tip_namewindow.png` / `_txspace.png` |
| `cgsys/main/nextvoice/icon_01` | `main/nextvoice/icon_01`(0.494 包) | 无 `main/nextvoice/`(有 `skipicon/autoicon`) |
| `cg/face/face_back` | 通用面部部件路径 | 0.494/0.481 包无(SDK 有) |
| `cgsys/config/exp/playstyle/02、08、09` | s91 pc8..24 逐号 `CG[FILE=…/NN]` | 02/09 只发行了 `_02.png`/`_09.png`(脚本不引用下划线版);08 完全没有 |
| `cgsys/config/exp/sound/07`、`voice/04` | 同上(设置项说明图) | 无 |
| `voice/GLO_M01_0458` | 语音测试表的 ID 拼接(s86 pc181..) | vo/vof 只有 `mit_*`/`sin_*` 命名,无 GLO 组 |

- 旁证: `pac/ed01..ed04.ypf` 被 VFS 跳过是**正确**的 —— 文件头
  `00 00 01 BA` 是 MPEG-PS 影片,不是 YPF 容器。

## 官方行为

- `CG[FILE=…]` 装载失败 = 该层没有图片;**不弹窗、不写日志、不中止脚本**
  (引擎文件查找按原始字节哈希,查不到即空层)。
- 之后对空层的 `CGINFO[EXIST]` 等按缺失语义返回;`E=0` 的未用层从不绘制。
- 语音同理: `SOUND` 找不到文件即静音,无可见告警。

## 内核行为

`preload_assets`(soft `render.rs` / wgpu `lib.rs`)对每个 miss 打
`log::warn!`,启动即出现 2–10 行 WARN;诊断事实本身正确(资产业已缺失),
但级别误导为「引擎缺陷」。

## 根因

预加载日志级别沿用了开发期「缺资产要显眼」的口径,没有区分
「本作未发行(官方静默)」与「读取/解码失败(真缺陷)」。

## 修法

- 预加载 miss 降为 `log::debug!`(`[asset] preload miss key=…`),与
  `RenderReport` 既有统计(`missing assets: N` / `asset.missing` 跳过项)
  分工:默认日志不重复,`--log-level debug` 仍可看逐条原因。
- 不改变查找逻辑、不改变 `missing assets` 计数、不改变跳过项。

## 回归

- 三款商业包 headless 1100 帧:启动日志 `[asset] preload failed` 计数 0;
  `missing assets: 1 / 0 / 3` 与修复前一致(报告口径不变)。
- 0.494 首屏 PNG sha 修复前后一致 `1d684027…dcd75d`(像素不变)。
- 残留(与本条无关,记录在案):
  - `fontdb: Failed to load a font face 0 from 'C:\\Windows\\Fonts\\mstmc.ttf'
    cause malformed font` —— 第三方字库解析告警,系统字体本身异常,
    内核已正常回退;
  - `sv.file.write … 拒绝访问` 在本机沙箱里由「工作区编译产物写
    `D:\\Download`」触发;把 `--save-root` 指到可写目录后写入成功、
    文件时间戳更新 —— 环境限制,不是内核写盘缺陷。
