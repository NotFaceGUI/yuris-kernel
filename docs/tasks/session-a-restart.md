# 会话 A 重启计划(yu-vfs 拆包修正)

> 适用: 重启的会话 A(地基:`yu-log` + `yu-vfs`)。**只改 `crates/yu-vfs`(与必要的
> `crates/yu-log` 配套),不动 B/C 的 crate,不做任何 git 写操作。**
> 本计划基于官方 YSPac 0.495 包逐字节实证(2026-10-19),替代旧笔记与旧实现假设。

## 语料与范围(先看这条)

- **语料 = 你编译出来的 ERIS demo 包**:`<work-root>/projects/<side-repo>/workbench/project-eris/data.ypf`
  (官方 YSPac 0.495 产出,1407 条目)。旧的商业样本(`bn.ypf`、0.479 SDK 散盘等)**不在本计划范围**。
- **方言范围 = 只做 `LenEncNot`**(本 demo 包所属方言,`docs/formats/ypf.md` §1.1)。
  `NulXor`(NUL 结尾 + 名字 XOR key、步长 nameLen+26)未验证:读取器遇到应显式报
  "未验证方言"并挂 DIV-002,**禁止猜 key**。旧线的"v494 族 0x24 起点"只作为
  **起点探测**的兼容线索,不是本次要适配的语料。

## 0. 开工前必读(顺序)

1. `AGENTS.md`(纪律/占位符/样本)
2. `docs/formats/ypf.md`(**已验证的 YPF 布局;本次修正的依据**)
3. `docs/eris-corpus.md`(语料位置与复现流程)
4. `docs/references/open-source-tools.md`(可对照的开源实现与许可边界)
5. 自己上一轮的 `PROGRESS.md` 成果与 `docs/divergences/` DIV-001/002/003

## 1. 要修什么(与旧实现的差异)

旧 `YpfSource` 沿用旧笔记/旧线实现,有两处**实证错误**:

1. **索引起点**: 官方 0.495 包从 **0x20** 开始(v494 族可能 0x24)。
   不能固定 0x24;按"按条目规则行走恰好抵达数据区起点(头 +0x0C 的值)"探测。
2. **字段切分**: 旧实现为 `code + NUL + usize + csize + off + zero4 + tail8`
   (整体偏 1 字节);官方包实测为:

```
u32 hash | u8 lenEnc | name[len] (逐字节取反) |
u8 flag(0=zlib, 2=原样) | u32 usize | u32 csize | u8 pad(0) |
u32 dataOffset | u32 unknown(0) | u32 unknown(非零)
```

步长 `nameLen + 27`;`dataOffset` **直接指向负载首字节,没有独立记录头**。
`hash` 与 `MurmurHash2(name, seed=0)` 恒等(官方包抽验 4/4、全包行走 1407/1407)。

## 2. 验收(必须跑真实包)

样本: `<work-root>/projects/<side-repo>/workbench/project-eris/data.ypf`
(官方 YSPac 0.495,1407 条目,31,501,753 字节)。

- [ ] 索引行走恰好 1407 条,终点 == 头部 `+0x0C` 的值;
- [ ] 每条: `dataOffset + csize <= 文件长度`;`flag ∈ {0,2}`(未知值显式报错,不猜);
- [ ] `flag==0`: zlib 解压后长度 == `usize`,且内容可识别(如 YSTB 头 `YSTB`);
- [ ] `flag==2`: 负载首字节与扩展名一致(如 `.png` → `89 50 4E 47`);
- [ ] `open("ysbin\\ysc.ybn")` 返回字节以 `YSCM` 开头、版本 495;
- [ ] 名字查找保持"仅 ASCII 大小写折叠"一次实现的契约(与 docs/architecture §1 一致)。

## 3. 档案与报告

- `docs/divergences/DIV-001`: 状态改 `fixed`,引用 `docs/formats/ypf.md` 与实测输出;
- `DIV-002`(NulXor 方言)/`DIV-003`(非 ASCII 名)保持原状,注明"本次未触达";
- `PROGRESS.md` 追加"成果 A2"(验证命令 + 原始输出摘要,注明样本来自官方产出)。

## 4. 启动提示词(复制粘贴)

```text
你在 <repo> 工作,只允许修改 crates/yu-vfs(及必要的 crates/yu-log)。
先按顺序读: AGENTS.md → docs/formats/ypf.md → docs/eris-corpus.md →
docs/references/open-source-tools.md → docs/tasks/session-a-restart.md,然后执行该重启计划。

语料与范围: 语料就是用户编译的 ERIS demo 包
<side-repo>\workbench\project-eris\data.ypf(官方 YSPac 0.495,1407 条);
旧商业样本(bn.ypf/0.479)不在范围。只实现 YPF 的 LenEncNot 方言(有显式长度码、名字取反、
步长 nameLen+27);NulXor 方言(NUL 结尾+XOR key、步长 nameLen+26)未验证——遇到必须显式报
"未验证方言"挂 DIV-002,禁止猜 key。

重点: 旧 YpfSource 有两处实证错误——索引起点固定 0x24(官方 0.495 包是 0x20,需探测)
和字段切分整体偏 1 字节(真实布局是 hash/u8 lenEnc/反名/u8 flag(0=zlib,2=原样)/
u32 usize/u32 csize/u8 pad/u32 dataOffset/u32 unk/u32 unk,dataOffset 直指负载、无独立记录头)。
用 demo 包回归: 行走恰好到数据区起点、zlib 解压长度==usize、raw 条目魔数正确、
open("ysbin\ysc.ybn") 以 YSCM 开头。禁止 <game-b> 语料;禁止 git 写操作;无证据不实现。
完成后把 DIV-001 标 fixed,PROGRESS.md 记"成果 A2"(含原始输出)。
```
