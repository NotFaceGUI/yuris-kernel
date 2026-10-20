# yuris-kernel

YU-RIS 引擎的净室兼容内核，GPL-3.0-or-later。**唯一目标：与官方引擎 1:1 对齐** ——
相同游戏数据与相同输入时间线下，封包解析、脚本执行、渲染语义、存档读写等一切可观测行为
逐位一致。

## 立场

- **唯一真值 = 官方引擎可观测行为**（官方 trace / SDK 二进制逆向事实 / 原版数据自证）。
- 旧接手线 `<repo-old>` 只读参考，不搬运代码；其 golden 只作回归参考，**不是真值**。
- 无证据的行为显式 `Unimplemented`，禁止“看起来应该是这样”。
- 通用运行时零硬编码（A5）：命令表/变量表/设置全部运行期从游戏工程装载，缺表拒绝启动。

## 当前状态（2026-10-20）

M3 渲染链路已打通到“软/硬双后端首帧 + 计数对账”：

| 阶段 | 状态 |
|---|---|
| M3a 语义显示列表 `CompositorFrame` | 完成 |
| M3b `yu-backend-soft` 首帧 PNG | 完成（部分语义挂账，见下） |
| M3c `yu-backend-wgpu` 主后端首帧 | 完成（离屏 RGBA8 + 读回；surface 呈现留宿主；alpha 语义与 soft 分叉，见 crate README） |
| M3d 桌面 / wasm / Android 宿主闭环 | 未开始 |

首帧实测（同一 ERIS 帧）：800×600、272 图层、15 pass、44 op（15 executable /
29 skipped）。soft 输出 `target/tmp/first_frame.png`；wgpu 输出
`target/tmp/wgpu_first_frame.png` + `wgpu_first_frame.report.json`，pass/op/
executable/skipped 计数与 `first_frame.display.json` 完全一致。两者在 9,451 个
半透明像素上不同：wgpu 按 D3D9 同因子 alpha（`As²`）执行，soft 当前为 `As`
（已知 soft P1）；不做逐像素门禁，差异记入 `crates/yu-backend-wgpu/README.md`
账本。文字栅格化、COPY、RECTPAINT、部分 MODE 仍是显式挂账项，见
`docs/gfx-unimplemented-todo.md`。

## 布局

| crate | 职责 |
|---|---|
| `yu-log` | 结构化诊断 + 日志；所有 crate 的错误唯一出口 |
| `yu-vfs` | 数据源抽象：DirSource / YpfSource / MemSource + 叠加挂载 + 归一路径 |
| `yu-format` | ybn 家族 / yssd / yscfg 等容器与元数据解码，纯字节进出 |
| `yu-rt` | 运行核：脚本 / VM / 任务 / 帧边界 / 16 槽寄存器 / 变量域 / 系统变量 ABI / InputTimeline |
| `yu-gfx` | 渲染语义层：`CompositorFrame`、图层 / 组 / 窗口 / 过渡 / 文本模型、GfxHost 查询 |
| `yu-font` | 字体发现 / 加载、字形栅格化、完整文本样式；不含换行 / 对齐 |
| `yu-backend-soft` | `CompositorFrame` 的确定性软件参考后端：纹理管线 + RGBA8 合成 + PNG |
| `yu-backend-wgpu` | `CompositorFrame` 的 wgpu 主后端：`Rgba8Unorm` 离屏合成 + 读回；纹理/管线/bind group 缓存 |
| `yu-trace` | 边界录制 / 回放 / diff；schema v3（producer-neutral、frame_description） |

未开始：桌面 / wasm / Android 宿主、`yu-sound` / `yu-save`、scenario 解释器与 CLI。
**没有 `yu-assets` 兄弟 crate**：纹理解析在装配 / 后端层，第一个实现是
`yu-backend-soft::TextureStore`；wgpu 后端复用同一 `TextureStore` 的 RGBA8 字节。

### 渲染链路

```text
yu-vfs → yu-format → yu-rt
                         │ VmEvent::DomainCall / Text
                         ▼
                  yu-gfx（CompositorFrame）
                         │
             ┌───────────┴───────────┐
             ▼                       ▼
   yu-backend-soft             yu-backend-wgpu
   确定性参考 / PNG / golden    主后端，同帧计数对账 / 差异账本
             │                       │
             └───────────┬───────────┘
                         ▼
              hosts：桌面 / wasm / Android（未开始）
```

## 快速开始

```bash
cargo test --workspace

# 真样本（仓库不附带游戏数据；缺失时测试显式失败 / 跳过并计数）
export YURIS_REQUIRE_SAMPLES=1
export YURIS_SAMPLE_ERIS_YSBIN="<project-eris>/save/data/ysbin"
export YURIS_SAMPLE_ERIS_YPF="<project-eris>/data.ypf"

# headless 首帧
cargo run -p yu-backend-soft --example first_frame
# → target/tmp/first_frame.png / first_frame.display.json

cargo run -p yu-backend-wgpu --example first_frame
# → target/tmp/wgpu_first_frame.png / wgpu_first_frame.report.json
# 可选：YURIS_WGPU_BACKEND=dx12 YURIS_WGPU_ADAPTER=5070
```

本机沙箱、样本路径与占位符约定见 `AGENTS.md`（本机文件，不入库）。

## 文档

- `PROGRESS.md` — **唯一进度真相**（成果、勘误、验证命令与原始结果）
- `docs/architecture.md` — 设计稿（v1.2；与 `PROGRESS.md` 架构现状冲突时以后者为准）
- `docs/render-parity.md` — 渲染语义证据表与后端执行契约
- `docs/gfx-verification-ledger.md` — gfx 复核台账（Confirmed / Refuted / Uncertain）
- `docs/gfx-unimplemented-todo.md` — 未实现模块 DIV 清单与解除条件
- `docs/font-contract.md` / `docs/font-provenance.md` — 字体接口与取证
- `docs/oracle.md` — 对拍方法论；`docs/divergences/` — 不一致档案

## 许可

GPL-3.0-or-later。净室实现；不包含原引擎代码或游戏数据。
