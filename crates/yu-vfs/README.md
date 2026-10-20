# yu-vfs

数据源抽象与路径归一:**散盘目录、`.ypf` 封包、内存夹具在其上是同一个程序**
(架构不变式 A1),配永久等价性测试自证。

## 负责

- [`DataSource`](src/lib.rs) trait(`open/exists/list`);数据**只读**(A2),
  存档写入走独立 SaveDir,不在本 crate。
- [`LookupKey`](src/key.rs):路径归一只做一次 —— 分隔符统一、ASCII 折叠、
  CP932/GBK 多字节整对跳步、拒绝 `.`/`..`/NUL。
- 三种实现:`MemSource`(测试夹具)/ `DirSource`(散盘)/ `YpfSource`(封包,
  懒索引 + 偏移直读);叠加挂载 [`Mount`](src/mount.rs)(后挂覆盖先挂)。
- YPF 方言判别(`src/ypf.rs`):按包自身字段判别索引形态,未知字段原样保留并
  计数告警,不猜测。
- 编码检测(`src/encoding.rs`):CP932/GBK 采样评分,结论供上层固定使用。

## 不负责

- ybn 家族容器/结构解码(归 `yu-format`);
- 存档读写;
- 任何游戏语义。

## 实测基线(证据见 PROGRESS 成果 A)

- v494 商业包:索引 12,212/12,212 条全部闭合,`flag↔压缩长度` 全量成立;
- `.ybn`/图像抽样 zlib/魔数命中;0.479 散盘 1,339 文件入索引、
  85 个非 ASCII 名显式跳过并计数;
- NUL-XOR 方言待真样本(`DIV-002`)。

## 验证

```bash
cargo test -p yu-vfs            # 等价性 + 合成包
YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-vfs --test real_sample
```
