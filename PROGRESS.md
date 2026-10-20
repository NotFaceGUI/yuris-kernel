# PROGRESS · 进度真相

> 唯一进度真相。每项改动追加"成果 N"(日期、内容、证据、验证命令与原始结果)。
> 勘误必须留痕: 旧结论、更正内容、发现方式。

## 结论汇总(2026-10-20 按实际落地重写)

| 子系统 | 状态 | oracle | 备注 |
|---|---|---|---|
| yu-log | **已实现** | 结构化码/JSONL 往返 | 成果 A |
| yu-vfs | **已实现** | 等价性 + 真包索引闭合 | 成果 A;两种方言,一种待真样本 |
| yu-format | **已实现** | 真实 ERIS/SDK 语料 + 字节一致 | 成果 B1/B2、E3 |
| yu-rt | **已实现** | 事件流/任务/帧边界 + 真语料 | 成果 C/E2;脚本/VM/任务/`InputTimeline` 均在此 |
| yu-gfx | **已实现(语义层)** | `CompositorFrame` + ERIS 首屏 golden | M3-GFX/F1/F2;COPY/图像加工/部分 MODE 挂账 |
| yu-font | **已实现** | 字形/样式 golden + ERIS 配置直出 | 成果 G1 |
| yu-backend-soft | **已实现(确定性参考)** | 同帧 PNG 逐字节确定 + 首帧已产出 | 成果 M3-SOFT;可执行子集,skipped 可审计 |
| yu-trace | **已实现(schema v3)** | 内核自回放 + 官方屏幕级 producer | 成果 H1;官方事件级 L1 未达成 |
| yu-backend-wgpu | 未开始 | soft 同帧逐像素 diff | 语义稳定后开工 |
| 桌面 / wasm / Android 宿主 | 未开始 | 同帧呈现 + 输入时间线 | wasm 仍是 M3 最终呈现目标 |
| yu-sound / yu-save | 未开始 | 各域命令 / 字节一致 | 尚无 crate |
| scenario 解释器 / yu-cli | 未开始 | 同文本同锚点 | CLI 是否保留待定 |

> **勘误(2026-10-20,架构与实现对账)**:本表替换 2026-10-19 的旧汇总。
> 旧结论是 12/9-crate 设计,列有 `yu-script`/`yu-macro`/`yu-vm`/`yu-graphics`/
> `yu-input`/`yu-player` 等;实际落地把脚本/VM/任务/帧/输入时间线收敛进
> `yu-rt`,渲染语义独立为 `yu-gfx`,字体独立为 `yu-font`,软后端为
> `yu-backend-soft`,**没有 `yu-assets` 兄弟 crate**(纹理解析在后端/装配层)。
> 旧表述“M3 = wasm 呈现闭环”保留为最终目标,但 M3 已拆成
> M3a 语义层 → M3b 软后端首帧 → M3c wgpu 对拍 → M3d wasm 宿主闭环。
> 发现方式 = `cargo metadata` + `git log` + 实际首帧产物;历史成果 1–7 不回改。
> `docs/architecture.md` 仍是 v1.2 设计稿,与本节冲突时以本节与代码为准。

## 架构现状(2026-10-20)

```text
yu-vfs → yu-format → yu-rt(脚本/VM/任务/帧/InputTimeline)
                         │ VmEvent::DomainCall / Text
                         ▼
                  yu-gfx(语义显示列表 CompositorFrame + GfxHost)
                         │
             ┌───────────┴───────────┐
             ▼                       ▼
   yu-backend-soft             yu-backend-wgpu(未开始)
   确定性参考 / PNG / golden    主后端,同帧逐像素对拍
             │                       │
             └───────────┬───────────┘
                         ▼
              hosts: 桌面 / wasm / Android(未开始)
yu-font: 字体发现/栅格/样式,供 gfx 文本与后端使用
yu-trace: vfs/vm/frame 边界录制、回放、跨 producer diff
```

- 纹理解析不在独立 crate:第一个实现是 `yu-backend-soft::TextureStore`
  (`yu-vfs` 取字节 + `frame.image_probe_order` + image 解码 + 缓存)。
- 文字链路: `yu-font` 已独立完成,尚未桥接进 `yu-gfx`/后端(DIV-GFX-012)。

## 成果 1 (2026-10-19) · 仓库脚手架

- 内容: 12 个子系统 crate 骨架 + oracle 方法论(docs/oracle.md)+ 不一致档案制度
  (docs/divergences/)+ 工作须知(AGENTS.md,不入库)。**零代码移植**;
  LICENSE 自旧接手线拷贝(GPL 全文,许可文本非代码)。
- 设计立场: 净室新写;真值 = 官方引擎行为;旧线 <repo-old> = 只读参考实现。
- 验证: `cargo check --workspace` 与 `cargo test --workspace` 结果见当日本机记录(骨架应全绿,
  但无实质测试 —— 这是起点,不是成果)。

## 成果 2 (2026-10-19) · SDK 一手脚本系统分析 + 架构结论

- 内容: 以官方 SDK 0.495 一手资料(71 个 .yst 源脚本 / 官方手册 html / YSCom.ycd 二进制 /
  REA 对 YSCom.exe 取证)完成脚本/语法/宏/命令/结构/推进/输入输出的完整分析,
  落盘 `docs/sdk-eris-analysis.md`(§7 为架构结论:宏=必需编译前端、16 槽寄存器调用约定、
  任务让出=WAIT/文本、全员让出才渲染、输入=带符号帧计数器、按名分派)。
- 勘误留痕: ①旁线"无文档化 CLI"说法不确 —— REA 取证 YSCom.exe 要求 argv[1] 以 "ys" 开头
  且依赖启动器创建的共享内存 "YU-RIS Compiler FileMap Object",双击秒退即此因;
  ②本日曾尝试 GUI 自动化编译 ERIS,**经用户决策取消**: 分析任务无需编译产物,
  商业游戏封包已足够当 yu-format 语料;方法留档 workbench(yscom-launch.ps1/yscom-compile.cmd)。
- 验证: YSCom.ycd 头部 `YSCD`+版本 0x1EF 与 105 个 COM_ 命令名经 Python 直读二进制复核;
  project-eris 副本自官方源重建,1446 文件计数一致 + 5 项 sha256 抽查一致。
- 附带: crates/ 12-crate 骨架已按用户指示整体移除,仓库形态待按 §7 结论重定(未定稿前保持空置)。

## 成果 3 (2026-10-19) · 架构基线 v1(散盘→封包迁移轴)

- 内容: `docs/architecture.md`。核心决策 A = 数据源抽象(DataSource: DirSource/YpfSource/MemSource
  + 叠加挂载 + 路径归一只做一次),散盘项目与 .ypf 封包游戏收敛为同一程序,配永久"等价性测试"自证;
  核心决策 B = 9 crate 分层(vfs/format/rt/gfx/snd/inp/save/trace/cli),依赖严格向下,
  四条可序列化边界(①vfs 读 ②事件流 ③帧描述 ④平台命令)挂 trace。
- 依据: sdk-eris-analysis §7 + 官方挂载顺序 REA 证据(base 包 + update1..9 依次覆盖)。
- 状态: 设计定稿待用户确认;crate 实体尚未创建(crates/ 空置)。

## 成果 4 (2026-10-19) · 文本编码策略(并入架构 v1)

- 内容: architecture.md 新增 §6——内核内部一律 Unicode;"编码"收敛为四条字节边界的编解码器参数
  (vfs 查找键的多字节跳步表 / format 文本字段 / save 原编码写回 / 字体呈现只看 Unicode);
  CLI `--codec cp932|gbk|auto`;auto=固定顺序采样+双解码评分+确定性裁决(缺省 CP932),
  结论进 trace/golden 头部,运行中绝不切换。
- 关键事实: CP932 的 0xA1–0xDF 是半角片假名单字节,GBK 同区间是双字节前导 ⇒ 资源名扫描
  必须按编解码器选跳步表;存档字节级兼容要求按**原编解码器**写回,不是按 Unicode。

## 成果 5 (2026-10-19) · 架构 v1.1:零硬编码不变式 + 渲染后端层(wasm 先行)

- 内容(用户指出两处缺失后修订 architecture.md):
  ① 新增不变式 A5「通用运行时零硬编码」——运行核/域状态机禁止任何游戏或版本特定常量,
  命令表/变量表/设置/清单全部运行期从游戏工程装载,缺表显式拒绝启动,禁止内置兜底表;
  CI grep 守卫为执行手段。② 新增渲染层:RenderBackend trait 定义在 yu-gfx(与 FrameDescription
  同处),平台实现各自一 crate —— yu-backend-wasm(当前实现,含 wasm-bindgen 宿主入口/rAF/OPFS)、
  yu-backend-win、yu-backend-android(后续);wasm 先行理由=调试回路最短(canvas 截图差分、刷新即看)。
  ③ 里程碑更新:M3=wasm 呈现闭环。
- 状态: 设计 v1.1;crate 实体尚未创建。

## 成果 6 (2026-10-19) · 架构 v1.2:错误+日志统一包 yu-log

- 内容(用户要求"明确的错误系统,错误就是 log 包"):新增 §8 与 crate `yu-log` ——
  Diag 结构(稳定命名空间错误码 + Fatal/Recoverable/Unimplemented 严重级 + 边界归属 +
  位置上下文 + 因果链)、七类码域(vfs/fmt/table/rt/gfx/snd/inp/save/platform)、
  LogSink+JSONL(与 trace 边界流同格式,trace 消费 log 事件入流,回放时错误序列一并比对)、
  禁止散落 println(与 A5 同族 CI 守卫)。严重级语义对齐官方:官方有 ERROR[COM_XX=n]
  按命令族设错误等级(yst 一手),错误策略是脚本可见语义,须建模。
- crate 清单更新:yuris-kernel = log/vfs/format/rt/gfx/snd/inp/save/trace/cli + backend-wasm(先行)
  + backend-win/android(后续)。

## 成果 7 (2026-10-19) · 三路并行任务书

- 内容: `docs/tasks/task-a-foundation.md`(workspace+yu-log+yu-vfs,先行)、
  `task-b-format.md`(yu-format,真实语料三方互证)、`task-c-runtime.md`(yu-rt,三层真值纪律:
  手册页/真实脚本印证/官方 0.479 行为对照,缺一即 Unimplemented)。B/C 接口契约冻结于任务书 B §4。
- 分工纪律: 各会话独占目录(workspace glob 自动入组);A 先行建骨架;共享文件只 A 可改。

## 成果 A (2026-10-19) · 地基: workspace + yu-log + yu-vfs(M0)

- 内容:
  - 根 `Cargo.toml`:`members = ["crates/*"]`、`resolver = "2"`、`workspace.package`
    (edition 2021 / GPL-3.0-or-later)、`workspace.dependencies = { flate2 = "1" }`;
    `.cargo/config.toml` 把构建产物重定向到工作沙箱的 `cargo-target`。
  - `yu-log`:`Diag { code, severity, boundary, where_: Loc, detail, causes }`、
    `Loc::{Script,Data,Table,None}`、稳定码表(18 个,唯一性/命名空间有单测)、
    `LogSink` + `JsonlSink` / `MemorySink` / `NullSink`(显式注入,无全局)。
    JSONL 为确定性单行输出,字节串按 `\u00XX` 转义可逐字节往返;零第三方依赖。
  - `yu-vfs`:`DataSource { open/exists/list }`;`LookupKey` 归一只做一次
    (`\`=`/`、ASCII A–Z 折叠、CP932/GBK 多字节整对跳过、拒绝 `.`/`..`/NUL);
    `MemSource` / `DirSource` / `YpfSource` / `Mount`(后挂覆盖先挂);
    `YpfSource` 懒索引 + 偏移直读,按包判别两种索引方言,未知字段
    (`0x10..0x20`、`indexPrefix`、`reserved`、`tail`)原样保留。
- 实测确证(本机真实数据):
  - v494 商业包 `data.ypf`(3,315,484,359 字节):`version=494`、`entryCount=12212`、
    `indexSize=0x9542A`;索引 **12,212/12,212 条**全部走完并闭合
    (`consumed=611334 = index_bytes`),名字全部可打印、偏移全部落在数据区,
    `flag=1 ↔ compressedSize<uncompressedSize` 2,702/2,702 成立,
    `flag=0 ↔ 两长度相等`(末条 tail 被索引区截断 4 字节)成立。
  - `.ybn` 抽样 `ysbin\yst00034.ybn`:索引声明 5,956 字节,zlib 解压后 5,956 字节,
    与独立 inflate 逐字节一致。
  - 图像抽样 `cg\ev\8_4e.jpg`:516,377 字节,解压后 JPEG 魔数命中。
  - 官方 0.479 SDK 散盘目录:1,339 个 ASCII 文件入索引,85 个非 ASCII 名按
    `DIV-003` 显式跳过并计数;大小写折叠查找命中(`data/bgm/bgm01.ogg`)。
- 按笔记/参考实现待验:
  - NUL-XOR 索引方言(v500 的 `bn.ypf` 样本缺失):仅由合成包测试覆盖,
    真样本复核待办(`DIV-002`)。
  - 索引 MurmurHash2 在官方查找中的确切作用未复现;本实现按归一名精确匹配
    (`DIV-001`)。
  - >4 GiB 封包的偏移表达未验证,当前显式 `vfs.ypf.unsupported`(`DIV-002` 附注)。
- 验证命令与原始结果:

```text
$ cargo test --workspace -p yu-log -p yu-vfs
yu-log unittests:  13 passed; 0 failed
yu-vfs unittests:  27 passed; 0 failed
equivalence.rs:     7 passed; 0 failed
nul_xor.rs:         2 passed; 0 failed
real_sample.rs:     4 passed; 0 failed

$ cargo test -p yu-vfs --test real_sample -- --nocapture
REAL SAMPLE: <data.ypf>
header: version=494 entries=12212 indexSize=0x9542a fileLen=3315484359 dialect=lenenc-not warnings=0
index: parsed=12212 valid=12212 clean=12212 pathlike=12212 lengths=12212 closed=true consumed=611334/611334
ybn sample: ysbin\yst00034.ybn declared=5956 read=5956 compressed=true
image sample: cg\ev\8_4e.jpg declared=516377 magic=ok
0.479 DirSource: files=1339 skipped=85 (non-ASCII names are explicitly skipped)
loose sample: data/bgm/bgm01.ogg bytes=907689 skipped_total=85
test result: ok. 4 passed

$ YURIS_SAMPLE_PAC=<missing> cargo test -p yu-vfs --test real_sample -- --nocapture
SKIP[count=1] YURIS_SAMPLE_PAC/bn.ypf (...) does not exist   (x3, 累计计数)
SKIP[count=4] YURIS_SAMPLE_SDK0479 is not set
test result: ok. 4 passed        ← 跳过显式可见,不是静默通过

$ cargo clippy --workspace --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
```

- 永久等价性测试:`crates/yu-vfs/tests/equivalence.rs` —— 同一棵临时文件树经
  `DirSource` 与测试专用 ypf 写入器打包后的 `YpfSource` 分别挂载,
  `open/exists/list` 逐项一致;含 root/中间目录/文件/缺失/非目录五类键。
- 勘误与未决档案:`DIV-001`(ypf 索引字段布局与旁线笔记不符,以真实字节为准)、
  `DIV-002`(NUL-XOR 方言无真样本)、`DIV-003`(非 ASCII 资源名缺编解码表)。
- 纪律:零 git 写操作;实现内无 `println!/eprintln!/dbg!`;除 `flate2` 外零依赖;
  游戏路径与条目名未进入实现(仅测试从环境变量取样本)。

## 成果 8 (2026-10-19) · ERIS 官方编译打通(143 产物 + 引擎验收)

- 内容: 以引擎侧协议(命名映射握手 + CreateProcess)驱动官方 YSCom 0.495 完成 ERIS 全量编译,
  产物落 `workbench/project-eris/data/ysbin/`(137 个 YSTB 脚本 + ysc/yscfg/yse/ysl/yst/yst_list/ysv),
  **官方 yu-ris.exe 直接启动并显示标题画面** = 编译链验收通过。
- 一手事实(已并入 `docs/sdk-eris-analysis.md` §8): 映射名精确无空格;项目路径必须带尾部反斜杠;
  PowerShell Start-Process(ShellExecuteEx) 不给这个 VC6 程序传 argv(秒退主因);
  CP932 文件名在非日文系统会损坏(需转区或 ASCII 化);YSTB 磁盘形态=明文头+分段异或
  (密钥 CRC32("Yu-ris")=0x96AC6FD3,LE 字节序,V5 四段各自重启);
  七个元数据文件各有独立魔数(YSCM/YSCF/YSER/YSLB/YSTD/YSTL/YSVR)。
- 工具: `workbench/yscom_harvest_rs`(Rust,零依赖,LE 模式 + 逐秒状态诊断)。
- 给 A/B 任务的语料: 上述 ysbin 可直接作 yu-format/ybn 解码的一手对照(文件名已 ASCII 化,
  映射清单 workbench/rename-manifest.json)。
- 下一步: 打包成 .ypf(商业形态),再作 yu-vfs 的 YpfSource 真实语料。

## 成果 9 (2026-10-19) · ERIS 测试工程立档 + YPF 布局实证修正

- 新增 `docs/eris-corpus.md`(测试工程):官方核心语料 = 编译产物 `data/ysbin`
  (137 YSTB + 7 元数据表)+ 官方 YSPac 包 `data.ypf`;四步复现(编译/调试运行/打包/发布启动);
  **发布模式实测通过**:`DEBUGMODE=0` + 包放游戏根 → 引擎直接读包内 ysbin 出标题,无编译。
- 新增 `docs/formats/ypf.md`(官方包逐字节实证):头 32B(索引自 0x20 起,v494 族可能 0x24——需探测)、
  步长 nameLen+27、hash 抽验 4/4 与 MurmurHash2(name,seed0) 相等、
  flag 0=zlib/2=原样、dataOffset **直指负载**(无独立记录头)、末 8 字节未知字段不依赖。
- 勘误(会话 A 的 DIV-001 定位):旧接手线 `LenEncNot` 解析器按 0x24 起索引且字段切分为
  `code+NUL+usize+...`,整体偏 1 字节 → 大包读出垃圾;修正依据与症状已写入 ypf.md §6。
- 验证: 1407/1407 条目行走恰好抵达数据区起点;抽验 3 条负载(zlib 头/PNG 字节)与索引字段完全吻合。

## 成果 10 (2026-10-19) · 开源工具参考档案 + 会话 A 重启计划(仅文档)

- 新增 `docs/references/open-source-tools.md`: 收录 9 个可参考的开源 YU-RIS 工具
  (RxYuris / YU-RIS-Script-Editor / YuRISTools / YURIS_TOOLS / yuri / YPF_Tool /
  YPF-Manager / ypf-repacker / YurisKeyFinder),按子系统索引"出问题先查谁",
  并写死许可边界(无声明许可=仅事实参考,如 Script-Editor)与冲突裁决(官方优先)。
  关键新增: Script-Editor 的 **YSTB V2 支路**、密钥/编码自检测、文本三行对照导出,
  以及其宣称支持的换密钥商业作品(含 Whirlpool 系)场景。
- 新增 `docs/tasks/session-a-restart.md`: A 的重启计划(修 YpfSource 两处实证错误 +
  官方真包 data.ypf 回归清单 + DIV-001 结案 + 启动提示词)。
- 本成果只写文档,未改任何代码。

## 成果 11 (2026-10-19) · 参考档案补充:翻译实现方式与解密代际(仅文档)

- `docs/references/open-source-tools.md` 增补:
  ①Script-Editor 行注明"**翻译=数据层嵌入**(写回 YBN 文本段、纯脚本 YPF 自动回封),
  无 exe/DLL 注入",并记其 README 自述已测 Whirlpool 系及多部商业作品;
  ②新增 GARbro(Ypf 解密第三实现)与"解密代际对照"(YURIS_TOOLS=新版 / RxYuris=旧版 /
  GARbro=YPF,均为二手说法待核实);
  ③新增「翻译实现方式对照」小节:数据层嵌入 vs exe 注入,结论为本项目只需字节级
  YSTB/YPF 往返,exe 注入因违反 A2(数据只读)不作参考方向。
- 仅文档改动。

## 成果 12 (2026-10-19) · 勘误:会话 A 计划的方言范围与语料指向

- 发现: A 的重启提示词只写了"0x24/0x20 起点探测",**未写明索引方言范围**;
  `task-a-foundation.md` 还残留"0.479 SDK 跨版本包"的旧指令,与"语料已替换为用户编译的
  ERIS demo 包"相矛盾。
- 更正(留痕,旧结论作废):
  ①`docs/formats/ypf.md` 新增 §1.1「索引方言」:LenEncNot(本 demo 包,已实证)与
  NulXor(NUL 结尾+XOR key、步长 nameLen+26,未验证)分列;本次只做 LenEncNot,
  NulXor 遇包显式报"未验证方言"挂 DIV-002,禁止猜 key。
  ②`docs/tasks/session-a-restart.md` 顶部新增「语料与范围」:语料 = `project-eris/data.ypf`
  (官方 YSPac 0.495,1407 条);旧商业样本(bn.ypf/0.479)不在范围。
  ③启动提示词同步补上这两条;`task-a-foundation.md` 的"0.479 跨版本包"改为方言范围条目。
- 仅文档改动。

## 成果 A2 (2026-10-19) · yu-vfs 拆包修正(官方 YSPac 0.495 实测驱动)

- 触发: 会话 A 重启计划 `docs/tasks/session-a-restart.md`;依据 `docs/formats/ypf.md`。
  旧 `YpfSource` 有两处实证错误: 索引起点固定 `0x24`、字段切分整体偏 1 字节。
- **实测确证**(官方 `data.ypf`: 1,407 条 / 31,501,753 字节,版本 495):
  - 索引起点 `0x20`;行走 1,407 条后 `consumed = 79,647 = index_bytes`,恰好等于头部
    `+0x0C` 的 `dataStart = 0x1373F`。
  - 条目布局 `u32 nameHash | u8 lenEnc | 反名 | u8 fileType | u8 isCompressed |
    u32 usize | u32 csize | u32 dataOffset | u32 unknown0 | u32 payloadHash`,
    步长 `nameLen + 27`;`dataOffset` 直指负载,无独立记录头。
  - `nameHash` = MurmurHash2(名字, seed 0): **1,407/1,407** 命中。
  - `payloadHash` = MurmurHash2(存储字节, seed 0): **1,407/1,407** 命中
    (这条把"校验字段"从猜测变成实证)。
  - `isCompressed=1` 共 152 条,全部是 zlib 流且 `csize` 等于流实际消耗字节;
    `isCompressed=0` 共 1,255 条且 `csize == usize`。
  - 24 条 zlib 解压长度 == `usize` 且与独立 inflate 逐字节一致;
    64 条 raw 首字节与扩展名一致(png/jpg/ogg/wav/ybn 全中,image magic 64/64)。
  - 点名验收: `open("ysbin\ysc.ybn")` = 11,156 字节,`YSCM`,版本 495
    (大小写与 `/` 分隔符变体同样命中)。
  - 类型分布: `{0:146, 2:1245, 3:7, 6:8, 9:1}` —— 其中 `type=3` 且 zlib、
    `type=0` 且原样并存,**证明 `fileType` 与 `isCompressed` 是两个独立字节**,
    旧文档的单字节 `flag` 模型无法同时解释。
- 修复:
  - 索引起点改为**探测**: 候选 `{0x20, 0x24}`,取"恰好闭合 + 名字哈希命中最多"者,
    平局取更小的起点(确定性)。
  - 字段按上述布局重写;`isCompressed` 非 0/1 → `vfs.ypf.unsupported`(Unimplemented);
    名字哈希不符 → `vfs.ypf.hash_mismatch`(Recoverable,可读但记账);
    负载哈希不符 → `vfs.ypf.checksum`(Unimplemented,引擎多段续读未实现,不猜)。
  - 未验证的 NUL-XOR 方言收窄为回退路径(须条数相等+走到块尾+全可打印名+含路径分隔符),
    命中即挂 `vfs.ypf.unsupported` 警告,不再吞掉官方布局的字段错误。
- 新增 `yu-vfs/src/encoding.rs`(用户要求"编码也要条理清晰"):
  零依赖、无浮点的确定性检测器,候选 `UTF-8/CP932/GBK/Big5`,输出
  `Decision { encoding, only_candidate, decisive, bom, scores }` 与 `evidence_line()`
  (可直接进 trace/golden 头)。判据参考开源实现 `YU-RIS-Script-Editor/core/encoding.py`;
  该实现未声明许可证,仅借鉴判据、未复制代码。码表仍归 `yu-format`(DIV-003 保持 partial)。
- 验证命令与原始结果:

```text
$ YURIS_SAMPLE_ERIS_YPF=<official 0.495 data.ypf> \
  YURIS_SAMPLE_SDK0495=<0.495 SDK システム> \
  cargo test -p yu-vfs -- --nocapture
yu-vfs lib:        37 passed; 0 failed
equivalence.rs:    10 passed; 0 failed
nul_xor.rs:         2 passed; 0 failed
real_sample.rs:     4 passed; 0 failed

REAL SAMPLE: <official data.ypf>
header: version=495 entries=1407 dataStart=0x1373f fileLen=31501753
index: start=0x20 bytes=79647 consumed=79647 closed=true nameHash=1407/1407 compressed=152 types=[(0,146),(2,1245),(3,7),(6,8),(9,1)]
dialect: hashed-len
ysc via "ysbin\ysc.ybn": 11156 bytes, magic YSCM, version 495
ysc via "YSBIN\YSC.YBN": 11156 bytes, magic YSCM, version 495
ysc via "ysbin/ysc.ybn": 11156 bytes, magic YSCM, version 495
0.495 DirSource: files=1358 skipped=92
loose sample: data/bgm/bgm01.ogg bytes=907689
zlib entries verified: 24
raw entries verified: 64 (image magic ok: 64)

$ cargo clippy --workspace --all-targets
（零告警）
```

- 追加(同日,用户要求对照开源实现补全算法):
  - **名字 XOR key 探测**: 官方 0.495 实测 key=`0xFF`(逐字节取反);
    参考实现给出逐游戏 key 的推导 `enc[nameLen-4] ^ '.'`,本实现以
    nameHash 命中数在 `{0xFF, 推导值, 0..=255}` 中择优(确定性: 命中数优先,
    平局优先 0xFF、其次更小 key)。官方包 1,407/1,407 命中。
  - **按版本切换长度交换表**: `version < 0x100` 用 10 对表、
    `0x12C..0x196` 用 8 对表、其余 12 对表(前两张表只有开源参考实现,
    使用时挂 `vfs.ypf.unsupported` 警告 → DIV-004)。
  - **meta 长度按版本**: `0x12 + extra`(extra=4 for >=0x1D9 / 8 for ==0xDE / 0);
    495 实测 22 字节,读取第 22–26 字节作为 `payloadHash`。
  - 实测输出更新: `index: start=0x20 key=0xff meta=22 ... nameHash=1407/1407`。
- 档案: `DIV-001` → **fixed**(记录旧/新布局对照与回归输出);
  `DIV-002` 保持 open 并注明"本次未触达"(NUL-XOR 无真样本);
  `DIV-003` 改为 partial(检测器已落地,码表待 B);新增 `DIV-004`
  (名字 key / 版本交换表 / meta 长度: 算法已实装,非 495 分支缺真样本)。
- 纪律: 零 git 写操作;未使用 <game-b> 语料;实现内无 `println!/eprintln!/dbg!`;
  测试仅从环境变量取样本路径。

## 成果 13 (2026-10-19) · 算法采用策略 + B/C v2 启动提示词

- 策略变更(用户决定): **算法/常量/流程可直接采用**;采用处文件头注明
  `参考 <项目>/<文件>`,并在 `docs/references/open-source-tools.md` §4 致谢链登记
  (README 致谢由仓库主人汇总)。整段复用代码前确认许可状态。
- 致谢链(照开源生态的"代际+职责"结构): YURIS_TOOLS=新版引擎解密、RxYuris=旧版引擎解密/
  YSCOM Loader/YSCM·YSTB 解析、GARbro=YPF 解密、Script-Editor=YSTB V2/V5 与编码自检测、
  yuri=反编译参照,其余按实际采用列。
- `task-b-format.md` / `task-c-runtime.md` 均升 v2: 新增可采用的算法来源表(文件级),
  并重申真值层级不变(B: 官方产物; C: 手册/真实脚本/官方引擎三层,缺一 Unimplemented)。
- 仅文档改动;两个 v2 启动提示词已发给用户。

## 成果 14 (2026-10-19) · 勘误:v2 提示词未点名旧线仓库可读路径

- 发现: B/C v2 提示词与任务书只在表格里写占位符 `<repo-old>`,未告知"本机可直接读
  `<repo-old>`",导致会话可能找不到参考实现。
- 更正: `task-b-format.md`、`task-c-runtime.md` 的参考索引上方新增说明:
  旧线仓库根 = `<repo-old>`(真名见 AGENTS.md),
  **只读参考,禁止修改其工作区/禁止在其中执行 git 操作**;算法采用需注来源并登记致谢。
- v2 启动提示词同步补上该路径与各会话应读的文件清单。
- 仅文档改动。

## 成果 C2 (2026-10-19) · yu-rt 运行核(M2):表装载 / 表达式 / 任务调度 / 按命令频度实现

> 会话 C 独占 `crates/yu-rt`;未改根 `Cargo.toml` 与其它 crate;零 git 写操作。
> 语料 = 官方 YSCom 0.495 编译的 ERIS 产物(136 个 `ystNNNNN.ybn` + 7 张表);
> 另加**商业发行版**(<game-b> v494,`data.ypf`)作为第二套真实语料。

### 内容

- **表装载(A5 零硬编码)**:`YSCM`(命令名/参数表)、`YSLB`(1941 标签,含
  `步长=名长+13`、`hash=MurmurHash2(seed 0)`)、`YSVR`(2568 条变量镜像)、
  `YSTL`(141 条清单)、`YSCF`(分辨率/扩展名优先序)、`YSTB`(四段各自异或,
  密钥 `CRC32("Yu-ris")`)、`YSCD`(编译器定义:命令参数属性 + 548 项系统变量)。
  缺必需表 ⇒ `table.missing` **拒绝启动**。
- **表达式**:`[op][len u16][payload]` 记录流 + 27 项已取证操作码(算术/比较/
  位运算/字符串拼接/数组取值/两个显式转换);左值形态(标量 / 引用+下标+aload)。
- **变量域**:INT/FLT/STR;行主序数组;写时改类型标签、读时按口味转换;
  `LET` 复合赋值码 0..8(语料实测)。
- **任务调度**:单一扁平链、链长入口快照、每逻辑帧每任务一步、FRAME 递减、
  TIME 绝对期限、TASK 续帧、泵到让出无预算(算法参考旧线 `task.rs`,语义按
  手册 + 引擎 §3.2b/§3.13 逐条核对)。
- **命令处理器(按频度)**:GOSUB/RETURN/GO/IF/ELSE/IFBLEND/IFEND/IFBREAK/
  IFCONTINUE/LOOP/LOOPEND/LOOPBREAK/LOOPCONTINUE/WAIT/FPS/END/LET/INT/FLT/STR/
  S_INT/S_FLT/S_STR/VARINFO(含 SEARCH)/VARACT(COPY/CUT/TYPE/UPPER/LOWER/
  DIMSIZE/PUSH/POP)/MATH(除 RND)/TASK/TASKEND/TASKINFO/TEXT/LABELINFO。
- **输入**:带符号帧计数器(手册三个例子逐条实现)+ 输入时间线注入 + 系统变量绑定
  **全部按名字**从表解析(`_KEY_A`=167 等 id 不硬编码)。
- **事件流**:确定性 JSONL(`frame/sub/kind/payload`),时钟与输入全注入。
- **商业发行版支持(用户指出)**:`YSCom.ycd` 改为**可选**;无 ycd 时用
  `data/sysvar-abi-0495.tsv`(由 `scripts/gen_sysvar_abi.py` 从官方 ycd 生成,
  带来源 SHA-256),并用**工程自己的 `ysv.ybn` 逐项校验**(548/548 才算通过)。

### 实测确证(原始输出)

```text
$ YURIS_SAMPLE_ERIS_YSBIN=<project-eris/data/ysbin> \
  YURIS_SAMPLE_GAME_YPF=<<game-b>/data.ypf> cargo test -p yu-rt -- --nocapture
running 31 tests   (lib 单测)                      ... ok. 31 passed
running 5 tests    (corpus.rs: 真实语料)
LABELS: 1941 条全部闭合;无标签引用的脚本 = [28, 59, 63]
SCRIPTS: files=136 groups=36974 windows=80102 record_failures=0
BOOT: entry SYSTEM_START -> s73 pc0 (2 组), 源 data\script\system_start.yst
COMMERCIAL: ysc v494 (119 命令) labels=2132 vars=2568
  sysvars<-engine ABI profile sysvar-abi-0495.tsv
  (ycd sha256 a626e40a8b25290dcca14c13aa3314c1fd123514b19a0ce9badd8559c7488d1d)
COMMERCIAL: SYSTEM_START -> s73 pc0 (2 组)
COMMERCIAL: frame1 commands=2438 errors=[] unimplemented=5
TABLES: yscm=119 params=1122 labels=1941 vars=2568 scripts=141 ycd_sysvars=548
                                                    ... ok. 5 passed
running 4 tests    (corpus_golden.rs: 真实脚本事件流 golden)  ... ok. 4 passed
running 20 tests   (fixture.rs: 控制流/变量/输入/错误路径)     ... ok. 20 passed
running 3 tests    (golden.rs: 事件流/错误路径 golden)         ... ok. 3 passed

$ python crates/yu-rt/scripts/audit_hardcode.py
audit: files=15 violations=0
```

关键实测数字(全部由测试钉住,语料变动即测试失败):

- ERIS 0.495:`ysc=119 命令/1122 参数槽`、`ysl=1941 标签(hash 全命中)`、
  `ysv=2568 记录(前 548 条 = 系统变量镜像)`、`yst_list=141 条`、
  `136 个脚本/36,974 组/80,102 窗口`。
- 命令频度(前三):`GOSUB 10215`、`LET 8136`、`IF 3326`;`IFEND 3326`、
  `RETURN 1750`、`INT 1428`、`IFBLEND/ELSE 887`、`LOOP 522`、`WAIT 95`。
- 系统变量 id(官方 0.495 ycd,548 项):`_TIME=0`、`_PAI=10`、`_LC=48`、
  `_PINT=53`、`_PFLT=54`、`_PSTR=55`、`_PFLG=56`、`_RINT=60`、
  `_SCREEN_SIZE_X=115`、`_MOUSE_X=133`、`_INP=144`、`_KEY_A=167`、`_KEY_ANY=275`;
  语料频度与之吻合(`_LC 18521`、`_PINT 12380`、`_RINT 7302` 等)。
- 商业 v494(无 ycd):ysv 前 548 条 kind=1 与 0.495 ycd 的 (type, dims)
  **548/548 逐项一致,0 差异** ⇒ 系统变量 id 空间是引擎 ABI。

### 三层出处对照表

见 `crates/yu-rt/docs/command-provenance.md`(逐命令:手册页 / 语料频度 /
引擎地址级证据 / 实现状态)。

### 未决(全部挂 `crates/yu-rt/docs/divergences/README.md`)

DIV-RT-001..020;本次新增并已修的三条:

- **DIV-RT-016**(fixed):商业发行版不带 `YSCom.ycd` → ycd 可选 + 引擎 ABI 表 +
  `ysv.ybn` 自证校验。
- **DIV-RT-018**(fixed):`ELSE` 的 **0 窗口**形态(商业包 726 次)⇒ 无条件进
  else 体;ERIS 的 3 窗口形态不变。
- **DIV-RT-019**(fixed):`VARACT[DIMSIZE/PUSH/POP]` **不需要 LET**(手册例 9/10)。

仍 open 的重点:`DIV-RT-002`(类型提升/溢出/负下标/越界访问的精确行为)、
`DIV-RT-005`(官方脚本错误继续策略)、`DIV-RT-006`(输入键码表)、
`DIV-RT-007`(手册无页系统变量)、`DIV-RT-011`(手册有页但语义未取证:
`RETURNCODE`/`F_*`/`WORD`/`VARINFO SEARCH 部分`/`VARACT INIT·G_*`)、
`DIV-RT-017`(变量空间前缀 `0x60`/`0x23`)。

### 说明

- 本会话未改动 `docs/architecture.md` 与其它 crate;`crates/yu-format` 由会话 B
  并发创建,期间整仓 `cargo test` 一度因该 crate 缺 `Cargo.toml` 不可用;
  上述结果为**沙箱工作区**(仅 log/vfs/rt)运行所得,代码与仓库内 `crates/yu-rt`
  逐字节一致(每次跑测前同步复制)。

### 补记(整仓构建恢复后)

`crates/yu-format` 落地后整仓构建恢复,以上结果在**仓库内**复跑一致:

```text
$ YURIS_SAMPLE_ERIS_YSBIN=... YURIS_SAMPLE_GAME_YPF=... cargo test -p yu-rt
yu-rt lib:            31 passed
corpus.rs:             5 passed   (含商业包 2438 命令/0 Fatal)
corpus_golden.rs:      4 passed
fixture.rs:           20 passed
golden.rs:             3 passed
$ python crates/yu-rt/scripts/audit_hardcode.py
audit: files=15 violations=0
```

`cargo clippy -p yu-rt --all-targets` 已 `--fix` 一轮;剩余为提示级
(类型别名/切片复制风格),不影响语义。

## 成果 15 (2026-10-19) · 任务书 D:`yu-trace`(旁挂对拍基建)

- 新增 `docs/tasks/task-d-trace.md`(v1):范围锁定边界①(vfs 读)与②(事件流);
  ③帧描述、④平台命令只留接口等 M3;明确**不做**官方 WOW64 采集器(独立工具任务)。
- 并行结论: D 可立即与 A/B/C 并行。边界①随 A 已解锁;边界②只消费 `yu-rt` 公开 API
  (`VmEvent`/`Vm::take_events`/`table_provenance`/`BootConfig`),C 改 API 时 D 适配,
  D 不改其它 crate,也不被任何 crate 依赖;B 与 D 无依赖关系。
- 冻结契约: 记录封套 `{schema,seq,frame,sub,kind,payload}`,header 也是 `seq=0` 的记录;
  `VmEvent` 穷尽匹配(新增变体会让 D 编译失败 = 强制 schema 评审);
  diff 严格逐行、首分歧定位、无容差;trace 语料不入库,仅合成 fixture 入库。
- 验收: roundtrip 字节一致 / `TracingSource` 等价性 / 官方 `data.ypf` 冒烟(env gate)/
  事件流 diff 闭环 / 零 `println!` / 改动仅限 `crates/yu-trace`。
- 仅文档改动;未执行 git 操作。

## 成果 D1(2026-10-19)· yu-trace(旁挂对拍基建)

**范围**:边界①(vfs 读)与边界②(事件流);③帧描述、④平台命令只留枚举占位,
schema 1 显式拒绝其记录(M3 再定义)。未实现官方 WOW64 采集器(独立工具任务)。

**新增文件**(全部在 `crates/yu-trace/`):
- `Cargo.toml` — 依赖仅 `yu-log` / `yu-vfs` / `yu-rt`;无第三方依赖。
- `SCHEMA.md` — 记录协议唯一定义(schema 1):封套、全部 kind/payload、字节/浮点编码、
  输入摘要算法、diff 语义、错误码、与 `docs/oracle.md` 草案的差异、版本历史。
- `src/lib.rs` — crate 文档、公开导出、`trace_dir()` / `write_trace()`。
- `src/record.rs` — `Record` / `TraceLog` / `Payload` / `Kind` / `Sub`;稳定 JSONL
  读写、严格字段游标、`seq` 连续性与 header 位置校验、`prepend_header`。
- `src/json.rs` — 本 crate 自有确定性 JSON(键序固定、`\u00xx` 字节转义、最短往返浮点)。
- `src/hash.rs` — 本地 SHA-256(零新依赖;空/abc/双块测试向量)。
- `src/source.rs` — `TracingSource<D: DataSource>` + `BoxedSource` + 帧号句柄。
- `src/event.rs` — `EventRecorder`;`VmEvent` **穷尽 match**(`JumpKind`/`Domain`
  亦由本 crate 映射,无 `_ =>` 兜底);输入时间线摘要。
- `src/diff.rs` — 逐物理行字节比较 + 首分歧 JSON Pointer;独立错误码。
- `src/replay.rs` — `VmTrace` 在线回环(只用公开 API `boot/boot_with_aux/tick_at/take_events`)。
- `tests/{roundtrip,equivalence,event_trace,diff,golden,real_sample}.rs` +
  `tests/support/mod.rs`(只读复用 `yu-rt/tests/support`,不复制)。
- `tests/golden/synthetic_frame.jsonl` — 3,690 B 合成 fixture(<32KB)。

**关键结论/决策**:
- 记录封套为 `{schema,seq,frame,sub,kind,payload}`;header 是 `seq=0` 的
  `meta/header` 记录。启动链 vfs 读取先入流,收尾用 `prepend_header` 补头并重编号。
- `frame=null` 允许 `vfs`(帧外读取),`vm` 子系统必须带帧号(schema 强制)。
- diff **不做任何归一化**:先协议校验,再逐行比原始字节;空白/键序变化也算分歧,
  定位不到单键时 `path=""`(整条记录)。少行 → `trace.truncated`;非法 JSON → `trace.parse`;
  未知 schema/kind/字段/多余字段 → `trace.schema`。
- 在线回放**未阻塞**:`VmTrace::tick_at` 已可用;已知限制 = 一次 tick 可能含多个逻辑帧,
  其内的 vfs 读取标注为该 tick 首个逻辑帧(需 `yu-rt` 帧钩子才能更精确,D 不改 yu-rt)。
- 当前 `yu-rt` 只发射 `FrameStart`(无 `FrameEnd`)、`TEXT` 走 `SystemCall`、
  `_` 走 `Unimplemented` Diag;13 个 `VmEvent` 变体的映射覆盖由单元测试守护,
  事件流集成测试只断言运行核实际发射的事件。

**验收原始输出**:

```text
$ cargo test -p yu-trace          # YURIS_SAMPLE_ERIS_YPF=<official data.ypf>
yu-trace lib:          26 passed
diff.rs:               13 passed
equivalence.rs:         6 passed
event_trace.rs:         8 passed
golden.rs:              2 passed
real_sample.rs:         2 passed   (官方 data.ypf 冒烟:字节/SHA-256 一致、3 帧回放 diff 0)
roundtrip.rs:           7 passed
Doc-tests:              0 passed
```

```text
$ cargo test -p yu-trace          # 未设置 YURIS_SAMPLE_ERIS_YPF
real_sample.rs: 2 passed — official_data_ypf_smoke 走 skip 分支:
  产 trace.sample.missing Info 诊断、SKIPPED 计数 = 1、断言后返回(非静默通过)
```

```text
$ cargo clippy -p yu-trace --all-targets
    Finished `dev` profile ... (yu-trace 0 告警;yu-rt 的 4 条为既有告警,未改动)
```

```text
$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-trace   → 0 命中
$ grep -rn "SystemTime\|Instant" crates/yu-trace         → 0 命中
$ grep -rn "yu-trace" --include=Cargo.toml .(排除自身)   → 0 命中(未被任何 crate 依赖)
```

**未做/阻塞**:
- ③ `frame_description`、④ `platform_call`:仅枚举占位,解析/写出均拒绝(M3)。
- 官方采集器(WOW64/Python):不做;schema 已按同一格式落定。
- 在线逐帧 vfs 归属:受 `yu-rt` 无帧钩子限制,当前为 tick 级标注。

仅改动 `crates/yu-trace/**` 与本次 `PROGRESS.md` 追加;未执行任何 git 写操作。

## 成果 B2 (2026-10-19) · yu-format:ybn 家族解码 + 码表 + 契约(任务书 B v2)

**交付**:`crates/yu-format`(仅依赖 `yu-log`;零第三方依赖;实现内无
`println!/eprintln!/dbg!`)。

- **容器/密钥**:`key.rs`(CRC-32 反射 0xEDB88320 + 分段异或,对合运算);
  `container.rs`(16 字节头;YSLB/YSTL 头实测仅 12 字节,见勘误)。
- **七张元数据表**:YSCM(命令表,`name_of/index_of`)、YSLB(全局标签表)、
  YSVR(变量声明)、YSTD(槽位表头)、YSTL(脚本清单)、YSCF(引擎配置,
  含引擎侧宽高/开关/标题校验)、YSER(id↔名字)。
- **脚本体**:YSTB V5 四区段解密(`Segments::decrypt/encrypt`,异或对合)
  → 契约类型 `DecodedScript{script_id, groups, labels}`(`Group{command,
  windows}`、`Window{[u8;12]}` 冻结接口未动);V2 → `Unimplemented`(DIV-007)。
- **编解码器**:CP932(9,604 双字节条目 + 398 别名)与 GBK(23,940 条,
  零别名)静态表,由 `gen_tables.py` 从 **Windows kernel32 码页 932/936**
  生成(`MultiByteToWideChar/WideCharToMultiByte`,即引擎平台系统映射;
  Python 自带 cp932 的 PUA 扩表被弃用);严格 decode/encode + auto 评分器
  (架构 §7:严格双解码 + 构成统计,固定次序,平局缺省 CP932)。
- **fixtures**:三个最小官方产物原样切片(yst.ybn 16B / ystb_00073.ybn 72B /
  yscfg.ybn 90B)+ 钉死清单 `pinned_unresolved.rs`;所有解析器过"逐字节
  截断安全"测试。

**实测确证项**(全部跑在官方 ERIS 产物上,非笔记转抄):

1. **opcode = ysc 记录下标(字母序)**:system_start=`GO[#=es.ERIS]`+`END[]`
   ↔ op43/op14;es_First 首指令 `TEXT[OUTPUTMODE=0]`↔op96、
   `FILEACT[CREATE=1 FOLDER=$u]`↔op21、S_STR/S_INT 声明↔op82/81。
   注意:**命令只在源码写出参数时才发射参数记录**,ysc argc 不约束指令
   参数个数(控制流命令在表中 argc=0 是占位)。
2. **yst_list 尾字段 = 时间戳×2 + 变量/标签/文本计数**(RxYuris 解释),
   旁线"脚本文件号"被否定:编号 = 记录 index(0..140);141 条中 5 条
   纯声明文件(varCount/labelCount=0xFFFFFFFF 哨兵)恰对应磁盘缺号
   {26,27,69,74,108};labelCount 与 ysl 逐条相等(136/136)。→ DIV-005。
3. **标签闭环**:10410 个 GO/GOSUB 静态引用中 10174 个在 ysl 解析,
   137 个动态目标(`H`/`V` 载荷),99 处(76 组/69 标签)引用 ysl 缺失的
   标签 —— 全部为 ERIS 样板的 `ES.*.VoicePlay.N` 音声钩子与更新钩子;
   YSCom 不校验目标,引擎靠 ERROR[] 错误等级容忍(启动实测到标题)。
   → DIV-006,清单钉死于 fixtures。
4. **ysv 字节级闭合**:2568/2568 条;`ty=0`=仅声明无初值段(452 条);
   kind 1/2/3 = 全局 1167/局部 928/静态 473;槽位全部 < yst 的
   varCount+1(4601);G_* 恢复组号实测 0..=3(未见 4)。
5. **三方互证**:ysc 119 条 ⊇ 手册 71 条;ycd 的 105 个 `COM_*` 中
   `PRODUCT` 不在 ysc(编译器/引擎表版本差)→ DIV-008;ERROR 命令参数表
   = 105 个可设错误级命令(NO/SET + COM_*),不能当命令全集用。
6. **ysl 字段佐证**:field_b 对全部 1941 个标签都落在对应脚本指令数内
   ("脚本内指令位置"假设成立);fieldC=脚本号(旁线实证一致);
   0x400 索引块实测为递增 u32 序列(语义待证,原样保留)。
7. **ysc 尾部 256 字节表 = 键位表**(A–Z=65..89、0–9=48..57、F1–12=112..123、
   方向键/小键盘/修饰键全序列),不是 opcode 映射;旁线"顺序即 opcode"
   的结论以 ysc 记录序的形式被证实。

**勘误留痕**:

- `sdk-eris-analysis.md` §8 密钥值表述:`CRC32("Yu-ris")` 标准值 =
  **0xD36FAC96**;引擎把 CRC **按大端**写进内存,按小端读回的 u32 =
  0x96AC6FD3,小端字节序列 = `D3 6F AC 96`(即异或实际使用的 4 字节)。
  两种值描述同指一字节序列;`key.rs` 单测钉死三向关系。
- 旁线 ysl "+0x0C byte[0x400]"、yst_list "16 字节未知前缀"实为
  12 字节头(魔数+版本+条数),块/记录起点在 `+0x0C`(本档 DIV-005)。

**未决/待验**(进 divergences 或代码内留痕):

- YSTB V2 无语料不实现(DIV-007,`Unimplemented` 显式)。
- ysl 0x400 块、field_a/field_d/field_e 语义;YSTB 头 +0x1C 字段;
  指令 anc0/anc1(观测 0..9/0);行号区(=count×4,已留 `Segments`)。
- `data.ypf` 交叉验证未做:ypf 布局归 yu-vfs(任务书边界),且本 crate
  零依赖无 zlib;如需可在 yu-vfs 侧做"包内 yst == 磁盘 yst"比对。

**验证命令与原始输出**(2026-10-19):

```
$ cargo test -p yu-format
running 6 tests   (codec 全表往返/单字节往返 ×2 码表 + key)
test result: ok. 6 passed; 0 failed
running 1 test    (doctest)
test result: ok. 1 passed
running 16 tests  (合成字节 + fixtures:七表/YSTB/契约/锚点/auto)
test result: ok. 16 passed; 0 failed

$ export YURIS_SAMPLE_ERIS_YSBIN=…/project-eris/data/ysbin
$ export YURIS_SAMPLE_SDK0495=…/yu-ris_0495_032
$ cargo test -p yu-format --test corpus_eris -- --nocapture
ysc − COM_* (15 条):{");", "CDDA", "COMPILEMODE", "EMOTE", "EMOTEINFO", "F_FLT", "F_INT", "F_STR", "F_VAR", "MATH3D", "MIDIEND", "MIDIINFO", "PROJECTFOLDER", "RELEASEMODE", "SYSTEMMODE"}
手册文档化命令:71
YSTB: files=136 insts=36974 windows=80102 GO/GOSUB=10410 resolved=10174 dynamic=137 unresolved=99 (unique 76)
未解析标签 76 组全部为 ERIS 样板的音声/更新钩子(根因见 docs/divergences/DIV-006)
auto: cp932=4.0 gbk=4.0
ysl field_b < 指令数: 1941/1941(field_b = 脚本内位置,假设成立)
corpus_acceptance: 全部通过(ysc=119, ysl=1941, ysv=2568, yst_list=141, yst 文件=136)
test result: ok. 1 passed; 0 failed
(auto 平分 4.0 = 采样池为标签名等 ASCII 串,双解码全合法 → 按架构 §7 缺省 CP932)

$ cargo clippy -p yu-format --all-targets
    Finished `dev` profile …(零告警)

$ 环境变量缺失时(显式 skip 并计数,不许空跑通过)
SKIP corpus_acceptance: 缺少 YURIS_SAMPLE_* 环境变量
test result: ok. 1 passed

锚点抽测(全量在测试内):CP932 "日"=93FA/"ｱ"=B1/"ソ"=835C(0x5C 后随)/
"メモリ不足です。"(yse.ybn 首条逐字节);GBK "你"=C4E3/"国"=B9FA/"€"=80。
全表往返:GBK 零别名双向全等;CP932 的 398 条别名 decode∘encode 恒等、
encode 取 Windows 系统首选(生成器导出的 ALIAS_OVERRIDES 覆盖表)。
```

**与开源参考实现的交叉验证**(把 Script-Editor 原版 Python 代码直接跑在本语料上):

```
① core/ystb.YSTBFile.from_bytes(data, key=0x96AC6FD3) 逐文件解码 136 个官方 YSTB:
   insts=36974 windows=80102 —— 与 yu-format 钉死总数逐项一致;
   且其 to_bytes(KEY) == 原文件(异或对合 + 段布局的第三方佐证)。
② core/ystb.YSTBFile.guess_key_from_bytes(yst00000) = 0x96ac6fd3
   —— 他们的密钥猜测启发式独立恢复出同一密钥(第三方佐证)。
③ core/encoding.detect_text_encoding:
   官方日文文本池(76 条/2,919 字节)→ shift_jis(= yu-format auto → cp932);
   GBK 中文样本 → gbk(与 yu-format auto 同判)。
   注:评分公式未采用他们的(本仓库重造为确定性裁决),交叉验证是
   "结论级"而非"逐分值级"——这是 auto 部件当前的验证深度。
```

RxYuris 为 C++,未编译执行;其 YSTL/YSCM 布局解释已在本语料上逐字节
吻合(等于事实性交叉验证),密钥处理与 `YSTB_Coder::Xor` 分段语义与
Script-Editor、本实现三方一致。

- 纪律:零 git 写操作;未使用 <game-b> 语料;逐文件头注明参考来源,
  致谢链登记于 `docs/references/open-source-tools.md` §6。

## 成果 D1 补记(2026-10-19)· yu-trace 代码审查修订

审查对象:`crates/yu-trace` 全部源码与测试。发现并修复 7 项,协议升至 **schema 2**
(按任务书"字段增删必须递增"规则;v1 从未被外部消费,但版本历史如实留痕)。

**修复项**:

1. **输入摘要帧窗口错位(确定性缺陷)**:`yu-rt` 的 `tick/tick_at` 首帧先
   `advance(scheduler.frame()+1)`(新建 VM 首帧 = 1),原摘要覆盖 `[0,N)` ⇒ 漏掉
   最后消费帧、多算一帧。改为 `[1,1+N)`;新增 `input_summary_range` 供非新建 VM 回放。
   回归用例:`event_trace::input_summary_window_matches_consumed_frames`。
2. **回放无法闭合(设计缺陷)**:header 原先只记 entry/fps/start_ms/frame_ms,
   而 `BootConfig` 还含 script_key/debug_mode/argv(审查期间 C 又新增
   screen_color/paths/media_readonly/os_sound/dx_mode)。补齐 `boot.script_key`、
   `debug_mode`、`argv{len,sha256}`、`host{screen_color,media_readonly,os_sound,dx_mode,
   paths{len,sha256}}`;路径与命令行只记摘要(禁止绝对路径入库),回放时校验。
   新增 `VmTrace::from_header` + `BootSummary::to_config` + `verify_input(_range)`。
   回归用例:`header_replays_to_a_byte_identical_trace`(录制→from_header 重跑→逐字节相等)。
3. **行数语义错误码合并**:原"少行/多行"都报 `trace.truncated`。按任务书"分别给独立
   错误码"拆为 `trace.length`(两侧各自合法但条数不同)与 `trace.truncated`
   (末行不完整/缺 LF/空文件);`trace.schema` 仍负责 seq 重复等结构错误。
4. **`vfs/list` 失败态未规范化**:协议文档要求失败时 `entries` 为空,解析器原先未强制;
   现在带 `diag` 且 `entries` 非空 ⇒ `trace.schema`。
5. **`TraceLog::push` 可写入 header**:会产生"自己解析不回来"的文件;现在 panic 并
   指向 `push_header`/`prepend_header`(带 `#[should_panic]` 用例)。
6. **`write_trace` 路径校验漏洞**:`"."`/`".."` 可通过;已拒绝并加用例。
7. **jump/domain 字符串双份来源**:生成侧(`event.rs`)与解析校验侧(`record.rs`)各一份,
   存在漂移风险。统一为 `JUMP_KINDS`/`DOMAIN_NAMES` 单表 + 全变体/唯一性用例。

**并行适配**:审查期间 C 扩展了 `BootConfig`(screen_color/paths/media_readonly/
os_sound/dx_mode)。D 按约定适配(不改 yu-rt):`BootSummary::from_config/to_config`
使用**穷尽字段字面量**,C 再增字段会使 D 编译失败 ⇒ 强制一次 header schema 评审。

**复核原始输出**:

```text
$ cargo test -p yu-trace          # YURIS_SAMPLE_ERIS_YPF=<official data.ypf>
lib:            31 passed
diff.rs:        13 passed
equivalence.rs:  6 passed
event_trace.rs: 12 passed   (含 from_header 逐字节回放、输入摘要窗口回归)
golden.rs:       2 passed   (fixture 3,998 B < 32KB,schema 2)
real_sample.rs:  2 passed   (官方 data.ypf;本机 debug 构建约 144s,成本在 yu-rt 启动)
roundtrip.rs:    7 passed
Doc-tests:       0 passed
$ cargo clippy -p yu-trace --all-targets   → yu-trace 0 告警
$ grep println!/eprintln!/dbg!             → 0
$ grep SystemTime/Instant                  → 0
$ 其它 crate 依赖 yu-trace                 → 0
```

`SCHEMA.md` 已同步到 v2(header 新字段、argv/host paths/输入摘要算法、`trace.length`/
`trace.replay` 错误码、diff 行数语义、版本历史)。本次仍只改 `crates/yu-trace/**`
与本文件追加;未执行任何 git 写操作。

### 追加(同日)· 端到端交叉验证与 7 类真缺陷修复

按"每一条都实现、继续对齐到完美 runtime"的要求,用**官方入口链**
(`SYSTEM_START → es.ERIS`)做端到端交叉验证,而不是停在单命令。

**发现的真缺陷(全部有真实语料证据,已修)**:

| # | 缺陷 | 证据 | 后果 | 对应 DIV |
|---|---|---|---|---|
| 1 | `eval_lvalue` 跳过**全部** `0x29`,只有末条才是左值标记 | ERIS s2 pc16 `$arr[@cnt(0)-1]` 真实字节 | 441 次负下标;数组追加全错 | DIV-RT-020 |
| 2 | `VARINFO[LENGTH]` 对数组返回 0 | ERIS 用 `LENGTH` 对数组取元素数 | `DIMSIZE+=1` 后下标算成 -1 | DIV-RT-020 |
| 3 | `DIMSIZE` 忽略复合码(`+=` 当绝对值),且**重填整个数组** | 语料 104/104 都是 `+=`;`VAR.ADD` 宏 | 13 次追加只剩最后 1 个 | DIV-RT-019 |
| 4 | `IF` 先求值后压块帧(引擎是先压帧) | 旁线 §3.13 opcode 45 | 条件求值失败时 `IFEND` 失配 | DIV-RT-027 |
| 5 | `ELSE` 只接受 3 窗口 | 商业包 726 次 0 窗口 | 商业包此处 Fatal | DIV-RT-018 |
| 6 | `VARACT` 把系统变量源当非法;`INIT/G_*` 未实现;缺 SET 检查过早 | ERIS `ystdlib.yst` 29 处 `$_PSTR` 源;s12 `VARACT[INIT G_*]` | stdlib 全废;全局恢复缺失 | DIV-RT-007/024 |
| 7 | `SYSTEMINFO`/`TASKINFO` 归为"域命令"未实现 | ERIS 启动链硬依赖(缺 → 除零) | 帧 1 除零、主线终止 | DIV-RT-023/025 |

**附带**:`@_MOUSE_L=5` 一类输入系统变量写入(手册标 ○,ERIS 注释
"切场景时让输入失效")实现为设定帧计数器(DIV-RT-021);色深/路径/`_DXMODE`/
`_MEDIA_READONLY`/`_OS_SOUND` 收进 `BootConfig` 由宿主注入(DIV-RT-022);
`yst_list` 逐脚本文本数按实测字段解析(DIV-RT-026)。

**跨语料交叉验证(原始输出)**:

```text
$ cargo run -p yu-rt --example run_eris -- <project-eris> 12
frame 0: cmds=1290322 errors=0
frame 1: cmds=329227  errors=0
frame 2: cmds=26      errors=0
frame 3: cmds=249     errors=0
frame 4..11: cmds=8765 errors=0        ← 稳态

$ cargo run -p yu-rt --example run_game -- <<game-b>> 8
  ysc: v494 (119 命令)  labels=2132  vars=2568  yst_list=160  yscfg=1100x825
  ycd: 不存在(商业发行版;系统变量改用引擎 ABI 表 + ysv 校验)
frame 0: cmds=850609 errors=0
frame 1: cmds=307356 errors=0
frame 2: cmds=26     errors=0
frame 3: cmds=248    errors=0
frame 4..7: cmds=8681 errors=0        ← 稳态

$ cargo test -p yu-rt
lib 31 + corpus 8 + corpus_golden 4 + fixture 21 + golden 3 = 67 passed; 0 failed
$ python crates/yu-rt/scripts/audit_hardcode.py
audit: files=15 violations=0
```

**A1 执行层等价性**(新增永久回归测试):

```text
loose  = ([1290322, 329227, 26, 249, 8765, 8765], 0, "cd1790…4bd2e")
packed = ([1290322, 329227, 26, 249, 8765, 8765], 0, "cd1790…4bd2e")
```

散盘 `data/` 与官方 `data.ypf` 逐帧命令数、错误数、**变量域终值 SHA-256** 完全相同。
另核对:两处 ysbin 的 143 个 `.ybn` 逐文件 SHA-256 **143/143 一致**(diff=0)。

**剩余未实现(均为域命令或手册缺页,已在事件流逐条挂号)**:
`CG/CGACT/CGEND/CGINFO`(图形)、`SOUND/SOUNDINFO`、`WINDOW/WINDOWINFO`、
`FONT/FONTINFO`、`FILEINFO`、`EMOTE`、`RETURNCODE`(无手册页)、`WORD`(无页)、
`SYSTEM`/`INPUT`/`MOUSE`(域)。ERIS 稳态帧里 `CG` 出现 3 万+ 次 —— 下一步
接 `yu-gfx` 时按事件流逐条实现。

## 成果 16 (2026-10-19) · 四线代码审查(A/B/C/D)与 REA 复核

- 范围:`yu-log`/`yu-vfs`(A)、`yu-format`(B)、`yu-rt`(C)、`yu-trace`(D) 全量源码 + 测试;
  证据要求:官方手册页、官方产物字节、反编译/REA、代码行四类分级;纯代码自证不作为语义证据。
- REA 直接复核(官方 0.495 `yu-ris.exe`,sha256 `4f5eefa3…`):
  - `FUN_00462a10`:恒等表 + **恰好 12 对交换**,无版本分支;同函数生成 CRC32("Yu-ris") 四字节。
  - `FUN_0045e3b8`:每个包索引起点恒为 `0x20`,步长 `nameLen+0x1B`,
    `T[0xFF-lenEnc]`、名字逐字节取反,循环内**无版本分支**。
  - `FUN_00404398`:任务泵按等待计数器(+0x18)与 `timeGetTime` 期限(+0x1c)门控;
    处理器表返回非 0 = 让出;`FUN_004046dc` 是按索引取链表节点,不是优先级排序本身。
  - 勘误:旧笔记中的 TASK 处理器地址 `0x4506f4/0x4521dc/0x4567c0` 在 0.495 官方 exe 中
    不是函数入口(REA 落在图形/变量恢复函数),不能作为 0.495 的 TASK 行为证据。
- 结构性问题:`yu-format` 当前**零依赖者**;`yu-rt` 自己重实现了 YSCM/YSLB/YSVR/YSTL/YSCF/YSTB
  解析(`crates/yu-rt/src/tables.rs`),形成两份真相。B 的格式缺陷暂不影响运行核,但两边会漂移。
- 主要确认缺陷(详见审查报告):
  - rt 编排与手册冲突:主任务 ID 应为 `SYSTEM`(代码 `@main`);`TASK/TASKEND` 无 ID 应作用于自身
    (代码落到 `@main`);`TASKEND` 不停止当前泵/不移除任务;显示文本 `_` 不让出;Z 优先级未参与调度;
    ERIS 的 10 处 TASK 全部因 TINT/SCRIPTPOS/RESTART 未实现而未生成。
  - rt 可触发 panic:`IFBREAK[LV=0]`、`GOSUB` 参数名 `PINT0/PINT17+`、`VARACT[DIMSIZE]` 溢出、
    数组维长乘积溢出/巨型分配。
  - vfs:未验证的 NUL-XOR 回退会把结构不匹配的包读成 `Ok` + 垃圾名字;
    版本化 8/10 对表与 18/26 字节 meta 与 REA 的“单一 12 对表”冲突;36 字节输入可触发放大分配 208MB。
  - format:YSTB 头 count 被忽略、回封丢弃尾部;YSCF 宽高按 u16(引擎为 u32);
    TASK 非首位 `#` 标签漏收;32 位目标存在回绕 panic。
  - trace:header 含内核版本/prose 且被严格 diff;无按子系统投影;`tick_at` 时钟与 codec 未回放闭合;
    `hex4` 对坏 `\u` 会 panic;缺样本静默 skip。
- 日志:`Diag` 结构化程度好,但脚本错误无 task/script/pc 定位(`diag::at_script` 从未调用)、
  多处 `let _ =`/类型回退吞错、`ChainReport.errors` 不含可恢复 Unimplemented,导致 E2E 测试“零 Fatal”假绿。
- 本次仅审查与文档记录;未修改任何源码,未执行 git 操作。

## 成果 17 (2026-10-19) · 修复会话任务书 E

- 新增 `docs/tasks/task-e-fixes.md`:把成果 16 的审查结论转成可执行修复清单
  (F=yu-format / V=yu-vfs / R=yu-rt / T=yu-trace / P2=性能),每条带证据与验收。
- P0 重点:format 的 TASK 非首位 `#` 标签、YSTB 头 count/尾部、YSCF u32;
  vfs 的 NUL-XOR 回滚、版本表对齐 REA、64 位 offset、hash 校验、条目上限;
  rt 的 SYSTEM 主任务 ID、TASK/TASKEND 自身目标与立即停止、`_` 让出、
  Z 优先级、ERIS TASK 阻塞项、panic 收口、Diag 定位、解析器统一。
- 并发守则:启动先检查 10 分钟内被改文件;C 仍活跃时禁改 yu-rt/yu-vfs/yu-log,
  先做 format/trace,再复查;blocked 项只报告不抢写。
- 仅文档改动;未执行 git 操作。

## 成果 C3 (2026-10-19) · yu-rt 第三轮对齐:文本流 / 按钮码 REA / 手册字节级重审 / 热路径 / panic 收口

> 承接任务书 C v2 与用户指示「抓游戏事件、ERIS 有源码、宏要能正常执行、
> 手册没覆盖的细节都要考虑到、和官方保持一致」。本轮全部改动在
> `crates/yu-rt/` 与文档;零 git 写操作。

### 1. 源码 ↔ 编译产物 ↔ 手册/社区 三方对齐(新实现)

- **文本流三件套**(DIV-RT-028):一行剧本文本被 YSCom 编译为
  `WORD(段)→[_($_TEXT(239)+$_TEXT(240))]→WORD(段)→RETURNCODE(n)`。
  `RETURNCODE` 双表无参,唯一窗口 `kind` = **每脚本文本行计数**(137 文件
  396 处全量 1..N 递增,回归钉死);`_` = 手册 `basic/basic.html`
  ●テキスト表示的 `_[式]`(撤销 DIV-RT-008 旧挂账);`WORD` 载荷 =
  CP932 原始字节(RxYuris `YSTB_TextEditor_V5.cpp` 同法互证,
  YSTL `text_count` == WORD 数)。新增 `VmEvent::ReturnCode`。
- **按钮码 `_CODE_*`(DIV-RT-029,REA)**:0.495 引擎 `FUN_00427350`
  证得——`_INP(n)` 与 `_KEY_*`/`_MOUSE_*` 共用一张 VK 计数表
  (`0x685800`,guard `n<=0→0`);`_CODE_*` = 运行期 BSS 字节表(静态不可读),
  由手册不变量 `@_INP(@_CODE_Z)≡@_KEY_Z` ⇒ VK 值 + JP106 OEM 映射;
  手柄纯算术 `384+n*36(+4+m%32)`。落成数据文件
  `data/button-code-abi.tsv`(113 基名;L1..3 同基码);手册 ○ 写 =
  键位重映射(`InputState::set_code`)。
- **TASK[TINT..10/TFLT..10/TSTR..10]**(DIV-RT-015 部分):两版手册均无页
  (旧注误引已勘误),语义取自官方 ERIS 源码生产者/消费者对
  (`es_ERIS.yst:524 TASK[TINT=…] → :533 @_TINT(1)`;stdlib `es._task` 同型),
  寄存器随新生 ExecState 落位。
- **TASKINFO[SCRIPTPOS] ↔ TASK[SCRIPTPOS]**(手册 taskinfo/task.html 有页):
  不透明位置 blob(8 字节 script_id+pc)往返;TASK 无 `#` 形态 = 位置装载。
- **状态系统变量**(DIV-RT-031):`$_TEXT`(WORD/_ 追加,写替换)、
  `○@_MENUCODE`、`○@_WINDOW_EXEC_MAX/CLOSE`(内核状态 + 宿主 setter)。
- **域归类修正**:`FONT/FONTINFO` → Graphics 域(手册 `font/` 有页),
  不再报「命令无处理器」。

### 2. 手册字节级重审(DIV-RT-030)

SDK 手册 html 含非文本字节,普通 grep 按 binary 跳过 → 曾把
`_WINDOW_EXEC_CLOSE`/`_MENUCODE` 等误判「无手册页」。全 548 名按字节级
重审:族谱(`_KEY_〜`/`_CODE_〜`/`_PAD_〜`/`_SERIAL_〜` 等)确认有页;
**63 项非族谱名确认真无页**(`_OS_MEMUSE`/`_MEDIA_TYPE`/`_DROPFILE`/
`_VARTYPE`/`_CHR`/`_SYSTEM_FRAME` 族/`_CGSET` 等,继续挂账)。错误引用
全部勘误:`_TINT`(曾误引 svariable)、`_KEYLAST/_MOUSELAST`(曾误引手册,
现改挂账 DIV-RT-033,InputState 内部照常记录)、`$_PROJECT_PATH`
(手册无页,按路径族宿主注入处理)。

### 3. panic 收口(任务书 E R7,DIV-RT-034)

四条可硬崩路径全部转 Fatal 诊断:①`IFBREAK[LV=0]`(越界 panic → no-op);
②GOSUB/RETURN 参数名 `PINT0/PINT17+`(回绕/OOB → 范围检查);
③`VARACT[DIMSIZE=<巨型>]`(分配 abort → 2^26 防御上限);
④数组维长乘积(回绕 → checked_mul+上限)。三条「修复前 panic / 修复后
Err」回归入 fixture(复现:`cargo test -p yu-rt --test fixture panic`)。

### 4. 热路径与可观测性税

- 启动期按工程 yscm 名字解析 **opcode→处理器表**(`HandlerKind`,
  替代每命令字符串 match);`BootConfig::record_events`(默认开,关闭时
  `Diag`/挂号照常);`Cmd`/表达式栈/解码缓冲三重复用。
- 基准(ERIS 4 帧 292.9 万条):300k → **582k cmds/s**(events 关);
  events 开 316k(事件流为可观测性税,可关)。

### 5. 验证(原始输出)

```
$ cargo test -p yu-rt --release        # 73 项全绿
  lib 31 ok / corpus 9 ok / corpus_golden 4 ok / fixture 26 ok / golden 3 ok
$ python crates/yu-rt/scripts/audit_hardcode.py
  audit: files=15 violations=0
$ cargo run --release --example bench_eris -- <eris> 4 0
  boot=74ms frames=4 cmds=2928592 run=5.03s (582436 cmds/s)
$ cargo run --release --example diag_census -- <eris> 8   # 8 帧诊断普查
  62(上轮) → 33 条:余 = 域命令 17 种(宿主)+ 手册无页挂账 16 条
$ ERIS E2E: frames=[1290326, 332550, 3315, 1302401, 15331, 15340, 15320, 15331]
  零 Fatal;FADE 任务链(TINT 参数)首次真实激活,主循环持续运行
$ 商业包 <game-b> v494(无 ycd): frames=[850613, 310456, 3091, 814287,
  14511, 14521, 14511, 14511],零 Fatal
```

钉值更新(钉值=回归金丝雀,变化必须显式注明):ERIS 首帧
1290322→1290326、商业包首帧 850609→850613(新语义路径 +4);
「稳态帧恒等」断言作废——FADE 链激活后每帧随场景切换进度波动,
这比旧的 TINT=0 空转稳态**更接近官方引擎行为**。

### 6. 文档与合规

- `docs/divergences/`:DIV-RT-028..034 新增;006/008/011/015 状态更新;
  三处错误引用勘误留痕。
- `crates/yu-rt/docs/command-provenance.md`:新命令行(WORD/RETURNCODE/_/
  TASKINFO SCRIPTPOS)与新系统变量行(_TEXT/_CODE_*/_INP/_MENUCODE/
  _WINDOW_EXEC_*)入「已实现命令 × 三层出处」表;§4.5 E2E 数字更新。
- 真实绝对路径 → AGENTS §1 占位符(`<side-repo>` 等)。
- 新增公开 API:`Vm::tables_mut/input_state_mut/set_menu_code`、
  `ScriptFile::pool`、`HandlerKind/build_handler_table`(crates 内)。
- 零 git 写操作。

## 成果 E1 (2026-10-19) · 审查缺陷修复(F/T/V 完成;R 阶段 blocked)

按 `docs/tasks/task-e-fixes.md` 执行。并发守则实测:R 阶段开始时 `yu-rt` 仍有
10 分钟内改动(`src/value.rs`、`src/vm/handlers.rs`、`tests/corpus.rs`),按守则
**不对 yu-rt 写入**,R1–R12 标 blocked(细节见末尾);C 会话已自行推进其中一部分。

### 完成编号

- **F1–F6(yu-format)**:TASK 非首位 `#` 按**窗口 slot id** 索引;YSTB 以头 count
  迭代并保留尾部字节;YSCF 宽高改 u32 且按引擎无符号减法判定;YSTB/YSTL/YSTD/
  YSLB/YSVR/YSER 全部 checked 算术 + `fmt.limit`;错误密钥结构自检 → `fmt.ybn.key`;
  V2 版本带改 `fmt.ybn.unsupported` 并写 DIV-009。
- **V1–V9(yu-vfs)**:删除 NUL-XOR 的 `Ok` 回退;版本表统一为 0x20 + 12 对 +
  22 字节 meta;`dataOffset` 按 u64;payload hash 无条件校验;重复名 Fatal;
  `is_verified()` 要求 0x20/0xFF;entry_count 上限 99,999 + `index_bytes/27`;
  key 探测满分提前退出;索引错误写实际失败偏移到 `Loc::Data.offset`。
- **T1–T2(yu-trace)**:`hex4` 改字节级解析(跨 UTF-8 字符边界不再 panic);
  JSON 嵌套上限 128;F/V/T 三个 crate 的样本缺失在 `YURIS_REQUIRE_SAMPLES=1`
  时硬失败,否则打印 `SKIP[count=N]`(format/vfs 由 `println!/eprintln!`,
  trace 由 `trace.sample.missing` Info 诊断)。

### 证据(REA 直接复核 0.495 `yu-ris.exe`,sha256 `4f5eefa3…`)

- `FUN_004518e9`:YSTB 以 `+0x08` count 为循环上界、`argc × 0xC` 推进窗口;
  四段各自从密钥下标 0 异或。
- `FUN_0045dbb0`:YSCF 宽高按 **u32** 读;`(w!=0||h!=0)` 时按
  `0x1F80 < (u32)(w-0x80) || 0x1FF0 < (u32)(h-0x10)` 判定(等价 0x80..=0x2000,
  且宽高必须同时为 0 或同时有效)。
- `FUN_00462a10`:长度表 = 恒等表 + 恰好 12 对交换,**无版本分支**。
- `FUN_0045e3b8`:索引起点恒 `0x20`,步长 `nameLen + 0x1B`,名字逐字节取反。
- `FUN_0043fb9c`:从条目读 **8 字节**偏移(`dataOffset` = u64,旧 `unknown0` 是高半)。
- `FUN_0043f71c`:payload MurmurHash2 **无条件**比较,0 不是哨兵。
- 真实语料:`yst00008.ybn` 第 232 条 TASK 窗口 id = 0/34/3/4,slot 34 = `#`,
  载荷 `"es.SS.FADE.TASK.MAIN"`;源文件 `es_ERIS.yst:524` 为
  `TASK[ID=es.SS.FADE.TASK #=es.SS.FADE.TASK.MAIN ...]`(ID 是任务 ID,不是标签)。
  `ysl.ybn` 对应标签 `(script_no=8, field_b=232/234)`。

### 修复前失败 / 修复后通过(原始输出摘录)

```text
# F1(修复前)
panicked at crates\yu-format\tests\corpus_eris.rs:216:
  left: []  right: [("es.SS.FADE.TASK", 232), ("es.SS.FADE.TASK.MAIN", 234)]
# F2(修复前)
thread ... panicked at synthetic.rs:519: 必须按头 count=1 解码: left: 2 right: 1
# F3(修复前)
thread ... panicked: 宽 0x2000 在引擎的无符号范围内,必须接受
# F5(修复前)
called `Result::unwrap_err()` on an `Ok` value: Segments { inst: [233, 76, ...] }
# T1(修复前)
`&self.src[self.pos..self.pos + 4]` 在 `\uあ` 上跨字符边界 panic
# V1(修复前)
结构不匹配包 → Ok + 解码垃圾名;V300 wrong12 包可读
# 修复后
cargo test -p yu-format -p yu-vfs -p yu-trace --no-fail-fast
  6 / 1 / 23 / 33 / 13 / 6 / 12 / 2 / 2 / 7 / 40 / 12 / 2 / 4 passed; 0 failed
YURIS_REQUIRE_SAMPLES=1 <同一命令>:全部 ok(含官方 data.ypf / ERIS ysbin 真样本)
cargo clippy -p yu-format -p yu-vfs -p yu-trace --all-targets:三 crate 0 告警
grep -rn "println!\|eprintln!\|dbg!" crates/*/src → 0
```

### 全工作区验收(23:00 运行时)

```text
cargo test --workspace --no-fail-fast
yu-format 6+1+23 / yu-log 13 / yu-rt 31+9+4+23+3 / yu-trace 33+13+6+12+2+2+7 /
yu-vfs 40+12+2+4  → 全部 ok,0 failed
```

(注:本机 `TEMP` 目录上某些 ephemeral 子目录会 `PermissionDenied`;
测试运行时另设 `YURIS_TEST_TMP=<repo>/target/test-tmp` 规避,属环境问题,
不是代码缺陷。)

### 改动文件

- `crates/yu-format/src/{ystb,yscf,ystl,ystd,yslb,ysvr,yser,code}.rs`
- `crates/yu-format/tests/{synthetic,corpus_eris}.rs`
- `crates/yu-vfs/src/ypf.rs`
- `crates/yu-vfs/tests/{equivalence,nul_xor,real_sample,support/ypf_writer}.rs`
- `crates/yu-trace/src/{json,event,record}.rs`
- `crates/yu-trace/tests/real_sample.rs`
- `docs/divergences/DIV-002`(→fixed)、`DIV-004`(→fixed)、新增 `DIV-009`
- 仅本文件追加;未执行任何 git 写操作。

### blocked · R1–R12(yu-rt)

复查时 `yu-rt` 仍在活跃写入(`src/value.rs`、`src/vm/handlers.rs`、
`tests/corpus.rs` 于 23:0x 仍在变),按并发守则**未写入**。已读现状与任务书对照:

| 编号 | 现状(只读核对) |
|---|---|
| R1 | **未完成**:`MAIN_TASK_ID` 仍为 `@main`;`TASK/TASKEND` 无 ID 仍落到 `@main` |
| R2 | 部分:C 已改为发 `Effect::FinishTask`;但缺省目标仍是 `@main`,"立即停止当前泵"未见 |
| R3 | **未完成**:`op_text_out` 仍返回 `Continue`,未让出并发 `VmEvent::Text`(已有事件) |
| R4 | **未完成**:调度按链表序,`priority` 字段未被排序使用;SYSTEM 默认 100 未落地 |
| R5 | 部分:C 已实现 TINT/TFLT 寄存器与 SCRIPTPOS 读;RESTART/TEXTPOS 仍 Unimplemented |
| R6 | 部分:`op_end` 返回 `Finish` 并发 `End`;`RESTART=1` 仅记录,未实现重启 |
| R7 | 部分:`PINT` 已加范围诊断;`IFBREAK[LV=0]`、`VARACT[DIMSIZE]`/数组维长乘积的 checked 收口未逐条确认 |
| R8 | 未核对完成(需逐条回归) |
| R9 | 未确认 GOSUB 64 上限;`IFCONTINUE` 帧累积未改 |
| R10 | 部分:有 `TaskFinish`;`FrameEnd` 与事件容量上限未确认 |
| R11 | 部分:`diag::at_script` 是否接线未确认 |
| R12 | **未完成**:`crates/yu-rt/src/tables.rs` 仍存在,未改为依赖 `yu-format` |

交接建议:由 C 会话继续 R1/R3/R4/R12(这四项是行为对齐的关键);R2/R5/R6/R7/R10/R11
按上表逐条收尾;每项补回归后再更新 golden。

## 成果 18 (2026-10-20) · 优化后快速复审(E1 + C3)

- 验证:E1 的 F/T/V 修复与 C3 第三轮对齐已在工作区;全工作区
  `YURIS_REQUIRE_SAMPLES=1 cargo test --workspace --no-fail-fast`
  共 249 个用例 **全部通过**(含官方 data.ypf 94s、ERIS ysbin 117s 真样本套件)。
- 抽查确认:
  - format F1 已改按窗口 `slot` 索引 ysc 参数名;F2 头 count/尾部;F3 YSCF u32;
    F4 checked 算术;F5 `fmt.ybn.key`;F6 V2 挂 Unsupported。
  - vfs V1 NUL-XOR 拒绝;V2 仅 0x20/12 对/22 字节;V3 dataOffset u64;
    V4 payload hash 无条件;V6 `is_verified` 严格;V7 entry cap;V8 提前退出。
  - trace T1 `hex4` 字节级解析 + 深度上限;T2 样本强制。
  - rt R7 四条 panic 路径收口;R5 的 TINT/TFLT/TSTR 与 SCRIPTPOS 已落地。
- 仍未完成(E1 标记 blocked、C3 未覆盖):
  R1 主任务 ID/SYSTEM 与 TASK/TASKEND 无 ID 自身目标;
  R2 TASKEND 立即停止(当前只置 finished);
  R3 显示文本 `_` 仍返回 Continue,未作为让出点(手册 `basic/task.html`);
  R4 Z 优先级字段存在但调度未使用;
  R9 GOSUB 上限仍为 63(基帧占 1 层,`vm.rs:208` + `handlers.rs:278`);
  R10 `FrameEnd` 从未发射、事件 Vec 仍无上限;
  R11 `diag::at_script` 从未调用;
  R12 `yu-rt` 仍未依赖 `yu-format`,解析器双份。
- clippy:全工作区无 error,但 `yu-rt` lib 有 6 条 warning、examples/tests 有未用 import 等,
  未达到“零告警”。
- 仅复审与文档记录;未修改源码,未执行 git 操作。

## 成果 E2 (2026-10-20) · 任务书 E 剩余 rt 项对齐(R1/R2/R3/R4/R9/R10/R11)

> 会话 E2。范围仅 `crates/yu-rt`(+ 一个 `yu-trace` golden 重生成)与文档;
> **未做 R12**;零 git 写操作。全部行为改动以 0.495 `yu-ris.exe`
> (sha256 `4f5eefa3…`)的 REA 反编译为证据,证据表见下。

### 1. 证据表(硬门槛逐项)

| 编号 | 手册页 + 原文 | 真实脚本位置/形态 | 引擎证据(REA,observed) | 结论 |
|---|---|---|---|---|
| R3 | `basic/task.html`「「ウェイト命令」か「表示テキスト」が来ると、タスクはそれを実行したあと、一旦休憩します(約0.016秒)」 | C3 已证一行 = `WORD→[_]→WORD→RETURNCODE`;`ystdlib.yst` 文本行同形 | `RETURNCODE` 处理体 `0x44c390`:`0x44c451 MOV byte [ESI+9],1`、`0x44c455 MOV EAX,-1`;泵 `FUN_00404398` `0x40442e` 检查 `task+9` 非零即跳过;`WORD` `0x458ee0` 成功尾 `0x459054 XOR EAX,EAX`, `_` `0x4567c0` 成功尾 `0x4569cc XOR EAX,EAX`;恢复判据 `FUN_004046fc` + 按钮表 `0x5b48ac..0x5b48bc`(MOUSE_L/ENTER/PAD×2/CTRL) | 让出点是 **RETURNCODE**,不是 `WORD`/`_`;一行文本一 vsync,直到推进输入 |
| R1 | `basic/task.html`「…「SYSTEM」というIDが名づけられています」;`control/task.html`/`taskend.html`「タスクＩＤを指定しなかった場合…実行した自身のタスク」 | `es_ERIS.yst:524 TASK[ID=… Z=@es.63BITMAX TINT=…]`;`stdlib/ystdlib.yst:759/781/789/816/824/850/858`(`TASK[E=0]`/`TASKEND[]` 无 ID 形态) | 启动链 `FUN_00465384` `0x4652f0 MOV EDX,0x807b00`("SYSTEM")→`0x414559` 按名查任务;`TASK` `0x45074a MOV EBP,[0x801720]`(当前任务);`TASKEND` `0x4521dc` 同取当前任务 | `MAIN_TASK_ID="SYSTEM"`,系统任务 Z=100;无 ID 目标 = 执行者自身 |
| R2 | `control/taskend.html`「タスクを終了します」 | `ystdlib.yst:850 TASKEND[]` / `:858 TASKEND[ID=$_PSTR(1)]` | `0x4521dc`:自身 → `CALL 0x452364`(双向链表解链 + `DEC [pool+0x20]`)→ `MOV EAX,-1`(让出);他人 → 摘链 + 返回 0;`0x45237f..0x4523b9` 为解链实现 | 立即摘链;同命令之后的指令不执行;`EXIST` 立即 0;同 ID 重建从新标签开始 |
| R4 | `basic/task.html`「優先度の高い順から実行されます」「同じだった場合の実行順序は不確定」;系统任务默认 100 | ERIS `Z=@es.63BITMAX` / `Z=@_PINT(3)`(stdlib `_task`) | 插入 `FUN_004613c0`:`+0x28/+0x2c` 组 64 位键 `jl` 前插(降序),等键插在既有节点之后;重配置 `FUN_0042f7bc` = 摘链后重插;启动 `FUN_004621d0` `0x4623f4` Z=0x64 | 降序按 Z;同 Z 取引擎实际序(创建序),钉死以保证 A3 |
| R9 | `control/gosub.html`「GOSUBのネストは64回まで可能です」;`control/ifcontinue.html`「直前の IF 命令まで戻り」 | ERIS 10,215 次 GOSUB;`ystdlib.yst` 深调用链 | `FUN_004459c4`:`0x4459e6 MOVSX ESI,[task+0x140]`、`0x4459ed LEA EAX,[ESI+1]`、`0x4459f0 CMP EAX,0x40; JE 报错`;`IFCONTINUE 0x446490` 深度=`depth-LV`(目标 IF 重新压帧);`IFBREAK 0x446408` 深度=`depth-LV+1`(保留目标层,由其 IFEND 弹) | 允许 64 次嵌套;第 65 次 Fatal;IFCONTINUE 丢弃目标层、IFBREAK 保留目标层 |
| R10 | 内部契约:`docs/architecture.md` §3/A3、`docs/oracle.md`(非官方语义) | — | — | 每逻辑帧 `FrameStart`/`FrameEnd`;事件队列有界 + `drain_events` |
| R11 | `docs/architecture.md` §8「每个 Fatal 报告自带…哪个脚本位置」 | 夹具坏标签用例 | — | `Diag.where_` 补 `Loc::Script{file,label=任务ID,pc,cmd}` |

### 2. 实现要点(改动文件)

- `src/task.rs`
  - `MAIN_TASK_ID = "SYSTEM"`;`TaskSeed::main` Z=100。
  - `TaskNode` 增 `text_rest`(`task+9`)与 `seq`(创建序);新增 `TaskView` 投影。
  - 调度:按 `(priority desc, seq asc)` 有序链(`insert_sorted`/`sort_nodes`);
    `upsert` 摘链后按新 Z 重插;`finish_by_id` 立即摘链;`find_by_id` 过滤已结束。
  - `StepOutcome::TextYield`(文本行尾让出);`TaskExecutor` 增
    `end_logical_frame`/`task_removed`/`text_advance_requested`/`sync_task_view`。
  - 帧尾 `retain` 前读取主线结束标记(`main_finished` 覆盖正常 END 路径)。
- `src/vm.rs`
  - `FrameEnd` 发射;`events` 有界(`DEFAULT_EVENT_CAPACITY = 1<<20`),
    队满**批量**丢弃最旧 1/4 并计 `events_dropped()`(逐条 `remove(0)` 会 O(n²));
    新增 `drain_events`/`with_event_capacity`/`events_dropped`。
  - `cur_script/cur_pc/cur_cmd/cur_task` 定位簿记(复用 String,零分配);
    `report()` 与错误路径经 `locate()` 补脚本位置;处理器内部 `Unimplemented`
    计入 `unimplemented_cmds`。
  - `TaskExecutor::task_removed` 回收执行位并发 `TaskFinish`。
- `src/vm/handlers.rs`
  - `RETURNCODE` → `TextYield`;`TASK`/`TASKEND`/`TASKINFO` 无 ID = 自身;
    `TASKEND[self]` → `Finish`(泵立即停),他人 → `Effect::FinishTask`;
    `TASK` 建任务/重配置时 T* 寄存器先清零再落参(REA `0x450916..0x450943`);
    目标=自身时直接改写当前执行位。
  - GOSUB 上限 64 次(`frames.len() > 64` 才报错);
    `IFCONTINUE` 改为丢弃目标层、`IFBREAK` 保留目标层。
  - `TASKINFO` 读 `task_view`(泵期间调度器被 `mem::take` 借出,原实现恒读到空链)。
- `src/input.rs`:`text_advance_requested()`(MOUSE_L/ENTER/CTRL/手柄计数器 > 0)。

### 3. 新增回归(全部通过)

`cargo test -p yu-rt --lib --test fixture --test golden` 原始输出:
```
test result: ok. 35 passed; 0 failed; ...        # lib
test result: ok. 38 passed; 0 failed; ...        # fixture
test result: ok. 3 passed; 0 failed; ...         # golden
```

| 用例 | 覆盖 |
|---|---|
| `task::tests::main_task_defaults_to_priority_100_and_id_system` | R1 |
| `task::tests::higher_priority_runs_first_and_equal_priority_keeps_creation_order` | R4 |
| `task::tests::taskend_of_self_stops_the_current_pump` | R2 |
| `task::tests::taskend_effect_unlinks_by_id_immediately` | R2 |
| `task::tests::same_id_rebuild_starts_at_new_entry_after_taskend` | R2 |
| `fixture::main_task_id_is_system_and_script_visible` | R1 |
| `fixture::taskend_without_id_ends_only_self_and_main_continues` | R1/R2 |
| `fixture::taskend_stops_neighbor_commands_and_rebuild_uses_new_entry` | R2 |
| `fixture::higher_z_task_runs_before_lower_z` | R4 |
| `fixture::one_text_line_per_frame_until_advance_input` | R3 |
| `fixture::gosub_allows_64_nested_calls` / `gosub_65_nested_calls_is_fatal` | R9 |
| `fixture::ifcontinue_does_not_accumulate_if_frames` | R9 |
| `fixture::frame_events_are_bounded_by_start_and_end` / `event_queue_is_bounded_and_reports_drops` | R10 |
| `fixture::script_diag_carries_task_script_and_pc` | R11 |
| `fixture::main_end_sets_main_finished` | 结束路径 |
| `fixture::task_without_tint_does_not_inherit_caller_registers` | T* 清零(REA) |

### 4. golden 变更清单(逐条理由)

| 文件 | 变更 | 理由 |
|---|---|---|
| `crates/yu-rt/tests/golden/logic_slice.jsonl` | `@main`→`SYSTEM`;+`frame_start/frame_end` | R1(手册+REA 启动链)、R10(帧边界) |
| `crates/yu-rt/tests/golden/input_counter.jsonl` | 同上 | 同上 |
| `crates/yu-rt/tests/golden/error_paths.jsonl` | 同上 | 同上 |
| `crates/yu-rt/tests/golden/corpus_{ev,menu,se}_define.jsonl` | 同上 | 同上 |
| `crates/yu-trace/tests/golden/synthetic_frame.jsonl` | 同上(seq 顺延) | 同上 |

更新方法 = `UPDATE_GOLDEN=1`(已有机制);更新后逐文件核对:**去掉帧边界行后,
新旧 golden 只差 `@main`→`SYSTEM` 一处**。

### 5. 真样本与全工作区验收

```
$ cargo test -p yu-rt --release --test corpus --test corpus_golden   # YURIS_REQUIRE_SAMPLES=1
E2E frames: [1290326, 332550, 3315, 1302412, 15340, 15320, 15331, 15320]
test result: ok. 9 passed; 0 failed; ...          # corpus
test result: ok. 4 passed; 0 failed; ...          # corpus_golden
loose  = ([1290326, 332550, 3315, 1302412, 15340, 15320], 0, "2bc95198…")
packed = ([1290326, 332550, 3315, 1302412, 15340, 15320], 0, "2bc95198…")

$ cargo test --workspace --no-fail-fast               # YURIS_REQUIRE_SAMPLES=1
CARGO_EXIT=0
```
- ERIS 首帧钉值 **1,290,326 保持不变**;后续帧因 Z 排序生效有 ±11 条差异
  (`1302401→1302412`、`15331→15340`),无 Fatal、主线不结束。
- 散盘 vs 官方 `data.ypf` 逐帧命令数 + 变量域摘要哈希 **完全一致**(A1)。

### 6. 性能(修 R10 时踩到的两个坑,已修)

- 一度把任务链投影**每条命令**克隆一次 ⇒ ERIS 首帧 130 万命令直接卡死;
  改为「每条任务泵一次 + 链变化时刷新」。
- 有界队列用 `Vec::remove(0)` ⇒ 队满后 O(n²);改为批量 `drain(..cap/4)`。
- 现状:`bench_eris` 4 帧 events 关 **615,094 cmds/s**(基线 582k);
  events 开 **212,205 cmds/s**(基线 316k;差额 = 帧边界事件 + 有界队列摊还)。
- `audit_hardcode.py`: `files=15 violations=0`;`grep println!/eprintln!/dbg!`
  在 `crates/*/src` = **0**。
- clippy:`yu-rt` lib = 基线 6 条(无新增),workspace 无 error。
  (注:本轮收尾时并行会话正在改 `yu-format` 的 `CommandTable/Segments`,
  其间 `--workspace` clippy 报的 E0063 属该会话在途改动,与本轮无关。)

### 7. 未完成 / 挂账

- **R12 未做**(任务书明确排除)。
- `TASKINFO` 的 `TEXTPOS/RESTART/A`、`TEXT[WAIT=…]` 对文本等待的影响仍
  `Unimplemented`(无手册页/无引擎证据);见 DIV-RT-035/015。
- 新增 7 条 divergence 档案:DIV-RT-035..041(`crates/yu-rt/docs/divergences/README.md`)。
- 行为差异记录:手册称同 Z 次序「不確定」,内核取引擎实际插入序(创建序),
  已写入 DIV-RT-038。

## 成果 E3 (2026-10-20) · R12 统一解析器:yu-rt 依赖 yu-format,删除重复表解析

> 任务书 E 的 R12(重构,非行为改造;唯一允许的行为面 = YSCF 宽高 u16→u32)。
> 基线取自成果 E2 完成态(`target/baseline-E3.log`);改后全量日志
> `target/final-workspace.log`、`target/final-corpus.log`。

### 依赖图变化

- `yu-rt` 新增依赖 `yu-format`(架构 §2 的 `RT → FMT` 落地);
  `cargo tree -p yu-rt | grep yu-format` 命中。
- 解码唯一来源:YSCM/YSLB/YSVR/YSTL/YSCF/YSTB 全部经 `yu-format`;
  YSCD 不在 yu-format 范围,仍在 `yu-rt/src/ycd.rs`(未动)。

### 删除的重复实现(旧 `crates/yu-rt/src/tables.rs` 行号)

- 容器解析底座:`MAGIC_YSCM/YSLB/YSVR/YSTL/YSCF/YSTB/YSTD/YSER`(L37-44,
  其中 YSTD/YSER 本就只有常量无解析器)、`YSTB_HEADER_LEN`(L47)、
  `WINDOW_SIZE`(L49)、`need`(L54)/`eight`(L64)/`u16_at`(L76)/
  `u32_at`(L81)/`cstr`(L86)/`check_header`(L104)、`xor_cyclic`(L844)。
- 逐表 `parse` 实现:YscmTable(L194)/LabelTable(L287)/VarImage(L403)/
  ScriptList(L531)/ScriptFile(L706)/EngineConfig(L611,整个结构删除)。
- 元数据表的字节入口签名改为 `from_format(fmt)`(审计 grep
  `fn parse(data` → 0);`ScriptFile::parse(script_id,data,key)` 按任务
  保留清单保留签名,内部 = `Segments::decrypt` + 区段类型化投影。
- 保留:运行时结构(BootTables/TableSet/ScriptFile/GroupRec/WindowRec、
  by_name 索引、pool/part4、first_window)、`ParamKind`/`Label`/`VarRecord`/
  `VarInit`/`ScriptEntry` 投影、`globals/locals_of/text_count/path_of`
  访问器、错误码包装、`DEFAULT_SCRIPT_KEY`(改为引用
  `yu_format::key::DEFAULT_KEY_BYTES`)。

### yu-format 加法式 API(最小,不改既有字段)

- `version: u32` 字段补进 `yscm::CommandTable`/`yslb::YslLabels`/
  `ysvr::VarDecls`/`ystl::ScriptList`/`yscf::Yscf`/`ystb::Segments`:
  运行核 provenance/`engine_version` 需要各表版本,解码层此前未保留。
- 无其它 API 变化。

### YSTL 字段勘误(唯一实质行为变化,已按用户复核裁决)

- 旧 rt 把尾部第 3 个 u32 当 `text_count`、第 5 个当 `file_no`;按
  yu-format 定性(第 3=`var_count`、第 4=`label_count` 与 ysl 逐条互证、
  第 5=`text_count`,`meta`/`file_no` 命名错位)修正,`meta`/`file_no`
  删除(无消费者)。
- 唯一受影响消费者 = `SYSTEMINFO[TEXTNUM]`:改后返回**真实文本条数**。
- 可观测影响(ERIS E2E,命令数):

  | 帧 | 改前(E2 基线) | 改后 | 说明 |
  |---|---|---|---|
  | 0(钉值) | 1,290,326 | 1,290,326 | 不变 |
  | 1 | 332,550 | **274,466** | 启动期 `SYSTEMINFO[TEXTNUM,NO=1..n]` 逐脚本查询循环,值修正后下游工作量变化(−58,084) |
  | 2..7 | 3,315 / 1,302,412 / 15,340 / 15,320 / 15,331 / 15,320 | 同左 | 逐条不变 |

  loose/packed 等价保持(两侧同为新值);变量域哈希
  `2bc95198…` → `18a94a71…`。**钉值未改**(corpus 钉的是首帧与">0"
  断言,均不变);是否把新值钉进回归,留主人裁决。
- YSCF 宽高 u16→u32:ERIS 800x600 数值不变,`_SCREEN_SIZE_X/Y` 输出
  不变(读 `vm.screen()`,来源改为 `Yscf.width/height` u32)。
- 容错面差异(仅畸形输入;真实语料零差异):YSTB「精确闭合」改为
  format 语义(count×4≤s1 富余保留 + trailing 保留);YSLB/YSVR/YSTL
  旧闭合检查随 format 不再重复;YSVR kind/ty 越界、YSCF 引擎范围、
  YSTB 结构自检(`fmt.ybn.key`)由 format 带入(更严)。缺表仍
  `table.missing`、结构缺陷包装为 `table.bad_magic`/`table.incomplete`
  (fmt 原诊断进 `causes`)。

### 夹具修正

- `tests/support/mod.rs` `build_ystl`:0 条清单补齐 16 字节容器头
  (format `read_header` 下界;真实语料不存在 0 条清单)。其余夹具
  (37 串+256 尾表、0x400 桶表、20 字节尾部)本就与 format 兼容。

### 新增测试

- `crates/yu-rt/tests/consistency.rs`(3 项,真实 ERIS 语料):①元数据
  表投影逐字段同源(name/args/ParamKind×flag 字节交叉/检查器、标签
  字节键→script/pc/base、变量 kind/group/script/slot/type/dims/初值
  按位、清单 index/path/var_count/label_count/text_count/file_time、
  YSCF 800x600);②136 个 YSTB 的 ScriptFile 投影 ↔ `Segments` 逐字段
  (组/窗口/池/part4/源码位置,组数 36,974 / 窗口 80,102 钉住);
  ③TableSet 生产装载路径 ↔ 直解(入口标签、config、按需装载脚本、
  `vm.screen()==(800,600)`)。缺样本显式 skip,`YURIS_REQUIRE_SAMPLES=1`
  硬失败。
- `tables.rs` 单测 `script_file_projection_roundtrip`(合成 YSTB 投影)。

### 验证(原始输出)

```
$ cargo test --workspace --no-fail-fast        # YURIS_REQUIRE_SAMPLES=1 + ERIS/SDK 样本
  exit=0;268 项全 ok,0 failed(含 corpus 9 / corpus_golden 4 /
  fixture 38 / golden 3 / consistency 3 / 真样本 data.ypf+ysbin 套件)
$ cargo test -p yu-rt --test corpus -- --nocapture   # 最终态
  E2E frames: [1290326, 274466, 3315, 1302412, 15340, 15320, 15331, 15320]
  loose  = ([1290326, 274466, 3315, 1302412, 15340, 15320], 0, "18a94a71…")
  packed = (同上)                                    # 9 passed; 0 failed
$ cargo tree -p yu-rt | grep yu-format         # 命中
$ grep -rn "MAGIC_YSCM|…|fn check_header" crates/yu-rt/src   → 0
$ grep -rn "fn parse(data" crates/yu-rt/src/tables.rs        → 0
$ cargo clippy -p yu-rt -p yu-format --all-targets
  新增告警 0(存量 15 处不变,另修复 1 处存量 doc 缩进;
  本会话引入的 3 处已全部清零)
```

### 并发注记

- 基线期间成果 E2 会话仍在收尾(11:58 最后写 `handlers.rs`;12:05 向
  `fixture.rs` 追加 `task_without_tint_does_not_inherit_caller_registers`,
  该测试在本重构下通过)。12:05 后仅本会话写入。
- 未修改 `<repo-old>`;零 git 写操作;真实路径均以占位符表述。

### E3 续记(同日,主人裁决后)

- **钉入 TEXTNUM 修正后的新钉值**:`eris_entry_chain_runs_end_to_end`
  新增 `counts[1] == 274_466`(第 2 帧启动期 TEXTNUM 查询循环,证据链
  见上文「YSTL 字段勘误」);商业包 850_613 钉值加注警示(本机无样本,
  TEXTNUM 修正可能使其漂移,失败时按 E3 解释重新基线)。
- **兜底日志(主人要求)**:商业发行版无 `YSCom.ycd` 时,系统变量启用
  内嵌引擎 ABI 兜底**不再静默**——启动链发 `table.fallback`(Info,
  Boundary::Table,定位 `YSCom.ycd`):说明兜底已用工程 `ysv.ybn` 逐项
  校验(DIV-RT-016)、命令表/参数名仍来自工程 `ysbin/ysc.ybn`(数据
  驱动,无版本兜底)。日志只进诊断汇(`Vm::diags`/`take_diags`),
  **不进事件流**(事件流是对齐对象,golden 不因此变化;表来源另见
  `table_provenance`)。新增错误码 `table.fallback`(code.rs + ALL_CODES)。
- **新增测试** `fallback_abi_logs_notice_on_real_data`(真实 ERIS 数据,
  单根无 aux 启动):ycd=None、sysvars=548、yscm=119(命令来自工程表)、
  诊断汇含 `table.fallback`、事件流不含之。
- 真实参考:主人提供的 `ysbin/ysc.ybn` 实测 YSCM v495 / 119 命令 /
  37 条消息(全非空)+ 256 字节键表,与 yu-format 解析一致。
- 验证:`cargo test -p yu-rt --test corpus` 9 ok(新钉值生效);
  快套件(lib 35 / fixture 38 / golden 3 / consistency 4 / corpus_golden 4)
  全绿;clippy 存量 16 处不变、新增 0。

## 成果 19 (2026-10-20) · yu-trace 录制路径关闭事件丢弃阈值

- 问题:运行核事件队列默认 `1<<20`;ERIS 首帧约 129 万命令,`VmTrace` 在
  `take_events` 前就会丢最旧事件,导致 trace 静默缺段而 diff 仍可能通过。
- 修复(`crates/yu-trace/src/replay.rs`):
  - 新增 `pub const TRACE_EVENT_CAPACITY: usize = usize::MAX`;`VmTrace` 启动后
    调用 `Vm::with_event_capacity(TRACE_EVENT_CAPACITY)`。该值只是丢弃阈值、
    不预分配,实际内存 = 单次 tick 的事件量。
  - 模块文档补充:录制路径不丢事件;若单 tick 超内存,应缩小录制粒度。
- 回归(`crates/yu-trace/tests/real_sample.rs`):
  - 录制后断言 `vm.events_dropped() == 0`;
  - 断言 `event_records > 1<<20`,保证该用例真的覆盖默认丢弃阈值。
- 验证(原始输出):
  ```
  $ cargo test -p yu-trace --test real_sample -- --nocapture
  official_data_ypf_smoke ... ok
  test result: ok. 2 passed; 0 failed; finished in 104.39s
  $ cargo test -p yu-trace --lib --test diff --test equivalence --test event_trace \
      --test golden --test roundtrip
  33 / 13 / 6 / 12 / 2 / 7 passed; 0 failed
  $ cargo clippy -p yu-trace --all-targets
  yu-trace 自身 0 告警(输出中的 warning 均来自依赖 yu-rt)
  ```
- 改动仅 2 个文件;未执行 git 操作。
- **版本判别示例** `crates/yu-rt/examples/version_probe.rs`(主人要求):
  散盘目录 / `data.ypf` 封包两形态直读(不经落盘拆包),打印七张元数据表
  头(魔数/版本/计数)、yscfg 分辨率与标题、脚本数与一份解密自检、ycd
  探测;七表版本交叉一致即判别引擎版本。实测:ERIS `data.ypf` = 0.495
  (1407 条目,136 脚本,800x600),<game-b> `data.ypf` = 0.494(12212
  条目,160 脚本,1100x825,标题「<game-b> ver1.02」);两包 ycd 均缺席
  → 兜底日志为常态。Tyn 旁线同法(读工程 YSCM 头 version)。
  注:`project-eris/data` 散盘语料已于当日 13:02 被删除(12:59 的
  全工作区真数据跑在其之前完成,269 项全绿);此后
  `YURIS_SAMPLE_ERIS_YSBIN` 缺样本,corpus 真样本测试显式 SKIP,
  ERIS 相关验证以官方 `data.ypf` 为准。

### E3 附:多版本官方 SDK 的 YSCD 对照(2026-10-20,主人指示)

- 官方下载页(https://yu-ris.net/download)公开存档仅四个大版本:0.292/0.92(Beta3)、
  0.466/0.19(Beta4.5)、0.479/0.14(Beta4.7 安定版)、0.495/0.32(Beta4.8);
  小版本(0.494 等)不单独归档。四个包已解压并规范到
  `<side-repo>/target/sdk/0.{292,466,479,495}/yu-ris_*`(0.466 的 SJIS 乱码目录名
  已还原),原始包集中在 `target/sdk/dl/`,四份编译器定义集中在
  `target/sdk/ycd-all/YSCom_0.*.ycd`。
- 逐版本实测(内核自己的 `YcdTable::parse` 四份**全部可读**,段定位对布局平移不敏感):

  | | v292 | v466 | v479 | v495 |
  |---|---|---|---|---|
  | 命令段 | 91 | 100 | 104 | 119 |
  | 系统变量 | 482 | 513 | 526 | 548 |
  | 消息 | 122 | 121 | 121 | 121 |
  | 尾部常量区 | 3374B | 3374B | 3374B | 3374B |
  | `_LC` | 32 | 48 | 48 | 48 |
  | `_PINT` | 34 | 50 | 50 | 53 |
  | `_SCREEN_SIZE_X` | 54 | 80 | 93 | 115 |
  | `_KEY_A` | 101 | 132 | 145 | 167 |
  | `_KEY_ANY` | 209 | 240 | 253 | 275 |

- 结论(直接回答"4xx/2xx/5xx 识别"):**4xx 内部并不一致**——466→479→495 的
  系统变量 id 逐个漂移(表按插入历史增长);2xx(292)形状完全不同(482 项/91 命令)。
  因此 ABI 不能按版本号段判别;唯一正确机制 = **每局用其自带 `ysv.ybn` 逐项校验,
  按"哪个 ABI 表能通过校验"选择**(数据驱动,A5)。现状:内嵌 ABI=0.495(548),
  v494 商业包已实证 548/548 吻合(即 0.49x 线稳定在 548 布局,大跳变发生在
  479→494 之间);旧线游戏会**校验失败拒绝启动**(安全失败,不静默读错)。
- 恒定项:`_TIME=0`、`_PAI=10` 四版一致;消息 121/122;尾部 3374 字节四版恒定
  (版本无关的固定表)。四份 ycd 可立即用于生成逐版本 ABI 表(下一步建议:
  装载时按 ysv 校验选择 ABI 表,不做版本号分支)。
- 「能否直接从游戏拿系统变量名」的实证(2026-10-20):**不能**。逐字节检查:
  0.495 `yu-ris.exe`、<game-b> 发行版 exe/`MyDll.dll` 对
  `_KEY_ANY/_SCREEN_SIZE_X/_MOUSE_L/_LC/_PINT` **全部 0 命中**(exe 里唯一
  `_UPPER` 串是 D3D 常量);包内 ysv 只有 id/类型/维数、yse 只有错误文案、
  ysc 里的 `_MOUSE/_SYSTEM/...` 是**命令名**不是系统变量;SDK 的
  `YSCom.exe` 同样 0 命中——编译器也是读 `YSCom.ycd` 才拿到名字。
  即名字表的唯一宿主是开发侧 ycd,商业包把它与编译器一起剥掉。
  游戏数据能提供的是**指纹**:ysv 前 N 条 kind=1 记录的 (id,type,dims),
  足够校验/选择 ABI 表、识别未知构建并拒绝启动,但推不出名字。

## 成果 E3 复核补充 (2026-10-20) · R12 只读验收

> 本会话只做复核;未改生产代码、测试或数据(一次测试路径试改已撤回)。
> 既有「成果 E3」条目保留不动,此处只补复核结果与环境说明。

- 生产代码与既有 E3 完成态一致:
  `find crates/yu-rt/src crates/yu-format/src crates/*/Cargo.toml -newer target/final2-workspace.log`
  无输出,故 `target/final2-workspace.log` 的 269 项通过仍覆盖当前生产代码。
- 只读复核:
  - `cargo tree -p yu-rt` 命中 `yu-format`;
  - `MAGIC_*`/`fn check_header` 审计 0;`tables.rs` 的 `fn parse(data` 0;
  - 非样本套件 lib 35 / fixture 38 / golden 3 全过;
  - `yu-format` 真语料 1 过(`ysc=119 / ysl=1941 / ysv=2568 / yst_list=141 / yst=136`);
  - `yu-vfs` 官方 `data.ypf` 4 过。
- 环境说明:当前封包为 `<side-repo>/workbench/project-eris/data.ypf`;
  `project-eris/data` 已不存在,散盘在 `project-eris/save/data`(用户故意放置)。
  测试的工程根推导 `data_root.parent()` 因此找不到 `system/YSCom/YSCom.ycd`;
  按用户指示未搭临时根、未改 `save/data`,ycd 相关用例本次跳过。
- E2E 对照(既有 E3 日志):首帧 `1,290,326` 不变;第 2 帧
  `332,550 → 274,466`(YSTL `text_count` 勘误,已按 E3 续记裁决钉值);
  总命令 `2,989,914 → 2,931,830`;变量域哈希
  `2bc95198… → 18a94a71…`;其余帧与 0 Fatal 不变。
- YSCF 宽高 u32:ERIS 800x600 前后一致;引擎合法范围 `0x80..=0x2000`,
  合法值均落在 u16 内,故有效游戏无数量变化;商业包样本当前不可用,未测。
- 风险/挂账:YSTL 勘误是 YSCF 之外的唯一行为变化;`ScriptFile` 用
  `Segments` 投影而非 `ystb::decode`(运行核按 opcode 分派且需要 `param`,
  `decode` 只给命令名);`BootTables.config` 直接使用 `yu_format::yscf::Yscf`,
  `EngineConfig` 删除。

- 「能否集齐小版本」结论(2026-10-20):**不能,也不需要**。官方 `/history` 是
  纯变更日志(0.400→0.495 每版都有条目,`down.cgi` 链接 0 个);Wayback 对
  `down.cgi` 无任何存档捕获。且履历本身就是"小版本会动 ABI"的铁证:
  **ver0.480/0.01** 一次性追加 `_PINT/_PFLT/_PSTR/_PFLG 11..16`、
  `_RINT/_RFLT/_RSTR/_RFLG 11..16`、`$_M11..16`、`$_MB11..16` —— 正是
  479(526 项,PINT dims=11)→494/495(548 项,dims=17)的跳变点(与四份
  ycd 逐项数据双向印证);另有 0.470/0.472/0.400 等变量追加。故 ABI 只能
  按**族**覆盖(四份 SDK 恰是四个族:Beta3 / ≤0.469 / 0.470–0.479 /
  ≥0.480),游戏用自身 ysv 指纹选族;指纹不匹配 = 未知构建,拒绝启动后
  再针对该构建取 ABI(自带 ycd 或逆向其 exe),不做版本号对齐。

### E3 附:系统变量 ABI 指纹选族(2026-10-20,主人指示"加入指纹实现")

- 新增三份官方 ABI 数据表(由 `scripts/gen_sysvar_abi.py` 从官方 SDK 的
  `YSCom.ycd` 生成,带来源版本/SHA-256):
  `data/sysvar-abi-0292.tsv`(482)、`sysvar-abi-0466.tsv`(513)、
  `sysvar-abi-0479.tsv`(526);原 `sysvar-abi-0495.tsv`(548)经同一脚本
  复现校验(仅首行注释差异)。
- `sysvar_abi.rs`:`known_profiles()` 四族(选择顺序新→旧)+
  `select_for(&VarImage)`——**按游戏自带 `ysv.ybn` 指纹**(var_id<表长 的
  kind=1 镜像恰好覆盖 0..表长,且 type/dims 逐项相等)选族,不看版本号;
  全不匹配 ⇒ `table.version` Fatal 并逐族列出失配原因(未知构建拒绝启动)。
  `builtin_abi()` 保留为 0495 兼容入口。
- `tables.rs`:无 ycd 时的路径由"内置 0495"改为"指纹选族";ycd 路径不变。
  `vm.rs` 兜底日志改为报告所选族(如 `engine ABI profile sysvar-abi-0495.tsv`),
  仍为 sink-only(不进事件流)。
- 测试:`fingerprint_selects_each_known_family`(四族指纹各自选中,长度
  548/526/513/482)、`fingerprint_rejects_unknown_and_lists_reasons`
  (改一个维数 ⇒ 四族全拒、错误含族名)、`trailing_extra_records_keep_newest_family`
  (v555 形态:548 前缀 + 尾部追加 ⇒ 归 0495 族,Likely);
  真实数据:`fallback_abi_logs_notice_on_real_data` 断言 ERIS 选中
  `sysvar-abi-0495.tsv` 且日志含族名。
- 测试基建:散盘语料被移到 `project-eris/save/data` 后,三个测试文件的
  "工程根 = ysbin 上两级"假设失效 → 改为**向上查找**
  `system/YSCom/YSCom.ycd`(`find_project_root`,最多 4 层)。
- 验证:corpus E2E 与上一轮**逐字节一致**
  (`frames=[1290326, 274466, 3315, 1302412, 15340, 15320, 15331, 15320]`,
  total 2,931,830, loose/packed 哈希 `18a94a71…`);全工作区套件见
  `target/abi-workspace.log`;clippy 无新增。

## 成果 M3-GFX (2026-10-20) · yu-gfx 渲染语义层 + CompositorFrame + 证据表

> 会话 M3-GFX。范围 = `yu-gfx`(语义层/可序列化显示列表/证据表/回归);
> 不含 wgpu/wasm/窗口。零 git 写操作。

### 1. 交付物

- 新 crate `crates/yu-gfx/`:
  - `frame.rs`(`CompositorFrame`/`Viewport`/`Clear`/`ColorSpace`/`TextureFilter`)
  - `pass.rs`(`Pass`/`PassTarget`)、`op.rs`(`Op`/`CopyOp`/`ImageOp`)
  - `layer.rs`(Layer/Part/BlendMode 0..18 状态表/Transition/Filter)
  - `group.rs`、`transition.rs`、`text.rs`(文本模型)
  - `state.rs`(`GfxState`:消费 `VmEvent::DomainCall`,维护图层树/组/复制)
  - `table.rs`(槽号→参数名,唯一来源 = 工程 `ysc.ybn`)
  - `path.rs`(ID/IDNO 路径规范化:`W0`/`:`/`.`/`..`/尾 `/`)
  - `json.rs`(固定字段顺序;`to_json`/`to_display_json`/`blend_table_json`)
  - `error.rs`(`gfx.*` 诊断码)
- 文档:`docs/render-parity.md`(证据表 + 后端执行契约 + 旧线勘误);
  `crates/yu-gfx/docs/divergences/README.md`(DIV-GFX-001..018)。
- 取证工具:`crates/yu-rt/examples/gfx_census.rs`、`crates/yu-gfx/examples/eris_dump.rs`。
- 必要 plumbing(非 gfx 语义):`yu-rt` 的 `DomainCall` 以前 `args: Vec::new()`,
  宿主域拿不到参数;现按窗口求值并填充(`handlers.rs` 新增
  `eval_arg_windows`,域分支不再空参)。通用 `Command` 事件路径不变。
- 首屏 golden:
  - `crates/yu-gfx/tests/golden/eris_first_screen_display.json`(20,620 B)
  - `crates/yu-gfx/tests/golden/synthetic_group_layers_display.json`

### 2. 证据表(详见 `docs/render-parity.md` §1)

| 语义 | 手册 | 真实语料 | 引擎证据 | 结论 |
|---|---|---|---|---|
| 图层/ID/路径/Z/组 | `basic/layer.html`、`cg/cg.html` | ERIS 660 `CG`/307 `CGACT`/103 `CGEND` | `FUN_00413dc8` 34-49;`FUN_00414ba4` 115-156 | Confirmed;同 Z 取创建序(手册称不確定) |
| `A` 0..256 | `cg/cg.html` `A=0～256` | 首屏 A=0/256 | `FUN_00414ba4` 116/135-154/910-913 | Confirmed;固定 `min(A,255)/255`,着色器 `A/256` |
| `MODE=0..18` | `cg/cg.html` 全表 | 首屏 MODE=0/2 | `FUN_00414ba4` 391-874 | 0 Likely;1/2 Confirmed;3/5/6/7/9 Unverified;4/8/10..18 Unimplemented |
| 纹理过滤 MAG/MIN/MIP | 无手册页(引擎设定) | 全局 | `FUN_0043cd6c` 46-117 | Confirmed 全局;旧线固定 Linear ❌ |
| X/Y/FX/FR/FC/FEX | 手册只文档化 X/Y | ERIS `FX=@fx*100` 等 | `FUN_00414ba4` 117-125/220-336 | X/Y Confirmed;FX 家族槽位绑定 Unimplemented |
| 多部件层 | 无直接页 | 首屏未出现 | `FUN_00414ba4` 155-156/200/219/985 | 结构 Confirmed;几何 Unimplemented |
| 组/离屏/COPY | `cg/cg.html`/`basic/layer.html` | W0/W3、`COPY2=1 ID=:/W0/` | `FUN_00413dc8` 34-36/51-102;`FUN_00462e40` | 机制 Confirmed;COPY 时序 Unimplemented |
| T/TA/TID | `cg/cg.html` | 源有 `T/TID/TA` | `FUN_00414ba4` 905-909 | 公式 Confirmed;TID 配对 Likely |
| 文本 | `font/font.html` | FONT 3 次 | 内部绘制种类 98/100 | 模型 Confirmed;栅格化 Unimplemented |
| 颜色空间 | 无 | 未采集截图 | 全树无 `D3DRS_SRGBWRITEENABLE` | Likely:gamma 空间混合 |

### 3. ERIS 首屏 census(实测)

- frame 0(初始化):1,290,326 命令;`CG` 30,668、`CGACT` 280、
  `CGEND` 1,183、`CGINFO` 606、`FONT` 3、`FONTINFO` 1。
- `CG` 参数:`ID` 30,668、`LINT` 29,068、`E` 1,058、`FILE` 552、
  `Z` 553、`MODE` 1,016、`X/Y` 1,023、`A` 36、`SX/SY` 13。
- 取值:`A` 256×34/0×2;`E` 0×1025/1×33;`MODE` 0×1015/2×1。
- 首个有可见层的逻辑帧:272 图层、34 visible、15 pass
  (1 可执行复制 + 11 图像加工 + 2 组离屏 + screen;screen 只含主窗口 W0)。
- 68 个按钮快照的复制源是已删除临时层 → 帧内不可重放,图层标
  `Unimplemented`;完整复制历史在 `GfxState::copies()`。

### 4. 回归(原始结果)

```text
$ cargo test -q -p yu-gfx          # YURIS_REQUIRE_SAMPLES=1,样本=save/data/ysbin
8 passed (lib) / 5 alpha / 9 blend_table / 1 eris_first_screen (12.8s)
6 filter / 6 group / 11 layer_order / 2 screen / 6 transform / 4 transition
1 display_list_golden — 全部 ok,0 failed

$ cargo test -q -p yu-rt --lib --test fixture --test golden
38 + 38 + 3 passed,0 failed

$ cargo test -q -p yu-rt --release --test corpus --test corpus_golden
9 + 4 passed,0 failed(21.8s + 0.2s;DomainCall 带参不改变 VM 状态)

$ cargo test -q -p yu-trace        # 带 YURIS_SAMPLE_ERIS_YPF
2 + 7 passed,0 failed(real_sample 116.8s)

$ cargo clippy -q -p yu-gfx --all-targets
yu-gfx: 0 warning;yu-rt 仍是既有 5 条基线 warning(无新增)

$ grep -rn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src   # 0
$ grep -rn "println!|eprintln!|dbg!" crates/yu-gfx/src           # 0
$ grep -rn "HashMap" crates/yu-gfx/src                          # 0(全部 BTree)
```

- 确定性:同一输入连续两次 `to_json`/`to_display_json` 逐字节一致;
  `eris_dump` 连跑两次输出 diff 为空。
- 首屏 golden 使用 `UPDATE_GFX_GOLDEN=1` 生成,默认测试比对字节。

### 5. 未实现/挂账(全部有 DIV 编号)

- MODE 3/5/6/7/9(状态有、辅助纹理/运算未取证);MODE 4/8/10..18
  (着色器本体未定位)。
- `MID` 掩码;`FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/
  FQU/FID/F` 的槽位→偏移;多部件层几何。
- `CGACT[COPY/COPY2]` 的上一帧时序/区域;`RECTPAINT*`/`SIZE*`/`ROTATION`/
  `MONO`/`BLUR*`/... 图像加工。
- 文本字形栅格化/行距/描边/窗口定位。
- 子窗口(`WINDOW[NO=1..3] CGID`)的叠放/位置/可见性:Screen 只合成主窗口
  `:/W0`,子窗口只出 Offscreen pass(交 window/host 会话)。
- 颜色空间/默认清屏/组无 SX/SY 时的离屏尺寸:均为 Likely/Open。

### 6. 给后端的契约摘要

- **只执行 `CompositorFrame.passes`**;`status.executable=false` 的 op
  必须跳过,不得近似。pass 顺序 = 纹理复制/加工 → 组离屏(子先父后)
  → screen。
- **Alpha**:`Layer.alpha` 原值 0..256;固定管线 `min(A,255)/255`,
  着色器 `A/256`;不要在线性空间混合(`color_space=gamma_encoded_srgb8`)。
- **Blend**:`json::blend_table_json()` 给出 19 模式的 D3D9 状态;
  0/1/2 可执行,3/5/6/7/9 状态可下发但默认跳过,4/8/10..18 跳过。
- **Filter**:`frame.filter.mag/min/mip` 是全局采样器设置,不是逐层。
- **Group**:先 `Offscreen` pass,再把组纹理按 `layer.alpha/blend` 合成;
  组 `E=0` 只影响父层合成,不影响组内渲染/COPY。
- **窗口**:Screen pass 只合成主窗口 `:/W0`(手册 `WINDOW[NO=0] CGID`);
  子窗口 `:/W1..W3` 只产出 `Offscreen` pass,窗口叠放/位置归 host(后续)。
- **Transition**:`from/to` 双纹理 + 已算好的 `fade`;`to=None` 跳过。
- **Transform**:X/Y 是引擎 i64;FX 家族原值在 `transform.filter`,
  当前 Unimplemented;后端不得自行解释槽位。
- **Copy/Image**:帧内只含可执行项;`GfxState::copies()/images()` 是审计
  历史,不是执行清单;`Snapshot` 纹理在 COPY 语义关闭前无效。

### 7. 环境勘误

任务书给的 `YURIS_SAMPLE_ERIS_YSBIN=<side-repo>/workbench/project-eris/data/ysbin`
已不存在(E3 已记语料迁到 `project-eris/save/data`)。本次用
`<side-repo>/workbench/project-eris/save/data/ysbin`;`data.ypf` 仍在
`<side-repo>/workbench/project-eris/data.ypf`。测试缺样本时:
`YURIS_REQUIRE_SAMPLES=1` 显式失败,否则打印 `SKIP` 可见跳过。

## 成果 20 (2026-10-20) · gfx 复核修复任务书(验证优先)

- 新增 `docs/tasks/task-f-gfx-repair.md`。
- **文件开头明确警示**:审查报告中的问题可能误读手册/反编译/代码,全部默认“不一定正确”;
  必须先产出 `gfx-verification-ledger.md`(Confirmed/Refuted/Uncertain + 决策),禁止照单修改。
- 核心目标:完美渲染。判定顺序 = 语义(层/顺序/可见性/Alpha/混合/变换/窗口/文本)
  → 显示列表 → 软参考像素 → 官方截图有据容差;本会话只做前两层。
- 审查主张按 A(手册/真实脚本)、B(REA/引擎)、C(硬编码/A5)、D(缺失)、
  E(健壮性/性能)、F(测试真实性)分组,每条附先看哪里;特别标注:
  - 手册“SX/SY 仅组有效”与 ERIS 图像层用法冲突,必须以引擎为准;
  - Z 重排/E 偏移/组 RT/TFACTOR/DIFFUSE 等只有审查主张,需直接 REA 复核;
  - 旧地址 0x42c4a4/0x43d334/0x43d4a4 不可复核,必须重新锚定。
- 决策规则:Confirmed+可见影响才修;Refuted 只勘误;Uncertain 保持 Unimplemented+DIV;
  手册与引擎冲突以引擎为准并留手册勘误。
- 仅文档改动;未执行 git 操作。

## 成果 21 (2026-10-20) · 字体会话任务书 `yu-font`(可并行)

- 新增 `docs/tasks/task-g-font.md`:新建独立 crate `crates/yu-font`,与 gfx/后端会话并行。
- 边界:`yu-font` 只做字体来源解析、字形栅格化、合成粗体/斜体、描边/阴影字形、
  glyph cache/atlas;排版/换行/对齐/ruby/颜色/渐变仍归 `yu-gfx/text` 与后端。
- **Phase 0 取证优先**:先确认官方字体来源(自带 TTF/系统字体/位图字体)、
  官方栅格化路径(GDI/自绘/ClearType)、`FONT` 参数语义、描边/阴影/旋转算法;
  产出 `docs/font-provenance.md`,未取证不写字体选择/样式公式。
- 技术默认:`swash` + `fontdb` + 可选 `rustybuzz` + `tiny-skia` 描边/阴影;
  `fontdue` 作为简化备选;Windows `GdiRasterizer` 仅作参考/可选 feature。
- 验收:确定性 coverage、字形/样式 golden、缺失字体/字形诊断、
  wasm32/Android 编译、示例 PNG 与官方文本截图并排;视觉相似为底线,
  逐像素差异必须归因(字体/AA/描边/取整)并留 DIV。
- 仅文档改动;未执行 git 操作。

## 成果 22 (2026-10-20) · 字体会话改为一次性完整实现(参考游戏配置直出)

- 重写 `docs/tasks/task-g-font.md`:取消“第一版/最小版”式分阶段,
  一次性实现完整字体 + 文本样式管线,并用 ERIS 真实 FONT 配置直接渲染
  PNG contact sheet 供对比;桥接留给后续会话。
- 范围:字体来源解析、字形栅格化、粗体/斜体/AA/颜色/双色/渐变/描边/阴影/旋转/
  DRAWMODE,全部按取证结果实现;排版(换行/对齐/ruby/逐字节奏)只定义接口。
- 硬门禁:Phase 0 必须先完成字体来源/引擎栅格路径/FONT 参数/官方截图取证,
  产出 `docs/font-provenance.md`;未取证项显式 Unimplemented,禁止猜测。
- 参考渲染器 `examples/render_reference.rs`:读取参考游戏 FONT 配置(编译产物扫描,
  宏展开后才有),生成单字/整行/描边/阴影/渐变/旋转/粗体样例,输出 PNG + JSON,
  与官方截图并排并给出 diff 统计。
- 并行:`yu-font` 为新 crate,不改 yu-gfx/yu-rt/后端;REA 单会话。
- 仅文档改动;未执行 git 操作。

## 成果 F1 (2026-10-20) · yu-gfx 复核修复:验证台账 + 14 项 Confirmed 修复

> 会话 F。**先复核、后修复**:先产出 `docs/gfx-verification-ledger.md`,
> 每条审查主张重新取证(A/B/C 共 28 条:C=19 / R=7 / U=2),未进台账不改代码。
> 零 git 写操作;旧线工作区未动。会话 G(`yu-font`)并行,本会话未触碰其文件。

### 1. 台账结论(详见 `docs/gfx-verification-ledger.md`)

- **Confirmed 19**:修复 14(A1–A4/A6/A7/B11/B12/B13/C1–C5),
  证据成立但保持现状 5(A5 CGINFO LET、A9 MODE 3/5/6/7/9、B6 辅助纹理、
  B9 清屏、B10 颜色空间)。
- **Refuted 7**(审查读错):
  - B1“每帧按 Z 重排是错的”:引擎链表确实不排序,但可见顺序=Z 升序;
    同 Z 创建序是 DIV-GFX-014 的确定性决策。
  - B2“E 可能是 +0xa4”:`+0x89`=E、`+0xa4`=层种类(1 图像/2 组/0 死),
    由 `0x413e34/0x413e47`、`0x42c400`、`0x40a38c`、`0x40e02c`、`0x43d4bc` 确认。
  - B3“+0x61c/+0x90 可能不是 RT/纹理”:`0x413f6f-0x413fab` 明确
    `SetRenderTarget(+0x61c)`/`GetSurfaceLevel(+0x21c)`。
  - B4“TFACTOR 只在 0/3/7”:实际集合 `{0,1,2,3,5,6,7,9,100,98}`
    (`FUN_00414ba4` 行 144-154);3/7 只是分支内覆盖为白。
  - B7“A 被 `in_stack_00000010==0` 清零”:调用点 `0x413f29` 恒传 1,
    清零分支不属于图层遍历。
  - B8“X/Y 可能 /100/16.16”:ERIS 的 X/Y 是像素值;`/100` 只作用于
    `+0x1b0..+0x1d8` 滤镜字段。
  - C6“域分类用前缀”:属路由启发式,不是可见语义错误。
- **Uncertain 2**:A8(`CG` 无 FILE 的 get-or-create 未重新锚定,保持
  DIV-GFX-016)、B5(顶点 DIFFUSE RGB 未还原,新增 DIV-GFX-019)。
- **B12 地址失效**:`0x42c4a4/0x43d334/0x43d4a4` 在本地反编译树不存在;
  改用 `0x413dc8`(遍历/TID/MID)、`0x4141e4`(路径规范化)、
  `0x414559`(哈希查表)、`0x43d4bc`(析构)、`0x42f57d`(SWAP)、
  `0x42f809`(CGACT 动作分派)。

### 2. Confirmed 修复(证据 → 代码 → 回归)

| 项 | 证据 | 代码 | 回归 |
|---|---|---|---|
| A1/B13 `TID=""` | `es_Button.yst:4526/4540`;`FUN_00413dc8 0x413e7b`:空串跳过配对 | `state.rs` TID 分支:空串清 `tid=None`;`op_for_layer` 无 TID 输出普通层 | `parity_fixes::empty_tid_clears_pairing_instead_of_w0` |
| A2 图像 SX/SY | 手册称仅组有效,但 `es_ERIS.yst:579/600` 用 `FILE="" SX/SY` 画黑幕;`FUN_00465730/0043772c/00408724` | `LayerKind::Image{size}`;`SolidBlack1x1` 按尺寸可执行,资源图+尺寸标 `gfx.image.size.unsupported` | `parity_fixes::solid_black_with_sx_sy_draws_at_that_size`、`asset_with_sx_sy_stays_unimplemented`;ERIS 首屏断言 `:/W1/DBG` 260×600、`:/W0/FONTL2` 233×27 |
| A3/C2 WINDOW | `window/window.html`;`es_First.yst:97/98/140`、`es_ERIS.yst:22` | 消费 `Domain::Window`;`CompositorFrame.windows`;Screen 用 `WINDOW[NO=0].CGID/E/A`;`WINDOWEND NO=0` 拒绝;子窗口归 host | `parity_fixes::window_cgid_selects_main_layer_set_and_e_zero_hides_screen`、`screen.rs` |
| A4 TEXT | `text/text.html` ID/CLEAR;`es_Text.yst:154` | `yu-rt` 保存 `text_layer_id`;`WORD`/`_` 事件带 ID;`CLEAR=1` 清文本缓冲 | `yu-rt fixture::text_id_sets_following_run_id_and_clear_empties_buffer` |
| A5 CGINFO/FONTINFO/WINDOWINFO LET | 手册三页 + 125 处真实使用 | **不修**:需脚本执行期同步查询 ABI;保持 Unimplemented | 新增 DIV-GFX-020 |
| A6 CGACT | `cg/cgact.html` 例 3/4/21/22;真实 ysc 表 70 参数 | `ImageOp.sx/sy`;动作表补 `RESIZE/TRIM/ROTATION2/TRANSITION2/XOR/AND/BLUR2..6`;像素仍 Unimplemented | `parity_fixes::cgact_size_keeps_sx_sy_and_resize_trim_are_recognized` |
| A7 FONT NO/BNO | 手册 `font/font.html` NO;脚本 `FONT[BNO=1]` | `TextStyle.font_no/font_bno`;BNO 值保留并挂 DIV | `parity_fixes::font_no_and_bno_are_recorded` |
| C1 视口 | `yscfg` 800×600 | `from_tables` 在宽高非 0 时覆盖配置 | `eris_first_screen` 以 1×1 占位构造后断言 800×600 |
| C4 T 默认 | `FUN_00465730 +0x82=0x100` | `Transition::default().t=256` | `parity_fixes::transition_defaults_to_t_256` |
| C5 psb | `FUN_0040c9b4` 行 383 `.psb` | `ImageFormat::Psb` | `filter::probe_order_from_eris_yscfg`、真实 yscfg 测试 |
| D15/D16 主组/图像子层 | `FUN_00413dc8 0x413e1a` 任意节点先递归子层 | 主组缺失不再猜所有顶层;图像层子层按后序扁平输出 | `parity_fixes::image_children_are_drawn_before_the_parent` |
| E1/E3/E9 | 诊断无界/粘滞/非法 JSON | 每帧清空+`count`;`texture_status`/`param_status` 重算;`128.0`/`null`/`-0.0` | `diagnostics_are_per_frame_and_count_duplicates`、`mode_update_recovers_executable_status`、`persistent_unsupported_attributes_keep_layer_unimplemented`、json 单测 |
| E2/E6/E7/E8/E10/E5 | 日志无界/递归/丢层/缺表/O(n²)/比较器分配 | 日志上限 16,384+`dropped_ops()`;显式栈;图像子层;必需命令 fail-fast;动作集合一次扫描;排序去分配 | `deep_path_and_deep_group_nesting_do_not_recurse`、`missing_required_graphics_command_fails_validation`、全量测试 |

### 3. 修复前后原始证据

**A1 旧行为**(`git show HEAD:...state.rs:1531`):

```text
"TID" => match value.as_bytes() {
    Some(b) => {
        let p = path::normalize(b, None, false).path;
        layer.transition.get_or_insert_with(Transition::default).tid = Some(p);
```

`normalize(b"")` = `:/W0`,因此旧代码把 `TID=""` 变成整屏 W0 过渡;
新回归断言此时**不得**出现 `Op::Transition`,`tid=None`(测试通过)。

**E9 旧/新**:

```text
旧: fmt_f64(128.0) == "1.3e2"(非法精度);NaN → "NaN"(非法 JSON)
新: fmt_f64(128.0) == "128.0";NaN/±Inf → null;-0.0 → "-0.0"
```

**Golden 前后**(ERIS 首屏):

```text
旧:"name":"other","code":5           新:"name":"psb","code":5
旧:"kind":{"kind":"image"}           新:"kind":{"kind":"image","size":{"width":260,"height":600}}
旧:无 windows 字段                    新:"windows":[{"number":0,"layer_set":":/W0",...}]
旧:20620 B(跨帧累计诊断)             新:19822 B(本帧诊断;帧 3 diag=0)
```

### 4. 测试真实性补强(F 类)

- ERIS 首屏从 `>100/>10/>=2` 改为精确 census:272 层 / 34 visible /
  15 pass / 44 op / 12 texture pass / W0 30 op / W1 1 op / screen 1 op;
  断言真实 `MODE=2` 层 `:/W0/SPLDST2`(A=0、E=0、SolidBlack1x1)、
  E=0 共 238 层、主窗口 `:/W0`、帧内诊断为空。
- golden 新增 `tests/golden/README.md`:样本 `ysc.ybn` sha256
  `741d4caf…`、`yscfg.ybn` sha256 `f451d3a7…`、生成命令。
- blend JSON 与 `fixed_state()` 逐 stage/arg/op/blend 字段交叉验证。
- filter 测试读取真实 `yscfg.ybn`(500 样本 gate);`Other(5)` 改为 `Psb`。
- SWAP2 增加 X/Y 断言;COPY2/属性不复制继续执行断言。
- 深路径 600 级 / 深组嵌套不递归爆栈。

### 5. 未完成 / blocked

- A5 `CGINFO/FONTINFO/WINDOWINFO` 的 `LET`:保持 Unimplemented + DIV-GFX-020。
  原因:必须脚本执行期同步写 VM 变量,而 gfx 事件是 tick 后异步消费;
  需要 VM↔gfx 同步查询 ABI,不是 gfx 单层可修。
- A8 `CG` 无 FILE:未重新锚定 CG 处理体,保持 DIV-GFX-016。
- A9 MODE 3/5/6/7/9、4/8/10..18:DIV-GFX-001/002 不变。
- B5 顶点 DIFFUSE RGB:DIV-GFX-019;可执行模式 0/1 的 DIFFUSE 只进 alpha
  stage,不影响可见 RGB。
- E4 每命令分配优化:记录为后续项,本会话未改(不改变语义)。
- 像素/官方截图对拍:属后端会话;本会话只到显示列表层。

### 6. 给后端的契约变化(只列已确认)

1. `LayerKind::Image { size }`:`SolidBlack1x1` + `size` 必须铺满该矩形;
   资源图 + size 的图层 `status.executable=false`。
2. `Op::Image` 新增 `sx/sy`;新增动作名 `resize/trim/rotation2/transition2/
   xor/and/blur2..blur6`;所有 ImageOp 仍 Unimplemented。
3. `CompositorFrame.windows`(按 NO):`NO=0` 的 `layer_set/visible/
   alpha/x/y/size/z`;Screen ops 为空表示主窗口隐藏。
4. `FrameDiag.count`:同键诊断合并计数;诊断**每帧重置**。
5. `ImageFormat::Psb`(code 5,`.psb`)。
6. `TextStyle.font_no/font_bno`;`VmEvent::Text.id` 现在可能非空
   (`TEXT[ID]`)。
7. `LayerStatus` 由纹理级/属性级重算;MODE/FX/MID 持久字段不会因一次
   无参数更新被洗白。
8. `GfxState::dropped_ops()`:日志上限丢弃计数;`gfx.log.truncated`。

### 7. 验收原始输出

```text
$ export YURIS_REQUIRE_SAMPLES=1 YURIS_SAMPLE_ERIS_YSBIN=.../save/data/ysbin
$ cargo test -p yu-gfx
10 lib + 5 alpha + 10 blend_table + 1 display_list_golden + 1 eris_first_screen
+ 7 filter + 6 group + 11 layer_order + 15 parity_fixes + 2 screen
+ 6 transform + 4 transition + 1 doc = 79 passed; 0 failed

$ cargo test --workspace --no-fail-fast      # 同一套样本 env
352 passed; 0 failed(exit=0)

$ cargo clippy -p yu-gfx --all-targets
yu-gfx: 0 warning(yu-rt 既有 5 条基线 warning 不变)

$ grep -rEn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src   # 0
$ grep -rEn "println!|eprintln!|dbg!" crates/yu-gfx/src           # 0
$ grep -rEn "HashMap" crates/yu-gfx/src                          # 0
```

- `cargo fmt -p yu-gfx -- --check` 通过。
- 确定性:`json::to_display_json` 两次逐字节一致(既有测试 + golden)。

### 8. 并行纪律

- 会话 G 期间新增 `crates/yu-font`、`docs/tasks/task-g-font.md` 并改
  `PROGRESS.md`/`Cargo.lock`;本会话只追加本节,未改其文件。
- 本会话未执行任何 git 写操作。

## 成果 23 (2026-10-20) · 并行任务书:官方 trace 对拍(H)

- 新增 `docs/tasks/task-h-trace-oracle.md`。
- 目标:把 `yu-trace` 从“内核自证”推进到“官方 trace 对拍”。
- 范围:边界①vfs/②vm;③帧描述/④平台命令留给后端会话。
- 关键:producer-neutral schema(v3)、按 sub/kind/frame 投影对齐、官方采集器
  (WOW64/调试器/屏幕级 fallback,注明等级)、tick_at 时钟/codec/aux 回放闭合。
- 不改 yu-rt/yu-gfx;采集器放 tools/ 或仓库外;语料/截图不入库。

## 成果 24 (2026-10-20) · 并行任务书:渲染后端(I)

- 新增 `docs/tasks/task-i-backend.md`。
- 新建 `yu-assets` + `yu-backend-soft` + `yu-backend-wgpu`;不改 gfx/rt/font。
- 顺序:纹理管线(YPF/Dir、扩展名探测、解码、缓存)→ 软后端确定性 PNG →
  headless 首帧 → wgpu 同帧对比。
- 文本由 G-FONT 并行提供;本会话遇到 TextRun 跳过并诊断,不阻塞第一帧。
- 验收:first_frame.png、soft vs wgpu diff、缺失资产/跳过项清单、无硬编码屏幕尺寸。
- 未执行 git 操作。

## 成果 25 (2026-10-20) · 复审并修正 trace/backend 任务书范围

- 复核 `yu-trace` 实际实现:①vfs、②vm、roundtrip/diff/replay 已完成;
  ③`frame_description`、④`platform_call` 只是 schema 2 占位,写入/解析均显式拒绝;
  官方 producer 不存在。此前提示词把①/②也当成待做,范围不准,已修正。
- `task-h-trace-oracle.md`:明确“不重写①②;本会话补③帧描述 + 官方 producer +
  producer-neutral schema/投影 + 回放闭合;④保持拒绝”。
- 复核后端:仓库尚无 `yu-assets`/`yu-backend-*`,是从零新建;gfx(F)与字体(G-FONT)
  正在并行写。`task-i-backend.md`:改为只消费 gfx 公开类型/最小适配层,
  第一帧输出 `target/tmp/first_frame.display.json` 供 trace③ 文件协作。
- 仅文档改动;未执行 git 操作。

## 成果 F2 (2026-10-20) · INFO/LET 同步查询闭环 + 未实现模块 TODO

> 会话 F 续。目标:推进上一轮最大的 1:1 缺口
> `CGINFO/FONTINFO/WINDOWINFO` 的 `LET`(A5/D2),并给所有未实现模块
> 落可检索的 TODO;不猜未取证语义。

### 1. 架构:同步 GraphicsHost

- `yu-rt` 新增 `event::GraphicsHost`:
  - `apply_event(&VmEvent)`:FrameStart/FrameEnd/Text/Graphics|Window DomainCall
    在**产生时**同步送入宿主;
  - `query(command, args) -> Result<Value, Diag>`:脚本执行期查询。
- `Vm::set_graphics_host/take_graphics_host`。安装后:
  - `emit` 同步转发图形事件(Command 热路径不转发);
  - Graphics/Window 域命令不再重复挂 `rt.cmd.unimplemented`;
  - **调用方不得再把同一批事件 drain 后喂给同一 GfxState**(已写进文档)。
- `yu-rt` 新增 `HandlerKind::Cginfo/Fontinfo/Windowinfo` 与 `op_info`:
  - 参数名按工程 ysc 表(opcode→参数表)解析,不写死槽号;
  - `LET` 用 `eval_lvalue` 求值;查询结果用 `write_target` 写回;
  - 无宿主/字段未取证 ⇒ 只诊断,不写 LET(不猜 0)。

### 2. yu-gfx 查询实现

- 新增 `GfxHost`(`Rc<RefCell<GfxState>>`),与调用方共享状态:
  tick 后 `state.borrow().frame()`;tick 期间 VM 同步 `apply_event`。
- `GfxState::query_info`:
  - **CGINFO**:EXIST / E / A / X / Y / Z / MODE / T / TA / TID;
    SX/SY 对组(含 W0 视口)、显式尺寸层、`SolidBlack1x1` 可答。
  - **FONTINFO**:当前 TextStyle 的 COLOR / COLOR2(已设置)/
    SX / SY / BOLD / ANTIALIASING / DRAWMODE / NAME(无 NO 索引)。
  - **WINDOWINFO**:EXIST / E / X / Y / SX / SY(主窗口默认 yscfg 尺寸)。
  - 多字段同时置位:顺序未取证 ⇒ `gfx.info.multi`,不写 LET。
  - 资源图尺寸:需要资产元数据 provider ⇒ `gfx.info.asset_size`,
    不写 LET(DIV-GFX-024)。
  - COLOR/ONMOUSE*/CAPTION/ACTIVE/FULLSCREEN 等 ⇒
    `gfx.info.unsupported`(DIV-GFX-026)。
- `Transition` 新增 `tid_raw`:引擎 `+0x4c` 保存脚本原文,
  `CGINFO[TID]` 返回原文而不是规范化路径;JSON 同步输出 `tid_raw`。

### 3. 回归(原始结果)

```text
$ cargo test -p yu-gfx --test info_query
7 passed; 0 failed
  - cginfo_answers_documented_attributes
  - cginfo_group_and_solid_black_sizes_are_known
  - cginfo_missing_layer_only_answers_exist
  - cginfo_multiple_fields_are_rejected_not_guessed
  - fontinfo_answers_current_style_fields
  - fontinfo_unset_fields_are_not_fabricated
  - windowinfo_answers_main_geometry

$ cargo test -p yu-rt --test fixture info_query_writes_let_with_synchronously_applied_state
1 passed; 0 failed
  - 宿主先看到 CG DomainCall,再收到 CGINFO 查询;
  - 查询值 42 写回 @v;LET 自身不进入查询参数。

$ cargo test -p yu-gfx --test eris_first_screen
2 passed; 0 failed(约 25s,两次真实 ERIS boot)
  - 旧模式 census:272 层 / 34 visible / 15 pass / 44 op
  - 同步宿主模式 census:**完全一致**;查询日志出现 CGINFO + WINDOWINFO
```

### 4. 未实现模块 TODO

- 新增 `docs/gfx-unimplemented-todo.md`:DIV-GFX-001..040 的状态、
  当前行为、解除条件、代码标记位置、推进顺序。
- 代码内新增 `TODO(DIV-GFX-xxx)`:
  - `layer.rs`:MODE 3/5/6/7/9、4/8/10..18;
  - `state.rs`:MID、滤镜、COPY、图像加工、资源图尺寸、组 RT 清屏;
  - `op.rs`:CopyOp/ImageOp 恒 Unimplemented;
  - `frame.rs`:清屏/颜色空间;
  - `text.rs`:字形栅格化/排版;
  - `transition.rs`:配对缺失/推进者;
  - `path.rs`:IDNO 后缀;
  - `yu-rt handlers.rs`:TEXT 显示节奏属性。
- 新增 DIV-GFX-024(资源图 `CGINFO SX/SY` 需资产元数据)、
  DIV-GFX-025(多字段写回顺序)、DIV-GFX-026(像素/鼠标/窗口 host 字段)、
  DIV-GFX-027(IDNO)、DIV-GFX-028(图像子层目标 RT)、
  DIV-GFX-029(pass 交错序)、DIV-GFX-030(`TIDNO`)、
  DIV-GFX-031(`CGACT ID3/ID4/Z2/BNO`)、DIV-GFX-032(SWAP 模式对应)、
  DIV-GFX-033(TEXT 节奏/OUTPUTMODE)、DIV-GFX-034(文本跨帧持久/CLEAR)、
  DIV-GFX-035(FONT NO/BNO/字体解析)、DIV-GFX-036(FONTINFO 字体列表)、
  DIV-GFX-037(WINDOW host 属性/主窗口 A/SX-SY)、DIV-GFX-038(INFO 缺层语义)、
  DIV-GFX-039(CGACT RASTER1/FSX/...)、DIV-GFX-040(TEXTINFO)。
- 复查后确认:**上一版 TODO 不是全部**;补入上述 030..040 后仍不能保证
  “宇宙级完整”,但 gfx 已知缺口已全部有编号/状态/解除条件。

### 5. 验收

```text
$ cargo test -p yu-gfx
10 lib + 5 alpha + 10 blend_table + 1 display_list_golden + 2 eris_first_screen
+ 7 filter + 6 group + 7 info_query + 11 layer_order + 15 parity_fixes
+ 2 screen + 6 transform + 4 transition + 2 doc = 88 passed; 0 failed

$ cargo test --workspace --exclude yu-trace --exclude yu-backend-soft --no-fail-fast
328 passed; 0 failed

$ cargo test -p yu-rt
98 passed; 0 failed

$ cargo clippy -p yu-gfx --all-targets
yu-gfx: 0 warning(yu-rt 既有 5 条基线 warning 不变)

$ cargo fmt -p yu-gfx -- --check     # 通过
$ grep -rEn "winit|wgpu|web-sys|wasm-bindgen" crates/yu-gfx/src   # 0
$ grep -rEn "println!|eprintln!|dbg!" crates/yu-gfx/src           # 0
$ grep -rEn "HashMap" crates/yu-gfx/src                          # 0
```

- **全 workspace 当前被并行会话的中间态阻塞**(本会话未改这些 crate):
  - `yu-trace`:golden schema 2 vs 3(`synthetic_trace_matches_golden`);
  - `yu-backend-soft`:`render.rs`/`texture.rs` 编译错误(借用/移动)。
  按“活跃写者只读”纪律等待其完成。`--exclude yu-trace --exclude
  yu-backend-soft` 下 328 项全绿。

### 6. 契约变化

1. `yu_rt::event::GraphicsHost` + `Vm::set_graphics_host`;安装后事件同步消费,
   不得重复喂。
2. `yu_gfx::GfxHost`(Rc<RefCell<GfxState>>)供宿主/测试使用。
3. `Transition.tid_raw`(JSON `tid_raw`):原始 TID 字符串。
4. INFO 查询失败时的诊断码:`gfx.info.multi/missing/unsupported/asset_size`;
   失败时 VM 不写 LET。
5. 资源图 `CGINFO SX/SY` 仍不可用,需资产元数据 provider(DIV-GFX-024)。

### 7. 仍未 1:1(按优先级)

1. 资产元数据 provider → `CGINFO SX/SY`(ERIS 按钮布局最大缺口)。
2. 官方引擎受控脚本/截图 → 过渡无配对、COPY 时序、组 RT 清屏/尺寸、
   MODE 0/1/2 像素对拍。
3. MODE 3/5/6/7/9 辅助纹理;4/8/10..18 着色器。
4. 图像加工/多部件/MID/顶点 DIFFUSE。
5. 文本栅格化(与 `yu-font` 会话合流)。

## 成果 G1 (2026-10-20) · yu-font:字体 + 完整文本样式 + ERIS 配置直出

> 会话 G-FONT。范围:新建 `crates/yu-font`、`docs/font-*`、示例与测试;
> 未改 `yu-gfx` / `yu-rt` / 后端 / 根 `Cargo.toml`。

### 1. 取证(硬门禁)

- `docs/font-provenance.md`:字体来源/栅格路径/FONT 参数/参考图证据表。
- 参考工程 `data.ypf` 1,407 条,0 个字体扩展;唯一 `.psb` 是 E-mote 模型 →
  **无自带字体**。
- 引擎启动 `FUN_00403afc` @ `0x403afc` 用 `EnumFontFamiliesExA`
  (`lfCharSet=SHIFTJIS_CHARSET`) 枚举,回退表 `0x807008..0x807044`:
  `ＭＳ ゴシック` → `標準ゴシック` → `ＭＳ Ｐゴシック` → `ＭＳ 明朝` → `標準明朝`。
- 栅格路径:GDI `CreateFontIndirectA` + `TextOutA` 到 8-bit DIB
  (`FUN_00445421` @ `0x445421`、`FUN_004450c0` @ `0x4450c0`);无
  `GetGlyphOutline`/`ExtTextOut`/`DrawText`/`AddFontResource`/`ID3DXFont`,
  非 ClearType,非自绘 outline。
- FONT 参数存储/clamp 由 `0x444428..0x444b1c` 逐条钉死;默认值由
  `FUN_00464fac` @ `0x464fac` 钉死(SX/SY=20,AA=1,COLOR=COLOR2=0xffffff,
  GRADATION=1,ROTATION=0,DRAWMODE=0,BOLD=0)。
- 42 个编译后 FONT 组;参考配置:es_text g1517 `12×12/0x999999/AA=0`,
  g1698 `BOLD=1/AA=1`,g1606 `COLOR=#ffffff COLOR2=#ffff00`,
  es_system g132 `14×15/BOLD/DRAWMODE=1`。
- 官方参考图:用户提供 `official_title_user.png`(1200×900);`スタート`
  墨迹框 54×16;不启动引擎自动截图(用户指示)。

### 2. 实现

- `FontSource`:fontdb + 自带字节优先;引擎默认 family 回退;`NAME`/`NO`
  解析;缺失显式 `font.missing`,无静默 fallback;wasm 禁止系统枚举。
- 栅格:swash/skrifa outline → 8-bit coverage;SX/SY 非等比用 transform;
  bold 优先 bold face,否则固定 `embolden`;italic face 优先,否则 shear;
  AA 灰度/阈值 128;ClearType 显式 Unimplemented。
- 样式:COLOR/COLOR2/GRADATION、DRAWMODE、outline(round 膨胀)、shadow
  (整数偏移,无模糊)、ROTATION(octant×45°)、kerning(legacy kern)、
  `StylePolicy::Strict/Reference` 证据门禁。
- 固定合成顺序:base → bold/italic → outline → shadow → color/gradient →
  rotation → 调用方 DRAWMODE 合成。
- 有界确定性 LRU cache(条目+像素双上限)。
- 排版只定义 `TextLayout` 接口;`render_run` 是参考单行 helper,拒绝换行。

### 3. 验收

```text
$ cargo test -p yu-font
14 lib + 1 eris_config + 5 glyphs + 7 source_config + 1 style_golden
+ 12 styles + 1 doc = 41 passed; 0 failed

$ cargo check -p yu-font --target wasm32-unknown-unknown     # pass
$ cargo check -p yu-font --target aarch64-linux-android     # pass
$ cargo clippy -p yu-font --all-targets                     # 0 warning
$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-font/src   # 0
```

- 确定性:同 `(font,char,style)` 两次 coverage/RGBA hash 逐字节一致。
- 字形 golden:ASCII/假名/汉字/标点 coverage hash + metrics
  (`crates/yu-font/tests/golden/glyphs.txt`)。
- 样式 golden:粗体/斜体/AA off/描边 1,3/阴影/渐变/旋转/非等比
  (`crates/yu-font/tests/golden/styles.txt`)。
- 缺失字体/缺字形/坏字体 → `font.bad_face`/`font.glyph_missing`,不 panic。

### 4. 参考渲染器

```text
$ cargo run -p yu-font --example render_reference
== samples: 19 written to target/tmp/font-ref/samples.json
== contact sheet: target/tmp/font-ref/contact_sheet.png
== official diff: iou=0.537 max_channel=178 mean=73.88
```

- contact sheet 真读 ERIS 编译产物;运行时打印 42 个 FONT 组与 5 个
  参考槽位(见 `target/tmp/font-ref/render.log`)。
- 官方 diff:best size=15 bold MS Gothic;official ink 227,ours 211,
  intersection 153,fp 58,fn 74,max channel 178,mean 73.88;归因 GDI vs
  swash outline/AA + 未建模的浅色 rim。
- 性能(release):1000 个可渲染 CJK × 2 字号 × 1px outline
  - 12px:first 1799.87 ms (1.800 ms/glyph),cache hit 0.38 ms
    (0.0004 ms/glyph),848 KiB
  - 24px:first 2054.27 ms (2.054 ms/glyph),cache hit 0.42 ms
    (0.0004 ms/glyph),2393 KiB
  - 命中 1000/1000,miss 0;cache entries=1000。

### 5. 未解决/需 REA 复核

- `GRADATION` 方向映射未取证(默认 1 解释为竖直上→下;Strict 只允许纯色)。
- `ROTATION` 0..8 的渲染语义未观察到(octant×45°,Reference only)。
- `BNO` 字体列表分支 `0x444ce7` 未解析(仅保存值)。
- GDI `lfHeight` 与 SX/SY 的精确映射、合成粗体像素量未完全取证。
- `FONT[NAME]` 找不到时引擎的回退/报错行为未取证(本 crate 显式失败)。

## 成果 H1 (2026-10-20) · yu-trace schema v3 + 帧描述 + producer-neutral 对齐 + 官方屏幕级 oracle

> 任务书 `docs/tasks/task-h-trace-oracle.md`。只改 `crates/yu-trace` 与 `tools/`;
> 未改 `yu-rt`/`yu-gfx`/字体/后端;无 git 写操作。

### 1. schema v3(producer-neutral)

- `SCHEMA_VERSION = 3`;v2 文件不迁移、直接 `trace.schema`。
- header 新增:
  - `producer`: `kind`(kernel/official)、`name`、`version`(official 侧是采集器版本)、
    `sample_sha256`;
  - `clock`: `mode`(none/tick/tick_at/mixed) + 逐 tick `now_ms` 序列。
- 跨 producer 语义比较只投影 `codec` + `boot` + `input` + 双方都存在的
  `producer.sample_sha256`;`kernel`/`tables`/producer 名称版本/`clock` 不参与
  跨 producer 整行比较(见 `HeaderCompare`)。
- `seq` 仍为同一 producer 内唯一序;跨 producer 对齐改用
  `(sub, kind, frame, ordinal)`。

### 2. ③ frame_description(真实记录)

- `sub=frame` / `kind=frame_description` 从占位变为真实记录,payload:
  `frame, level(event|screen), display_sha256, pass_count, op_count,
  executable_ops, skipped_ops, display`。
- 调用方输入结构 `FrameDescriptionInput`;本 crate 不硬绑 `yu-gfx` 字段:
  - 传 `display` JSON → 解析、规范序列化、计算/校验内容寻址;
  - 只传摘要 → `display=null` 的摘要级记录;
  - 坏 hash 格式 = `trace.parse`,display 与摘要不一致 = `trace.schema`;
  - 同一输入两次写入逐字节一致。
- ④ `platform/platform_call` 仍显式拒绝。

### 3. 投影/对齐 diff

- 新增 `diff_aligned`/`diff_aligned_logs` 与 `diff_traces` example:
  - 逻辑键 `(sub, kind, frame, ordinal)`;
  - `Projection` 按 sub/kind/frame 过滤;
  - `HeaderCompare::{Ignore, CodecSample, Semantic, Strict}`;
  - `payload_fields` 显式选择可比 payload 顶层键(用于 official 没有 args 的场景);
  - 首分歧报告 `key / expected_line / actual_line / path / expected / actual / context`。
- 原有 `diff_text` 逐行字节严格 diff 保持不变。

### 4. 回放闭合

- `VmTrace` 记录逐 tick 时钟,`finish` 写入 header;`verify_clock` 回放校验。
- `from_header_checked` / `boot_checked` 校验数据源编码与 `header.codec` 一致。
- aux 源与主源共享同一个 `FrameHandle`(`from_recorder_with_frame`)。
- `consumed_input_summary` 只覆盖 VM 实际消费的逻辑帧;`finish_with_timeline`
  使用它。

### 5. 官方 producer

- `tools/official_trace/capture_screen.py`:官方 0.495 `yu-ris.exe` + ERIS
  `data.ypf`,PrintWindow 截取 800×600 客户区,写 schema v3
  `frame/frame_description`(`level=screen`),另写 input timeline 与 manifest。
  **已验证**:启动、截图、trace 解析、与内核屏幕 trace 对齐 diff 均可跑通。
- `tools/official_trace/probe_dispatch.py`:事件级 L1 探针(WOW64 只读调试,
  0.495 分派点 `0x4044db`,任务全局 `0x80171c`,脚本 ID `task+0x3c`)。
  **未打通**:标题画面/点击后 25 s 内 0 命中;延迟布防后
  `ReadProcessMemory(0x400000/0x401000/0x4044db)` 仍返回 error 299
  (ERROR_PARTIAL_COPY)。**未伪造事件级 trace,L1 明确未达成**。
- 屏幕级对齐尝试已按用户裁决撤回:官方标题画面与内核 `first_frame` 不是同一
  逻辑锚点,不能作为渲染分歧证据。见 `docs/divergences/DIV-H-001`
  (status=withdrawn)。

### 6. 验证(原始结果)

```text
$ cargo test -p yu-trace   # YURIS_REQUIRE_SAMPLES=1, 真样本 data.ypf
lib 43 passed; diff 13; equivalence 7; event_trace 14; golden 2;
real_sample 2 (146.85s); roundtrip 7; doc-tests 0; 0 failed
```

```text
$ cargo clippy -p yu-trace --all-targets
yu-trace 自身 0 告警(输出中的 warning 均来自依赖 yu-rt)
```

官方屏幕级采集(原始输出摘录):

```text
$ python tools/official_trace/capture_screen.py --exe <官方 exe> --cwd <工程根> \
    --out target/test-tmp/official/eris_screen --frames 6 --interval 0.4 --warmup 8
window pid=... hwnd=... client=800x600
wrote target/test-tmp/official/eris_screen/official_screen.jsonl (6 frames)
```

事件级探针(原始输出摘录,证明未达成而非静默):

```text
$ python tools/official_trace/probe_dispatch.py --exe <官方 exe> --cwd <工程根> \
    --out target/test-tmp/official/dispatch_hits.jsonl --seconds 15 --click-after 5
base 0x400000 reloc 0x0 hide True
defer breakpoint for 3s
probe 0x44db None err 299
probe 0x400000 4d err 299
probe 0x401000 53 err 299
arm still failing err 299
done timeout hits 0
```

### 7. 未完成/挂账

- ④ `platform_call`:等平台后端,继续显式拒绝。
- 官方事件级 L1:分派探针 read error 299 未解决;下一步先在任务泵入口
  `0x404398` 验证泵是否运行,再处理 WOW64 读内存权限/反调试。
- 官方首屏 vs 内核首帧:**暂停**,锚点不成立,不作一致性结论。

## 成果 M3-SOFT (2026-10-20) · 软后端与首帧 PNG

> 提交 771cf79(`feat(gfx): 修正渲染语义并新增软件后端`)。范围
> `crates/yu-backend-soft`;只消费 `yu-gfx` 公开契约,不改语义层。

### 1. 实现

- `TextureStore`:`yu-vfs::DataSource` 取字节,按 `frame.image_probe_order`
  探测扩展名,`image` crate 解码为 RGBA8;按资产 key 缓存;`SolidBlack1x1`
  内建;`Snapshot`/`Offscreen` 跳过并计数;缺失/坏图产生 `asset.*` 诊断。
- `SoftRenderer`:严格按 pass 顺序;Offscreen 用组 `SX/SY`(未给时视口)+
  `pass.clear`;Screen 用 `frame.viewport` + `frame.clear`;MODE 0/1/2;
  固定管线 alpha `min(A,255)/255`;Point/Linear 采样;gamma 存储值混合;
  `TextRun` 只计数,不栅格化。
- `first_frame` 示例:GfxHost 同步路径 boot ERIS;`take_events()` 只做计数/
  drain,不把同一批事件重复喂给 `GfxState`;输出
  `target/tmp/first_frame.png` + `first_frame.display.json`。

### 2. 首帧实测

- frame 3,800×600,272 层,15 pass,44 op(15 executable / 29 skipped),
  `text=[]`。
- 17 个 `snapshot` 层因 `Op::Copy` 未实现不可渲染;11 个 `Op::Image`
  (`RECTPAINT` 族)+ 1 个 `Op::Copy` 被跳过;`filter.source=placeholder`。
- PNG 已产出;同帧连续两次编码逐字节一致(测试覆盖)。

### 3. 验证

```text
$ cargo test -p yu-backend-soft
10 lib + 5 render + 1 doc = 16 passed; 0 failed
```

### 4. 未完成(全部是 gfx 挂账,不是后端近似)

- DIV-GFX-007 COPY/COPY2 时序与区域;
- DIV-GFX-013 `RECTPAINT*`/`SIZE*`/`RESIZE` 等图像加工像素;
- DIV-GFX-012 文本栅格化/排版(与 `yu-font` 合流);
- DIV-GFX-009 颜色空间、DIV-GFX-008 组 RT 尺寸/清屏。
