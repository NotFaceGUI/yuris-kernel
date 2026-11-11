# 任务书 B14-S · `\MOVIE(...)` 宏路径影片态调度(修正版结论)

> 状态:**已实现(2026-11-11)**。原任务书按「场景任务先于系统任务消费
> `SCENARIOSTOP`」立项;实测推翻该口径:调度/让出语义本身正确,真正的
> 断点是 `MOVIEINFO[CODEC]` 探测错对象。下文保留取证过程与修正依据。
>
> 仓库:`<repo>`;实现文件:
> `crates/yu-rt/src/vm/handlers.rs`(`op_movieinfo`)、
> `crates/yu-rt/tests/fixture.rs`(回归)。

## 1. 现象与原始假设

`\MOVIE(file)` 宏展开为:

```text
GOSUB[#=es.MOVIEPLAY.SET pstr=$_M pint2=$_M2] WAIT[FRAME=1]
```

`es.MOVIEPLAY.SET` 只写 `$FN` / `@SkipFlag` / `@es.SCENARIOSTOP=2`;
真正播片由系统任务(Z=100)的 `ES.GAMEMAIN.LOOP` 在下一帧
`IF[@es.SCENARIOSTOP==@e.MOVIE]` → `es.S.GOSUB("es.MOVIEPLAY")` 消费。

原假设:场景任务(`es.ScenarioScript`,Z=2)在 `WAIT[FRAME=1]` 后先于
系统任务继续,随后 `\TITLE` 把 `SCENARIOSTOP` 改回 1,导致消费未发生。

## 2. 修正后的根因(有运行时 + REA 证据)

### 2.1 调度链实际正常

用真实 MOV 测试工程(`target/tmp/movie-clean`,官方 0.495 SDK 编译;
`SCENARIO_START → SCENARIO_LOGO → SCENARIO_OPMOVIE → \MOVIE(pac/op_movie.mpg)`)
逐 tick 记录任务表:

```text
t89  f92  s139 pc3 GOSUB es.MOVIEPLAY.SET   ← 场景任务写入 SCENARIOSTOP=2
t89  f92  s139 pc4 WAIT[FRAME=1]
t90  f93  s20  pc75 IF → Int(1)             ← 系统任务(先于 Z=2)消费成功
t90  f93  s20  pc76 GOSUB es.S.MOVIEPLAY
t91  f94  s13  pc22 MOVIEINFO[CODEC=...]
```

- 任务链顺序 = Z 降序(系统 100 先于场景 2),`FRAME=1` 在下一逻辑帧苏醒,
  与手册 `basic/task.html`「優先度の高い順」一致;
- `SCENARIOSTOP!=0` 时 `ES.GAMEMAIN.LOOP` 调 `\es.TXTASK.SN.E0` →
  `_task.pause($es.ScenarioTaskID)`(`TASK[E=0]`),场景任务在影片期间
  `enabled=0`,不可能先跑 `\TITLE`;
- 因此**不需要改调度器/WAIT 语义**;原「场景先跑」现象是下一节断点的
  次生结果(播片立即放弃 → `SCENARIOSTOP=0` → 场景恢复 → `\TITLE` 写 1)。

### 2.2 真正断点:`MOVIEINFO[CODEC]` 探测了当前已打开影片

`es_MoviePlay.yst`(`#=es.MOVIEPLAY.LOOP`)在 `MOVIE[FILE]` **之前**:

```text
MOVIEINFO[CODEC=$FN LET=@codeccheck]
IF[@codeccheck==0] IFBREAK[LV=3]     ← 文件不存在则整个播片放弃
```

内核旧实现:`CODEC` 分支读 `self.movie.file`(即 `MOVIE[FILE]` 的当前
影片);此时尚未 `MOVIE[FILE]`,恒为 `None` → 写 0 → `IFBREAK`。
运行时 trace 落在 `s13 pc30 IF → Int(1)`(`codeccheck==0`),随后
`s13 pc36 LOOPBREAK` / `pc87 LET SCENARIOSTOP=0` / `pc88 es.S.RETURN`。

REA(0.495 `0x44ba2c`,sha256 `4f5eefa3…138f`)CODEC 分支:

```text
44baba  movzx edx,[0x684166]      ; CODEC 参数存在标志
44bac3  je 0x44bbe3               ; 未给 CODEC → 不写 LET
44bac9  mov ecx,[edx*4+0x683fc0]  ; CODEC 参数值(文件名字符串)
44bad4  mov edi,0x681300          ; CWD/工程基路径
44bad9  …拷贝基路径到栈缓冲…
44baf3  …把参数串追加到基路径尾部…
44bb01  call 0x45f348             ; 路径规范化('/'→'\')
44bb42  call 0x469df2             ; fopen
44bb84  读 5 字节头:00 00 01 BA + 次字节高位 → 1/2/255
```

⇒ 官方 `CODEC` 探测的是**参数本身**(基路径 + 参数),不是
`0x7fab00`(`MOVIE[FILE]` 的当前文件名)。旧实现对「存在标志」也有偏差:
官方按参数存在选择字段,`PLAY=0` 也应写回 0;旧实现只认非零数值。

### 2.3 为什么 `\TITLE` 会被观察到改回 1

播片在 `MOVIEPLAY.LOOP` 一开始就被 `IFBREAK` 放弃 → `es.S.RETURN(996,0)`
恢复场景任务;场景继续 `\WA(500)` 后到 `SCENARIO_TITLE`,
`macro.yst` 的 `\TITLE` 展开首指令即 `@es.SCENARIOSTOP=1`。这是**失败后**
的正常脚本走向,不是调度竞争。

## 3. 实现

`op_movieinfo`:

1. 非 `LET` 参数按**存在**(`w.rec.length != 0`)进入字段候选,不再过滤
   零值(`PLAY=0` 正确写回 0);
2. `CODEC` 分支保存该参数的字符串值,用 `probe_movie_codec(&参数)` 探测;
   参数缺失/非字符串 → 0;
3. 探测结果规则不变:不存在 0、MPEG-1 1、MPEG-2 2、其它 255
   (`yu_format::movie::MovieCodec::official_code`)。

回归 `movie_state_machine_and_info_queries`:把 `MOVIEINFO[CODEC=...]`
移到 `MOVIE[FILE]` 之前,用真实文件名断言写回 1,直接覆盖宏路径的预检。

### 3.2 `MOVIEEND` 后 UI 快照恢复(`yu-gfx`)

影片帧(`movie_only_frame`)只投影影片层,后端 `prune_snapshots` 按该帧
回收按钮/UI 的快照纹理;`GfxState.baked_copies` 仍认为对应 COPY 已冻结,
`frame()` 继续跳过重发 ⇒ `MOVIEEND` 后底部 `GAMEMAIN.BTN.*`(快照纹理)
永久黑,只剩 SAVE(资产纹理)。修法:退出影片模式时清空 `baked_copies`,
下一帧按 `copy_log` 重发一次 COPY,后端用 `source_texture`(按钮
sprite sheet + `COPY2` 区域)重建快照。一次性,不进逐帧热路径。

回归 `movie_overlay_exit_replays_baked_copy_snapshots`:建 `SRC→DST`
COPY2、`commit_baked` 标记冻结、进/出影片模式,断言退出后 COPY 重发且
DST 仍引用快照。

## 4. 验收(原始结果)

### 4.1 真实 MOV 工程

`target/tmp/movie-clean`(官方 SDK 0.495 编译)headless:

| 帧 | 画面 |
|---|---|
| 200 | OP 影片实际帧(`POWERED BY yu-ris`) |
| 320 | 影片末帧(MOVIEEND 前) |
| 500 | `SCENARIO_TITLE` 标题菜单(按钮组正常) |

逐 tick 相位:`movie_mode=1` 从 f94 持续到片尾;场景任务在影片期间
`s139 pc5 enabled=0`;`MOVIEEND` 后 `SCENARIOSTOP=0`,场景恢复并进入
`SCENARIO_TITLE`。

debug 模式进入主菜单(`--debug-mode on`,shot `target/tmp/shot-f1100-soft.png`):
底部 15 个按钮(セーブ/ロード/クイックセーブ/…/メニュー)与官方
0.495 截图逐项一致;修复前这些按钮的快照纹理全部为黑,只剩 SAVE。

### 4.2 构建/测试/基准

原始结果(本机,2026-11-11):

```text
cargo build --workspace --all-targets
  Finished `dev` profile ... 0 error / 0 warning

cargo test -p yu-rt -p yu-engine -p yu-gfx --lib
  yu-engine: 32 passed; yu-gfx: 15 passed; yu-rt: 94 passed; 0 failed

cargo test -p yu-gfx --test ymv_movie movie_overlay
  2 passed(含新增 movie_overlay_exit_replays_baked_copy_snapshots)

cargo test -p yu-rt --test fixture movie_state_machine_and_info_queries
  1 passed(CODEC 查询在 MOVIE[FILE] 之前)
```

真实工程验收:

```text
target/tmp/movie-clean, --frames 200/320/500(soft/wgpu)
  frame 200 = OP 影片帧("POWERED BY yu-ris")
  frame 320 = 影片末帧
  frame 500 = SCENARIO_TITLE 标题菜单
  逐 tick:movie_mode=1 f94..片尾;场景 s139 pc5 enabled=0;MOVIEEND 后恢复
```

性能(对照 = `git archive HEAD` 未改动基线,同机 release):

```text
movie-clean bench --frames 12
  ours:     steady avg 24.04 ms, slowest frame 6 = 112.3 ms
  baseline: steady avg 23.19 ms, slowest frame 6 = 108.2 ms
  ⇒ 1.3M 命令突发帧 ~86 ns/命令(基线 ~83–84 ns),差异在噪声内

kotoneiro bench --frames 2100
  ours:     avg 34.93 ms(28.6 fps),p95 56.97 ms,slowest 249 = 226.0 ms
  baseline: avg 37.31 ms(26.8 fps),p95 60.00 ms,slowest 249 = 249.1 ms
  ⇒ 未劣化(本次改动不在逐命令/逐帧热路径)
```

`case --all`(project-eris)说明:本机当前金标与 HEAD 不一致,未改动基线
同样失败(`first-screen`/`until-visible` 的 actual display hash =
`f67a4150…`,与本次改动构建逐字相同);`save-load-loop`/`save-title-flow`
两例通过。该门禁失败属既有金标/`until-visible` 早停漂移,不计入本次
改动回归;未擅自 `--update` 重写金标。

## 5. 性能红线

`op_movieinfo` 只在影片族命令处理体内(语料低频);`set_movie_overlay`
只在进/出影片模式时跑一次,退出时清 `baked_copies` 是 O(已烘焙 COPY 数),
随后一帧重发 COPY 是一次性过渡成本,不进入逐命令/逐帧路径。既有
`frame()`/`prune_snapshots` 循环未改。基准见 §4.2:1.3M 突发帧
≈86ns/命令(基线 ≈83–84ns,噪声内),koto 2100 帧 avg 34.93ms
(基线 37.31ms,未劣化)。
