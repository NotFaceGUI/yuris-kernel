# DIV-SND-005 · 声音层是路径树(音量按祖先继承),不是扁平表

状态:**fixed(2026-10-29;内核改为官方同构的路径树 + 有效音量折叠)**

## 现象

设置画面的**主音量**与**频道整体音量**(BGM/SE/VO/环境音)滑杆在内核里
没有可听效果:脚本把主音量写在 `SOUND[ID=":/S0/"]`、频道音量写在
`SOUND[ID="BGM/"]`,而内核把 ID 当扁平键,只认 `BGM/A1` 这类最终键,
父层音量不参与混音。

`SOUNDEND` 后 `SOUNDINFO[EXIST]` 也从 1 变 0(节点被删),与官方相反。

## 官方行为(0.495 REA + 运行时内存)

- **ID 规范化** `FUN_004141e4`:前缀 `"S0"`(`.data:0x80bf54`,
  `[0x801718]`);ID 含 `.`/`/`/`:` 走慢路径,支持 `:/` 绝对写法、
  `//` 折叠、`./`、`../`、末尾 `/`;空格 ⇒ -1(整条命令不应用)。
  字符表 `0x689b80` 运行时实测只有 `'.'(0x2e)`、`'/'(0x2f)`、`':'(0x3a)`
  三项非零。
- **层树**:`FUN_00414559` 按 `/` 逐段下行;节点 `+0x10` 父、`+0x00`
  首子、`+0x0c` 兄弟、`+0x30` 容器标记、`+0x34` 声音对象。
- **有效音量** `FUN_00424dfc`:`v = (E==1)*VOLUME; v = v*parent_eff>>8`
  (每级截断)。`SOUND[ID=":/S0/" VOLUME=]` 改根层 `S0` 自身,主音量因此
  对全树生效。
- **容器改音量** `FUN_004238e0`(type 5):`+0x30 != 0` ⇒
  `FUN_00424db8` 递归重写全部后代的有效音量;叶子直接写自身。
- **SOUNDINFO[VOLUME]** 读 `+0x148`(`0x44fb87`),返回自身字段。
- **SOUNDEND** `0x44f308` 不删节点:入队 type 0xe(`0x4253b8`)释放
  声音对象(state==0 时 no-op),保留元数据/TOTALTIME/位置/PAN/PITCH/
  LOOP/LOOPCOUNT/E/VOLUME;ID 以 `/` 结尾时递归停止所有叶子后代
  (`0x44f388` → `0x44f3a4`)。

运行时层树(官方引擎跑 ERIS 后 `ReadProcessMemory`):

```text
root(空名) → S0(容器) → BGM(容器) → B1(叶子, 播放中)
                        → SE / VO / … / S2VO(15 条频道容器)
prefix="S0"; 0x689b80 非零项={'.','/',':'}
```

`BGM/` 与 `:/S0/` 都没有产生 `S0/BGM/` 空名子节点或 `S0/:/S0` 节点,
证明规范化路径确实生效。

## 内核行为(修复前)

`crates/yu-sound/src/state.rs`:层表 `BTreeMap<Vec<u8>, Layer>` 直接用
脚本 ID 作键;`Layer::audible_volume()` 只取自身 `E × volume`;混音器
按自身音量出增益;`SOUNDEND` 走 `remove_layer` 删表项。

## 根因

把官方"名字树"当成了"扁平层表",漏了 `FUN_004141e4` 的路径规范化与
`FUN_00424dfc` 的祖先折叠;`SOUNDEND` 也误判为删除节点。

## 修法

- 新增 `crates/yu-sound/src/path.rs`:`normalize`
  (含快速/慢路径、绝对 ID、`./`/`../`、末尾 `/`、空格拒绝)。
- `state.rs`:`Layer.container`;`SoundState::new` 建根层 `S0`;
  `apply_sound`/`query`/`layer` 全部走规范化键;创建规则按官方
  (`0x44ef31..0x44f245`):带 FILE 建叶子、末尾 `/` 建容器、其余丢弃、
  父链必须已存在;`effective_for_key` 逐级 `>>8`;`apply_soundend`
  实现 type 0xe 释放语义与带 `/` 的递归停止。
- `mixer.rs`:每块按 `effective_for_key` 取增益(`(v/256)^2` 曲线不变)。

## 回归

- 单测:`crates/yu-sound/tests/state.rs`
  `layer_ids_are_path_normalized` / `effective_volume_folds_master_channel_and_sound` /
  `ancestor_e_zero_mutes_descendants` / `mixer_applies_effective_volume` /
  `soundend_releases_layer_but_keeps_node` / `soundend_with_trailing_slash_stops_subtree`、`layer_creation_follows_official_rules`;
  `crates/yu-sound/src/path.rs` 7 项。
- 语料:官方 ERIS 工程 `yu-cli case --all`(见 PROGRESS 成果 SND-LAYER-TREE)。
- 证据锚点与完整推导:`docs/game-settings-ledger.md` §1、
  `docs/sound-ledger.md` N0-14。
