# DIV-RT-056 · 玩家存档的 `DNO=1024` 是嵌套容器槽(8 字节长度头 + zlib 内层 YSSD)

- 状态: open(格式已定性;内层之后的第二段负载未定性)
- 优先级: 低(当前 `LOAD` 不读该槽;影响的是存档槽缩略图/元数据)
- 发现日期 / 发现方式: 2026-10-30 / 逐块解压普查(24 个商业包存档 / 3568 块)

## 复现

- 语料: 0.481 商业包 `save/save00151.sd` … `save00168.sd`(官方玩家存档,
  2016 年时间戳),每个文件 198 块,其中 `DNO=1024` 一块。
- 锚点: 偏移表第 1024 项 → 块头 `dno=1024 btype=0 strict=1 var=0`。

## 形态(实测 18/18 个文件一致)

```text
载荷 = u32 inner_len | u32 len_b | zlib(inner_len 字节的内层 YSSD 容器)
```

- `inner_len` 与 zlib 输出长度**逐文件相等**(55376..57369);
- 内层以 `YSSD` 魔数开头、版本 `0x1E1`、块数 `0xC3`(195)、容量 `0x400`
  —— 即一个完整的二级存档容器;
- `len_b`(10170..11956)与 zlib 流之后剩余的字节段相关,但该段**不是
  第二段 zlib 流**(`incorrect header check`),`len_b` 的语义未定性;
- 外层块头的 `ulen`(5.5M~5.7M)与 `inner_len` 不同源(约为其 100 倍),
  对外层块不成立 —— 说明这一槽不是普通「单层压缩块」。

## 内核行为

`YssdEntry::decompress` 依次试 zlib / Snappy,两者都失败 ⇒
`fmt.yssd.codec`。普查结果:24 文件 / 3568 块中 3550 块正常,18 块即本槽。

## 修法(本轮)

- **不**把这条塞进通用解压路径(按偏移试 zlib 属启发式)。测试
  `commercial_save_blocks_recompress_as_snappy` 里对 `DNO=1024` 单列一支:
  按 `u32 inner_len` 解 `payload[8..]`,断言输出长度与 `YSSD` 魔数。
- 通用路径保持严格:普通块只认 zlib / Snappy 两种整块压缩流。

## 回归

- `cargo test -p yu-format --test yssd_roundtrip commercial_save_blocks`
  → `24 文件 / 单块 3511(Snappy 3476 + zlib 35)/ 多块 57 / 嵌套 18 全部往返一致`

## 残余

- `len_b` 与 zlib 流之后的字节段语义未定性(疑为存档槽缩略图或元数据块)。
  需要拿到官方**读档界面**对该槽的消费路径(REA 定位 `LOAD` 侧读 DNO=1024
  的调用点)才能定案。
