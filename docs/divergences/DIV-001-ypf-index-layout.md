# DIV-001 · ypf 索引条目与数据记录的字段布局:旧笔记/旧实现 vs 官方包

- 状态: **fixed**(2026-10-19,按官方 YSPac 0.495 真包重写解析器并通过全量回归)
- 优先级: 高
- 发现日期 / 发现方式: 2026-10-19,任务书 A 开工前用真实商业包逐条解析索引并交叉核对字段分布;
  2026-10-19 会话 A 重启,用官方 0.495 `data.ypf`(1,407 条)逐字节确证最终布局。
- 依据: `docs/formats/ypf.md`(官方 0.495 实测)+ 本档案回归原始输出。

## 复现

- 数据: 官方 YSPac 0.495 产出的 `data.ypf`(1,407 条,31,501,753 字节)。
- 锚点: 索引区 `0x20 .. dataStart`,无需执行游戏。

## 官方行为(逐字节实测)

```text
header 0x20 bytes:
  +0x00 "YPF\0"; +0x04 u32 version(495); +0x08 u32 entryCount(1407);
  +0x0C u32 dataStart(0x1373F); +0x10 u8[16] reserved(全 0)
index at 0x20, entry 步长 = nameLen + 27:
  u32 nameHash(MurmurHash2 of name, seed 0)
  u8  lenEnc; name[nameLen] 逐字节取反
  u8  fileType(0=ybn 2=png 3=jpg 6=ogg 9=psb)
  u8  isCompressed(0=原样, 1=zlib)
  u32 usize; u32 csize; u32 dataOffset; u32 unknown0(恒 0)
  u32 payloadHash(MurmurHash2 of 存储字节, seed 0)
```

## 内核行为(修复前 → 修复后)

| 项 | 修复前(旧实现) | 修复后 |
|---|---|---|
| 索引起点 | 固定 `0x24` | 探测 `0x20`/`0x24`,取"恰好闭合 + 名字哈希命中最多"者 |
| 字段切分 | `code+NUL+usize+csize+off+zero4+tail8`(整体偏 1 字节) | `type+compressed+usize+csize+off+unknown0+payloadHash` |
| 单字节 flag | 0=zlib / 1=原样(后改为 0=zlib / 2=原样) | 两个独立字节:`fileType` + `isCompressed` |
| 压缩语义 | 推测 | 实测 `isCompressed=1` 的 152 条全部 zlib 流、`csize` 等于流实际消耗字节 |
| 校验 | 无 | 名字哈希与负载哈希双校验(各 1,407/1,407 命中) |

**为什么单字节 flag 一定错**: 官方包同时存在 `fileType=3`(jpg)且 zlib 压缩的 7 条、
`fileType=0`(ybn)且原样存储的 1 条;把类型与压缩并成一个字节无法同时解释这两类。

## 二分记录

- [x] 结构解析一致: 1,407 条行走恰好闭合(`consumed=79,647 == index_bytes`)
- [x] 名字哈希一致: 1,407/1,407 `murmur2(name,0) == nameHash`
- [x] 负载哈希一致: 1,407/1,407 `murmur2(stored,0) == payloadHash`
- [x] 解压长度一致: 24 条 zlib 抽样 `inflate 长度 == usize`,且与独立 inflate 逐字节相同
- [x] raw 魔数一致: 64 条抽样 `png→89 50 4E 47`、`jpg→FF D8 FF`、`ogg→OggS` 全部命中
- [x] 点名项: `open("ysbin\ysc.ybn")` → 11,156 字节,`YSCM`,版本 495
- [ ] 未知字段语义: `unknown0`(实测恒 0)与索引尾 `payloadHash` 之外的语义仍未定;
      `payloadHash` 虽已实证为 MurmurHash2(存储字节),引擎"不符时多段续读"的细节未实现

## 根因

旧笔记把**两个独立字节**(资源类型码 + 压缩标志)合并成一个 `flag`,并据此把
其后各字段整体错位 1 字节;索引起点又按 `0x24` 写死,使官方 0.495 包从第一条起就错。

## 修法

`crates/yu-vfs/src/ypf.rs` 按官方布局重写:起点探测、严格字段切分、双 MurmurHash2
校验、`isCompressed` 非 0/1 时 `vfs.ypf.unsupported`、负载哈希不符时
`vfs.ypf.checksum`(Unimplemented,不猜多段续读)。

## 回归

```text
$ YURIS_SAMPLE_ERIS_YPF=<official data.ypf> cargo test -p yu-vfs --test real_sample -- --nocapture
REAL SAMPLE: <official data.ypf>
header: version=495 entries=1407 dataStart=0x1373f fileLen=31501753
index: start=0x20 bytes=79647 consumed=79647 closed=true nameHash=1407/1407 compressed=152 types=[(0,146),(2,1245),(3,7),(6,8),(9,1)]
dialect: hashed-len
ysc via "ysbin\\ysc.ybn": 11156 bytes, magic YSCM, version 495
ysc via "YSBIN\\YSC.YBN": 11156 bytes, magic YSCM, version 495
ysc via "ysbin/ysc.ybn": 11156 bytes, magic YSCM, version 495
zlib entries verified: 24
raw entries verified: 64 (image magic ok: 64)
test result: ok. 4 passed; 0 failed
```

- 永久等价性测试: `crates/yu-vfs/tests/equivalence.rs`(DirSource ≡ YpfSource,含 0x20/0x24 起点探测)。
- 同类旧版本包(本机一次性核对,按用户要求**不入语料、不进测试**): 同一布局在
  `0x20` 下全量闭合且名字哈希全命中,证实旧实现"固定 0x24"是错的,不是"两代方言"。


