# yu-rt

YU-RIS 运行核:把游戏工程数据变成可执行的任务系统,产出事件流。
脚本解释、命令分派、任务调度、变量域、帧边界全在这个 crate。

## 它干什么

- **启动链**(`tables::TableSet::load_from`):从工程数据装载全部表 ——
  `ysc.ybn`(命令表,顺序即 opcode)、`ysl.ybn`(标签)、`ysv.ybn`(变量镜像)、
  `yst_list.ybn`(脚本清单)、`yscfg.ybn`(引擎设置)、`system/YSCom/YSCom.ycd`
  (编译器语言定义);按 `BootConfig::entry_label` 查 ysl 落到
  `ysbin/yst%05d.ybn` 建主线任务。缺任一张表 ⇒ 拒绝启动。
  运行核内**没有**内置命令表/默认表(A5 零硬编码)。
- **执行**(`vm/`):按名分派到处理器;12 字节参数窗口按工程 ysc 表解析槽名;
  表达式/16 槽寄存器(PINT/PFLT/PSTR ×16 等)/变量域;GOSUB 嵌套 64。
- **任务与帧**(`task.rs`):任务泵到让出点(真实让出点 = `RETURNCODE`,
  非 `_`/`WORD`);全员让出 = 一个逻辑帧,`FrameStart`/`FrameEnd` 事件界帧。
- **时钟与输入**:时钟严格由调用方注入(`tick` 递增 / `tick_at` 固定值,
  对拍用);[`InputTimeline`](src/input.rs) 每逻辑帧推进一次。
- **事件流**(`event.rs`):`VmEvent` 有界队列,满则批量丢最旧 1/4 并计数
  (`events_dropped()`),长跑不无界增长。
- **同步图形宿主**:`set_graphics_host` 后,图形/窗口/文本/帧事件在**产生时**
  同步进入宿主;`CGINFO`/`FONTINFO`/`WINDOWINFO` 在脚本执行期查询并写回
  `LET`。**调用方不得再把同一批事件 drain 后重喂**(双消费)。
- **系统变量 ABI**(`sysvar_abi.rs`):按 `ysv.ybn` 指纹选族(四族数据表),
  不写死 id。

## 真值纪律

每条已实现语义标注三层出处:①官方手册页 ②真实 ERIS 编译语料(含频度)
③官方 0.495 引擎逆向事实。缺一层 ⇒ `Diag::Unimplemented` 挂号,
档案在 `crates/yu-rt/docs/divergences/`,逐命令证据在
`crates/yu-rt/docs/command-provenance.md`。

## 边界

- 容器解码唯一来源 = `yu-format`(本 crate 只做运行核适配投影);
- 渲染语义归 `yu-gfx`(本 crate 只发 `DomainCall` 事件,不解释图形);
- 窗口/音频/存档写盘归平台层;
- 入口标签名由调用方给出(默认 `SYSTEM_START`;官方 ERIS 线实证,机制 =
  按名查 ysl;跨作品是否字面通用为 Likely 约定,尚未逐作品验证)。

## 验证

```bash
cargo test -p yu-rt
YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-rt --test corpus --test corpus_golden --test consistency
```

进度与证据:`PROGRESS.md` 成果 C / E2 / F1 / F2。
