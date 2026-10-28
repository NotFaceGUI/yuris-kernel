# 开源工具参考档案(算法可直接采用,须致谢)

> 用途: 遇到格式/语义问题时,先来这里找**可对照的开源实现**。
> **策略(用户决定,2026-10-19): 算法/常量/流程可直接采用**,采用处在文件头注明来源,
> 并在 README 致谢(见 §4 致谢链);整段复用代码前确认许可状态。
> 真值优先级不变: 官方引擎行为/官方产物 > 开源实现。

## 1. 工具清单

| 项目 | 语言/许可 | 能力 | 对本仓库最有用的点 |
|---|---|---|---|
| [ZQF-ReVN/RxYuris](https://github.com/ZQF-ReVN/RxYuris) | C++/GPL(见仓库) | `.ypf`/`.ybn` 工具集: YSCOM Loader(调官方编译器并保存产物)、YSCM/YSTB 解析、**YSTB_GuessXorKey** | 编译协议参考实现(已用);XOR key 猜测思路;`YSTB_Coder::Xor` 分段语义(已实证) |
| [Aionfatedio/YU-RIS-Script-Editor](https://github.com/Aionfatedio/YU-RIS-Script-Editor) | Python/**未声明许可** | YBN 解析(V2/V5)、文本编辑/回写、YPF 读写(纯脚本封包可回封)、**自检测密钥/编码/引擎信息**、导出纯文本/翻译三行对照;**README 自述已测 Whirlpool 系及多部商业作品** | ①YSTB **V2** 支路(我们目前只实测 V5);②密钥/编码自检测策略;③**翻译=数据层嵌入**(写回 YBN 文本段、纯脚本 YPF 自动回封),**无 exe/DLL 注入**;④文本提取与三行对照格式 |
| [fengberd/YuRISTools](https://github.com/fengberd/YuRISTools) | 见仓库 | YU-RIS 工具集 | 脚本/封包辅助对照 |
| [jyxjyx1234/YURIS_TOOLS](https://github.com/jyxjyx1234/YURIS_TOOLS) | 见仓库 | 汉化工具包 | 文本提取/回写流程、编码处理 |
| [shimamura-sakura/yuri](https://github.com/shimamura-sakura/yuri) | Rust | Yu-Ris 反编译/编译器(并行、增量) | 命令语义与结构化反编译思路;可与我们的 yu-script/yu-rt 对照 |
| [crskycode/YPF_Tool](https://github.com/crskycode/YPF_Tool) | 见仓库 | YPF 创建 | 打包器行为对照(与我们已实证布局互证) |
| [Xorboth/YPF-Manager](https://github.com/Xorboth/YPF-Manager) | 见仓库 | YPF 解包/重打包 | 索引遍历与重打包实践 |
| [dreamsavior/ypf-repacker](https://github.com/dreamsavior/ypf-repacker) | 见仓库 | YPF 重打包 | 同族对照 |
| [MishaIac/YurisKeyFinder](https://github.com/MishaIac/YurisKeyFinder) | 见仓库 | `.ybn` 密钥查找 | 换密钥作品(.ybn 家族)的 key 恢复思路 |
| [morkt/GARbro](https://github.com/morkt/GARbro) | C#/MIT(见仓库) | 通用 galgame 资源浏览器,含 YPF 解密 | YPF 解密的第三种独立实现;Script-Editor 鸣谢其 YPF 解密算法 |
| [storycraft/emote-psb-rs](https://github.com/storycraft/emote-psb-rs) | Rust/MIT | PSB/MDF 读写、serde、二进制资源流;**不渲染** | `yu-emote` 的 PSB 二进制层依赖;E-mote M1 基线 |
| [Mieluoxxx/amadeus-emote-loader](https://github.com/Mieluoxxx/amadeus-emote-loader) | Rust/Apache-2.0 | PSB/JSON 规范化、部件查表、**显式 wasm32**;不渲染;已归档 | M1 规范化/部件语义参考;wasm 证据 |
| [xmoezzz/eluna](https://github.com/xmoezzz/eluna) | Rust/**未声明许可** | PSB 解析、motion/layer/timeline 提取、wgpu 预览播放器 | 行为参考;**无许可,不复制代码** |
| [UlyssesWu/FreeMote](https://github.com/UlyssesWu/FreeMote) | C#/CC BY-NC-SA 4.0 | PSB/EMT 工具 + 桌面 Viewer;PSB body cipher 文档 | 只作 oracle/事实参考;非商业许可 |
| [**NotFaceGUI/yu-emote**](https://github.com/NotFaceGUI/yu-emote) | Rust/MIT OR Apache-2.0 | **本线实现仓库**:PSB 解析/body 解密/key 搜索、YU-RIS 适配骨架 | E1 数据层(M1 起);以 **git submodule** 挂在 `yu-emote/` |

> **解密算法代际对照**(来自 Script-Editor 鸣谢,均为二手说法,使用前自行核实):
> `YURIS_TOOLS` = 新版引擎解密;`RxYuris` = 旧版引擎解密;`GARbro` = YPF 解密。
> 三代对照正好覆盖我们跨 `v49x/v55x` 的密钥恢复需求。

## 2.5 翻译实现方式对照

| 方式 | 代表 | 说明 | 对本仓库的意义 |
|---|---|---|---|
| **数据层嵌入** | YU-RIS-Script-Editor、YURIS_TOOLS | 改 `.ybn` 文本段(参数数据区)并回封 `.ypf`/写回 `ysbin` | 与我们的目标一致:只要 yu-format 能字节级读写 YSTB 与 YPF,翻译即自然支持 |
| exe 注入/hook | 不在本清单内 | 运行时挂钩替换文本,通常需要匹配具体 exe | **不作为参考方向**——违反"数据只读"不变式(A2),且版本耦合 |

> 结论:本项目(内核)不需要任何"翻译器"形态;文本编辑能力是 yu-format 字节级
> 往返的副产品。编译器/打包器保持官方工具链。

> 各项目许可证以仓库当前文件为准,表中未写明的需先核实;未声明许可证的项目
> (如 Script-Editor)**按"仅事实参考"处理**。
> E-mote 相关项目与许可边界见 `docs/emote-parity.md` §5:FreeMote/FreeMote
> WebGL 为非商业许可(仅 oracle),`eluna` 无许可(仅行为参考),`yu-emote`
> 是本线独立实现(MIT OR Apache-2.0)。
> 部分工具宣称支持通过自检测密钥处理换密钥的商业作品(含 Whirlpool 系等发行商作品)——
> 这与我们的 `<game-a>`(Whirlpool 系,换密钥情形)场景直接相关,可用于**验证 key 恢复策略**,
> 但其结论仍需落到官方引擎/真实数据上。

## 2. 按子系统索引(出问题时先查谁)

| 子系统 | 参考项目 | 用法 |
|---|---|---|
| YPF 索引/打包 | `docs/formats/ypf.md`(本仓库实证) → YPF_Tool / YPF-Manager / ypf-repacker / Script-Editor `core/ypf.py` | 我们的布局以官方 YSPac 包为准;工具仅用于交叉验证"别人怎么解释" |
| YSTB V5 | RxYuris `YSTB.cpp`;Script-Editor `core/ystb.py` | V5 分段异或已实证;对比文本段/参数段的读写序 |
| YSTB **V2**(旧版本游戏) | Script-Editor `core/ystb.py`(README 明示 V2/V5) | 尚无 V2 真实样本——引入时先找语料再实现,禁止臆造 |
| 密钥恢复(换 key) | YurisKeyFinder;RxYuris `YSTB_GuessXorKey`;Script-Editor 自检测 | 只作策略参考;失败一律显式报错/挂账 |
| 文本提取/回写 | Script-Editor(导出纯文本/三行对照)、YURIS_TOOLS | 用于生成"文本↔字节"对拍语料;注意编码(CP932/GBK)边界 |
| 编码自检测 | Script-Editor(自动编码检测) | 与 `docs/architecture.md` §7 的确定性裁决策略对照;我们要求可复现 |
| 命令语义 | `yuri` 反编译器;官方手册(一手) | 手册优先;反编译结论只作待验证线索 |
| 引擎启动/挂载 | 本仓库 `docs/sdk-eris-analysis.md` §8、`docs/formats/ypf.md` §4 | 已由官方二进制实证,不必依赖第三方说法 |

## 3. 使用纪律

1. **算法/常量/流程可直接采用**(用户决定,2026-10-19):从上述开源实现得到的解析算法、密钥恢复
   流程、字段布局、编解码策略等,**允许直接采用**;采用处必须在文件头注明来源
   (`参考 <项目名>/<文件>`),并在 README 致谢。
2. **整段复用代码前看许可**:无声明许可证的项目(如 Script-Editor)优先按其算法与行为重写;
   确需整段复用则记录来源与许可状态,由仓库主人在 README 致谢与合规说明中处理。
3. **事实与代码分开记**:从工具得到的结论写入 `docs/`,并标注"来自开源实现,未在官方引擎验证"。
4. **冲突裁决**:工具行为与官方包/官方引擎冲突时,以官方为准,并在 `docs/divergences/` 留勘误。
5. 新增参考项目时,补一行"能力 + 可参考点 + 许可证状态",并写清是否亲自验证过。

## 4. README 致谢(只列实际采用)

README 只列**实际采用/实际参考**的项目；参考但未采用的不进入 README 致谢，
保留在本文件 §1。当前实际采用：

- **旧接手线**(`<repo-old>`): 启动链、任务/驱动形态、格式事实与 golden 回归参照
- **RxYuris**(ZQF-ReVN): YSTB/YSCM/YSTL 解析流程、YSCOM Loader
- **YU-RIS-Script-Editor**(Aionfatedio): YSTB V5 段序、V2 版本带判定、编码自检测
  （未声明许可，仅事实参考，未复制代码）

未采用(保留在 §1，不进入 README): YURIS_TOOLS、GARbro、yuri、YuRISTools、
YPF_Tool、YPF-Manager、ypf-repacker、YurisKeyFinder 等。

## 5. 实际采用登记(yu-rt,成果 C2)

> 任务书 C v2 要求:采用处文件头注明 `参考 <项目>/<文件>`,并在本节登记。
> 以下为**实际采用**的条目(算法/工程形态),语义结论仍以官方文档/引擎为准。

| 采用处(`crates/yu-rt/`) | 来源 | 采用内容 | 许可/状态 |
|---|---|---|---|
| `src/task.rs` | `<repo-old>/crates/yuris-vm/src/task.rs` | 单一扁平任务链、链长入口快照、每逻辑帧每任务一步、泵到让出无预算、TASK upsert、续帧标志(`Effect::RequestExtraFrame`) | 同仓库旧线,GPL-3.0-or-later |
| `src/vm.rs`(`exec: BTreeMap<TaskId, ExecState>)` | `<repo-old>/crates/yuris-vm/src/driver.rs` | 「执行位取出—执行—放回」的宿主形态(对应旧线 `swap_exec`);主线与任务共用同一执行路径 | 同上 |
| `src/vm.rs`(`BootConfig`/启动顺序) | `<repo-old>/crates/yuris-vm/src/boot.rs` | 启动链顺序:表 → 标签 → 入口 → 主线任务 → 装载入口脚本;入口标签缺省 `SYSTEM_START` | 同上 |
| `src/expr.rs`(记录切分与 27 项操作码) | `<side-repo>/docs/format-vm-opcodes.md` §3.9(引擎一手) + `<repo-old>/crates/yuris-script/src/insn.rs`(工程形态) | `[op][len u16][payload]` 走查、立即数宽度、变量引用、数组取值(引用+下标+aload) | 引擎事实为真值;形态参考旧线 |
| `src/tables.rs`(YSCM 解析) | `RxYuris/lib/YurisStaticLibrary/YSCM.cpp` | 命令表构建流程(名字 + 参数数 + 参数名/属性)交叉校验 | GPL(见仓库) |
| `src/sysvar_abi.rs` + `data/sysvar-abi-0495.tsv` | 官方 0.495 `YSCom.ycd`(脚本生成,附 SHA-256) | 548 项系统变量 ABI(名字/类型/维数) | 官方产物事实 |
| `scripts/audit_hardcode.py` | 本仓库(原创) | A5 零硬编码守卫 | 自有 |

未采用:`shimamura-sakura/yuri` 的反编译结构(本任务只做运行核,未涉及反编译);
`YU-RIS-Script-Editor` 的 V2 支路(未引入 V2 语料)。

## 6. 实际采用登记(yu-format,成果 B2)

> 以下为**实际采用**的条目(算法/流程/形态);一切行为真值以官方产物实测为准,
> 与笔记/参考实现冲突处见 `docs/divergences/DIV-005/006/007/008`。

| 采用处 | 来源 | 采用内容 | 验证方式 |
|---|---|---|---|
| `crates/yu-format/src/ystb.rs` | RxYuris `lib/YurisStaticLibrary/YSTB.cpp` | `YSTB_Coder::Xor` 分段异或语义(四段各自从密钥下标 0 重启)、`YSTB_V5::Init` 三段索引解析流程 | 136 个官方 YSTB 全量解码 + 回封逐字节一致 |
| `crates/yu-format/src/ystb.rs` | YU-RIS-Script-Editor `core/ystb.py` | `_parse_v5` 段序/`_decrypt` 结构、V2 版本带判定(200<v<300)、`M` 引号串载荷形态 | 语料实测 + 合成单测(V2 → `Unimplemented`,DIV-007) |
| `crates/yu-format/src/yscm.rs` | RxYuris `YSCM.cpp` | 记录流解析流程(名\0 + argc + 参数名\0 + 2×u8)、尾段 37 消息 + 0x100 附表 | 11,156 字节官方 ysc.ybn 精确闭合 |
| `crates/yu-format/src/ystl.rs` | RxYuris `YSTL.cpp`(`YSTL_V5`) | 记录尾五 u32 字段定性(时间戳×2 + 变量/标签/文本计数)——旁线"脚本文件号"解释被数据否定(DIV-005) | 141 条清单 ↔ 136 文件 ↔ ysl 三方一致 |
| `crates/yu-format/src/codec/auto.rs` | YU-RIS-Script-Editor `core/encoding.py` | "双编码严格解码 + 字符构成统计"思路;裁决公式/平局规则为本仓库重造(A3 确定性,缺省 CP932) | 日/中样本判定 + 确定性断言 |
| `crates/yu-format/src/ysvr.rs` | 旧接手线 `<repo-old>/crates/yuris-format/src/ysvr.rs` | `ty=0` = 仅声明无初值段(其 v555 商业游戏 3362/3362 闭合;本仓库 495 语料 2568/2568 闭合,双语料互证) | 全语料字节级闭合 |
| 密钥自检测(`YSTB_GuessXorKey`/Script-Editor/YurisKeyFinder)、YURIS_TOOLS 新版解密 | (未采用,登记备用) | 本次语料均为默认键/495;换密钥场景留待真实作品 | — |

> 采用纪律执行情况:算法级采用、逐处文件头注明来源;未整段复用任何代码
> (Script-Editor 未声明许可,按"仅事实参考"处理);编码自检测的确定性裁决、
> 评分权重为原创实现(参考 `docs/architecture.md` §7)。
>
> **交叉验证(成果 B2 补充)**:Script-Editor 原版 `core/ystb.py` /
> `core/encoding.py` 原样跑在本语料上——136 文件 insts=36974/windows=80102
> 与 yu-format 逐项一致、其 `to_bytes` 回封 == 原文件、其 `guess_key`
> 独立恢复 0x96AC6FD3、编码判定 shift_jis/gbk 与本实现同判;
> 原始输出见 `PROGRESS.md` 成果 B2。
