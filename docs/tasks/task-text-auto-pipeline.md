# 任务书 · 文本管线三缺口(H-P2-04 / H-P2-01 / H-P2-02)

> 状态:**取证完成;主人已裁决(2026-11-04)**:
> H-P2-01 = 方案 B(`id: Vec<u8>`);H-P2-04 = 按现有实现统一语义,
> **不做大范围重构**(M2 表面模型登记为 DIV-GFX-075,后续再做)。
> 落地结果见 §12。本文取代 `docs/tasks/task-p2-text-pipeline.md` 与三份
> H-P2 交接件中与证据冲突的假设;冲突逐条列在 §5,先改档案再写码。
>
> 证据基线:
> - 官方引擎 `0.495` `yu-ris.exe`,sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`;
>   用 Ghidra 12.1.4(REA)逐指令复核,本文件引用的地址均为**文件内 VA**。
> - 手册 `0.495`:`text/text.html`、`text/textinfo.html`、
>   `basic/svariable.html`、`basic/basic.html`。
> - 语料 `<project-eris>`:141 个脚本(`census_params`,2026-11-04 实测)。
> - 旧线 `<repo-old>` 只作事实对照,未复制代码。

---

## 0. 一页结论

1. **`+0x6c` 不是“已显字符数”**,而是 `_TEXT_FLAG` 的**消耗游标**
   (含 `C/R/P/E` 控制标志);`+0x68` 是文本字节游标;`+0x60/+0x64`
   是文本/标志总数。已显字符数 = `flags[0..+0x6c]` 中 `'1'/'2'` 的个数。
2. **WAIT 是“每字符延迟”,不是“每帧一字符”**;单位 ms,时间源
   `timeGetTime()`(0x404c21)。全角字符延迟 = `WAIT`,半角字符延迟 =
   **`WAIT/2`**(整数除法,向零取整)。延迟在字符**追加时**快照进
   `+0x48` 表,之后改 `WAIT` 不影响已追加字符。
3. **`TEXT[CLEAR]` 只清“待显示缓冲”**(十个堆数组 + `+0x60/+0x64/+0x68/+0x6c`),
   **不清图层表面**;`'P'` 标志才清表面(memset + 光标归零)。
   官方 `+0x38` 缓冲在**一行完成时**由 `FUN_004046fc` 清空,
   已画出的像素留在图层表面。
4. **`TEXT[ID]` 是 CP932 字节串**,按 MurmurHash2 + **逐字节比较**
   解析**已存在的图层**;无 UTF-8 校验、无损转换。未知 ID = 错误路径。
5. 官方自动模式默认 `AUTO*=1`:一次 `RETURNCODE` 追加 `C,R,P,E`;
   点击后 `R` 换行、`P` **清表面**、`E` 结束 ⇒ 默认语义是**一行一页**。
   当前 gfx 把 `P` 当换行且跨帧累积,只对“手动模式 + `CGACT[TEXT]`”
   (ERIS 真实路径)**近似**成立,对默认自动模式不成立。

---

## 1. 官方文本状态结构

`DAT_00801800` 是堆上 0x70 字节的状态块,初始化 `FUN_0045fcb4`
(`0x0045fcb4`),另分配十块堆缓冲:

| 偏移 | 类型/容量 | 语义 | 写入点 | CLEAR(`0x4638e4`) |
|---|---|---|---|---|
| `+0x00` | ptr | **当前文本层对象**(`TEXT[ID]` 解析结果) | `TEXT` `0x4534c4` | 不清 |
| `+0x04` | i32 | `WAIT` 毫秒 | `TEXT` `0x45363f` | 不清 |
| `+0x08` | i32 | `AUTORETURN`(默认 1) | `TEXT` `0x453800` | 不清 |
| `+0x0c` | i32 | `AUTOPAGE`(默认 1) | `TEXT` `0x45381b` | 不清 |
| `+0x10` | i32 | `AUTOCLICK`(默认 1) | `TEXT` `0x453836` | 不清 |
| `+0x14` | i32 | 文本光标 X | 绘制例程 | **不清** |
| `+0x18` | i32 | 文本光标 Y | 绘制例程 | **不清** |
| `+0x1c` | u8 | `OUTPUTMODE`(默认 1 自动) | `TEXT` `0x4534f2` | 不清 |
| `+0x20` | i32 | `SX`(默认 0x14) | `FONT` | 不清 |
| `+0x24` | i32 | `SY`(默认 0x14) | `FONT` | 不清 |
| `+0x28` | i32 | 前景色(默认 0xffffff) | `FONT` | 不清 |
| `+0x2c` | i32 | 渐变/背景色(默认 0xffffffff) | `FONT` | 不清 |
| `+0x30/31/32` | u8 | 每字符样式快照 | `FONT` | 不清 |
| `+0x35` | u8 | **点击等待中**(`'C'` 已处理) | `FUN_00404914` | **不清** |
| `+0x36` | u8 | **清缓冲请求** | `RETURNCODE`/`TEXT[CLEAR]` | 清零 |
| `+0x37` | u8 | **重置计时基准** | `FUN_004638e4` / 推进 | 置 1 |
| `+0x38` | `char*` 0x2000 | 待显示文本(CP932,NUL 结尾) | `WORD`/`_` | **清零 + 重置长度** |
| `+0x3c` | `u8*` 0x2000 | `_TEXT_FLAG` 标志流 | `WORD`/`_`/`RETURNCODE`/`TEXT` | 清零 |
| `+0x40` | `i16[8192]` 0x4000 | 每标志**单元宽**(=当时 `SX`) | `WORD`/`_`/`TEXT` 控制标志 | 清零 |
| `+0x44` | `i16[8192]` 0x4000 | 每标志**行高**(=当时 `SY`) | 同上 | 清零 |
| `+0x48` | `i16[8192]` 0x4000 | 每标志**延迟 ms** | `WORD`/`_` | 清零 |
| `+0x4c` | `i32[8192]` 0x8000 | 每标志前景色 | `WORD`/`_` | 清零 |
| `+0x50` | `i32[8192]` 0x8000 | 每标志渐变/背景色 | `WORD`/`_` | 清零 |
| `+0x54/58/5c` | `u8[8192]` 0x2000 | 每标志样式字节(`+0x30/31/32`) | `WORD`/`_` | 清零 |
| `+0x60` | i32 | 文本字节数 | `WORD`/`_`/`$_TEXT` 写 | **清 0** |
| `+0x64` | i32 | 标志数 | `WORD`/`_`/`RETURNCODE`/`TEXT` | **清 0** |
| `+0x68` | i32 | 文本字节游标(已布局) | `FUN_00404914`/`FUN_00404c60` | **清 0** |
| `+0x6c` | i32 | **标志游标**(已消耗) | `FUN_004046fc`/`FUN_00404914` | **清 0** |

初始化证据 `FUN_0045fcb4`:`+0x1c=1`、`+0x08/+0x0c/+0x10=1`、
`+0x20/+0x24=0x14`、`+0x28=0xffffff`、`+0x2c=0xffffffff`、
`+0x36=1`;十块缓冲 `_malloc` 后立即 `FUN_004638e4`
(此时 `+0x36=1`,所以首帧就是全零)。

> `+0x04` 不在初始化写入列表里。`FUN_00403564`(`0x00403564`)实测为
> `_malloc(n)` 后立刻 `DAT_005d15c0(ptr, n)`(与 `FUN_004638e4` 同一清零
> 原语)⇒ 分配即全零。**结论:默认 WAIT=0、默认光标 X/Y=0**;
> 语料 ERIS 也未使用 `TEXT[WAIT]`。

---

## 2. 处理体逐条写入序

### 2.1 `TEXT`(`0x00453448..0x00453893`)

参数槽按 ysc 表下标读取(`0x684160+idx`),处理序:

1. `ID`(槽 0,`0x453467..0x4534d8`):
   - 表达式求值到 `0x5e0dc0` 字节缓冲;
   - `FUN_00414559(0x8016fc, buf, mode)` 解析图层:
     **MurmurHash2(`0x5bd1e995`)+ 逐字节 `strcmp`**(`0x41462f..0x4146da`);
     长度按 CP932 前导表 `0x5b40c0` 计(`0x4142xx`);
   - 命中: `[state+0x00] = 图层对象`,`[layer+0x34][1] = 1`;
   - 未命中: `0x4538a7` 调 `0x465c7c(0x18b82, …)` 并返回 `-1`
     (错误路径,不创建图层)。
2. `OUTPUTMODE`(`0x4534da`):`[state+0x1c] = 值低字节`。
3. `CLICKBUTTON1..4`(`0x4534f5..0x453543`):值 0 → 全局 `0x3ff`。
4. `SKIPBUTTON1..4`(`0x45356a..0x4535b8`):同上。
5. `WAIT`(`0x45362d`):`[state+0x04] = 值`。
6. `CLICK=1`(`0x45383f`):在当前标志位写**宽/高表** → 追加 `'C'`;
   若 `[state+0x1c] != 0` 则**立即返回 -1**(本帧让出)。
7. `RETURN/PAGE/CLICKRETURN/CLICKPAGE=1`
   (`0x453684/0x4536dd/0x453734/0x45378d`):
   写宽/高表 → 追加 `'R'/'P'/'r'/'p'`,标志数 +1。
8. `CLEAR=1`(`0x453894`):`[state+0x36] = 1` → `call 0x4638e4`。
9. `AUTORETURN/AUTOPAGE/AUTOCLICK`(`0x4537e8..`):原值写 `+0x08/+0x0c/+0x10`。

### 2.2 `WORD`(`0x00458d44..0x00458eba`)

1. 参数长度 `len`;若 `当前文本长度 + len - 1 >= 0x1ffe` → 溢出分支
   `0x458ec5`。
2. `memcpy([state+0x38]+len_old, payload, len)`(`0x5d15cc`);
   `+0x60 += len`;`text[+0x60] = 0`。
3. 逐字符循环(`0x458dd1..0x458e9e`):
   - `lead = 0x5b40c0[byte]`(0x81..0x9F / 0xE0..0xFC = 1,其余 0);
   - 写 `+0x4c[idx] = 前景色`、`+0x50[idx] = 背景色`、
     `+0x54/58/5c[idx] = +0x30/31/32`;
   - 写 `+0x40[idx] = SX`、`+0x44[idx] = SY`;
   - 写 `+0x48[idx] = WAIT / (lead ? 1 : 2)`(`idiv`,`0x458e78`);
   - 写 `+0x3c[idx] = lead ? '2' : '1'`;
   - `+0x64 += 1`;文本下标 `+= lead + 1`。
4. 末尾 `flags[+0x64] = 0`;返回 0(成功路径不让出)。

### 2.3 `_`(`0x004567c0..0x00456ad3`)

1. 按值类型(1/2/3)格式化到 `0x5e2680`(整数 `itoa` 基数 10;浮点
   `0x4699fb`);空串走空路径。
2. 长度/容量检查:`+0x60 + len >= 0x1ffe` → 错误 `0x18e70`;
   `len > 0x400` → 错误 `0x18e8e`。
3. 逐字符:
   - CP932 前导字节 → 双字节原样写入;非前导且非 `0x20..0x7e` → 写空格
     `0x20`(`0x4568c9..0x4568ea`);
   - 写与 `WORD` 相同的五组每字符表;
   - `+0x48[idx] = WAIT / (lead ? 1 : 2)`(半角 `sar 1`,`0x45697f`;
     全角直接写 `WAIT`,`0x456a8e`);
   - 写 `+0x3c[idx] = '2'/'1'`;`+0x64 += 1`。
4. 末尾写 NUL 到文本与标志;返回 0。

### 2.4 `RETURNCODE`(`0x0044c390..0x0044c45b`)

先把当前指令首窗口两个 dword 抄进 `task+0xc/+0x10`(TASKINFO 用)。
然后:

- **自动模式**(`[+0x1c] != 0`):
  - `AUTOCLICK != 0` → 写宽/高表 → 追加 `'C'`;
  - `AUTORETURN != 0` → 写宽/高表(`SX/SY`)→ 追加 `'R'`;
  - `AUTOPAGE != 0` → 追加 `'P'`;
  - 恒追加 `'E'`;
- **手动模式**(`[+0x1c] == 0`):
  - `[+0x36] = 1`;
  - 恒追加 `'E'`;
- 最后 `task+9 = 1`,`eax = -1`(让出)。

关键:**`'C'` 在最前**。`FUN_00404914` 遇到 `'C'` 立刻置 `+0x35=1`
并返回,所以同一行的 `R/P/E` 要等点击/跳过推进 `+0x6c` 越过 `'C'`
之后才被处理。

### 2.5 `TEXTINFO`(`0x004538cc..`)

按**参数是否出现**依次检查槽 `0x161..0x167`:
`OUTPUTMODE → CX → CY → CLICK → AUTORETURN → AUTOPAGE → AUTOCLICK`;
取第一个命中的值,经 `0x455540` 写入 `LET` 目标。
`CX/CY` 读 `+0x14/+0x18`;`CLICK` 读 `+0x35`。

### 2.6 `TEXT[CLEAR]` / `FUN_004638e4`

`TEXT[CLEAR=1]` 的 CLEAR 分支只做两件事:`+0x36 = 1`,`call FUN_004638e4`
(随后 handler 继续处理同一条命令里的 `AUTO*` 参数,`0x4538a2 → 0x4537e8`)。

`FUN_004638e4`(`0x004638e4`):

```
if state[0x36] == 0: return
state[0x36] = 0
state[0x60] = 0; state[0x64] = 0; state[0x68] = 0; state[0x6c] = 0
state[0x37] = 1
zero_fill(state+0x38, 0x2000); zero_fill(state+0x3c, 0x2000)
zero_fill(state+0x40, 0x4000); zero_fill(state+0x44, 0x4000)
zero_fill(state+0x48, 0x4000)
zero_fill(state+0x4c, 0x8000); zero_fill(state+0x50, 0x8000)
zero_fill(state+0x54/58/5c, 0x2000)
```

`zero_fill` = `DAT_005d15c0(ptr, size)`(十处同型;与 `0x5d15cc=memcpy`
同族;分配器 `0x403564` 也用它,故它是清零原语而非 free)。
**不触碰** `+0x14/+0x18` 光标、`+0x00` 当前层对象、`+0x04`、
`+0x08..0x1c`、色/字号;也**不触碰图层表面**(表面在
`task+0x34` 指向对象的 `+0x90`,只由 `'P'` 清)。

### 2.7 每帧文本 tick(`FUN_004046fc`;调度器 `0x404423` 调用)

调度器每帧对“文本让出任务”调用一次;先判 `task+9`(未让出直接返回)。
`[+0x1c] != 1`(手动/其它)→ `0x404828`:`FUN_004638e4()`(有 `+0x36`
才清)→ `task+9=0` → 恢复。

自动模式:

1. 顶部检查 `SKIPBUTTON1..4`:
   - 有按下且 `+0x35 != 0`:`+0x37=1`、`+0x35=0`、`+0x6c += 1`
     (消费 `'C'`);
   - 有按下且 `+0x35 == 0`:`0x801c70 = 1`(**快进模式**:本帧不等 WAIT)。
2. `FUN_00404914()` 布局/推进(见 2.8)。
3. `total = +0x64`;若 `total <= 0` → 直接做收尾判定。
4. 否则检查 `CLICKBUTTON1..4` 与 `SKIPBUTTON1..4`:
   - 任一按下且 `+0x35 != 0`:`+0x37=1`、`+0x35=0`、`+0x6c += 1`;
   - 任一按下且 `+0x35 == 0` 且 `+0x6c < total`:`0x801c70 = 1`
     (下一帧快进整行)。
5. 收尾(`0x404818`):
   - `+0x6c < total` → 返回(下一帧继续);
   - `+0x6c <= 0` → 清+恢复;
   - `+0x6c >= total && +0x6c > 0` → `+0x36=1` → 清+恢复。

**行完成即清待显示缓冲并恢复任务**;已画像素保留在表面。
`+0x6c` 随 `FUN_004638e4` 归零 ⇒ 下一行的 reveal 从 0 开始。

### 2.8 布局/推进(`FUN_00404914`)

入口:`[state+0x00] == 0`(从未设 `TEXT[ID]`)→ 直接跳过(**没有当前层
就不自动绘制**)。

循环(每个标志):

1. `+0x64 <= +0x6c` → `0x801c70 = 0` 并返回(全部消费完)。
2. `+0x37 != 0` → `+0x37 = 0`, `0x801c74 = timeGetTime()`(重置基准)。
3. 读 `flags[+0x6c]`:
   - `'1'` / `'2'`:先 `FUN_00404c0c` 计时门,再 `FUN_00404c60` 行宽/折行;
     通过则按 1/2 字节取字、绘制(`FUN_004056b8`)、`+0x14 += 字宽`、
     标脏(`FUN_00408724`)、`+0x68 += 1/2`;
   - `'C'`:`+0x35 = 1`,`0x801c70 = 0`,返回;
   - `'E'`:无副作用;
   - `'P'`/`'p'`:清表面(`zero_fill(layer_surface, w*4*h)`)、`+0x14=0`、
     `+0x18=0`,标脏;
   - `'R'`/`'r'`:`+0x14=0`,`+0x18 += +0x44[+0x6c]`(该标志行高),标脏;
   - 其它:`0x465d08(0xdbbaa, 1)` 错误并返回。
4. `+0x6c += 1`;若本步发生绘制/清屏/换行 → 标脏。

### 2.9 计时门(`FUN_00404c0c`)

```
if 0x801c70 != 0: return 0                 # 快进:不等待
if +0x68 <= 0: return 0                    # 一行的第一个字符立即显示
now = timeGetTime()                        # 0x404c21,ms
delay = (i16)(+0x48)[+0x6c]                # 有符号 16 位
if (u32)(now - 0x801c74) < delay: return -1
0x801c74 += delay                          # 累加,不是 = now
return 0
```

要点:
- 时间基是 **`timeGetTime()` 毫秒**,与 `tick_at(now_ms)` 同单位;
  `now - last` 按 u32 回绕减法,`last += delay` 也按 u32 回绕。
- 延迟取**当前待处理标志**的 `+0x48` 值;控制标志不经过此门。
- 只有一个全局 `0x801c74`,所有文本层/任务共用(官方如此)。
- `0x801c70` 只由“跳过/点击按钮且不在点击等待”置 1;由
  `FUN_00404914` 消费完或遇 `'C'` 清 0。**不是每帧置位**。

### 2.10 行宽/折行(`FUN_00404c60`)

读窗口对象 `task+0x34` 的 `+0x78`(宽)/`+0x7c`(高)、当前 `+0x14/+0x18`、
`+0x40/+0x44` 每字宽高;按窗口宽度决定是否换行,并对特定标点做
行首/行尾禁则处理(函数内多组 `strcmp` 与 `0x8080xx` 常量表)。
溢出路径把 `+0x68 = +0x60`(文本耗尽)并返回 -1。
**这是当前 gfx `layout_text` 完全没有的宽度折行**;本轮最小脚本应使用
短行,避免误判。

---

## 3. 语料与样本

### 3.1 ERIS 普查(2026-11-04 实测)

```text
cargo run -p yu-engine --example census_params -- <project-eris> TEXT TEXTINFO
脚本数 = 141
== TEXT: 2 组 ==
  CLEAR            1
  OUTPUTMODE       1
  @ s10 pc0 line22
  @ s71 pc96 line154
== TEXTINFO: 0 组 ==
```

结论:

- ERIS **没有任何 `TEXT[ID]`**、没有 `WAIT`、没有 `TEXTINFO`;
- 两处 `TEXT` 分别是 `CLEAR` 与 `OUTPUTMODE`(与 `es_first.yst` 的
  `TEXT[OUTPUTMODE=0]` 一致);
- ERIS 的真实文本路径是**手动模式 + 脚本 `CGACT[TEXT]`**
  (`Text/es_Text.yst` 逐字读 `$_TEXT_FLAG`),不是引擎自动层。

### 3.2 样本缺口(必须显式计数)

- `<project-eris>` 无 `TEXT[ID]` ⇒ H-P2-01/H-P2-04 的自动层用例只能
  **合成**(YSCom 最小脚本或 `ProjectBuilder`),测试名/注释必须标
  “合成样本”;
- 商业包语料不可用(`<games>` 不存在);无法证明真实工程是否使用非 ASCII
  `TEXT[ID]`,但 REA 已证明**字节语义本身**;
- `TEXT[WAIT]` 在 ERIS 为 0 次 ⇒ reveal 曲线用合成脚本 +
  确定性 `tick_at(now_ms)`,不依赖真实游戏;
- `text_probe` / `wgpu_text_probe` 需要 `<project-eris>`;缺样本时必须
  显式 skip + 计数,禁止空跑通过。

---

## 4. 真值锚点汇总(每条结论 → 证据)

| # | 结论 | 手册 | REA | 语料/其它 |
|---|---|---|---|---|
| T1 | `WAIT` = 每字符毫秒 | `text/text.html` 例 1 | `TEXT` `0x45363f`;`WORD` `0x458e78`;`_` `0x45697f/0x456a8e` | ERIS 0 次 |
| T2 | 全角=WAIT,半角=WAIT/2 | — | `WORD` `idiv edi`(edi=1/2);`_` 半角 `sar 1`、全角直写 | 合成 |
| T3 | 时间源 ms、累加截止 | — | `FUN_00404c0c` `0x404c21..0x404c45` | — |
| T4 | `+0x6c`=标志游标,`+0x68`=字节游标 | — | `FUN_00404914` `0x404930..`;`FUN_004638e4` 清四游标 | — |
| T5 | `TEXT[CLEAR]` 清缓冲+四游标,不清表面 | `text/text.html` “完全にクリア” | `0x453894..0x4538a2`;`0x4638e4` | — |
| T6 | `'P'` 清表面并复位光标 | — | `FUN_00404914` case `0x50/0x70` | — |
| T7 | `'R'` 换行:`X=0`,`Y+=行高` | — | `FUN_00404914` case `0x52/0x72` | — |
| T8 | `'C'` 置点击等待并停 | `text/text.html` CLICK | `FUN_00404914` case `0x43` | — |
| T9 | `RETURNCODE` 自动追加 C/R/P/E | `text/text.html` AUTO* | `0x44c390..0x44c45b` | ERIS 396 处静态 |
| T10 | `RETURNCODE` 手动只置 `+0x36`+E | — | `0x44c437..0x44c447` | ERIS `OUTPUTMODE=0` |
| T11 | `WORD` 每字符写宽/高/延迟/标志 | `basic/basic.html` | `0x458dd1..0x458e9e`;前导表 `0x5b40c0` | 464 处静态 |
| T12 | `_` 控制字符折空格、同表写入 | `basic/basic.html` ●テキスト表示 | `0x4568c9..0x456aa8` | 33 处 |
| T13 | `TEXT[ID]` 是 CP932 字节、哈希+逐字节比较 | `text/text.html` ID | `TEXT` `0x453467`;`FUN_00414559`;未知 ID `0x4538a7` | ERIS 0 次 |
| T14 | 无当前层(`+0x00==0`)不自动绘制 | — | `FUN_00404914` `0x404921..0x404925` | — |
| T15 | 自动模式一行一页(默认 AUTO*=1) | `text/text.html` AUTOPAGE | §2.4 + §2.8 | 合成 |
| T16 | 行完成清待显示缓冲并恢复 | — | `FUN_004046fc` `0x404818..0x40482d` | — |
| T17 | 手动模式下一 tick 清缓冲并恢复 | — | `0x404711..0x404715`、`0x404828` | `fixture::manual_outputmode_returncode_resumes_without_input` |
| T18 | 官方按窗口宽度折行 + 标点禁则 | — | `FUN_00404c60` | gfx 未实现(新 DIV) |
| T19 | 未知标志 = 错误 `0xdbbaa` | — | `FUN_00404914` default | — |
| T20 | 每字符色/样式快照 | `font/font.html` | `WORD` `0x458de0..0x458e51`;`_` 同型 | gfx 只存整段样式(新 DIV) |

---

## 5. 与任务书/台账假设的冲突(先改档案)

> 规则:证据与任务书冲突时,先改档案再写码;以下每条都要求主人确认
> 处理方式。

### C1 `TEXT[CLEAR]` 不清显示面

任务书 §任务 1 验收写“CLEAR 后为空”,`text_pipeline` 现有测试
`clear_sync_clears_cross_frame_text` 也把“空快照 = CLEAR”写成契约。
REA 证明官方 CLEAR 只清**待显示缓冲**,已画像素留在图层表面;
清表面的是 `'P'`。因此:

- “空快照 = 清显示层”**不是**官方等价语义;
- 内核当前 `TextBuffer` 把“待显示缓冲”和“显示面”合成一个模型;
  这在 ERIS(手动模式 + CGACT)下无可观测差异,但默认自动模式不成立。

### C2 官方待显示缓冲在行完成时清空

`FUN_004046fc` 在 `+0x6c >= +0x64` 时置 `+0x36` 并清空十个数组、
四个游标,然后恢复任务。内核 VM 目前**不执行**这一步
(`op_returncode` 只让出),所以 `_TEXT`/`_TEXT_FLAG` 会跨行累积。
这与 `fixture` 现测一致,但与官方 `_TEXT`(“現在表示されているテキスト”)
的**行作用域**不一致。需要决定是否在 VM 侧补这条状态迁移。

### C3 WAIT 不是同权

全角 = `WAIT`,半角 = `WAIT/2`;`+0x48` 是**每字符快照**,不是全局
“当前 WAIT”。任务书 §任务 3 的“WAIT=15 时逐帧 reveal 曲线”必须按
字符宽度分别推导(全角 15ms、半角 7ms)。

### C4 `'P'` 不是换行,是清屏

gfx `layout_text` 把 `R/r/P/p/E` 统一当“有字才换行”;官方 `P` 是
memset 表面 + `X=Y=0`。默认 `AUTOPAGE=1` 时每个 `RETURNCODE` 都带 `P`,
所以默认自动模式**每行清屏**。

### C5 `TEXT[ID]` 未命中是错误,不自动建层

官方 `0x4538a7` 走错误打印并返回 -1;内核 `op_text` 目前接受任意 ID,
gfx 对不存在层退化为“屏幕坐标叠加”(`text_layout_now` 的
`target = None`)。两者都需要按“已存在层”语义收口。

### C6 自动层只属于自动模式

`FUN_004046fc` 对 `OUTPUTMODE != 1` 直接清+恢复,不调用
`FUN_00404914` ⇒ **手动模式没有引擎自动绘制**。ERIS 的 `_TEXT_FLAG`
只供脚本读取,真实绘制走 `CGACT[TEXT]`。因此 H-P2-04 的自动层同步
应以 `output_mode == 1` 为前提;把手动模式缓冲也画到自动层会多画。

**实机回归(2026-11-04)**:Engine 曾无条件同步,ERIS 手动模式在
`(0,0)` 多画一行(左上角重复正文)。已修:`Engine::tick_inner` 只在
`output_mode == 1` 同步自动层,手动模式 `gfx.clear_text()`(只清自动层,
不触碰 `CGACT[TEXT]` 的 `text_draws`)。回归
`engine_manual_mode_does_not_draw_auto_text_layer`。
`text_probe` 仍强制同步自动层,是**合成诊断探针**,不代表官方手动路径。

### C7 折行/每字符样式未实现

官方 `FUN_00404c60` 按窗口宽度折行(含标点禁则);gfx `layout_text`
只按标志换行。官方每字符快照色/字号/样式,`TextRun.style` 只有整段
一个快照。两者都是本轮之外的独立缺口,但会影响“按官方语义排版”的
完整验收。

---

## 6. 契约裁决点(必须主人拍板)

### 6.1 H-P2-01:`VmEvent::Text.id` 编码

现状:`op_text` 用 `std::str::from_utf8(&b)`,失败即清空 `id` 并挂号
(`handlers.rs:4587`)。官方是纯字节,必须改。

**方案 A(加法)**:

```rust
VmEvent::Text {
    frame: u64,
    /// 显示用(UTF-8 可解码时原样;否则空/替换,不参与层匹配)。
    id: String,
    /// CP932 原字节(权威层键)。
    id_bytes: Vec<u8>,
    bytes: Vec<u8>,
}
```

- 优点:`id` 的现有消费者(日志/诊断/测试)不改语义;
- 缺点:所有 `VmEvent::Text` 模式匹配点要加字段(编译器会强制);
  两个 ID 字段容易误用,需要在 gfx/trace 只读 `id_bytes`。

**方案 B(替换)**:

```rust
VmEvent::Text { frame: u64, id: Vec<u8>, bytes: Vec<u8> }
```

- 优点:单一权威值,不会误用;与 `LayerId(Vec<u8>)` 同构;
- 缺点:trace/JSON/测试/示例全部要改;`id` 不再是可直接打印的字符串。

**影响面(两方案共同)**:

| 文件 | 改动 |
|---|---|
| `crates/yu-rt/src/vm/handlers.rs` | `op_text` 不再 `from_utf8`;保存 `text_layer_id: Vec<u8>` |
| `crates/yu-rt/src/event.rs` | 变体定义;`kind()`/`domain()` 模式 |
| `crates/yu-rt/src/json.rs` | `id` 编码改为可逆(hex/base64 或字节数组) |
| `crates/yu-gfx/src/state.rs` | `LayerId::new(id_bytes)`;不再 `as_bytes()` |
| `crates/yu-trace/src/event.rs` | `Payload::VmText` 映射 |
| `crates/yu-trace/src/record.rs` | `Payload::VmText`、解析、JSON、roundtrip 测试 |
| `crates/yu-trace/src/fault/ring.rs` | 快照字段 |
| `crates/yu-rt/tests/fixture.rs`、`parity_fixes.rs`、`layer_order.rs`、`corpus.rs` | 构造/匹配 |
| `crates/yu-render/soft/tests/render.rs`、`wgpu/tests/wgpu.rs` | 构造 |
| `crates/yu-rt/examples/{boot_events,coverage_census,text_scenario}.rs` | 打印/匹配 |
| `crates/yu-gfx/tests/text_pipeline.rs` | 构造 |

**trace 兼容策略**:

- `SCHEMA_VERSION` 已由 4 升到 5;解析器对未知字段直接 `trace.schema`
  拒绝(`record.rs:1278`)⇒ 后续加字段/改编码仍必须继续升版;
- 旧 trace 不能静默混读:升版后旧文件按“不支持 schema”拒绝,
  这是现协议行为,不需要兼容层;
- golden 文件里**没有** `kind:"text"` 记录
  (`crates/yu-trace/tests/golden/synthetic_frame.jsonl` 只有 command;
  `crates/yu-rt/tests/golden/*.jsonl` 也无 Text 事件),但封套 schema
  仍要重生成;本次同时校正了 HEAD 已有的合成 ysc/ycd 表摘要漂移。
  另改 `record.rs` 的 roundtrip 单元测试(`Payload::VmText` 手构造)与
  `event.rs` 的穷尽映射测试;
- `crates/yu-rt/src/json.rs` 的事件 JSON 若被外部当稳定格式,同样按
  “id 字节可逆”升级说明写入 `docs/`;

**建议**:方案 B。官方语义只有字节一个权威值;方案 A 的 `id: String`
会继续诱导消费者用错。若主人要求最小 diff,则 A,但必须把
`id_bytes` 设为唯一层键并在文档/命名上禁止 `id` 参与匹配。

**裁决(2026-11-04):方案 B,已落地。** 见 §12.2。

### 6.2 H-P2-04:同步抽象层

官方有两层:

1. **待显示缓冲**(`+0x38/+0x3c`,行结束清空);
2. **图层表面**(像素,`'P'` 清,`R` 换行)。

内核 gfx 现在只有一层 `TextBuffer`(字节+标志),`sync_text_state`
是“整段替换”,`frame()` 每帧重排。两种收口:

**M1(最小接线,推荐本轮)**:

- `TextSnapshot` 增加 `layer_id: Vec<u8>`(加法;`TextSnapshot` 是公开
  结构,4 处测试构造要补字段);
- Engine `tick_inner` 在 `vm.tick_at(...)` 之后、`state.frame()` 之前:
  `gfx.sync_text_state(&vm.text_state())`;
- `sync_text_state` 只在 `output_mode == 1` 时写当前层;
  当前层由 `snapshot.layer_id` 决定,不再依赖宿主是否消费了 `Text` 事件;
- `GfxState::text_layout_now` 改为**只取当前层**,不再 `BTreeMap::find_map`
  取第一个非空层;
- VM 增加 `clear_seq: u64`(每次 `TEXT[CLEAR]` 自增),快照携带;
  gfx 检测变化后调用显式 `clear_text()`(不靠“空 bytes”推断);
- **已知分歧**:行完成不清待显示缓冲(内核累积),`P` 仍当换行,
  自动多行/翻页与官方不一致 ⇒ 新建 DIV,标注本轮范围只覆盖
  “单行/短行 + 手动 CLEAR”。

**M2(忠实表面模型,推荐最终 1:1)**:

- VM 复刻 `FUN_004046fc` 行完成清空 + `+0x6c` 归零(先做 C2 的决策);
- gfx 增加“已提交字形(表面)”与“待显示行”两层:
  `'R'` 推进已提交基线,`'P'` 清空已提交,行完成把当前行提交;
- `layout_text` 算法不改,只改帧投影的输入分层;
- 工作量明显大于 M1,且需要先决定 C1/C2。

**建议**:本轮 M1 接线 + 明确 DIV;M2 另立任务(可与 H-P2-02 合并为
“文本表面模型”任务)。若主人要求本轮就 1:1,则直接 M2,但 H-P2-02
的 reveal 必须与行完成清空一起做,不能拆。

**裁决(2026-11-04):不做大范围重构,按现有实现统一语义(M1)。**
M2(待显示缓冲/图层表面分离、`'P'` 清屏、宽度折行、每字符样式)登记为
`DIV-GFX-075`,后续任务处理;本轮以 `clear_seq` 显式清层、`text_layout_now`
只取当前层、`TextSnapshot.layer_id` 权威化来消除语义歧义。见 §12.1。

### 6.3 H-P2-02:透出字段与时钟

官方状态到宿主的映射(裁决后实际落地):

```rust
pub struct TextSnapshot {
    // 既有字段 ...
    pub layer_id: Vec<u8>,       // 当前文本层(新增)
    pub clear_seq: u64,          // TEXT[CLEAR] 代数(新增)
    pub reveal_flags: usize,     // +0x6c(当前显示段内)
    pub reveal_bytes: usize,     // +0x68(当前显示段内)
    pub reveal_chars: usize,     // 已完成段 + 本段已显字符数
    pub skip_mode: bool,         // 0x801c70(审计)
    // click 既有(+0x35)
}
```

- `reveal_chars` 由 VM 计算(单一权威),gfx 只消费
  `set_text_reveal(...)`;内核累积显示缓冲,故它是**已完成段 + 本段**
  的可见字符总数(不是单一 `+0x6c` 前缀)。
- 时间基:`tick_at(now_ms)` 的毫秒,与官方 `timeGetTime()` 同单位;
  **禁止**宿主/后端 `Instant::now()`。
- 宿主只在 `output_mode == 1` 传 `Some(reveal_chars)`;手动模式传
  `None`(官方手动模式不调用 `FUN_00404914`,没有 auto reveal)。
- 已复刻 `FUN_004046fc` 的推进/点击停/无 `'C'` 自动恢复:
  `advance_reveal(now_ms)` + `finish_line()` + 调度器
  `text_reveal_complete`;行完成的**待显示缓冲清空**未复刻
  (M2/`DIV-GFX-075`)。

### 6.4 额外契约:`TextSnapshot` 加法字段

`TextSnapshot` 是 `pub` 结构,`yu-gfx` 与两个后端测试直接构造。
增加 `layer_id/clear_seq/reveal_*` 会破坏 4 处构造:
`yu-gfx/tests/text_pipeline.rs`、`soft/tests/render.rs`、
`wgpu/tests/wgpu.rs`、`src/text.rs` 的 `From<&TextState>`。
这是机械修改,但属于跨 crate 公共契约,一并请主人确认。

---

## 7. 建议实现顺序(拍板后)

1. **H-P2-04(M1)**:
   - `TextState` 增 `layer_id: Vec<u8>`、`clear_seq: u64`;
   - `TextSnapshot` 增同名字段;
   - `op_text` 的 ID 按 6.1 裁决落地(若选 B,本步一起改);
   - `GfxState::sync_text_state` 用 `snapshot.layer_id`;只在
     `output_mode == 1` 写;`clear_seq` 变化调 `clear_text()`;
   - `text_layout_now` 只取当前层;
   - `Engine::tick_inner` 接线(位置:`vm.tick_at` 之后、`frame()` 之前);
   - 新增 Engine 级合成脚本回归(见 §8)。
2. **H-P2-01(按裁决)**:
   - 事件/JSON/trace 全链改字节;`SCHEMA_VERSION` 升 5;
   - 新增 `TEXT[ID="あ"]`(0x82 0xA0)回归:事件字节 → gfx 目标层 →
     trace JSON 往返一致;
   - 不改 `TEXT[CLEAR]` 语义。
3. **H-P2-02**:
   - 先按 §2.9 写确定性单测(WAIT=15、全角/半角混合、`tick_at` 序列),
     用 REA 推导的曲线做期望;
   - 再透出 `reveal_*`,Engine 每帧 `set_text_reveal`;
   - 最后接 soft/wgpu 探针;后端不改算法。

---

## 8. 验收命令

基础:

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets
cargo test -p yu-rt --test fixture text_
cargo test -p yu-gfx --test text_pipeline
```

H-P2-04 新增回归(建议放 `crates/yu-engine/tests/text_sync.rs`,
用 `yu-rt` 的 `ProjectBuilder` 或等价最小工程):

- 脚本:`TEXT[ID="MSG"]` + `WORD("Aこ")` + `_("B")` + `RETURNCODE` +
  `TEXT[CLEAR=1]`;先 `CG` 建 `MSG` 层;
- 断言 tick 后 `GfxState.frame().text_layout` 的 glyph 数/坐标;
- `TEXT[CLEAR]` 后 `frame().text` 为空(注明:这是内核显示模型,
  官方 CLEAR 不清表面,见 C1);
- `output_mode=0` 时自动层不产出(见 C6)。

H-P2-01 新增回归:

- `TEXT[ID="あ"]`(CP932 `0x82 0xA0`)→ `VmEvent::Text` 的字节精确;
- `GfxState` 的 `TextLayoutFrame.target == Some(LayerId(b"\x82\xa0"))`;
- `yu_trace` JSON → 解析回 `Payload::VmText` 字节相等。

H-P2-02 新增回归:

- `TEXT[WAIT=15]` + 全角 3 字 + 半角 1 字;
- `tick_at` 序列 `[0, 7, 14, 15, 22, 29, 30]`(ms)对应
  `reveal_chars` 曲线;期望按 §2.9 手算,不读墙钟;
- `TEXT[CLEAR]` 后 `reveal_chars == 0`;
- soft/wgpu 同一帧可见 glyph 数一致。

探针(有 `<project-eris>` 时;缺样本显式 skip+计数):

```bash
export YURIS_SAMPLE_ERIS_ROOT=<project-eris>
cargo run -p yu-backend-soft --example text_probe -- 130
YURIS_ENTRY_LABEL=RP_101 cargo run -p yu-backend-soft --example text_probe -- 130
cargo run -p yu-backend-wgpu --example wgpu_text_probe -- 130
```

---

## 9. 台账更新清单(实现后)

- `docs/handoffs/H-P2-01-text-id-bytes.md`:按裁决写 fixed/方案;
- `docs/handoffs/H-P2-02-vm-text-reveal.md`:按 §6.3 写 fixed;
- `docs/handoffs/H-P2-04-host-text-sync.md`:`fixed` + 改动位置;
- `crates/yu-gfx/docs/divergences/README.md`:
  - `DIV-GFX-021` → fixed(no-manual,字节语义);
  - `DIV-GFX-033` → 按 reveal 实现范围更新(不得留 unimplemented);
  - `DIV-GFX-034` → 补 C1/C2 的真实语义与内核近似;
  - 新增 DIV:C1(显示面 vs 待显示缓冲)、C4(`P` 清屏)、
    C6(手动模式无自动层)、C7(折行/每字符样式);
- `crates/yu-rt/docs/coverage-matrix.md`:
  - `TEXT`/`WORD`/`_`/`RETURNCODE`/`TEXTINFO` 行补字节语义、
    WAIT 全/半角、`+0x6c` 游标;
- `docs/gfx-unimplemented-todo.md`:对应行;
- `docs/render-parity.md` / 文本相关文档:旧“P=换行”“空快照=CLEAR”
  口径勘误;
- `PROGRESS.md`:追加“成果 N”,含命令与原始输出、skip 计数。

---

## 10. 旧笔记勘误

| 旧口径 | 证据 | 新口径 |
|---|---|---|
| `+0x6c` = 已显字符数 | `FUN_00404914` 逐标志递增;`FUN_004638e4` 清四游标 | 标志游标(含控制标志);字符数需按 `'1'/'2'` 前缀统计 |
| WAIT 每字符同权 | `WORD 0x458e78 idiv`;`_ 0x45697f sar 1` | 全角 WAIT,半角 WAIT/2 |
| `TEXT[CLEAR]` 清显示 | `0x4638e4` 只清十数组+四游标 | 只清待显示缓冲;显示面由 `'P'` 清 |
| `P` 是换行标志 | `FUN_00404914 case 0x50/0x70` | 清表面 + 光标归零 |
| 自动模式可多行累积 | 默认 `AUTOPAGE=1`;`0x44c41a..0x44c42c` | 默认一行一页;多行需 `TEXT[AUTOPAGE=0]` |
| `$\_TEXT` 跨行累积 | `FUN_004046fc 0x404824..0x40482d` | 官方行完成即清;内核尚未复刻(C2) |
| 非 UTF-8 ID 只能清空 | `FUN_00414559` 按字节哈希+比较 | 字节 ID 完全合法;改事件契约即可无损 |

---

## 11. 证据复现方法(供复核)

- 官方 exe:REA `open_binary` → `inspect_native_instruction` /
  `read_function_instructions`;本文件引用的关键指令字节:
  - `0x44c390: 56 8b 0d 1c 17 80 00 …`(RETURNCODE 头)
  - `0x44c451: c6 46 09 01`(`task+9=1`)
  - `0x453894: a1 00 18 80 00 c6 40 36 01 e8 42 00 01 00`
    (`CLEAR`:set `+0x36`,call `0x4638e4`)
  - `0x404c21: ff 15 08 c4 58 00`(`call timeGetTime`)
  - `0x404c3d: 0f bf 3c 78`(`movsx edi, word [eax+edi*2]`)
  - `0x404c45: 03 cf`(`add ecx, edi` → `last += delay`)
  - `0x404824: c6 42 36 01`(`+0x36=1`);`0x404828: e8 b7 f0 05 00`
    (call `0x4638e4`);`0x40482d: c6 43 09 00`(`task+9=0`)
  - `0x404748..0x404750` / `0x4047f6..0x404802`:点击等待推进
    (`+0x37=1`,`+0x35=0`,`+0x6c++`)
  - `0x404755` / `0x40480e`:`0x801c70=1`(快进)
- 前导表 `0x5b40c0` 前 256 字节:仅 `0x81..0x9F`、`0xE0..0xFC` 为 1。
- 语料:`cargo run -p yu-engine --example census_params -- <project-eris> TEXT TEXTINFO`。
- 手册:上列 HTML 页(CP932);`$\_TEXT`/`$\_TEXT_FLAG` 语义见
  `basic/svariable.html` 第 28/29 行。

---

## 12. 落地结果(2026-11-04)

### 12.1 H-P2-04(按主人裁决:M1,现有实现统一语义)

- `TextState` / `TextSnapshot` 增 `layer_id: Vec<u8>`、`clear_seq: u64`;
  `Vm::text_layer_id: String` 删除。
- `GfxState::sync_text_state`:
  - 层 ID 经 `text_layer_path`(`path::normalize`,与 `CG[ID]` 同语义);
  - 只有 `clear_seq` 变化才清当前层;**空 `bytes` 不再隐式当 CLEAR**;
  - `text_layout_now` 只取当前层(旧实现取 BTreeMap 首个非空层)。
- `Engine::tick_inner` 在 `vm.tick_at` 后、`frame()` 前同步快照:
  **仅 `output_mode == 1`** 调 `sync_text_state` + `set_text_reveal`;
  手动模式 `clear_text()`(官方手动模式不自动绘制,见 C6)。
- 回归:`crates/yu-engine/tests/text_sync.rs`
  - `engine_manual_mode_does_not_draw_auto_text_layer`
    (实机左上角重复正文的回归);
  - `engine_auto_mode_reveals_and_clears_text_layer`
    (自动层 reveal + CLEAR)。
- 未做(M2):官方待显示行缓冲/图层表面分离、`'P'` 清屏、宽度折行、
  每字符样式快照 ⇒ `DIV-GFX-075`。

### 12.2 H-P2-01(方案 B)

- `VmEvent::Text { frame, id: Vec<u8>, bytes }`;
  `op_text` 直接保存 `TEXT[ID]` 字节。
- gfx 事件/快照层 ID 都走路径规范化;`LayerId` 字节精确。
- `yu-trace` `Payload::VmText.id: Vec<u8>`;JSON 用可逆字节编码;
  `SCHEMA_VERSION = 5`;旧 trace 以 `trace.schema` 拒绝。
- 回归:
  - `fixture::text_id_keeps_cp932_bytes_without_utf8_conversion`;
  - `text_pipeline::non_ascii_text_layer_id_targets_exact_layer`
    (CP932 `0x82A0` + 非法 CP932 `0xFFFE`);
  - `record.rs` roundtrip `id: vec![0x82,0xa0]`。
- golden:`synthetic_frame.jsonl` 用 `UPDATE_GOLDEN=1` 重生成。除 schema
  4→5 外,`ysbin/ysc.ybn`/`system/YSCom/YSCom.ycd` 的 `bytes`/`sha256`
  也变了:HEAD 的 golden 自 `dfa7b0b` 改过合成命令表后未刷新,本次一并
  校正;其余记录逐字节不变,且无 text 事件。

### 12.3 H-P2-02

- `TextState` 增 `reveal_flags/reveal_bytes/line_reveal_chars/
  committed_chars/delays/last_char_ms/timer_reset/skip_mode`;
  `advance_reveal(now_ms)` / `finish_line()` / `line_complete()`。
- `Vm::tick_at` 只在 `Scheduler::text_waiting()` 时推进;
  调度器 gate 用 `text_reveal_complete` 处理无 `'C'` 的自动行。
- `TextSnapshot` 透出 `reveal_flags/reveal_bytes/reveal_chars/skip_mode`;
  Engine `set_text_reveal`。
- 回归:见 §8 与 `H-P2-02-vm-text-reveal.md` 落地节;soft/wgpu
  reveal=1 同帧各 1 glyph。

### 12.4 验证命令与结果

```text
cargo build --workspace --all-targets                              # OK
cargo test -p yu-rt --test fixture text_                           # 13 passed
cargo test -p yu-gfx --test text_pipeline                          # 13 passed
cargo test -p yu-engine --test text_sync                           # 2 passed
cargo test -p yu-backend-soft --test render text                   # 8 passed
cargo test -p yu-backend-wgpu --test wgpu text_layout_frames...    # 1 passed
cargo test -p yu-trace --test golden                               # 2 passed
cargo test -p yu-cli --test logging_guards                         # 3 passed
```

已知非本轮红(见 §12.5):
- `yu-host-android --test session runs_real_corpus_and_emits_frames`
  —— 本机无音频端点,HEAD 同样失败(`docs/tasks/task-b14-ymv.md` 已记);
- `yu-sound --test state gated_ogg_*` 在设置 `YURIS_SAMPLE_SDK0495` 时
  失败(装载当帧解码 67008 / 循环回绕未 seek),与本轮文本改动无关;
  未设该环境变量时 4 个 gated 用例显式 SKIP。
- `yu-format --test yssd_roundtrip` 依赖 `<project-eris>/save` 的
  `save50001.sd`;live save 已被官方 exe 改写为 `save00001/save30002`。
  用 `YURIS_SAVE_CORPUS=<save-backup-.../root>` 可全绿。
- wgpu `diffusion_permutes_blocks_on_gpu_and_matches_soft` 在并行
  全量跑时偶发 1 像素差,单独重跑通过(非文本路径)。
