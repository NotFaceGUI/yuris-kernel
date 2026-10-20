# 任务书 A · 地基:workspace + yu-log + yu-vfs(M0)

> 会话定位:三个并行会话之一。你独占仓库根 Cargo.toml 与 `crates/yu-log`、`crates/yu-vfs`;
> **不要动** `crates/` 下其它 crate 的目录(它们归并行会话 B/C 所有,workspace 用 `crates/*` glob,
> 各会话建自己的目录即自动入组)。
> 本会话**先行启动**:B/C 依赖你定下的 workspace 骨架与约定。

## 0. 开工前必读(顺序)

1. `<repo>/AGENTS.md`(工作纪律/占位符/样本环境)
2. `<repo>/docs/architecture.md`(不变式 A1–A5;§1 数据源抽象;§8 yu-log)
3. `<repo>/docs/oracle.md`(真值优先级)
4. 旁线笔记 `<side-repo>\docs\format-ypf.md`(ypf 格式事实——**按事实实现,存疑处挂账**)

**参考实现索引(用户授权,提取事实、注明出处、写自己的实现):**
- 旧线 `<repo-old>/crates/yuris-format/src/` 的 ypf 读取与 `tests/sample.rs` 真实样本断言
  ——ypf 条目/压缩/取名的实测期望;
- 旧线 `crates/yuris-vm/src/host.rs` 的 `YpfScriptHost`(按 `$ysbin\yst%05d.ybn` 取脚本的路径约定)。

铁律:真实对照——每条实现依据必须可指认(格式笔记事实/实测数据/REA 地址/参考实现出处),无依据不实现,
显式 `Diag::Unimplemented` 并在 `docs/divergences/` 建档;②A5 零硬编码——游戏路径/条目名/序号
不得写进代码;③禁止任何 git 写操作;④注释/文档只写占位符(真名只在 AGENTS.md);⑤除 `flate2`
(zlib 解压,~/.cargo 已缓存)外零第三方依赖。

## 1. workspace 骨架(先做)

根 `Cargo.toml`:`members = ["crates/*"]`、`resolver = "2"`、`workspace.package`(edition 2021,
license GPL-3.0-or-later)、`workspace.dependencies = { flate2 = "1" }`。
`.cargo/config.toml`(本机文件,已由 `.gitignore` 排除):构建产物默认落仓库根 `target/`。
验证:`cargo check --workspace` 绿(此时只有你两个 crate)。

## 2. yu-log(错误+日志一体包,契约见 architecture §8)

- `Diag { code, severity: Fatal|Recoverable|Unimplemented|Info, boundary: Data|Format|Table|Script|Domain|Platform, where_: Loc, detail, causes }`
- `Loc` 能表达:脚本位置(file/label/pc/cmd)、数据位置(path/offset)、表名。序列化友好(serde 或手工 JSON,自选但**确定性输出**)。
- 错误码命名空间常量模块(`code::VFS_MISSING = "vfs.missing"` 等),码表有单元测试防重复。
- `LogSink` trait + `JsonlSink`(写文件)与 `MemorySink`(测试用);全局/显式注入二选一,**选显式注入**(A3:确定性,无隐藏全局)。
- 测试:Diag JSONL 往返、码唯一性、因果链打印。

## 3. yu-vfs(契约见 architecture §1)

- `trait DataSource { open/exists/list }`;实现 `MemSource`、`DirSource`、`YpfSource`。
- `Mount`:叠加栈,后挂覆盖先挂;`LookupKey` 归一(分隔符 `/`=`\`,仅 ASCII A–Z 折叠,字节键);
  多字节跳步参数化(cp932/gbk 跳步表先留接口,表本体归会话 B,你先用 cp932 常量表占位并注明)。
- `YpfSource` **以 `docs/formats/ypf.md` 为准**(官方 YSPac 0.495 包逐字节实证,替代旧笔记):
  头 32B、索引自 **0x20** 起(v494 族可能 0x24——按"行走恰好到数据区起点"探测)、
  步长 `nameLen+27`、长度解码表 T、名字取反、`flag` 0=zlib/2=原样、
  `dataOffset` **直指负载(无独立记录头)**;末 8 字节未知字段原样保留不解释。
- 参考实现 `<repo-old>/crates/yuris-format/src/ypf.rs` 的 `LenEncNot` 解析器**有两处实证错误**
  (索引起点 0x24、字段切分偏 1 字节,详见 ypf.md §6):可读其判别/探测思路,但字段切分以官方包为准。

## 4. 真实对照(本任务的验收核心,不许空跑)

1. **真包验证(已有真实语料)**:`workbench/project-eris/data.ypf`(官方 YSPac 0.495 产出,
   1407 条目)作为 YpfSource 实测样本:逐个条目读出负载,zlib 条目解压后字节数==索引 usize,
   裸存条目首字节为已知图像/文件魔数;**不使用 <game-b> 语料(用户决定)**。
2. **等价性测试(架构自证,永久)**:同一棵临时文件树,DirSource 与"用自实现的 ypf 写入器
   (仅测试用)打包后的 YpfSource"分别挂载,`open/exists/list` 结果逐字节一致。
   写入器按 `docs/formats/ypf.md` 的官方布局实现(index 自 0x20、flag 0/2、dataOffset 直指负载)。
3. **方言范围**:只实现并验证 **LenEncNot**(本 demo 包所属方言);**NulXor**
   (NUL 结尾 + 名字 XOR key、步长 nameLen+26)未验证→遇到显式报"未验证方言"并挂 DIV-002,
   禁止猜 key。旧商业样本(bn.ypf/0.479 散盘)**不在本任务范围**。

## 5. 交付与报告

- `cargo test --workspace -p yu-log -p yu-vfs` 全绿;`cargo clippy` 无告警。
- `PROGRESS.md` 追加"成果 A"(日期、内容、**验证命令与原始输出摘要**)。
- 新增未决问题逐条进 `docs/divergences/`(用 TEMPLATE.md)。
- 报告里明确列出:哪些字段是"实测确证",哪些是"按笔记实现待验"。
