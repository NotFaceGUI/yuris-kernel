# 任务书 P6 · E-mote 运行时续(物理 / 管线 / M4 对拍)

> 主仓:`<repo>`;**实现仓库:`yu-emote/` 子模块**
> (github.com/NotFaceGUI/yu-emote,MIT OR Apache-2.0)。
> 先读主仓 `AGENTS.md`、本文件、`docs/tasks/task-emote-runtime.md`
> (E1 总任务书)、`docs/emote-parity.md`(台账)、`PROGRESS.md`
> 成果 E1-M2 续 1..4。
>
> 背景:M1 数据层 + M2 动画 core + M3 渲染 baseline 已完成;剩余
> 物理控制器、wgpu 管线、官方对拍(M4)与内核接线。本任务**只在
> `yu-emote/` 子仓内动代码**,主仓零源码改动(主仓只追加 PROGRESS)。
> 与 P1–P5 完全并行,无文件冲突。
>
> 纪律:零 git 写操作(主仓与子仓都不做;子仓 gitlink 由主人提交);
> PROGRESS 只追加"成果 P6";参考实现仅取事实(eluna = MPL-2.0 元数据、
> player = MIT OR Apache-2.0;不复制代码),官方 DLL 只作 oracle。

## 0. 基线

```bash
cd yu-emote && cargo test && cargo clippy --all-targets && cargo fmt --check
```

## 1. 剩余清单(按 PROGRESS E1-M2 续 4 的挂账顺序)

### P0 · M4 oracle 打通(对拍是一切收敛的尺子)

1. oracle 宿主已能 `CreatePlayer/Update/Render` 但 **draw list 仍空**:
   补 motion 激活(首次 PLAY/DPLAY 等价调用序)、屏幕参数、
   stencil 设置;D3D9 回读全黑 ⇒ 换 `GetRenderTargetData` 时序或
   锁表面读回,直到拿到非空官方帧;
2. 拿到首帧后建立:静态姿态(ERIS `emote_test.psb` + Nekopara
   `chocola.pure.psb`)官方截图 vs 本实现 CPU 渲染,逐像素阈值
   (阈值先声明,写进台账);
3. 空白帧不得当对齐结果(既有纪律,重申)。

### P1 · 动画语义补全

1. 物理控制器(bust/hair/parts):参数表解析已有,补求值
   (弹簧/阻尼按 eluna 所引 native 语义取证);
2. 差分淡入曲线、官方帧率(60/30 切换点)取证;
3. type-7 clip rect、inner/outer stencil phase、type-9 anchor、
   type-10 feedback、particle、pixelate(逐项:取证→实现→台账)。

### P2 · wgpu 管线

1. 58 个 WGSL 变体选择器接线(plain/masked/pixelate × blend 0..5);
2. 与 CPU 光栅同帧一致(静态+时间线);
3. wasm32 可编译不回归(不引入 native-only 依赖)。

### P3 · 内核接线设计稿(不动主仓)

产出设计小节(写进本任务书附录/emote-parity.md 草案,主仓源码零改动):
`EMOTE/EMOTEINFO` 域事件 → 适配层(建议新 crate `yu-emote-host`,
消费 DomainCall,产出 RGBA 纹理/图层变换喂 `yu-gfx`,沿 M0 裁决);
`SYSTEMINFO[EMOTE]` 槽 3 注入 1 的时机(渲染可用前保持 0——既定纪律)。

## 2. 验证

- `yu-emote` 全测绿 + clippy/fmt clean;
- M4:oracle 官方帧 vs 本实现,声明阈值内;
- demo(`demo/make_demo.py`)更新,真实资产不入库(NOTICE 既有声明);
- 主仓 PROGRESS 追加"成果 P6"。

## 3. 停止/上报条件

- oracle 仍拿不到非空官方帧 ⇒ 挂账并给出替代证据方案
  (如 eluna draw list 顺序证据 + 官方窗口截图),不宣布对齐;
- 物理控制器参数语义无法闭合 ⇒ 挂账继续其余项;
- 需要改主仓 `CompositorFrame`/宿主契约 ⇒ 只出设计稿,等主人裁决。
