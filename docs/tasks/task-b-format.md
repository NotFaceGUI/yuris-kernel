# 任务书 B · 格式:yu-format(M1)

> 会话定位:三个并行会话之一。你独占 `crates/yu-format`;**不要动**根 Cargo.toml、
> `crates/yu-log`、`crates/yu-vfs`(会话 A 所有)或 `crates/yu-rt`(会话 C 所有)。
> 依赖:仅 `yu-log`(A 建好后可用;未落地前先用自有最小 Diag 占位并注明待切换)。
> **你产出的解码契约是会话 C(yu-rt)的输入,§4 接口契约不得擅改,有异议建 divergences 档案协商。**
> **边界: ypf 包索引布局归 `yu-vfs`(会话 A),本 crate 不实现 ypf;本 crate 只吃 `.ybn` 字节。**
>
> **v2 变更(2026-10-19,用户决定): 算法/常量/流程可直接采用**——从下列开源实现得到的
> 解析算法、密钥恢复流程、长度表、编解码策略等允许直接采用;采用处文件头注明
> `参考 <项目>/<文件>`,并在 `docs/references/open-source-tools.md` 的致谢链登记
> (README 致谢由仓库主人汇总)。整段复用代码前确认其许可状态。
> 新增必读: `docs/references/open-source-tools.md`。

## 0. 开工前必读(顺序)

1. `<repo>/AGENTS.md` → 2. `docs/architecture.md`(§2 yu-format 行、§7 编码策略)
   → 3. `docs/oracle.md` → 4. `docs/sdk-eris-analysis.md`(§4 命令集/ycd 事实)
   → 5. 旁线笔记 `<side-repo>/docs/format-ybn.md`、`format-ybn-loader.md`
   (**按事实实现,笔记与实数据冲突时以数据为准并挂账勘误**)。

**参考实现索引(用户授权:ybn 启动解码可参考旧接手线,它对真实商业游戏实测跑通):**

> **旧线仓库根 = `<repo-old>`**(真名见 `AGENTS.md` 的占位符对照表;本机即
> `<repo-old>`)。**只读参考,禁止修改其工作区、禁止在其中执行任何 git 操作。**
> 算法/流程可直接采用并致谢(v2)。

| 旧线文件(占位 `<repo-old>` = 旧接手线仓库根) | 可提取的事实 |
|---|---|
| `crates/yuris-format/src/`(ypf/ybn/yssd 解析) | ybn 容器与 XOR 键、四区段结构、各 `.ybn` 种类的记录布局 |
| `crates/yuris-vm/src/boot.rs` | 启动链顺序与各表的匹配方式(YSCM 命令表→YSLB 标签→YSTL/YSVR/YSCF→脚本体) |
| `crates/yuris-format/tests/sample.rs` | 对真实样本数据的**断言**(如 caption 字段实值)= 现成的验证期望 |
| `docs/known-issues.md`、`PROGRESS.md` | 已踩坑与勘误(变量字符数 vs 字节数等) |

**开源算法直接采用来源(v2):**

| 来源 | 可直接采用的算法/事实 |
|---|---|
| `RxYuris/lib/YurisStaticLibrary/YSTB.cpp` | YSTB V2/V5 分段异或算法(已与本仓库实证一致) |
| `RxYuris/lib/YurisStaticLibrary/YSCM.cpp`、`YSTL.cpp`、`YSCFG.cpp` | YSCM/YSTL/YSCFG 解析流程(交叉验证用) |
| `YU-RIS-Script-Editor/core/ystb.py` | V2/V5 解析、**V2 支路**(我们只实测过 V5)、文本段定位 |
| `YU-RIS-Script-Editor/core/encoding.py` | 编码自检测实践(需改造为确定性裁决) |
| `YURIS_TOOLS` | **新版引擎解密算法**(换密钥作品,如 `<game-a>` 场景) |
| `RxYuris` | **旧版引擎解密算法** |
| `GARbro` | YPF 解密(仅作对照,ypf 归 A) |

采用纪律: 文件头注来源;算法声明"参考实现,行为以官方产物为准";致谢条目写入
`docs/references/open-source-tools.md` §4。

使用界限:旧线是**事实来源与行为参照**,真值优先级仍在官方引擎/手册/真实数据之下;
提取事实与验证期望时注明出处(`参考 <repo-old>/...`),落笔必须是自己的实现与自己的测试,
不整段搬运代码。

铁律:真实对照(无出处不实现)、零硬编码(命令名/序号/变量名不进代码)、禁止 git 写操作、
占位符注释、零第三方依赖(含解码表全部自带)。

## 1. 范围:容器与结构解码(纯字节进出,不碰 IO)

1. **ybn 家族容器**:XOR 解密(默认键 = `CRC32("Yu-ris")` 大端字节,四区段各自从键下标 0 重启
   ——旁线事实,须在真实数据上验证;游戏换键时不得猜:显式 `Diag{code:"fmt.ybn.key", severity:Fatal}`
   并挂账"密钥来源待查")。
2. **`ysc.ybn` = 命令名表**(本任务最高价值产物):解码出 名字↔序号 全表,暴露
   `CommandTable::name_of(index) / index_of(name)`。这是运行核"按名分派、零硬编码"的数据来源。
3. **`ysl.ybn` 标签表**、**`ysv.ybn` 变量镜像**(记录布局见旁线笔记,含恢复组号 flag 1..4)、
   **`yst_list.ybn` 脚本清单**、**`yst%05d.ybn` 脚本体**(命令组+窗口/参数记录切片)。
4. **编解码器**:CP932 与 GBK 静态码表(自建或内嵌生成表)+ `decode/encode` +
   **架构 §7 的 auto 评分器**(双解码合法性+构成统计,确定性裁决,缺省 CP932)。
   码表正确性用已知锚点单测:CP932 "日"=0x93FA、GBK "你"=0xC4E3 等 ≥20 个锚点 + 全表往返。

## 2. 真实对照(验收核心)

语料 = **官方 YSCom 编译的 ERIS 产物**(`workbench/project-eris/data/ysbin/`:137 个 YSTB 脚本 +
YSCM/YSCF/YSER/YSLB/YSTD/YSTL/YSVR 七张元数据表;引擎启动验收通过)。
**不使用 <game-b> 语料(用户决定,那是旧线项目所用)。** 所有断言跑在**真实字节**上:

1. **ysc 名字表三方互证**:解码出的命令名集合 ⊆ SDK `YSCom.ycd` 的 105 个 `COM_*` 名
   (ycd 一手二进制,解析法见 `docs/sdk-eris-analysis.md` §4;去 COM_ 前缀比对),
   且 ⊇ 手册 menu.html 文档化命令集。任何名字只出现在一侧 ⇒ 建档(版本差异证据,珍贵)。
   **YSTB 磁盘形态已实测确证**:明文头 `"YSTB"`+版本(495),代码/参数段各自从密钥下标 0 重启
   异或(密钥 CRC32("Yu-ris")=0x96AC6FD3,LE 字节序;V5 四段=inst index/args index/args data/
   line numbers),见 `docs/sdk-eris-analysis.md` §8——解码器必须复现该形态。
   同一语料还含官方 `.ypf`(`workbench/project-eris/data.ypf`),包内可直接取出
   `ysbin\*.ybn` 做交叉验证;YPF 布局见 `docs/formats/ypf.md`。
2. **标签闭环**:解码全部 `yst%05d`,收集 GO/GOSUB 目标标签 ⇒ 必须全部能在 `ysl` 表解析。
   解析失败的标签 = 容器解码有 bug 或理解有误,**必须查到根因**,不许跳过。
3. **yst_list ↔ yst%05d 一致性**:清单条数、编号、映射对得上。
4. **变量镜像**:ysv 记录的 dims/初值/恢复组号抽样与笔记 §8 语义互证。
5. 样本缺失:打印 skip 并计数,不许静默通过。

## 3. 交付

- 解码器对真实语料全量跑通;`fixtures/`(小体量、脱敏的字节切片或 JSON 摘要)入测试;
- `PROGRESS.md` 追加"成果 B"(含验证命令与原始输出);未决进 divergences;
- 报告列出:实测确证项 / 按笔记待验项 / 三方互证的差异清单(这是跨版本对照的第一手材料)。

## 4. 与 yu-rt 的接口契约(冻结,双方遵守)

```rust
pub struct DecodedScript { pub script_id: u32, pub groups: Vec<Group>, pub labels: LabelTable }
pub struct Group { pub command: Cow<'static, str>,   // 经 ysc 表解析的命令名;查不到 = Diag,不是序号
                   pub windows: Vec<Window> }        // 窗口(参数记录)切片
pub struct Window { pub bytes: [u8; 12] }            // 原始窗口;求值归 yu-rt,yu-format 不解释
```

> 设计理由:窗口字节语义(参数槽 tag.B0 等)属运行核;格式层只负责**忠实切分**。
> C 端在 B 落地前以本契约为准先行开发(手工 fixture)。
