# DIV-RT-058 · 内核写出的存档官方读不了:「セーブデータのバージョンが異なります」

- 状态: fixed
- 优先级: 高(存档无法与官方互换)
- 发现日期 / 发现方式: 2026-11-15 / 主人实测:官方存档内核可读,内核存档官方拒收

## 复现

- 游戏位置 / 操作序列 / 所需数据(只写结构,不写本机路径):
  - 任一官方工程(实测 v474 旧运行时与 0.495 SDK 工程);
  - 内核 `SAVE[FILE=saveNNNNN]` 写出新槽 → 关闭 → 官方 exe 启动 → 选该槽;
  - 官方弹 yse `108040`「セーブデータのバージョンが異なります。お手数ですが
    セーブフォルダを一旦破棄してください。」
- 锚点(脚本/标签/帧号):`es_SaveLoad.yst` 的 `es.SaveDataCheck`
  (`LOAD[FILE=… DNO=0001 LET=@chk]` 之后比较 `\VERNO.SAVEDATA`)。

## 官方行为

官方 LOAD 在读完 `YSSD` 魔数后**立刻精确校验容器版本立即数**,不等即弹
上述消息、返回失败(REA 2026-11-15,`CMP EDX,'YSSD'` 后紧跟 `CMP reg,imm32`):

| 构建 | 校验/写盘常量 | 证据(exe 内**文件偏移**) |
|---|---|---|
| 0.495 SDK `yu-ris.exe` | `0x1E0` | `0x4672c`/`0x4c3ac` 两处;SDK `data/save/*.sd` 头同为 `0x1E0` |
| 0.481 商业包(随包 `YSSnp.dll`) | `0x1E1` | `natsunokusari.exe` `0x462fb`/`0x465dd`/`0x4d54d` 三处;实机存档头 `0x1E1` |
| 0.479 SDK | `0x138` | `0x3fac4`/`0x454e1` |
| 0.466 SDK | `0x0C0` | `0x38271`/`0x3dee4` |
| 2012 旧运行时(<0.474 旧运行时样本>) | `0x138` | `<0.474 exe>` `0x3bdf8`/`0x4188c` 两处 |

**关键事实:容器版本是写盘器常量,不是 yscfg/引擎版本。** 旧运行时数据由
0.474 编译器产出(`ysc/yscfg/yse` 头 = `0x1DA`),但运行时 LOAD 只认 `0x138`;
0.495 工程 yscfg 头 = `0x1EF`,官方写/读的容器版本仍是 `0x1E0`。

## 内核行为

`engine.rs` 把 `vm.tables().boot.config.version`(yscfg 头版本)注入
`SaveState::set_container_version`,且 `refresh_slots` 的
`container_version.get_or_insert(...)` 不会覆盖注入值 ⇒ 新建槽一律写成
「工程版本」:

- 0.495 工程 → `0x1EF`(官方要 `0x1E0`);
- v474 旧运行时 → `0x1DA`(官方要 `0x138`)。

盘上证据:旧运行时 `save/` 里系统槽 `gsd/seld/kidoku/save00001` 全是 `0x138`,
只有内核新写的 `save00002.sd` 是 `0x1DA`。官方因此只拒收新槽,旧槽正常。

## 二分记录

- [x] L1 译码一致
- [x] L2 执行/事件流一致
- [x] L3 帧层状态一致
- [x] L4 帧间编排一致
- 定位结论:写盘容器版本来源错误(不是块布局、压缩层或 DNO1 的
  `\VERNO.SAVEDATA` 值;DNO1 值与官方一致,官方在容器版本检查处即返回)。

## 根因

把「工程 yscfg/引擎版本」误当成「存档容器版本」并让它压过盘上官方存档
证据。容器版本是每个官方构建硬编码的格式修订号,与 yscfg 版本无换算关系。

## 修法

1. 写盘版本优先级(高→低):
   官方 exe 探测常量 → 盘上 `.sd` 学习值 → 目标文件自身版本 → 按压缩层的
   现代默认(zlib `0x1E0` / Snappy `0x1E1`);
2. 启动时从工程根 `*.exe` 探测官方 LOAD 常量(按体积降序,`81 FA 'YSSD'`
   后的版本比较立即数;跳过安装器/设置器/宿主自身——实测 `inst.exe` 也带
   YSSD 校验但常量是旧代 `0x0C0`;单 exe 内候选冲突即跳过,全部无候选 = `None`);
3. 盘上学习改为确定性选举:系统槽(非 `saveNNNNN.sd`)优先,其次多数票,
   平票取较小版本;
4. 兜底默认只覆盖现代两代,并在诊断里明确记为「兜底」。

## 回归

- 用例:
  - `yu-save::state::tests::new_slot_uses_learned_official_version`
  - `yu-save::state::tests::system_slot_version_beats_poisoned_player_slot`
  - `yu-save::state::tests::injected_official_version_wins_over_learned`
  - `yu-save::state::tests::empty_dir_falls_back_to_codec_default_version`
  - `yu-engine::engine::save_version_probe_tests::*`
- 验证命令与原始结果:

```text
cargo test -p yu-save (带语料环境变量)      → 22 passed; 0 failed
cargo test -p yu-engine --lib              → 39 passed; 0 failed
cargo test -p yu-format --test yssd_roundtrip → 2 passed; 0 failed
内核 headless 启动 v474 旧运行时, --log-level debug
  → save container version 0x138 … from <0.474 exe>
内核 headless 启动 0.495 工程, --log-level debug
  → save container version 0x1e0 … from yu-ris.exe
```
