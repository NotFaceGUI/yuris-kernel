# DIV-003 · 非 ASCII 资源名: yu-vfs 无编解码表,显式跳过并计数

- 状态: partial(检测器已落地;码表仍归任务书 B)
- 优先级: 中
- 发现日期 / 发现方式: 2026-10-19,用官方 0.479 SDK 散盘目录实测 DirSource。

## 复现

- 数据: 官方 0.479 SDK 的 `システム` 目录(散盘,1,424 个文件)。
- 操作: `DirSource::open(root, Cp932)`,统计索引结果。

## 官方行为

Windows 上引擎以 CP932 字节路径打开散盘文件;含日文名的资源可正常加载。
(官方 trace 未采,本条为文件系统事实 + 引擎读序推断。)

## 内核行为

```text
0.479 DirSource: files=1339 skipped=85 (non-ASCII names are explicitly skipped)
```

- 纯 ASCII 名: 正常索引,大小写折叠生效(实测 `DATA/BGM/BGM01.OGG` 命中)。
- 非 ASCII 名: `std::fs` 只给 Unicode,而本层没有 CP932 码表(任务书 A 明确
  “表本体归会话 B”),因此不猜编码,记入 `DirSource::skipped()` 并计
  `skipped_count()`。**这是显式行为,不是静默丢弃**: 调用方可据计数拒绝启动。

## 二分记录

- [x] ASCII 名 `open/exists/list` 与 `YpfSource` 一致(等价性测试)
- [ ] 非 ASCII 名往返 —— 待 `yu-format` 提供 CP932/GBK 编解码后,把 Unicode
      文件名按配置编解码器折回字节键;届时本档案改 fixed。

## 根因

分层边界: 编码表属 `yu-format`(architecture §7),`yu-vfs` 只做字节级归一与
多字节跳步参数化。

## 修法

`DirSource` 暴露 `skipped()` / `skipped_count()`;跳过原因分为
`non_ascii_name` / `non_utf8_name` / `symlink` / `not_a_regular_file`。

## 本次(2026-10-19 会话 A 重启)进展

新增 `yu-vfs/src/encoding.rs`: 零依赖、确定性的字节结构检测器,输出
`Decision { encoding, only_candidate, decisive, bom, scores }` 与可进 trace 的
`evidence_line()`。它**不含码表**,因此 `DirSource` 仍无法把 Unicode 文件名折回
CP932/GBK 字节键 —— 该步骤等 `yu-format` 的码表(DIV-003 保持 partial)。
判据参考开源实现 `YU-RIS-Script-Editor/core/encoding.py`(假名强信号 /
CJK 弱正分 / SJIS 近分优先 / BOM 直判);该实现未声明许可证,只借鉴判据。

## 回归

```text
cargo test -p yu-vfs --test real_sample sdk_0479 -- --nocapture
0.479 DirSource: files=1339 skipped=85
loose sample: data/bgm/bgm01.ogg bytes=907689 skipped_total=85
test result: ok
```
