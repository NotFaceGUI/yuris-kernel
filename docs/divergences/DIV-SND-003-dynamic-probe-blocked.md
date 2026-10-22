# DIV-SND-003 · 动态曲线采集受环境阻塞

状态：**open（静态证据已足够实现，动态对拍待补）**

## 现象

任务书 N0 要求“YSCom 编译探针 + 官方引擎逐帧 SOUNDINFO”的
LOOPCOUNT/TIME 曲线。本次尝试：

1. 官方 YSCom 0.495 编译最小探针工程**成功**（SDK 样板 + 自定义
   `system_start.yst`，82 个产物）。
2. 探针工程在 Locale Emulator 下进程存活，但窗口未创建、脚本未执行。
3. 直接调试正在运行的 ERIS 工程（WOW64 调试器，断点 `0x44f79a`
   LOOPCOUNT / `0x44f899` TIME）：
   - 从进程创建起布防：`SOUND` 处理体 `0x44e1e8` 命中 1 次；
   - `SOUNDINFO` 处理体 `0x44f4bc` 命中 **0** 次；
   - `0x8015c0`（声音初始化）= 1，ring 读写索引相等（空闲）。
   ⇒ title 的 BGM 轮询在当前 UI 状态未执行。

## 补充（2026-10-22）

内核侧真实语料验证已通过：`yu-cli/examples/sound_census` 600 帧装载并播放
`bgm/bgm01.ogg`（time=6000/90802，0 诊断）；`sound_loop_census` 6000 帧在
90.8s 边界得到 `LOOPCOUNT=1`、TIME 回 RESTART=0。该证据证明**本实现**的
状态机在真实数据上跨循环正确，但仍不是官方引擎的动态对拍。

## 裁决

- 台账对应项标 `Confirmed（静态 REA）`，不冒充动态已对拍。
- 实现按静态事实落地（循环边界、TIME 公式、淡变线性、映射公式），
  单测覆盖手册例 1..12 的可观测值。
- 复现步骤与断点保留在本档案；取得曲线后只需补 golden，不改语义。

## 复现

```text
probe: <workbench>/tmp/snd-probe（YSCom 0.495 编译产物）
tracer: <workbench>/tmp/snd-rea/trace_sound.py
断点: 0x44e1e8 / 0x44f4bc / 0x44f79a / 0x44f899
```
