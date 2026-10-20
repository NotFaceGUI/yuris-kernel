# yuris-kernel

YU-RIS 引擎的净室兼容内核。**唯一目标: 与官方引擎 1:1 对齐** —— 相同游戏数据与相同
输入时间线下,封包解析、宏展开、脚本执行、存档读写等一切可观测行为逐位一致。

## 立场(与旧接手线 <repo-old> 的区别)

- **唯一真值 = 官方引擎可观测行为**(官方 trace / SDK 二进制逆向事实 / 原版数据自证)。
- 旧接手线 <repo-old> 定位为**参考实现**: 只读对照,不搬运任何代码;
  它的 golden 快照仅作回归参考,**不是真值**。
- 无证据的行为显式 `Error::Unimplemented`,禁止"看起来应该是这样"。

## 布局

架构基线见 `docs/architecture.md`。要点:**散盘项目与 .ypf 封包游戏是同一程序**——
差异全部收敛在 `yu-vfs` 的 DataSource 抽象(DirSource / YpfSource / MemSource + 叠加挂载)之后,
并有永久等价性测试自证。

| crate | 职责 |
|---|---|
| `yu-vfs` | 数据源抽象与归一(散盘/封包/内存三实现 + 挂载栈) |
| `yu-format` | 容器解码(ybn/yssd/ymv/yscfg),纯字节进出 |
| `yu-rt` | 运行核:任务调度/按名分派/表达式/16 槽寄存器/变量域 |
| `yu-gfx` / `yu-snd` / `yu-inp` | 官方 graph/sound/input 子系统状态机 |
| `yu-save` | 存档链(只写真实 SaveDir) |
| `yu-trace` | 对拍基建:四条边界记录/回放/diff |
| `yu-cli` | 装配与帧循环薄壳 |

`docs/oracle.md` — 对拍方法论;`docs/divergences/` — 不一致档案;`docs/sdk-eris-analysis.md` — SDK 脚本系统一手分析;
`docs/eris-corpus.md` — 官方 ERIS 测试工程(编译/打包/发布启动全流程);`docs/formats/ypf.md` — YPF 布局实证。

## 快速开始

```bash
cargo test --workspace
```

样本游戏与环境变量约定见 `AGENTS.md`(本机文件,不入库)。

## 状态

一切子系统**未实现**。进度真相见 `PROGRESS.md`。
