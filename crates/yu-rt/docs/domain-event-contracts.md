# 域事件载荷契约(SOUND / SAVE / FILE / EMOTE)

> 任务书 M-2 的域侧交付:**只做事件建模,不做子系统**。yu-rt 不打开音频、
> 不读写存档、不碰文件系统;它只把脚本命令求值后的参数作为
> `VmEvent::DomainCall { frame, domain, name, args }` 发出。
>
> 证据:①手册页(相对 `<sdk495>/マニュアル/YU-RIS/html/`,CP932 原文句);
> ②0.495 ERIS 语料实测;③0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)
> 的分发表 `DAT_007ace20[opcode]`。缺③的字段只描述手册/语料,不声称引擎行为。

## 1. 消费方式(宿主/未来的 yu-sound、yu-save)

槽号(`ArgValue::slot` = 工程 `ysc.ybn` 的参数序号)**不跨版本稳定**。宿主应
按名字消费:

```rust
let Some(args) = event.domain_args(&vm.tables().boot.yscm) else { return };
for a in args {
    match (event_name, a.name.as_ref()) {
        ("SOUND", "ID") => { /* a.value = Value::Str */ }
        ("SOUND", "VOLUME") => { /* a.value = Value::Int, 0..256 */ }
        _ => {}
    }
}
```

`domain_args` 是加法式 API:事件字段不变,只把槽号映射成参数名;
查不到命令名时名字退化为 `@<slot>`。

## 2. SOUND(手册 `sound/sound.html`)

处理体(0.495 分发表 opcode 87):`0x44e1e8`。

| 参数 | 类型 | 值域/语义(手册原文) | 说明 |
|---|---|---|---|
| `ID` | 字符串 | サウンドデータを置くレイヤＩＤ | 宿主侧图层键 |
| `FILE` | 字符串 | 拡張子省略可;查找顺序「(拡張子なし)」「.OGG」「.WAV」 | 相对工程数据根 |
| `E` | 0/1 | レイヤの有効無効 | |
| `PLAY` | 0/1 | １で再生、０で停止 | |
| `LOOP` | 整数 | ０＝無限ループ、１以上＝再生回数 | |
| `FADE` | 整数 | フェードイン／アウトの時間 | 单位手册未写,按引擎毫秒惯例待复核 |
| `PAUSE` | 0/1 | １で一時停止、０で再開 | |
| `START` | 整数 | 再生開始位置(ミリ秒);負値=遅延再生 | |
| `RESTART` | 整数 | ループ２度目以降の開始位置(ミリ秒);負値=遅延 | |
| `END` | 整数 | 終端位置／ループ位置(ミリ秒) | |
| `VOLUME` | 整数 | **0～256** | 语料实测出现 256(最大值) |
| `PAN` | 整数 | **0～512**(左～右);デフォルト 256 | |
| `PITCH` | 整数 | **0～512**(0.5 倍～1.5 倍);デフォルト 256 | |

语料实测(600 帧,域事件样本):

```text
ID=BGM VOLUME=256 E=1
ID=E2BGM VOLUME=256 E=1
ID=VO VOLUME=256 E=1
ID=:/S0/ VOLUME=0
```

`SOUND` 的 531 次触发里,绝大多数是初始化 `VOLUME=256 E=1`;其余 16 种
组合共 515 次(含 `FILE/PLAY/LOOP`)。**值域校验应由 yu-sound 做**;yu-rt 不
因为 256/512 之外的脚本值改语义。

## 3. SOUNDINFO(手册 `sound/soundinfo.html`)

处理体(0.495 分发表 opcode 89):`0x44f4bc`(域事件;查询需 `yu-sound` 状态机)。

参数:查询标志(`EXIST/FILE/E/PLAY/LOOP/PAUSE/LOOPCOUNT/START/RESTART/
END/VOLUME/PAN/PITCH/TIME/TOTALTIME/CHANNEL/BIT/SAMPLE/CODEC`)+
`LET`(结果变量)+ `ID`(图层)。

`CODEC` 值域(手册):`0=WAVE / 1=OGG / 2=ACM WAVE`。
其余字段的返回类型由 `LET` 目标类型决定;`FILE` 返回字符串,其余为整数。

**LET 写回（N 任务已接）**:`LET` 在引擎里是变量引用,`DomainCall.args`
仍只带求值结果;但 VM 在 `op_soundinfo` 里自己 `eval_lvalue` 持左值,
把其余参数交给 `SoundHost::query`,再把返回值 `write_target` 写回。
因此 SOUNDINFO **不需要**给 `ArgValue` 加引用字段(与 `CGINFO` 的
`op_info` 同型)。`SAVE/LOAD/FILEINFO` 的引用身份仍是 §6 的 open 项。

## 4. SAVE / LOAD / SAVEEND(手册 `file/save.html` / `file/load.html` / `file/saveend.html`)

| 命令 | 参数 | 值域/语义 |
|---|---|---|
| `SAVE` | `FILE` | データファイル名;`.sd` 可省略;固定在 `save` 文件夹 |
| `SAVE` | `DNO` | **1～1024** 数据番号 |
| `SAVE` | `SET` | 保存する変数(标量或 `@ARR()` 全数组);**变量引用** |
| `SAVE` | `ID` | 保存するレイヤーＩＤ(图层快照) |
| `LOAD` | `FILE`/`DNO` | 同上 |
| `LOAD` | `LET` | 読み出した値を格納する変数;**变量引用** |
| `LOAD` | `ID` | レイヤーＩＤ |
| `SAVEEND` | — | `SAVE` の予約保存データを初期化 |

关键语义(手册 `SAVE` 例 6):`FILE` 之前的 `SAVE` 只做**予約保存**;
`FILE` 出现时一次性写出。`SAVEEND` 丢弃未写出的预约数据。这些是
yu-save 的职责;yu-rt 只保证命令顺序与参数求值顺序忠实。

语料实测(16 次 `SAVE`):

```text
FILE=extra.sd / gsd.sd / kidoku.sd / seld.sd
DNO=1..41
DNO=12 SET=2        ← SET 是变量引用,事件里只剩求值结果 2
DNO=22 SET=0
DNO=3 G_INT2=1      ← 声明组参数由工程 ysc 表命名
DNO=4 G_STR2=1
```

**警告**:`SET=2` 不代表脚本写的是字面量 2;它可能是 `SET=@SOMEVAR` 的当前值。
yu-save **不得**把 `SET` 当字面量存盘。

## 5. FILEINFO / FILEACT(手册 `file/fileinfo.html` / `file/fileact.html`)

`FILEINFO` 参数:

| 参数 | 类型 | 语义 |
|---|---|---|
| `FILE` | 字符串 | 文件名;**与 `FOLDER` 不可同时指定** |
| `FOLDER` | 字符串 | 文件夹名 |
| `LET` | 变量引用 | 结果变量 |
| `EXIST` | 0/1 | 存在性 |
| `SIZE` | 0/1 | 文件大小 |
| `RELATION` | 0/1 | 关联执行文件名 |

手册注:ver0.276 起可查包内文件;包内文件夹的存在性仍查不到;包内文件
大小也查不到。`RELATION` 返回字符串(如 `notepad.exe` 全路径)。

`FILEACT` 参数:

| 参数 | 类型 | 语义 |
|---|---|---|
| `FILE` | 字符串 | 操作対象 |
| `FILE2` | 字符串 | 复制目标(COPY) |
| `FOLDER` | 字符串 | 空文件夹删除对象 |
| `EXEC` | 0/1 | 执行/关联打开 |
| `COPY` | 0/1 | 复制 |
| `DELETE` | 0/1 | 删除存档数据或空文件夹 |
| `PARAM` | 字符串 | 传给执行文件的参数 |
| `ID` | 字符串 | 图层 ID(图层导出) |
| `BMP`/`PNG`/`JPG` | 0/1 | 图层导出格式 |
| `SET` | 整数 | PNG 压缩 0..9(默认 5)/ JPG 品质 0..100(默认 90) |

这些命令涉及真实 I/O,**yu-rt 永不执行**;宿主实现时必须自己做路径
归一化与越界检查(`save` 固定目录、包内只读等)。

## 6. 契约缺口(事件层,非子系统)

1. **变量引用身份丢失**:`SAVE[SET=@X]` / `LOAD[LET=@Y]` /
   `FILEINFO[LET=@Z]` 在引擎里传的是引用,`DomainCall.args` 只有求值结果。
   修复需要给 `ArgValue` 增加引用字段或新增事件,均属跨 crate 契约变更
   (当前 `yu-trace` 对新事件变体没有兜底分支)。台账:`DIV-RT-042`。
   **例外（已解决）**:`SOUNDINFO[LET=@W]` 由 VM 侧 `op_soundinfo` 持
   `LValue` → `SoundHost::query` → `write_target`,事件层不变。
2. **没有 script/pc**:`DomainCall` 只有 frame/domain/name/args,宿主无法
   把一次域调用定位回脚本。当前只能按 frame+name+参数名关联。
3. **没有求值顺序**:`eval_arg_windows` 按窗口顺序求值;引擎参数求值顺序
   未逐命令取证。对无副作用的表达式无影响;对含 `MATH[HASH/RND]` 的
   参数(当前 RND 挂账)未来需要复核。

在这些缺口关闭前,宿主应以"事件是**事实记录**,不是可重放的命令对象"
为前提设计接口。

## 7. EMOTE / EMOTEINFO(证据级;运行时是独立子系统)

处理体:`EMOTE` `0x4405a4`,`EMOTEINFO` `0x441014`。内核只发
`Domain::Emote` 域事件,不加载 E-mote、不解析 PSB、不渲染。

### 7.1 官方接入事实

- 引擎 `FUN_004661ec` 动态加载 `YSEmote.dll`(先 `LoadLibraryA`
  `emotedriver.dll` 探测存在性),再 `GetProcAddress` ordinal `0x1..0x3`、
  `0x65..0x100`;DLL 缺失时引擎照常启动。
- `YSEmote.dll` 导入 `emotedriver.dll` 的 `EmoteCreate`/`EmoteFilterTexture`;
  `emotedriver.dll` 自带 58 个 D3D9 shader(不是引擎 FXO 的 4 个)。
- 模型是 `.psb`(语料 `cg/stand/emote_test.psb`);ERIS 脚本层通过
  `\SP.ST` 加载,`\SP.EM.VAR/PLAY/DPLAY/STOP/DSTOP/VO` 驱动参数/时间线
  (`E-moteERIS命令マニュアル.txt`)。

### 7.2 语料实测参数(不是完整表)

120 帧 census(`project-eris`):

```text
EMOTE ×8:BUST=128 ×2 / HAIR=128 ×2 / PARTS=128 ×2 / SMOOTH=1 ×2
EMOTEINFO:静态 3 处,本窗口 0 次
```

### 7.3 处理体行为(2026-10-22 反编译取证)

`EMOTE`(`0x4405a4`)在 `DAT_00801750 != 0` 时先按“角色名 + '.' + 参数值”
解析对象,取 `obj+0x61c` 的 E-mote player,然后:

- 字符串参数(type 2)→ `FUN_00465ff0(player, name, value)`;
- 标志参数(type 1)→ `FUN_00466074(player, name)`;
- type 0 → `FUN_00465fd8(player)`;
- 可见/启用 → `(*DAT_00801404)(player, 0|1)`(YSEmote 序号 212 →
  player 槽 11,`player_impl+0x30` byte);
- 批量数值变量 → `(*DAT_00801458)(player, id, value, time*0.06, 0)`
  (序号 233 → player 槽 32)。`0.06` 是脚本时间单位到播放器时间的换算。

`EMOTEINFO`(`0x441014`)调用 `(*DAT_0080148c)`(序号 246 → player 槽 45),
结果与 `1` 比较;字段语义仍未完整取证。

### 7.4 契约字段

| 字段 | 类型 | 证据/语义 |
|---|---|---|
| 角色名 | 字符串 | 第一条参数;用于解析对象,不进 player |
| `BUST`/`HAIR`/`PARTS` | 整数 0..256 | 语料实测 128;按名透传给 player 变量 |
| `SMOOTH` | 整数 | 语料实测 1 |
| 字符串参数 | 字符串 | 时间线/动作名,交给 `FUN_00465ff0` |
| `time` | 整数/浮点 | 数值变量的淡入时间,×0.06 秒 |

- 其余参数名、类型、值域**未取证**;宿主应按名字消费并忽略未知参数,
  不得把槽号写死(`domain_args` 已把槽号映射成名字)。
- `EMOTEINFO` 的查询字段与 `LET` 写回未取证;当前不写 `LET`
  (与 §6.1 的引用身份缺口同源)。
- `.psb` 在 `CompositorFrame` 中的表达已裁决:宿主/`yu-emote` 产出
  **RGBA 纹理 + 图层变换**,不新增 `Op::Emote`。
- 视觉语义(PSB 模型/网格/时间线)不在 VM 事件契约内;见
  `docs/emote-parity.md` 与 `docs/tasks/task-emote-runtime.md`。
