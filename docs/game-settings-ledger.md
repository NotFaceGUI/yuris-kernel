# 游戏设置域(コンフィグ)取证台账

> 对象:官方 0.495 `yu-ris.exe`(sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`)。
> 语料:ERIS 工程(官方 YSCom 0.495 全编译,`project-eris`)。
> 手册:0.495 `マニュアル/YU-RIS/html/{sound,window,system,font}`。
> 方法:脚本源码(ERIS `save2/data/script`,CP932)+ REA 静态(Ghidra 12.1.4)
> + **运行时内存读取**(官方引擎启动后 `ReadProcessMemory`,用于层树/字符表)。
> 本文只记录"设置项 → 引擎命令 → 官方行为 → 内核状态"的对齐链;逐命令细节
> 见 `docs/sound-ledger.md` 与 `docs/divergences/`。

## 0. 设置面分层

官方"设置"不是单一命令,分三层,混在一起会误判:

| 层 | 载体 | 谁写 | 谁读 | 内核归属 |
|---|---|---|---|---|
| 引擎进程设置 | `yscfg.dat`(根目录,128 B) | `エンジン設定.exe` | 引擎启动 `FUN_00469df2` → `FUN_00460924` | 宿主注入(仅第 10 项已映射) |
| 游戏内设置画面 | `ERIS/Scene/CONFIG/ES_CONFIG.yst` | 玩家 | 脚本自身 | 脚本 + 域命令 |
| 设置存储 | `es.GSD` / `es.LSD`(游戏变量,进存档) | 脚本 | 脚本 | 与引擎无关 |

`yscfg.dat` 与工程编译产物 `yscfg.ybn` 是**两个文件**:后者是工程级编译配置
(分辨率、扩展名探测序、标题),前者是玩家级运行设置。内核解析前者已在
`yu-format::yscf`;后者只注入了 `_WINDOW_INIT_POS`(第 10 项,DIV-RT-047)。

## 1. 音量链(设置面的核心,已对齐)

### 1.1 脚本侧结构

`es_Sound.yst`:

```text
es.SOUND.SETVOL
  ├─ es.SND.SETVOL.MASTER    SOUND[ID=":/S0/"        VOLUME=主音量]
  ├─ es.SND.SETVOL.<CH>.LS   SOUND[ID=$SND.LID(n)    VOLUME=频道整体音量]
  └─ es.SND.SETVOL.SND       逐条 SOUND[ID="<CH>/A1" VOLUME=频道×角色/通道音量]
```

- `$SND.LID(00..14)` = `"BGM/" "SE/" "VO/" "EBGM/" … "S2VO/"`(15 条频道)。
- 单条声音 ID = `频道 + "A"|"B" + 槽号`(双缓冲,A/B 交替)。
- 主音量与频道整体音量**不**进单条 VOLUME 的乘法,而是靠引擎的层继承。

`es.VOL.*.GET` 只算"频道×角色/通道"的乘积(`GSD`/`LSD` 各两路),返回 0..256。

### 1.2 引擎侧:路径树 + 祖先折叠(REA + 运行时)

**ID 规范化** `FUN_004141e4`(调用点 `0x44e235` SOUND、`0x44f500` SOUNDINFO、
`0x44f346` SOUNDEND;前缀指针 `[0x801718]`):

- 前缀 `"S0"`(`.data:0x80bf54`,启动时 `0x462592..0x4625a6` 复制)。
- 字符分类表 `0x689b80` 运行时只有三项非零:`'.'` `'/'` `':'`(运行时实测)。
- ID 不含这三者 ⇒ 快速路径:`前缀 + "/" + ID`;含任一者 ⇒ 慢路径:
  - 以 `:` 开头为绝对 ID(`:/S0/` → `S0`,`:S0` → `S0`),不加前缀;
  - 折叠 `//`、去掉非末段 `./`、`../` 回退一级、去掉末尾 `/`;
  - 段内空格 ⇒ 返回 -1,调用方整条命令不应用。
- 原 ID 以 `/` 结尾时返回 1(`SOUNDEND` 用它区分"递归停止子树")。

**层树**:`FUN_00414559` 按 `/` 逐段下行,`FUN_0041478c` 按 MurmurHash2
查子节点。**建节点规则**(`0x44ef31..0x44f245`):带 FILE 且 ID 不以 `/`
结尾 ⇒ 建叶子(`+0x30=0`);不带 FILE 且以 `/` 结尾 ⇒ 建容器(`+0x30=1`);
带 FILE 且以 `/` 结尾 ⇒ 错误 `0x18bac`;其余(lookup 失败且无 FILE/无末尾
`/`)⇒ 整条命令丢弃,不建节点。父链必须已存在(`FUN_00461328` 逐段查找,
缺段 `0x18786` 失败)——官方**不**补建中间节点,频道容器由脚本
`es.SND.INIT` 的 `SOUND[ID="BGM/" …]` 预先建出。节点布局:

| 节点偏移 | 含义 |
|---|---|
| `+0x00` | 第一个子节点 |
| `+0x0c` | 下一个兄弟 |
| `+0x10` | 父节点 |
| `+0x14` / `+0x18` | 名字指针 / 名字长度 |
| `+0x1c` | hash(最高位标记) |
| `+0x30` | 容器标记(路径中间段 = 1) |
| `+0x34` | 声音对象(容器也有,`malloc 0x1d0`) |

**有效音量** `FUN_00424dfc`:

```text
v = (E == 1) * VOLUME(自身 +0x148)
parent = 节点+0x10
if parent 有名字:
    v = v * FUN_00424dfc(parent) >> 8      // 每级截断
```

即 `有效音量 = 自身 × 频道 × 主音量`,每级整数 `>>8`(不是一次性浮点积)。
`SOUND[ID=":/S0/" VOLUME=v]` 改的就是根层 `S0` 自身,所以主音量对全树生效。

**容器改音量** `FUN_004238e0`(type 5,`0x42511c` 写入 `+0x148/+0x154/+0x150`
后调用):节点 `+0x30 != 0` ⇒ `FUN_00424db8` 递归把每个后代的**有效音量**
重新写进各自的 DirectSound 缓冲;叶子 ⇒ 自身有效音量直接写。因此主/频道
滑杆一动,正在播的所有声音即时生效。

**SOUNDINFO[VOLUME]** 读 `+0x148`(`0x44fb87`)——返回**自身**字段,不是
折叠后的有效值。

**SOUNDEND** `0x44f308`:不删节点,入队 type 0xe(`0x4253b8`):state==0
时无操作;否则清文件名 `+0x000`、停播、释放缓冲、清 START/RESTART、
state=0,**保留**元数据/TOTALTIME/位置/PAN/PITCH/LOOP/LOOPCOUNT/E/VOLUME。
带 `/` 结尾的 ID 递归停止所有叶子后代(`0x44f388` → `0x44f3a4`)。

### 1.3 运行时证据(官方引擎层树 dump)

官方引擎启动 ERIS 工程后 `ReadProcessMemory` 实测:

```text
root(空名) → S0(容器, vol=256, E=1)
              ├─ BGM(容器) → B1(叶子, 正在播)
              ├─ SE / VO / EBGM / … / S2VO(15 条频道, 均为容器)
              └─ ROOT(引擎内部层, 非 SOUND 命令建)
前缀 [0x801718] = "S0";字符表 0x689b80 非零项 = {'.','/',':'}
```

`SOUND[ID="BGM/"]` 与 `SOUND[ID=":/S0/"]` 都**没有**产生额外 `":"`/空名
节点 ⇒ 规范化路径确实生效(否则会出现 `S0/:/S0` 与 `S0/BGM/` 空名子节点)。

### 1.4 内核实现与验收

- `crates/yu-sound/src/path.rs`:`normalize`(快速/慢路径、绝对 ID、`./`
  `../`、末尾 `/`、空格拒绝、`trailing_slash`)。
- `crates/yu-sound/src/state.rs`:`Layer.container`;`SoundState::new` 建根层
  `S0`;`parent_exists` + 官方创建规则(带 FILE 建叶子、末尾 `/` 建容器、
  其余丢弃、父链必须已存在);`effective_for_key` 逐级 `>>8`;
  `apply_soundend`(type 0xe 释放语义 + 带 `/` 递归);`query`/`layer`/
  `apply_sound` 全部走规范化键。
- `crates/yu-sound/src/mixer.rs`:每块按 `effective_for_key` 取增益
  (`volume_gain` 仍是 `(v/256)^2`,REA `0x424e4c` 的 DSBVOLUME 曲线)。
- 单测:`layer_ids_are_path_normalized`、`effective_volume_folds_master_
  channel_and_sound`、`ancestor_e_zero_mutes_descendants`、
  `mixer_applies_effective_volume`、`soundend_releases_layer_but_keeps_node`、
  `soundend_with_trailing_slash_stops_subtree`、
  `layer_creation_follows_official_rules`;`path` 模块 7 项。

## 2. 功能设置映射表

脚本入口:`ES_CONFIG.yst`(画面)/`es_GSD.yst` 的 `es.GSD.REFLECT`(应用)。

| 设置项 | 脚本动作 | 引擎命令 | 官方行为 | 内核状态 |
|---|---|---|---|---|
| 主/BGM/SE/VO/环境音量 | `es.SOUND.SETVOL` | `SOUND[VOLUME=]` × 层树 | 见 §1 | **fixed**(本轮) |
| 画面模式(全屏) | `WINDOW[NO=0 FULLSCREEN=]` | WINDOW | 0/1 切换;MOVIE 播放中不可切 | 字段已存(`WINDOWINFO` 可读),宿主窗口切换属桌面宿主 |
| ウィンドウを常に手前 | `WINDOW[NO=0 Z=]` | WINDOW | 手册 0..3:0=前面/1=常に手前/2=最背面/3=主窗后 | 字段已存(`WindowState.z`);桌面宿主未 `SetWindowPos(HWND_TOPMOST)` |
| 非アクティブ時の動作設定(极性未取证) | `SYSTEM[BACKGROUNDEXEC=]` | SYSTEM | 进程策略,无脚本可见返回 | aligned-stub(域事件;宿主策略) |
| スクリーンセーバー防止 | `SYSTEM[SCREENSAVER=]` | SYSTEM | 同上(`0x41b42c` 是屏保窗口创建,不是命令处理体) | aligned-stub |
| 文字速度/既読速度/オート/スキップ速度 | `es.GSD(120,*)` + `es.TX.SPD.SET` | 无(脚本 `WAIT`/`TEXT`) | 纯脚本节流 | 不涉引擎 |
| フォント | `es.FONT.REFRECT` | `FONT[...]` | 字体/抗锯齿/颜色 | 已实现(`yu-font`) |
| ウィンドウ/バックログ透過度 | `\BT.A.SET`、`CG[... A=]` | CG 层 alpha | 图层不透明度 | 已实现(gfx) |
| 選択肢色/オート持続/スキップ持続/セーブ確認/テキストカウント | `es.GSD` 位标志 | 无 | 纯脚本 | 不涉引擎 |
| E-mote 揺れスケール | `es.EMOTE.CFG.SET` | EMOTE | — | `deferred(master)` |

### 2.1 设置目录(GSD 全量,来自 `es_GSD.yst:1..130` 的逐项注释)

`@es.GSD` = int 组 `[256,128]`(下标 `组*128+项`);`$es.GSD` = str 组 `[256]`。
下表按组归档;「应用路径」是脚本里真正消费它的命令/子系统。

| 组 | 项 | 含义 | 应用路径 | 内核 |
|---|---|---|---|---|
| 001/002 | 01..04 | 主/BGM/SE/VO 音量(两路) | `es.VOL.*.GET` → `SOUND[VOLUME=]` | fixed(层树) |
| 001/002 | 05..08 | 主/BGM/SE/VO 静音(0/1) | 同上(乘子) | fixed |
| 001/002 | 11..14 | E 系主/EBGM/ESE/EVO 音量 | 同上 | fixed |
| 001/002 | 15..18 | E 系静音 | 同上 | fixed |
| 003/004/005 | 01..20 | 频道别/角色别音量(G1/L1) | `es.VOL.*.GET` | fixed |
| 006/007/008 | 01..20 | 频道别/角色别音量(G2/L2) | 同上 | fixed |
| 009/010 | 01..N | 角色别音量 | `es.VOL.VO.GET` | fixed |
| 092/093 | 01..50 | 自动存档槽(1=最新) | 存档槽表/`SAVE[FILE]` | 由脚本+SAVE 覆盖 |
| 094/095 | 01 | 调试开关/调试列表 | 调试 UI | 脚本侧 |
| 096 | 01 | GAMEMAIN 活动组 | 按钮组 | 脚本侧 |
| 097 | 01 | 脸窗口开关 | 文本窗布局 | 脚本侧 |
| 101 | 01..06 | 窗口 WSX/WSY/MAX/DSP/WLX/WLY | `WINDOWINFO[WSX/WSY/MAXMODE/DISPLAYNO]` → `WINDOW[WSX/WSY]` | 字段已实现;宿主窗口尺寸注入 |
| 110 | 01 | ウィンドウを常に手前 | `WINDOW[NO=0 Z=]` | 字段已存;宿主 `SetWindowPos` 未接 |
| 110 | 02 | 非アクティブ時の動作設定(`es_GSD` 注释写「停止」、画面标签写「でも動作させる」,极性未取证) | `SYSTEM[BACKGROUNDEXEC=]` | aligned-stub(宿主策略) |
| 110 | 03 | スクリーンセーバー防止 | `SYSTEM[SCREENSAVER=]` | aligned-stub(宿主策略) |
| 110 | 04/05 | 未読スキップ(ボタン/CTRL) | 文本推进状态机 | 脚本侧 |
| 110 | 06/07 | 選択肢後もオート/スキップ持続 | 文本状态机 | 脚本侧 |
| 110 | 08/09 | 選択済選択肢の色/既読色 | `FONT[COLOR=]` | 已实现 |
| 110 | 11 | シーンジャンプ | 场景选择 | 脚本侧 |
| 110 | 12/13 | ムービー再生/カットイン | `MOVIE`/`YMV` | B14 域 |
| 110 | 14/15 | マウスカーソル番号/向き | `MOUSE[CURSOR=]` | 已实现(宿主 `cursor_icon`) |
| 110 | 16 | ウィンドウリサイズ | `WINDOW[NO=0 RESIZE=]` | 字段未用(宿主策略) |
| 111 | 01 | オート時の音声終了待ち | `SOUNDINFO[PLAY]` 轮询 | 已实现 |
| 111 | 02 | 音声停止タイミング | `SOUND[PLAY=]` | 已实现 |
| 111 | 03 | ボイスループ | `SOUND[LOOP=]` | 已实现 |
| 112 | 01 | 画面エフェクト無効(`es_GSD` 注释 0=オン/1=オフ) | 脚本节流 | 脚本侧 |
| 112 | 02 | 既読時のテキスト速度 | `TEXT[WAIT=]`/推进 | 已实现 |
| 113 | 01 | 現在のフォント番号 | `FONTINFO[NAME]`/`FONT[NAME=]` | 已实现(枚举序见 §4) |
| 113(str) | — | 現在のフォント名 `$es.GSD(113)` | `FONT[NAME=$es.GSD(113)]` | 已实现 |
| 114 | 01..15 | 終了/タイトル/シーン/QS/QL/セーブ/上書き/ロード/コピー/移動/削除/初期化/前後選択肢 の確認 | `DIALOG[YESNO=]`/自绘确认 | 已实现 |
| 115 | 01..17 | カットイン/テキストカウント/次音声アイコン/オートモード/自動選択/縁取り/影/バックログ透過度/太字/グラデ/ESC | `FONT`/`CG`/脚本 | 已实现 |
| 116 | 01..04 | カットイン演出 | 脚本 | 脚本侧 |
| 120 | 01..05 | テキスト/オート/ウィンドウ透過率/スキップ/演出スキップ 速度 | `TEXT`/`WAIT`/`CG[A=]`/脚本 | 已实现 |
| 121 | 01..08 | アイキャッチ/システムメッセージ/エフェクト/オートセーブ/アイコンサイズ | 脚本 | 脚本侧 |
| 125 | 01..05 | SuperAutoPlayMode | 脚本 | 脚本侧 |
| 131 | 01..04 | E-mote 揺れ量/スムース | `EMOTE` | deferred(master) |
| 141 | 01 | 起動回数 | 脚本计数 | 脚本侧 |
| 141 | 02 | 現在のスクリーンモード | `WINDOW[NO=0 FULLSCREEN=]` | 字段已实现;宿主切换未接 |
| 141 | 03 | 前回 GSD 保存時点の累計起動時間 | `@_OS_TIME(1)` | 已实现 |
| 150 | 01/02 | 最後のセーブロード頁/セーブ番号 | 存档 UI | 脚本侧 |
| 151 | 01..05 | クイックセーブ新規順 | 存档槽表 | 脚本侧 |

> 结论:设置项绝大多数是**脚本侧状态机**(读 GSD → 驱动 `TEXT`/`CG`/`FONT`/
> `SOUND`/`DIALOG`),引擎侧只有音量、窗口/系统策略、字体、光标、存档槽几类。
> 「设置能不能生效」取决于这些底层命令,而不是另有一套引擎设置接口。

### 2.2 设置的读取与存储(`save/*.sd`)

**文件分工**(官方产物自证,`<project-eris>/save/`):

| 文件 | 内容 | 读 | 写 |
|---|---|---|---|
| `gsd.sd` | 全局设置(GSD int/str/组块) | `es.GSD.INIT`(`es_GSD.yst:154..165`) | `\GSD.SAVE`(`es_GSD.yst:558..585`),`ES.CONFIG.END` 与标题开始都会触发 |
| `kidoku.sd` | 既読テキスト | `es.GSD.INIT`(`:136..141`) | `\GSD.SAVE`(`:601..602`) |
| `extra.sd` | CG/リプレイ/ムービー/BGM 解锁 | `es.GSD.INIT`(`:145..152`) | `\GSD.SAVE`(`:593`) |
| `seld.sd` | 直前の選択肢 | `es.GSD.INIT`(`ES.SEL.FILELOAD`) | `SAVE[FILE="seld.sd"]` |
| `saveNNNNN.sd` / `save3xxxx.sd` / `save5xxxx.sd` | 游戏存档(含缩略图块) | `LOAD[FILE DNO]` | `SAVE[DNO ID=…][FILE]` |

**命令形态**(`es_GSD.yst` 实测):

```text
读:FILEINFO[EXIST=1 FILE=$es.SAVEPATH+"gsd.sd" LET=@mRET]
    LOAD[FILE="gsd.sd" DNO=01 LET=@es.GSD()]    // int 组 → 脚本数组
    LOAD[FILE="gsd.sd" DNO=02 LET=$es.GSD()]    // str 组
    LOAD[FILE="gsd.sd" DNO=03 G_INT2=1]         // 组块 → 全局 G_INT2
    LOAD[FILE="gsd.sd" DNO=04 G_STR2=1]
写:SAVE[DNO=01 SET=@es.GSD()]
    SAVE[DNO=02 SET=$es.GSD()]
    SAVE[DNO=03 G_INT2=1]
    SAVE[DNO=04 G_STR2=1]
    SAVE[FILE="gsd.sd"]
```

- 块格式:`YSSD v0x1E0` + 0x400 项偏移表 + 块(`dno u32 + btype u8 +
  strict u8 + var u16 + clen u32 + ulen u32 + zlib[clen]`);`btype 0` =
  单变量(含数组),`btype 2` = 多变量组块(外层不压缩)。**官方 7 个
  `.sd` 产物 parse→rebuild 逐字节一致**(`yu-format` `tests/yssd_roundtrip.rs`)。
- 读写闭环已用官方 `gsd.sd` 做端到端验证(`crates/yu-cli/tests/
  settings_persist.rs`,两个 `#[ignore]` 测试):
  1. `gsd_master_volume_is_loaded_and_applied`:把 `GSD(001,01)` 改成
     128 → 启动到标题 → 声音层 `S0/BGM/B1` 的**有效音量 = 128**(读 + 应用);
  2. `gsd_settings_survive_resave`:同文件跑「标题→スタート」触发
     `\GSD.SAVE` → 重写后的 `gsd.sd` 仍是 4 块、DNO/类型/变量 id/维数
     与官方一致,主音量 128 原样保留;与官方文件的差异只落在运行期字段
     (`141,01` 启动回数、`141,03` 累计时间、`101,01..03` 窗口尺寸、
     `113,01` 字体号)。

**已知模型差异(音量重算时机)**:

- 官方在**设置音量时**把有效音量写进 DSB;若父节点是**叶子**(先
  `SOUND[ID="X" FILE=…]` 建叶子、再 `SOUND[ID="X/Y" FILE=…]` 挂子节点,
  官方不把 X 升级为容器),改 X 的音量只写 X 自己的缓冲,子节点 Y 的
  缓冲要等下一次 Y 的音量命令才更新。内核按渲染时刻折叠祖先链,改父层
  会立即影响全部后代。语料用 `SOUND[ID="BGM/" …]` 预建容器,不走这个
  边角;若后续样本出现叶子当父层,需要按官方改成"命令时刻快照"。

**已知残余差异(非存储格式)**:

- `101,01..03` 窗口 WSX/WSY/MAX:headless 内核按工程视口(800×600)回写,
  官方按真实窗口(如 2560×1369 最大化)回写。属宿主窗口注入,不是设置格式问题。
- `113,01` 字体号:`es_Font.yst` 用 `FONTINFO[NUM/LANG/NAME]` 枚举系统字体
  后选默认字体;内核的字体枚举(fontdb)与官方 GDI `EnumFontFamiliesExA`
  的**顺序/过滤**不一致,导致默认字体下标 0 vs 1(本机还伴随
  `mstmc.ttf malformed` 告警)。这是字体域的枚举保真问题,记为待查项。

## 3. 引擎级设置 `yscfg.dat`

- 文件:根目录 128 B = 32 × int32;`エンジン設定.exe` 写。
- 引擎:`FUN_00460924` 把 `\yscfg.dat` 读进 32 int(`FUN_00469df2`),
  再逐项写进进程全局(`0x460924` 反编译):

| 项 | 目标全局 | 语义(推断,未逐项取证) |
|---|---|---|
| 0 | `DAT_005b4874` | 0/1 开关 |
| 1/2/3 | `DAT_005b4040/4c/50` | 0/1 开关(疑纹理过滤三项) |
| 4 | `DAT_005b480c` | 0 ⇒ 1(反相) |
| 5 | `DAT_0080132c` | 0/1 |
| 6 | `DAT_00801330` | 0 ⇒ 1、1 ⇒ 0(反相) |
| 7 | `DAT_005b4848` | 0..3 → 1..3/0(循环映射) |
| 8 | `DAT_00801334/005b484c/008012e8/008012e4` | 模式 0..3(疑画面模式) |
| 9 | `DAT_00801338` | 0/1 |
| 10 | `DAT_0080133c` | **`_WINDOW_INIT_POS`**(DIV-RT-047 已映射) |
| 12 | `DAT_00801310` | 0/1 |
| 13 | `DAT_00801314` + `DAT_00801d54/_00801d50` | 0/1;决定两组常量(0x15/0x20 vs 0x1a/0x10) |
| 14 | `DAT_005b4800` | 0/1 |
| 15 | `DAT_00801318` | 0..2 |
| 22/23 | `DAT_008015c8/00801340` | 0/1 |
| — | 固定 `DAT_00801dc0=0x200`、`0x1dc4=0x100`、`0x5b4804/08=1` | 引擎默认 |

- **结论**:`yscfg.dat` 全部是**引擎进程/渲染/窗口**级开关,没有游戏功能
  设置(音量/文本/演出都在 `gsd.sd`)。按主人令「窗口系统类变量可以不管」,
  除已映射的第 10 项外全部**挂账**;语义表留给后续按 `エンジン設定.exe`
  控件逐项对号(手册无页)。
- 注意:运行时 `yscfg.dat` 的取值(如 `0100 0000` = 1)是**玩家设置**,
  不能当作工程编译配置写进 `yscfg.ybn` 的解析。

## 4. 未决/挂账

1. **WINDOW Z / RESIZE / A 的宿主落地**:官方 Z 是窗口叠放顺序(手册原文),
   RESIZE 是窗口可缩放开关,A 是窗口不透明度;内核 `WindowState` 只存字段
   (WINDOWINFO 可读回),桌面宿主尚未 `SetWindowPos`/切换 resize/设置
   分层窗口。属宿主能力面(B7 同族),不是内核语义缺口。
2. **`FPS[SKIP=]`**:内核把 SKIP 存进 VM 状态(`handlers.rs:1126`)但渲染
   循环不消费;官方语义 = 落后时允许丢帧(手册 `system/fps.html`)。内核
   为保证确定性每 tick 都渲染,属宿主呈现策略;桌面宿主可按 SKIP 丢 present。
3. **SYSTEM 两参数**:`BACKGROUNDEXEC`/`SCREENSAVER` 是进程策略,
   官方处理体未逐参数定位;维持 aligned-stub(域事件),平台层可消费。
4. **空格 ID 拒绝**:慢路径空格检查的调用方 flag(`[esp+0x10]`)未被
   SOUND 处理体显式写入(栈残值);语料无含空格 ID,按"拒绝"实现并
   记 `Likely`。
5. **`yscfg.dat` 其余 31 项**:见 §3。

## 5. 验证命令(原始结果见 PROGRESS 成果 SND-LAYER-TREE / SETTINGS-PERSIST)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
cargo test -p yu-sound                       # 51 passed
cargo run -p yu-cli --bin yu-cli -- case "$YURIS_SAMPLE_ERIS_ROOT" --all
cargo run -p yu-cli --example sound_census -- 300   # 层树 + eff 音量
python tools/rea/sound_tree_probe.py 6              # 官方层树/前缀/字符表(只读)
# 设置读写闭环(读官方 gsd.sd → 应用 → GSD.SAVE 重写)
cargo test -p yu-cli --test settings_persist -- --ignored --nocapture
# → 2 passed（主音量 128 生效；重写保留设置与块结构）
```
