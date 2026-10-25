//! 命令处理器(按 ERIS 语料频度排序实现)。
//!
//! # 频度(官方 YSCom 编译的 137 个 YSTB,36,974 组 / 80,102 窗口全量扫描)
//!
//! | 命令 | 次数 | 本文件 | 三层出处 |
//! |---|---|---|---|
//! | `GOSUB` | 10,215 | ✅ | 手册 `control/gosub.html`;引擎 §3.16 |
//! | `LET` | 8,136 | ✅ | 手册 `variable/let.html`;引擎 §3.10/§3.17 |
//! | `IF` | 3,326 | ✅ | 手册 `control/if.html`;引擎 §3.13 |
//! | `IFEND` | 3,326 | ✅ | 同上 |
//! | `RETURN` | 1,750 | ✅ | 手册 `control/return.html`;引擎 §3.16 |
//! | `INT` | 1,428 | ✅ | 手册 `variable/int.html` |
//! | `IFBLEND` | 887 | ✅ | 手册 `control/if.html`(IF 链) |
//! | `ELSE` | 887 | ✅ | 手册 `control/else.html` |
//! | `S_INT` | 767 | ✅ | 手册 `variable/s_int.html` |
//! | `LOOP` | 522 | ✅ | 手册 `control/loop.html`;引擎 §3.13 |
//! | `STR` | 431 | ✅ | 手册 `variable/str.html` |
//! | `RETURNCODE` | 396 | ✅ | 行尾改行标记 + 自动 `C/R/P/E` 标志(REA `0x44c390`) |
//! | `WORD` | 464 | ✅ | 文本段 + 每字符 `'1'/'2'` 标志(REA `0x458d44`) |
//! | `F_INT` | 337 | ⛔ | **手册无页** ⇒ 不实现(DIV-RT-011) |
//! | `VARACT` | 310 | ✅部分 | 手册 `variable/varact.html`;引擎 §3.26 |
//! | `SAVE`/`LOAD` | 290/289 | ⛔ | 手册有页,存档容器未取证 ⇒ 交宿主(DIV-RT-012) |
//! | `CG*` | 534+274+100+100 | ⛔ | 手册有页,渲染域;事件流记 DomainCall |
//! | `GO` | 195 | ✅ | 手册 `control/go.html` |
//! | `FLT` | 173 | ✅ | 手册 `variable/flt.html` |
//! | `LOOPBREAK` | 159 | ✅ | 手册 `control/loopbreak.html` |
//! | `S_STR` | 144 | ✅ | 手册 `variable/s_str.html` |
//! | `VARINFO` | 119 | ✅部分 | 手册 `variable/varinfo.html`;引擎 §3.21 |
//! | `LOOPCONTINUE` | 40 | ✅ | 手册 `control/loopcontinue.html` |
//! | `WAIT` | 95 | ✅ | 手册 `control/wait.html`;引擎 §3.2b |
//! | `FPS` | 2 | ✅ | 手册 `system/fps.html` |
//! | `END` | 205 | ✅ | 手册 `control/end.html` |
//! | `TASK`/`TASKEND`/`TASKINFO` | 10/2/19 | ✅部分 | 手册 `control/task*.html` |
//! | `MATH` | 21 | ✅部分 | 手册 `math/math.html`;引擎 §3.20 |
//! | `TEXT`/`_`/`TEXTINFO` | 2/33/0 | ✅ | 手册 `text/text.html`+`basic/basic.html ●テキスト表示`+`text/textinfo.html`;REA `0x453448/0x4567c0/0x4538cc` |
//!
//! ⛔ = 按任务书军规「无手册页不实现 / 无引擎证据不猜」显式挂号。

use yu_log::Boundary;

use crate::code;
use crate::diag;
use crate::event::{ArgValue, Domain, JumpKind, VmEvent};
use crate::expr::LValue;
use crate::task::{Effect, StepOutcome, TaskSeed};
use crate::value::{ElemType, Value, VarRef, VarSpace};
use crate::vm::{Cmd, CmdWindow, ExecState, GosubFrame, IfFrame, LoopFrame, Vm, VmError};

/// 引擎嵌套上限(手册 `control/if.html` 64 / `control/loop.html` 64 /
/// `control/gosub.html`「GOSUBのネストは64回まで可能です」)。
const NEST_LIMIT: usize = 64;

/// GOSUB 嵌套上限的引擎判据(REA 0.495 `FUN_004459c4` @0x4459e6..0x4459f3):
/// `MOVSX ESI, byte [task+0x140]`(当前深度)→ `LEA EAX,[ESI+1]`
/// → `CMP EAX, 0x40; JE <报错>`。即**新深度 == 64** 就报错,允许的最大深度
/// 是 63 层调用栈帧;`task+0x140` 初值 0(只有调用帧计数,基帧不在其中)。
/// 故允许的 GOSUB 次数 = 64,对应本实现 `frames.len() == 65`(基帧 + 64 帧)。
const GOSUB_MAX_CALLS: usize = 64;

/// 运行核依赖的命令名(处理器注册表;缺任一 ⇒ 工程表不完整 ⇒ 拒绝启动)。
///
/// 这不是"内置命令表":它只列出**本 crate 实现了语义**的名字,名字本身仍
/// 必须由工程 `ysc.ybn` 提供;表里没有 = 该工程版本不兼容,直接拒启。
pub fn required_commands(yscm: &crate::tables::YscmTable) -> Result<(), yu_log::Diag> {
    const REQUIRED: &[&str] = &[
        "GOSUB", "RETURN", "GO", "IF", "ELSE", "IFBLEND", "IFEND", "LOOP", "LOOPEND", "WAIT",
        "LET", "INT", "FLT", "STR", "END",
    ];
    for name in REQUIRED {
        if yscm.index_of(name).is_none() {
            return Err(diag::fatal(
                code::table::INCOMPLETE,
                Boundary::Table,
                format!("ysc.ybn 缺少运行核必需命令 `{name}`(工程命令表不完整)"),
            )
            .at_table("ysc.ybn"));
        }
    }
    Ok(())
}

/// 分派热路径的处理器类别(启动期按工程 `ysc.ybn` 名字解析成 opcode 下标表;
/// 每条命令一次数组取下标,替代逐名字符串比较)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HandlerKind {
    Gosub,
    Return,
    Go,
    If,
    Else,
    Ifblend,
    Ifend,
    Ifbreak,
    Ifcontinue,
    Loop,
    Loopend,
    Loopbreak,
    Loopcontinue,
    Wait,
    Fps,
    End,
    Let,
    Declare,
    Varinfo,
    Varact,
    Math,
    Task,
    Taskend,
    Taskinfo,
    Systeminfo,
    /// `FILEINFO`:文件域查询(`EXIST` 写回 LET;EVID-J3-01)。
    Fileinfo,
    /// `MATH3D`:ERIS 专用 3D 投影(精灵坐标/缩放);REA `0x448900`。
    Math3d,
    /// 图形/字体/窗口只读查询(`LET` 同步写回,需 `GraphicsHost`)。
    Cginfo,
    Fontinfo,
    Windowinfo,
    Text,
    TextOut,
    Word,
    ReturnCode,
    Labelinfo,
    /// `TEXTINFO`:文本属性查询(VM 侧状态;REA `0x4538cc`)。
    Textinfo,
    /// `DIALOG`:模态对话框(VM 侧状态机;宿主呈现/回答;REA `0x43fc74`)。
    Dialog,
    /// `CLIPACT`:剪贴板写入(内核镜像 + 宿主同步;REA `0x43e2d0`)。
    Clipact,
    /// `CLIPINFO`:剪贴板读取并写 `LET`(REA `0x43e410`)。
    Clipinfo,
    /// `ERROR`:写「逐命令启用表」(`0x689b00`;REA `0x46358c`)。
    ErrorTable,
    /// 官方处理体只做参数求值、不产生副作用(`CSV`/`PLUG`/`ERRORINFO`)。
    NoopEval,
    /// 需要宿主能力(注册表/媒体元数据等),内核不猜。
    HostRequired(&'static str),
    /// 手册有页的域命令(CG/SOUND/WINDOW/…)→ DomainCall 事件。
    Domain(Domain),
    /// 官方 0.495 分发表里映射到 `0x4634d8`(`xor eax,eax; ret`)的槽:
    /// 真正的 no-op(DIV-RT-043)。`DEBUGLIST`/`LABEL`/`PREP`/`RETURNBREAK`/
    /// `S_INT`/`S_FLT`/`S_STR` 七条。
    Noop,
    /// 官方 0.495 分发表里未覆盖的默认槽 `0x459e90`:报「スクリプト実行
    /// エラー」并返回 -1(本任务本帧让出,不终止)。DIV-RT-043。
    ScriptError,
    /// 无处理器(手册无页且非域,且不在官方默认槽清单内)。
    Unknown,
}

/// 0.495 分发表中**未被覆盖**的槽(落到默认处理体 `0x459e90`)。
///
/// 证据(REA,0.495 `yu-ris.exe` sha256 `4f5eefa3…`):
/// `FUN_0045cf90` 先以 `MOV dword ptr [EAX+0x7ace20], 0x459e90` 填满
/// 119 槽(`0x45d1ae..0x45d1c0`,循环上限 `0x1dc`),随后逐槽覆盖;
/// 本清单 = 未被任何覆盖写到的命令名(按工程 `ysc.ybn` 序号反查)。
/// 默认体本身只有 6 条指令(`0x459e90..0x459ea4`):
/// `push 1; push 0xdbbd2; call 0x465d08; add esp,8; mov eax,-1; ret`。
///
/// `0xdbbd2` 是 `ysbin/yse.ybn` 的消息 id;官方语料 yse 解出
/// `「[内部エラー]\n\nスクリプト実行エラーが発生しました。」`。
/// 因此**不是 no-op**:报一次全局脚本执行错误 + 返回非 0(本帧让出),
/// 任务不终止(`FUN_004612b0` 用全局 `[0x5e4f84]` 抑制重复弹窗)。
const DEFAULT_SLOT_COMMANDS_0495: &[&str] = &[
    "F_FLT",
    "F_INT",
    "F_STR",
    "F_VAR",
    "G_FLT",
    "G_FLT2",
    "G_FLT3",
    "G_FLT4",
    "G_INT",
    "G_INT2",
    "G_INT3",
    "G_INT4",
    "G_STR",
    "G_STR2",
    "G_STR3",
    "G_STR4",
    "G_VAR",
    "MACRO",
    "S_VAR",
    "_IFDEF",
    "_IFNDEF",
    "_IFEND",
    ");",
    "SYSTEMMODE",
    "COMPILEMODE",
    "RELEASEMODE",
    "PROJECTFOLDER",
];

/// 0.495 分发表中映射到 `0x4634d8` 的命令:真正的 no-op(`xor eax,eax; ret`)。
///
/// REA `FUN_0045cf90` 的覆盖写:`0x7ace44`(DEBUGLIST)/`0x7acef0`(LABEL)/
/// `0x7acf4c`(PREP)/`0x7acf58`(RETURNBREAK)/`0x7acf60`(S_FLT)/
/// `0x7acf64`(S_INT)/`0x7acf68`(S_STR)。
const NOOP_COMMANDS_0495: &[&str] = &[
    "DEBUGLIST",
    "LABEL",
    "PREP",
    "RETURNBREAK",
    "S_INT",
    "S_FLT",
    "S_STR",
];

/// 按工程 yscm 建表(名字 → 处理器类别)。表 = 数据解析结果,不是内置命令表。
/// 注意:默认槽/无操作槽是 **0.495 引擎分发表**的属性;命令名与序号仍来自
/// 工程 `ysc.ybn`(DIV-RT-001 的版本隔离不因此改变)。
pub fn build_handler_table(yscm: &crate::tables::YscmTable) -> Vec<HandlerKind> {
    let mut table = Vec::with_capacity(yscm.len());
    for i in 0..yscm.len() {
        let name = yscm.command(i as u8).map(|c| c.name.as_str()).unwrap_or("");
        // 官方槽位判定先于域分类:`SYSTEMMODE` 虽以 SYSTEM 开头,但分发表
        // 未覆盖它 ⇒ 执行到就是脚本执行错误,不能当域事件静默放过。
        let kind = if DEFAULT_SLOT_COMMANDS_0495.contains(&name) {
            HandlerKind::ScriptError
        } else if NOOP_COMMANDS_0495.contains(&name) {
            HandlerKind::Noop
        } else {
            match name {
                "GOSUB" => HandlerKind::Gosub,
                "RETURN" => HandlerKind::Return,
                "GO" => HandlerKind::Go,
                "IF" => HandlerKind::If,
                "ELSE" => HandlerKind::Else,
                "IFBLEND" => HandlerKind::Ifblend,
                "IFEND" => HandlerKind::Ifend,
                "IFBREAK" => HandlerKind::Ifbreak,
                "IFCONTINUE" => HandlerKind::Ifcontinue,
                "LOOP" => HandlerKind::Loop,
                "LOOPEND" => HandlerKind::Loopend,
                "LOOPBREAK" => HandlerKind::Loopbreak,
                "LOOPCONTINUE" => HandlerKind::Loopcontinue,
                "WAIT" => HandlerKind::Wait,
                "FPS" => HandlerKind::Fps,
                "END" => HandlerKind::End,
                "LET" => HandlerKind::Let,
                "INT" | "FLT" | "STR" => HandlerKind::Declare,
                "VARINFO" => HandlerKind::Varinfo,
                "VARACT" => HandlerKind::Varact,
                "MATH" => HandlerKind::Math,
                "TASK" => HandlerKind::Task,
                "TASKEND" => HandlerKind::Taskend,
                "TASKINFO" => HandlerKind::Taskinfo,
                "SYSTEMINFO" => HandlerKind::Systeminfo,
                "FILEINFO" => HandlerKind::Fileinfo,
                "MATH3D" => HandlerKind::Math3d,
                "CGINFO" => HandlerKind::Cginfo,
                "FONTINFO" => HandlerKind::Fontinfo,
                "WINDOWINFO" => HandlerKind::Windowinfo,
                "TEXT" => HandlerKind::Text,
                "_" => HandlerKind::TextOut,
                "WORD" => HandlerKind::Word,
                "RETURNCODE" => HandlerKind::ReturnCode,
                "TEXTINFO" => HandlerKind::Textinfo,
                "LABELINFO" => HandlerKind::Labelinfo,
                "DIALOG" => HandlerKind::Dialog,
                // 0.495 有独立处理体但无脚本可见返回(TRAY=托盘图标)。
                "TRAY" => HandlerKind::Domain(Domain::System),
                "CLIPACT" => HandlerKind::Clipact,
                "CLIPINFO" => HandlerKind::Clipinfo,
                // REA:只调参数求值器后返回 0(`0x43e640`/`0x44bd10`/`0x46356c`)。
                "CSV" | "PLUG" | "ERRORINFO" => HandlerKind::NoopEval,
                "ERROR" => HandlerKind::ErrorTable,
                // 需要宿主:Windows 注册表 / YMV 媒体元数据。
                "REGINFO" => HandlerKind::HostRequired("Windows 注册表读取"),
                "YMVINFO" => HandlerKind::HostRequired("YMV 媒体元数据查询"),
                // YMV/YMVEND 只控制媒体对象生命周期,无脚本可见返回。
                "YMV" | "YMVEND" => HandlerKind::Domain(Domain::Movie),
                other => {
                    let domain = domain_of(other);
                    if domain != Domain::Other {
                        HandlerKind::Domain(domain)
                    } else {
                        HandlerKind::Unknown
                    }
                }
            }
        };
        table.push(kind);
    }
    table
}

impl Vm {
    /// 引用型参数槽白名单(DIV-RT-042,B1 裁 1-A):该命令的哪些槽在引擎里
    /// 传**变量地址**(REA SAVE `0x44c45c`:身份经 `0x683a60/0x683c00/
    /// 0x683da0[slot]` 三表;LOAD `0x44708c` 的 LET 同型;FILEINFO
    /// `0x4432dc`/SOUNDINFO `0x44f4bc` 的 LET 同族)。事件在求值结果之外
    /// 保留归一化 `VarRef`;其余命令/槽维持只传值(M-2 形态,普查不变)。
    fn reference_slot_names(cmd: &str) -> Option<&'static str> {
        match cmd {
            "SAVE" => Some("SET"),
            "LOAD" | "FILEINFO" | "SOUNDINFO" => Some("LET"),
            _ => None,
        }
    }

    /// 把命令窗口逐条求值成 `(槽号, 值)`。跳转窗口与空窗口跳过;
    /// 求值失败省略该窗口(处理器/诊断会给出真正的失败原因)。
    fn eval_arg_windows(&mut self, st: &mut ExecState, cmd: &Cmd) -> Vec<ArgValue> {
        // 引用槽号(有白名单时按工程 yscm 参数名解析;查不到表 = 无引用槽)。
        let ref_name = Self::reference_slot_names(&cmd.name);
        let ref_slots: Vec<u8> = ref_name
            .map(|want| {
                self.tables()
                    .boot
                    .yscm
                    .command(cmd.opcode)
                    .map(|c| {
                        c.params
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| p.name == want)
                            .map(|(i, _)| i as u8)
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let mut args = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            if ref_slots.contains(&w.rec.kind) {
                // 引用槽:保留变量身份。求值失败退回普通值求值(原形态)。
                if let Ok(lv) = self.eval_lvalue_window(st, w) {
                    let var = self.canonical_var(lv.var);
                    if let Ok(v) = self.read_var(st, var, &lv.indices) {
                        args.push(ArgValue::with_ref(w.rec.kind, v, var));
                        continue;
                    }
                }
            }
            if let Ok(v) = self.eval_window(st, w) {
                args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        args
    }

    pub(crate) fn dispatch(
        &mut self,
        st: &mut ExecState,
        task: &mut crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        // 统一事件:尽量把每个窗口求值结果作为参数记录(失败则省略该窗口,
        // 真正的失败会在处理器里以 Diag 报出)。控制流窗口(跳转目标)是
        // 「length 字段 = 组号」而不是表达式,所以只记录成功求值者。
        // `record_events=false` 时跳过(热路径最大开销;Diag 不受影响)。
        let args = if self.record_events {
            self.eval_arg_windows(st, cmd)
        } else {
            Vec::new()
        };
        self.record_command(st, cmd, args);

        // 启动期解析的 opcode → 处理器表(见 `build_handler_table`)。
        let kind = self
            .handler_of
            .get(cmd.opcode as usize)
            .copied()
            .unwrap_or(HandlerKind::Unknown);
        match kind {
            HandlerKind::Gosub => self.op_gosub(st, cmd),
            HandlerKind::Return => self.op_return(st, cmd),
            HandlerKind::Go => self.op_go(st, cmd),
            HandlerKind::If => self.op_if(st, cmd),
            HandlerKind::Else => self.op_else(st, cmd),
            HandlerKind::Ifblend => self.op_ifblend(st, cmd),
            HandlerKind::Ifend => self.op_ifend(st, cmd),
            HandlerKind::Ifbreak => self.op_ifbreak(st, cmd),
            HandlerKind::Ifcontinue => self.op_ifcontinue(st, cmd),
            HandlerKind::Loop => self.op_loop(st, cmd),
            HandlerKind::Loopend => self.op_loopend(st, cmd),
            HandlerKind::Loopbreak => self.op_loopbreak(st, cmd),
            HandlerKind::Loopcontinue => self.op_loopcontinue(st, cmd),
            HandlerKind::Wait => self.op_wait(st, task, cmd),
            HandlerKind::Fps => self.op_fps(st, cmd),
            HandlerKind::End => self.op_end(st, cmd),
            HandlerKind::Let => self.op_let(st, cmd),
            HandlerKind::Declare => self.op_declare(st, cmd),
            HandlerKind::Varinfo => self.op_varinfo(st, cmd),
            HandlerKind::Varact => self.op_varact(st, cmd),
            HandlerKind::Math => self.op_math(st, cmd),
            HandlerKind::Task => self.op_task(st, task, cmd),
            HandlerKind::Taskend => self.op_taskend(st, task, cmd),
            HandlerKind::Taskinfo => self.op_taskinfo(st, task, cmd),
            HandlerKind::Systeminfo => self.op_systeminfo(st, cmd),
            HandlerKind::Fileinfo => self.op_fileinfo(st, cmd),
            HandlerKind::Math3d => self.op_math3d(st, cmd),
            HandlerKind::Cginfo => self.op_info(st, cmd, "CGINFO"),
            HandlerKind::Fontinfo => self.op_info(st, cmd, "FONTINFO"),
            HandlerKind::Windowinfo => self.op_info(st, cmd, "WINDOWINFO"),
            HandlerKind::Text => self.op_text(st, cmd),
            HandlerKind::TextOut => self.op_text_out(st, cmd),
            HandlerKind::Word => self.op_word(st, cmd),
            HandlerKind::ReturnCode => self.op_returncode(st, cmd),
            HandlerKind::Textinfo => self.op_textinfo(st, cmd),
            HandlerKind::Labelinfo => self.op_labelinfo(st, cmd),
            HandlerKind::Dialog => self.op_dialog(st, cmd),
            HandlerKind::Clipact => self.op_clipact(st, cmd),
            HandlerKind::Clipinfo => self.op_clipinfo(st, cmd),
            HandlerKind::ErrorTable => self.op_error(st, cmd),
            HandlerKind::NoopEval => {
                // 官方处理体 = 参数求值 + `xor eax,eax; ret`。求值错误的
                // 可恢复诊断照常记录,然后继续。
                for w in &cmd.windows {
                    if w.rec.length == 0 {
                        continue;
                    }
                    let _ = self.eval_window(st, w);
                }
                Ok(StepOutcome::Continue)
            }
            HandlerKind::HostRequired(what) => Err(diag::unimplemented(
                code::rt::CMD_UNIMPLEMENTED,
                Boundary::Script,
                format!(
                    "命令 `{}` 需要宿主能力({what});内核不猜(DIV-RT-045)",
                    cmd.name
                ),
            )
            .into()),
            // 官方 0.495 `0x4634d8`(xor eax,eax; ret):真的什么都不做。
            HandlerKind::Noop => Ok(StepOutcome::Continue),
            // 官方 0.495 默认槽 `0x459e90`:报一次全局「スクリプト実行エラー」
            // (yse 0xdbbd2)并返回 -1 ⇒ 本任务本帧让出,pc 已前进,任务不终止。
            HandlerKind::ScriptError => {
                self.note_script_slot();
                if !self.script_error_reported() {
                    self.mark_script_error_reported();
                    self.report(diag::recoverable(
                        code::rt::SCRIPT_ERROR,
                        Boundary::Script,
                        format!(
                            "命令 `{}` 未在官方分发表覆盖,落到默认槽 0x459e90:\
                             官方报「[内部エラー] スクリプト実行エラーが発生しました。」\
                             (yse 0xdbbd2)并返回 -1(本帧让出,不终止任务;DIV-RT-043)",
                            cmd.name
                        ),
                    ));
                }
                Ok(StepOutcome::Yield)
            }
            HandlerKind::Domain(domain) => {
                // INPUT/MOUSE 是**输入状态机的一部分**(脚本可读计数器),
                // 不是纯宿主表现:内核必须自己应用(REA 0x446564/0x44aaa0)。
                if domain == Domain::System {
                    match cmd.name.as_str() {
                        "INPUT" => return self.op_input(st, cmd),
                        "MOUSE" => return self.op_mouse(st, cmd),
                        _ => {}
                    }
                }
                // 只在宿主已安装时走新的 LET 写回路径；未安装时保持
                // M-2 的既有行为（通用域事件 + rt.cmd.unimplemented），
                // 保证 600 帧普查逐位不变。
                if domain == Domain::Sound && cmd.name == "SOUNDINFO" && self.sound_host.is_some() {
                    return self.op_soundinfo(st, cmd);
                }
                // 存档域同步宿主(B1):SAVE/LOAD/SAVEEND 在命令时刻走
                // 预约/刷盘/拉取契约(REA:预约即快照 0x44d73d,LOAD 写回
                // 0x447b3d);SAVEINFO 无同步路径,落通用域事件。
                if domain == Domain::Save && self.save_host.is_some() {
                    match cmd.name.as_str() {
                        "SAVE" => return self.op_save(st, cmd),
                        "LOAD" => return self.op_load(st, cmd),
                        "SAVEEND" => return self.op_saveend(st, cmd),
                        _ => {}
                    }
                }
                // 域命令的参数是宿主域状态机唯一能看到的输入;必须带上
                // 求值结果(槽号 = 工程 ysc 表的参数序号)。`record_events`
                // 只控制通用 Command 事件,不影响域事件。
                let args = self.eval_arg_windows(st, cmd);
                self.emit_domain(domain, cmd.name.clone(), args);
                // Graphics/Window 已由同步宿主消费时不再挂号;声音域同理。
                let handled = (self.graphics_host.is_some()
                    && matches!(domain, Domain::Graphics | Domain::Window))
                    || (self.sound_host.is_some()
                        && domain == Domain::Sound
                        && matches!(cmd.name.as_str(), "SOUND" | "SOUNDEND"));
                if !handled {
                    if matches!(cmd.name.as_str(), "MIDI" | "MIDIEND" | "MIDIINFO" | "CDDA") {
                        // 官方 0.495 处理体是空桩(0x44aa40/0x44aa60/0x44aa80、
                        // CDDA 0x429fb0:仅 FUN_00429fd1 守卫后 return 0),无媒体
                        // 输出;内核同型对齐 —— 域事件照发、不建任何声音状态。
                        // 终态 aligned-stub(声音台账 0-9/N0-10),不计挂号。
                        self.report_aligned_stub(
                            &cmd.name,
                            "官方 0.495 处理体为空桩(0x44aa40/0x44aa60/0x44aa80、\
                             CDDA 0x429fb0),同型对齐:只发域事件,无媒体输出(声音台账 N0-10)"
                                .to_string(),
                        );
                    } else {
                        // 挂号文案给**可执行的**锚点:域事件契约 §9 是域边界的
                        // 正式定义处(`docs/domain-event-contracts.md`);旧文案
                        // 引「任务书 A」,该文档只覆盖 workspace/yu-log/yu-vfs,
                        // 从来没有域边界章节(2026-10-24 核对)。
                        self.report_unimplemented(
                            &cmd.name,
                            "域命令未接线:语义属宿主域状态机(域边界见 \
                             crates/yu-rt/docs/domain-event-contracts.md §9)"
                                .to_string(),
                        );
                    }
                }
                Ok(StepOutcome::Continue)
            }
            HandlerKind::Unknown => {
                self.report_unimplemented(
                    &cmd.name,
                    "命令无处理器(手册无页或语义未取证)".to_string(),
                );
                Ok(StepOutcome::Continue)
            }
        }
    }

    // ---------------------------------------------------------- 控制流

    /// `GOSUB`:手册 `control/gosub.html`;引擎 §3.16(参数组 A + 调用栈)。
    fn op_gosub(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            return Err(windows_error(cmd, "至少需要 # 标签窗口").into());
        }
        let label = self.label_from_window(st, cmd, 0)?;
        // 参数:窗口 kind ≥ 1 对应 PINT/PFLT/PSTRn(名字来自工程 ysc)。
        let mut frame = GosubFrame::new(st.script_id, st.pc, st.base_a);
        for w in cmd.windows.iter().skip(1) {
            if w.rec.length == 0 {
                continue;
            }
            let pname = self.param_name(cmd.opcode, w.rec.kind).unwrap_or_default();
            let idx = param_slot_index(&pname, cmd.windows[0].rec.kind, "GOSUB")?;
            let value = self.eval_window(st, w)?;
            let value = coerce_param(&pname, value);
            frame.args[idx] = Some(value);
            frame.arg_flags[idx] = true;
        }
        if st.frames.len() > GOSUB_MAX_CALLS {
            return Err(
                crate::task::nesting_error("GOSUB", st.frames.len() - 1, GOSUB_MAX_CALLS).into(),
            );
        }
        // 块/循环栈按调用层隔离:RETURN 时截断回进入前的深度。
        frame.if_depth = st.if_stack.len();
        frame.loop_depth = st.loops.len();
        st.frames.push(frame);
        let from_pc = cmd.pc;
        self.jump_to(
            st,
            label.script_id as u32,
            label.pc,
            label.base_a,
            JumpKind::Gosub,
            from_pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// `RETURN`:手册 `control/return.html`;引擎 §3.16(写调用方那一层后弹栈)。
    ///
    /// 手册 `basic/task.html`:`GOSUB` 嵌套不存在时到达 `RETURN` 的任务**自动消失**。
    fn op_return(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if st.frames.len() <= 1 {
            // 基帧 RETURN = 任务结束(手册 `basic/task.html`)。
            return Ok(StepOutcome::Finish);
        }
        let mut ret: [Option<Value>; 16] = Default::default();
        let mut flags = [false; 16];
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = self.param_name(cmd.opcode, w.rec.kind).unwrap_or_default();
            // 与 GOSUB 同一守卫:此前 RETURN 缺这条,**`RINT0` 会 usize 下溢、
            // `RINT17+` 会越界 panic**(DIV-RT-034「panic 收口」)。
            let idx = param_slot_index(&pname, w.rec.kind, "RETURN")?;
            let value = self.eval_window(st, w)?;
            let value = coerce_param(&pname, value);
            ret[idx] = Some(value);
            flags[idx] = true;
        }
        let frame = st.frames.pop().expect("帧数 > 1");
        // 被调层未闭合的 IF/LOOP 不得泄漏给调用方(块结构按调用层独立)。
        st.if_stack.truncate(frame.if_depth);
        st.loops.truncate(frame.loop_depth);
        // 返回值写进**调用方**那一层(弹出后 `last_mut()`)。
        if let Some(caller) = st.frames.last_mut() {
            caller.ret = ret;
            caller.ret_flags = flags;
        }
        let from_pc = cmd.pc;
        self.jump_to(
            st,
            frame.return_script,
            frame.return_pc,
            frame.base_a,
            JumpKind::Return,
            from_pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// `GO`:手册 `control/go.html`(标签跳转,可跨文件;标签名全局唯一)。
    fn op_go(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let label = self.label_from_window(st, cmd, 0)?;
        let from_pc = cmd.pc;
        self.jump_to(
            st,
            label.script_id as u32,
            label.pc,
            label.base_a,
            JumpKind::Go,
            from_pc,
        )?;
        Ok(StepOutcome::Continue)
    }

    /// `IF`:手册 `control/if.html`;引擎 §3.13(块帧 + 假分支跳 `item[2].length`,
    /// 为 0 则跳块出口)。
    fn op_if(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() < 3 {
            return Err(windows_error(cmd, "IF 需要 3 个窗口(条件/ELSE 目标/出口)").into());
        }
        if st.if_stack.len() >= NEST_LIMIT {
            return Err(crate::task::nesting_error("IF", st.if_stack.len(), NEST_LIMIT).into());
        }
        let end_pc = cmd.windows[2].rec.length;
        // **先压块帧,再求值** —— 引擎 §3.13 opcode 45 的顺序:
        // 「压块帧(base_a++、出口 = item[3].length)→ 求值 item[1]」。
        // 顺序不是风格问题:条件求值若失败(例如未取证的系统变量),
        // 先压帧能保证块结构完整,否则后面的 IFEND 会失配。
        st.if_stack.push(IfFrame {
            entry_pc: cmd.pc,
            end_pc,
            base_a: st.base_a,
        });
        // 条件求值:可恢复失败(Unimplemented)⇒ 记诊断并按「假」处理;
        // Fatal 照常抛出(任务终止)。
        let truthy = match self.eval_window(st, &cmd.windows[0]) {
            Ok(v) => v.truthy()?,
            Err(d) => {
                let recoverable = matches!(
                    d.severity,
                    yu_log::Severity::Unimplemented | yu_log::Severity::Recoverable
                );
                if !recoverable {
                    return Err(d.into());
                }
                self.report(d);
                false
            }
        };
        if truthy {
            return Ok(StepOutcome::Continue);
        }
        let else_pc = cmd.windows[1].rec.length;
        let to = if else_pc != 0 { else_pc } else { end_pc };
        if to == 0 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                "IF 假分支目标为 0(ELSE 目标与出口都未给)",
            )
            .into());
        }
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: JumpKind::IfFalse,
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `ELSE`:手册 `control/else.html`(条件可缺省;假 → 下一个 ELSE 或块出口)。
    ///
    /// 两种真实形态(两套语料都实测):
    /// - **3 窗口**:ERIS 887 次 / 商业包 704 次 —— `[条件, 下一 ELSE 组号, 下一 ELSE 组号]`;
    /// - **0 窗口**:商业包 726 次 —— 编译器在「无条件 else 体」处发出的形态
    ///   (实测结构:`ELSE wc=0` → 体 → `IFEND`,如 v494 s52 g60..g65)。
    ///   语义 = 进入 else 体(条件缺省为真)。
    fn op_else(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            // 无条件 else 体:继续执行下一条(与「条件缺省 = 真」同义)。
            return Ok(StepOutcome::Continue);
        }
        if cmd.windows.len() < 3 {
            return Err(windows_error(cmd, "ELSE 的窗口数既不是 0 也不是 3").into());
        }
        let top = *st.if_stack.last().ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                "ELSE 无配对 IF(引擎同族报错)",
            )
        })?;
        let cond = if cmd.windows[0].payload.is_empty() {
            None
        } else {
            Some(self.eval_window(st, &cmd.windows[0])?)
        };
        let truthy = match cond {
            Some(v) => v.truthy()?,
            None => true,
        };
        if truthy {
            return Ok(StepOutcome::Continue);
        }
        let next = cmd.windows[1].rec.length;
        let to = if next != 0 { next } else { top.end_pc };
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: JumpKind::Else,
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `IFBLEND`:无条件跳到当前块出口(§3.13;手册 IF 页)。
    fn op_ifblend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let top = *st.if_stack.last().ok_or_else(|| {
            diag::fatal(code::rt::CMD_WINDOWS, Boundary::Script, "IFBLEND 无配对 IF")
        })?;
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: top.end_pc,
            to_script: st.script_id,
            kind: JumpKind::Else,
        });
        st.pc = top.end_pc;
        Ok(StepOutcome::Continue)
    }

    /// `IFEND`:弹块帧(§3.13:depth-- 并清出口)。
    fn op_ifend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if st.if_stack.pop().is_none() {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("IFEND 无配对 IF(s{} pc={})", st.script_id, cmd.pc),
            )
            .into());
        }
        Ok(StepOutcome::Continue)
    }

    /// `IFBREAK[LV]`:退 LV 层块并跳该层出口(§3.13 opcode 47)。
    fn op_ifbreak(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.if_break_to(st, cmd, lv, false)
    }

    /// `IFCONTINUE[LV]`:退 LV 层并跳该层入口 pc(§3.13 opcode 48)。
    fn op_ifcontinue(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.if_break_to(st, cmd, lv, true)
    }

    fn if_break_to(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
        lv: usize,
        cont: bool,
    ) -> Result<StepOutcome, VmError> {
        // LV=0:无层可退(与 `LOOPBREAK[LV=0]` 同口径;修复越界 panic,
        // 任务书 E R7)。
        if lv == 0 {
            return Ok(StepOutcome::Continue);
        }
        if st.if_stack.len() < lv {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!(
                    "{} 层级 {lv} 越界(IF 深度 {})",
                    if cont { "IFCONTINUE" } else { "IFBREAK" },
                    st.if_stack.len()
                ),
            )
            .into());
        }
        let keep = st.if_stack.len() - lv;
        let target = &st.if_stack[keep];
        let to = if cont { target.entry_pc } else { target.end_pc };
        let base = target.base_a;
        if cont {
            // IFCONTINUE(REA 0x446490):引擎把深度设为 `depth-LV`,pc = 目标层
            // `+4`(入口 pc);目标 IF 会在入口处**重新压帧**。所以这里必须
            // **丢弃**目标层本身,否则 IF 再压一次会让块帧永久累积
            // (任务书 E R9)。
            st.if_stack.truncate(keep);
        } else {
            // IFBREAK(REA 0x446408):引擎把深度设为 `depth-LV+1` 并跳目标层
            // `+8`(IFEND 组);IFEND 执行时再 `depth--` 落到 `depth-LV`。
            // 即**保留**目标层,由目标 IFEND 弹掉它。
            st.if_stack.truncate(keep + 1);
        }
        st.base_a = base;
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: to,
            to_script: st.script_id,
            kind: if cont {
                JumpKind::IfContinue
            } else {
                JumpKind::IfBreak
            },
        });
        st.pc = to;
        Ok(StepOutcome::Continue)
    }

    /// `LOOP`:手册 `control/loop.html`;引擎 §3.13(opcode 56)。
    fn op_loop(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() < 2 {
            return Err(windows_error(cmd, "LOOP 需要 2 个窗口(SET/LOOPEND 目标)").into());
        }
        if st.loops.len() >= NEST_LIMIT {
            return Err(crate::task::nesting_error("LOOP", st.loops.len(), NEST_LIMIT).into());
        }
        // SET 缺省(空窗口)= 无限(手册)。
        let limit = if cmd.windows[0].payload.is_empty() {
            -1
        } else {
            match self.eval_window(st, &cmd.windows[0])? {
                Value::Int(n) => n,
                other => {
                    return Err(diag::fatal(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        format!("LOOP 计数必须是整数,得到 {}", other.type_name()),
                    )
                    .into())
                }
            }
        };
        let end_pc = cmd.windows[1].rec.length;
        st.loops.push(LoopFrame {
            body_pc: cmd.pc + 1,
            end_pc,
            limit,
            counter: 1,
            if_depth: st.if_stack.len(),
            base_a: st.base_a,
        });
        if limit == 0 {
            // 0 轮:直接去 LOOPEND(由它弹栈;手册「一度も通らず」)。
            st.pc = end_pc;
        }
        Ok(StepOutcome::Continue)
    }

    /// `LOOPEND`:手册 `control/loop.html`;引擎 §3.13(opcode 59)。
    fn op_loopend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let Some(top) = st.loops.last().copied() else {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("LOOPEND 无配对 LOOP(s{} pc={})", st.script_id, cmd.pc),
            )
            .into());
        };
        let counter = top.counter + 1;
        let continue_loop = if top.limit < 0 {
            true
        } else {
            counter <= top.limit
        };
        if continue_loop {
            if let Some(f) = st.loops.last_mut() {
                f.counter = counter;
            }
            // `_LC` 必须读到新值;引擎 LOOPEND 先 +1 再回跳。
            st.base_a = top.base_a;
            self.emit(VmEvent::Jump {
                frame: self.frame(),
                script: st.script_id,
                pc: cmd.pc,
                to_pc: top.body_pc,
                to_script: st.script_id,
                kind: JumpKind::LoopBack,
            });
            st.pc = top.body_pc;
        } else {
            st.loops.pop();
        }
        Ok(StepOutcome::Continue)
    }

    /// `LOOPBREAK[LV]`:手册 `control/loopbreak.html`;引擎 §3.13(opcode 57)。
    fn op_loopbreak(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.loop_jump(st, cmd, lv, false)
    }

    /// `LOOPCONTINUE[LV]`:手册 `control/loopcontinue.html`;§3.13(opcode 58)。
    fn op_loopcontinue(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let lv = self.level_param(st, cmd)?;
        self.loop_jump(st, cmd, lv, true)
    }

    fn loop_jump(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
        lv: usize,
        cont: bool,
    ) -> Result<StepOutcome, VmError> {
        if lv == 0 {
            return Ok(StepOutcome::Continue);
        }
        if st.loops.len() < lv {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!(
                    "{} 层级 {lv} 越界(LOOP 深度 {})",
                    if cont { "LOOPCONTINUE" } else { "LOOPBREAK" },
                    st.loops.len()
                ),
            )
            .into());
        }
        let keep = st.loops.len() - lv;
        let target = st.loops[keep];
        st.loops.truncate(keep + 1);
        if !cont {
            // break:把目标层的目标轮数清零 ⇒ 即将执行的 LOOPEND 立即结束。
            if let Some(f) = st.loops.last_mut() {
                f.limit = 0;
                f.counter = 0;
            }
        }
        // 体内残留 IF 帧一并展开(引擎块记录随执行位恢复;§3.13)。
        if st.if_stack.len() > target.if_depth {
            st.if_stack.truncate(target.if_depth);
        }
        st.base_a = target.base_a;
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: cmd.pc,
            to_pc: target.end_pc,
            to_script: st.script_id,
            kind: if cont {
                JumpKind::LoopContinue
            } else {
                JumpKind::LoopBreak
            },
        });
        st.pc = target.end_pc;
        Ok(StepOutcome::Continue)
    }

    /// `WAIT[FRAME=n|TIME=ms]`:手册 `control/wait.html`;引擎 §3.2b/§3.2c。
    fn op_wait(
        &mut self,
        st: &mut ExecState,
        task: &mut crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut frames = None;
        let mut time = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
            let n = v.as_int().ok_or_else(|| {
                diag::fatal(
                    code::rt::VAR_TYPE,
                    Boundary::Script,
                    format!("WAIT 参数必须是整数,得到 {}", v.type_name()),
                )
            })?;
            match w.rec.kind {
                0 => frames = Some(n.max(0) as u32),
                1 => time = Some(n.max(0) as u32),
                other => {
                    return Err(diag::fatal(
                        code::rt::CMD_SLOT,
                        Boundary::Script,
                        format!("WAIT 未声明参数序号 {other}(工程 ysc 只有 FRAME/TIME)"),
                    )
                    .into())
                }
            }
        }
        if frames.is_none() && time.is_none() {
            // 裸 `WAIT[]`:手册 `basic/task.html`「ウェイト命令…タスクは
            // それを実行したあと、一旦休憩します(約0.016秒)」⇒ 让出一帧
            // (`control/wait.html` 只定义 TIME/FRAME,无参时长未定义 ——
            // 取 1 帧,Likely;语料 95/95 全带参,零行为差)。
            task.wait_frames = 1;
            self.emit(VmEvent::Wait {
                frame: self.frame(),
                task: task.id.clone(),
                frames: Some(1),
                time_ms: None,
            });
            return Ok(StepOutcome::Yield);
        }
        task.wait_frames = frames.unwrap_or(0);
        task.deadline_ms = time.map(|ms| self.now_ms().wrapping_add(ms));
        self.emit(VmEvent::Wait {
            frame: self.frame(),
            task: task.id.clone(),
            frames,
            time_ms: task.deadline_ms.map(|d| d.wrapping_sub(self.now_ms())),
        });
        Ok(StepOutcome::Yield)
    }

    /// `FPS[SET=1..256 SKIP=0/1]`:手册 `system/fps.html`。
    fn op_fps(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut fps = self.fps();
        let mut skip = self.skip();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
            let n = v.as_int().unwrap_or(0);
            match w.rec.kind {
                0 => {
                    if !(1..=256).contains(&n) {
                        return Err(diag::fatal(
                            code::rt::CMD_SLOT,
                            Boundary::Script,
                            format!("FPS[SET={n}] 超出手册范围 1..256"),
                        )
                        .into());
                    }
                    fps = n as u32;
                }
                1 => skip = n != 0,
                other => {
                    return Err(diag::fatal(
                        code::rt::CMD_SLOT,
                        Boundary::Script,
                        format!("FPS 未声明参数序号 {other}"),
                    )
                    .into())
                }
            }
        }
        self.set_fps(fps, skip);
        self.emit(VmEvent::Fps {
            frame: self.frame(),
            fps,
            skip,
        });
        Ok(StepOutcome::Continue)
    }

    /// `END[RESTART]`:手册 `control/end.html`。
    fn op_end(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut restart = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
            if w.rec.kind == 0 {
                restart = v.as_int().unwrap_or(0) != 0;
            }
        }
        self.emit(VmEvent::End {
            frame: self.frame(),
            restart,
        });
        let _ = st;
        Ok(StepOutcome::Finish)
    }

    // ---------------------------------------------------------- 变量

    /// `LET`:手册 `variable/let.html`;引擎 §3.17(opcode 54)+ §3.10。
    ///
    /// 复合赋值码在窗口 `slot` 高字节(语料实测 0..8;`tag>>24` 同源)。
    fn op_let(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() != 2 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("LET 窗口数 {} != 2", cmd.windows.len()),
            )
            .into());
        }
        let lval = self.eval_lvalue_window(st, &cmd.windows[0])?;
        // ERIS 旧式空间按 id 的声明类型归一化;其它未知前缀挂号继续(DIV-RT-017)。
        let lval = crate::expr::LValue {
            var: self.canonical_var(lval.var),
            indices: lval.indices.clone(),
        };
        if matches!(lval.var.space, VarSpace::Other(_)) {
            return Err(diag::unimplemented(
                code::rt::EXPR_UNSUPPORTED,
                Boundary::Script,
                format!(
                    "写入未取证变量空间 {:#04x}({});本次写入被忽略(DIV-RT-017)",
                    lval.var.space.prefix(),
                    lval.var.display()
                ),
            )
            .into());
        }
        // 输入系统变量(手册标 ○ 可赋值):写入 = 设定该按钮的帧计数器。
        if lval.var.id < self.sys_count {
            let name = self
                .sysvars()
                .get(lval.var.id)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let button = name
                .strip_prefix("_KEY_")
                .or_else(|| name.strip_prefix("_MOUSE_"))
                .filter(|rest| {
                    // 只收按钮族(排除 _MOUSE_X/Y0..3 坐标槽与 _KEYNUM_* 等;
                    // X/Y 后缀数字 = 窗口槽坐标族,REA `FUN_00427350`
                    // case 0x85..0x8e,非按钮)。
                    !matches!(*rest, "LAST")
                        && !matches!(*rest, "X" | "Y")
                        && !rest
                            .strip_suffix(['0', '1', '2', '3'])
                            .map(|base| matches!(base, "X" | "Y"))
                            .unwrap_or(false)
                        && !rest
                            .chars()
                            .next()
                            .map(|c| c.is_ascii_digit())
                            .unwrap_or(true)
                })
                .map(|rest| {
                    if name.starts_with("_KEY_") {
                        format!("KEY_{rest}")
                    } else {
                        format!("MOUSE_{rest}")
                    }
                });
            if let Some(button) = button {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                self.input_state_mut().set_counter(&button, v);
                return Ok(StepOutcome::Continue);
            }
            // `@_MENUCODE` / `@_WINDOW_EXEC_MAX/CLOSE`(手册 ○)= 状态覆盖。
            if name == "_MENUCODE" {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                self.set_menu_code(v);
                return Ok(StepOutcome::Continue);
            }
            if name == "_WINDOW_EXEC_MAX"
                || name == "_WINDOW_EXEC_CLOSE"
                || name == "_WINDOW_EXEC_CAPTION"
            {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                let which = name.rsplit('_').next().unwrap_or("").to_string();
                self.set_window_exec(&which, v);
                return Ok(StepOutcome::Continue);
            }
            // `$_TEXT`(手册 ○)= 替换当前显示文本。
            if name == "_TEXT" {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let bytes = match rhs.cast_to_str()? {
                    Value::Str(b) => b,
                    _ => Vec::new(),
                };
                self.set_text(bytes);
                return Ok(StepOutcome::Continue);
            }
            // `$_TEXT_FLAG`(手册 ○)= 替换控制字节流。
            //
            // REA `0x446b98..0x446bb5`(id 0x7b):RHS 字符串逐字节拷进
            // `[state+0x3c]`;**不重建文本、不重算逐字符标志**(与 `$_TEXT`
            // 的 `+0x38` 写入并列)。DIV-RT-032 写入路径收口。
            if name == "_TEXT_FLAG" {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let bytes = match rhs.cast_to_str()? {
                    Value::Str(b) => b,
                    _ => Vec::new(),
                };
                self.set_text_flags(bytes);
                return Ok(StepOutcome::Continue);
            }
            // `_CODE_*`(手册 ○;键位重映射)= 覆盖引擎缺省按钮码。
            if name.starts_with("_CODE_") {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                let id = lval.var.id;
                self.input_state_mut().set_code(id, v);
                return Ok(StepOutcome::Continue);
            }
            // `_PAD_*` / `_PADNUM_*`(手册 ○)= 设定手柄码计数器。
            if name.starts_with("_PADNUM_") || name.starts_with("_PAD_") {
                let is_num = name.starts_with("_PADNUM_");
                let base = name
                    .strip_prefix("_PADNUM_")
                    .or_else(|| name.strip_prefix("_PAD_"))
                    .unwrap_or("");
                let idx: Vec<i64> = lval.indices.iter().copied().collect();
                let code = match (base, idx.as_slice()) {
                    ("UP", [n]) | ("DOWN", [n]) | ("LEFT", [n]) | ("RIGHT", [n]) => {
                        let dir = match base {
                            "UP" => 0,
                            "DOWN" => 1,
                            "LEFT" => 2,
                            _ => 3,
                        };
                        crate::input::pad_dir_code(dir, *n)
                    }
                    ("BUTTON", [n, m]) => crate::input::pad_button_code(*n, *m),
                    _ => {
                        return Err(diag::fatal(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            format!("{name}:写入下标形态不符"),
                        )
                        .into())
                    }
                };
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                self.input_state_mut().set_pad_counter(code, v, is_num);
                return Ok(StepOutcome::Continue);
            }
        }
        // 其它系统变量写入:语义未取证 ⇒ 挂号(DIV-RT-007)。
        //
        // 手册 `basic/svariable.html` 用 ○ 标出「可赋值」的系统变量
        // (输入/窗口状态族),但没有给出写入后的**可观测语义**;
        // 引擎侧只有间接线索。按军规:挂号 + 无副作用 + 继续执行(DIV-RT-007)。
        if lval.var.id < self.sys_count {
            let name = self
                .sysvars()
                .get(lval.var.id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| lval.var.display());
            return Err(diag::unimplemented(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "写入系统变量 {name}(手动 ○ 可赋值,但写入语义未取证);本次写入被忽略(DIV-RT-007)"
                ),
            )
            .into());
        }
        let rhs = self.eval_window(st, &cmd.windows[1])?;
        let code = (cmd.windows[0].rec.slot >> 8) as u8;
        let value = if code == 0 {
            rhs
        } else {
            let cur = self.read_lvalue(&lval)?;
            compound(code, &cur, &rhs)?
        };
        self.store.write(lval.var, &lval.indices, &value)?;
        Ok(StepOutcome::Continue)
    }

    /// `INT/FLT/STR/S_INT/S_FLT/S_STR`:手册 `variable/*.html`。
    ///
    /// 声明窗口 = 变量引用(数组时末条 `0x29` 前是各维长度);
    /// 初始值窗口 = 表达式(缺省 = 类型零值,手册「初期値を省略した場合０」)。
    fn op_declare(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.is_empty() {
            return Err(windows_error(cmd, "声明命令缺少变量窗口").into());
        }
        let ty = match cmd.name.as_str() {
            "INT" | "S_INT" => ElemType::Int,
            "FLT" | "S_FLT" => ElemType::Float,
            "STR" | "S_STR" => ElemType::Str,
            _ => unreachable!(),
        };
        let target = self.decl_target(st, &cmd.windows[0])?;
        let dims: Vec<usize> = target.iter().map(|&d| d.max(0) as usize).collect();
        let init = match cmd.windows.get(1) {
            Some(w) if !w.payload.is_empty() => self.eval_window(st, w)?,
            _ => ty.default_value(),
        };
        self.store
            .declare(target_var(&cmd.windows[0])?, ty, &dims, &init);
        let _ = st;
        Ok(StepOutcome::Continue)
    }

    /// 声明命令的变量窗口:引用 + 可选各维长度。
    fn decl_target(&mut self, st: &ExecState, w: &CmdWindow) -> Result<Vec<i64>, VmError> {
        // 单条变量指令 = 标量;含 `0x29` = 数组(前导值 = 各维长度)。
        let lv = self.eval_lvalue_window(st, w)?;
        Ok(lv.indices.into_vec())
    }

    /// `VARINFO`:手册 `variable/varinfo.html`;引擎 §3.21。
    fn op_varinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<crate::expr::LValue> = None;
        let mut set: Option<crate::expr::LValue> = None;
        let mut op: Option<(u8, i64)> = None;
        // 全部数值槽(SEARCH 的 NO=20 与 INT/FLT 针值要用)。
        let mut flags: Vec<(u8, i64)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => set = Some(self.eval_lvalue_window(st, w)?),
                1 => target = Some(self.eval_lvalue_window(st, w)?),
                slot => {
                    let v = self.eval_window(st, w)?;
                    if let Some(n) = v.as_int() {
                        flags.push((slot, n));
                        if op.is_none() && n != 0 && !matches!(slot, 17..=20) {
                            op = Some((slot, n));
                        }
                    }
                }
            }
        }
        let (slot, _) = op.ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARINFO 无查询项(手册:至少给 TYPE/LENGTH/DIMNUM/DIMSIZE*)",
            )
        })?;
        let set = set.ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARINFO 缺 SET(被查询变量)",
            )
        })?;
        let set_var = set.var;
        let (shape_ty, shape_dims) = match self.var_shape(set_var) {
            Some(v) => v,
            None => {
                return Err(diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("VARINFO 查询未声明变量 {}", set_var.display()),
                )
                .into())
            }
        };
        let shape = (shape_ty, shape_dims.as_slice());
        let result_val = match slot {
            2 => Value::Int(match shape.0 {
                ElemType::Int => 1,
                ElemType::Float => 2,
                ElemType::Str => 3,
            }),
            4 => Value::Int(shape.1.len() as i64),
            5..=12 => {
                let k = (slot - 5) as usize;
                match shape.1.get(k) {
                    Some(&d) => Value::Int(d as i64),
                    None => {
                        return Err(diag::fatal(
                            code::rt::VAR_SHAPE,
                            Boundary::Script,
                            format!("VARINFO DIMSIZE{} 超出维数 {}", k + 1, shape.1.len()),
                        )
                        .into())
                    }
                }
            }
            13 => {
                // 保留 SET 里写的下标(`$_PSTR(1)` 的 `(1)`);无下标才用 0 号元素。
                let idx: Vec<i64> = if set.indices.is_empty() {
                    vec![0; shape.1.len()]
                } else {
                    set.indices.to_vec()
                };
                let v = self.read_var(st, set_var, &idx)?;
                match v {
                    Value::Str(bytes) => Value::Int(str_char_count(&bytes) as i64),
                    other => {
                        return Err(diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!("VARINFO LENGTH 需要字符串变量,得到 {}", other.type_name()),
                        )
                        .into())
                    }
                }
            }
            // `SEARCH`:手册 `variable/varinfo.html` 例 5/6/7 ——
            // 从 NO 号元素开始在一维数组里找第一个相等的元素,返回下标,
            // 找不到返回 -1。要搜索的值由 `INT=`/`FLT=`/`STR=` 参数给出。
            14 => {
                let need = array_needle(self, cmd, st, set_var)?;
                let (ty, dims) = shape;
                let (ty, dims) = (ty, dims.to_vec());
                if dims.len() != 1 {
                    return Err(diag::fatal(
                        code::rt::VAR_SHAPE,
                        Boundary::Script,
                        format!("VARINFO SEARCH 只支持一维数组,目标 {} 维", dims.len()),
                    )
                    .into());
                }
                let start = flags_get(&flags, 20).unwrap_or(0).max(0);
                let found = match &need {
                    Some(want) => self
                        .store
                        .search_1d(set_var, start as usize, want, ty)
                        .map(|index| index as i64)
                        .unwrap_or(-1),
                    None => -1,
                };
                Value::Int(found)
            }
            other => {
                return Err(diag::unimplemented(
                    code::rt::CMD_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("VARINFO 参数序号 {other} 未实现(STRTYPE/SJISCODE/UTF8CODE 无手册例;DIV-RT-011)"),
                )
                .into())
            }
        };
        if let Some(t) = target {
            self.write_target(st, &t, &result_val)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// 写回一个左值(保留下标);系统变量目标 ⇒ 挂号(DIV-RT-007)。
    fn write_target(
        &mut self,
        _st: &ExecState,
        target: &crate::expr::LValue,
        v: &Value,
    ) -> Result<(), VmError> {
        let target = crate::expr::LValue {
            var: self.canonical_var(target.var),
            indices: target.indices.clone(),
        };
        let target = &target;
        if target.var.id < self.sys_count {
            return Err(diag::unimplemented(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "写入系统变量 {}(语义未取证;DIV-RT-007)",
                    target.var.display()
                ),
            )
            .into());
        }
        self.store.write(target.var, &target.indices, v)?;
        Ok(())
    }

    /// `VARACT`:手册 `variable/varact.html`;引擎 §3.26。
    fn op_varact(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        self.varact_calls = self.varact_calls.saturating_add(1);
        if self.prof_commands {
            *self
                .prof_varact_sites
                .entry((self.cur_script, self.cur_pc))
                .or_insert(0) += 1;
        }
        let mut set: Option<crate::expr::LValue> = None;
        let mut letv: Option<crate::expr::LValue> = None;
        let mut flags: Vec<(u8, i64)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => set = Some(self.eval_lvalue_window(st, w)?),
                1 => letv = Some(self.eval_lvalue_window(st, w)?),
                slot => {
                    let v = self.eval_window(st, w)?;
                    if let Some(n) = v.as_int() {
                        flags.push((slot, n));
                    }
                }
            }
        }
        // `INIT`/`G_*` 分支不读 `SET`(引擎 §3.26 ③:`0x454d3d..0x454e00`
        // 一次都没碰 `0x683a60`),因此把「缺 SET → Fatal」的判定推到它们之后。
        let set_missing = set.is_none();
        let set = set.unwrap_or(crate::expr::LValue {
            var: VarRef::new(VarSpace::At, 0),
            indices: smallvec::SmallVec::new(),
        });
        let set_var = set.var;
        // `SET` 为系统变量是合法用法:ERIS `stdlib/ystdlib.yst` 的
        // `_strlen($_PSTR(1))` / `_inttostr(@_PINT(1))` 等 29 处实测如此。
        // 写系统变量(LET=系统变量)语义未取证 ⇒ 单独挂号(DIV-RT-007)。
        if let Some(t) = &letv {
            if t.var.id < self.sys_count {
                return Err(diag::unimplemented(
                    code::rt::SYSVAR_WRITE,
                    Boundary::Script,
                    format!(
                        "VARACT 目标为系统变量 {}(写入语义未取证;DIV-RT-007)",
                        t.var.display()
                    ),
                )
                .into());
            }
        }
        let flag = |slot: u8| flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v);
        let num = |slot: u8| {
            flags
                .iter()
                .find(|(s, _)| *s == slot)
                .map(|(_, v)| *v)
                .unwrap_or(0)
        };
        if flag(6).is_some() {
            self.varact_type_calls = self.varact_type_calls.saturating_add(1);
            // TYPE:按目标类型转换(手册例 1/2;§3.22)。
            let target = letv.ok_or_else(|| {
                diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "VARACT TYPE 缺 LET")
            })?;
            let ty = self.var_shape(target.var).map(|s| s.0).ok_or_else(|| {
                diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("VARACT 目标未声明 {}", target.var.display()),
                )
            })?;
            let v = self.read_var(st, set_var, &set.indices)?;
            let converted = match ty {
                ElemType::Int => v.cast_to_int()?,
                ElemType::Float => Value::Float(v.as_f64().unwrap_or(0.0)),
                ElemType::Str => v.cast_to_str()?,
            };
            self.write_target(st, &target, &converted)?;
            return Ok(StepOutcome::Continue);
        }
        // ---- INIT(16) + G_INT..G_STR4(17..28):按变量镜像恢复全局变量 ----
        //
        // 手册 `variable/varact.html` **未列这一组**(参数表里没有 INIT/G_*),
        // 但引擎有完整算法(旁线 `format-vm-opcodes.md` §3.26 ④,
        // 处理体 `0x454d3d` 掩码构造 + `0x455b60` 恢复体,逐条反汇编):
        //
        //  - `INIT` 只是守卫:**不在场 → 整条指令什么都不做**;
        //  - 17..28 各置一位掩码(INT 组 0..3 / FLT 组 0..3 / STR 组 0..3);
        //  - 恢复时**只恢复 `ysv` 里 `kind == 1`(全局)且组号位被置位**的记录;
        //    组号 ≥5 的记录没有否决项 ⇒ 照常恢复;
        //  - 重建是「整槽按声明重建」(写描述符 + 类型数组),不是逐元素赋值;
        //  - 这一组**不读 `SET`**。
        //
        // 这是 ERIS 启动链的硬依赖(`es_SoundDefine` 之类在初始化时
        // `VARACT[INIT=1 G_*]` 把全局表恢复成初值)。
        if flag(16).is_some() || (17..=28).any(|s| flag(s).is_some()) {
            self.varact_init_calls = self.varact_init_calls.saturating_add(1);
            if flag(16).is_none() {
                // INIT **不在场**:引擎判定链 `je 0x454e05`(函数尾)——
                // 什么都不做(旁线 §3.26 ③)。判据是在场,与值无关。
                return Ok(StepOutcome::Continue);
            }
            let mut masks = [0u8; 3]; // INT / FLT / STR 三族的 4 位掩码
            for slot in 17..=28u8 {
                // 掩码位 = 载入器预写的「参数在场字节」(`0x454d3d
                // movzx eax,byte ptr [0x684171]  ; G_INT 在场?` +
                // `test/je`;旁线 §3.26 ③)—— 同样与值无关。
                if flag(slot).is_none() {
                    continue;
                }
                let (family, bit) = match slot {
                    17..=20 => (0usize, slot - 17),
                    21..=24 => (1usize, slot - 21),
                    _ => (2usize, slot - 25),
                };
                masks[family] |= 1 << bit;
            }
            self.restore_globals(st, &masks)?;
            return Ok(StepOutcome::Continue);
        }
        if set_missing {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARACT 缺 SET(源变量)",
            )
            .into());
        }
        // 手册 `variable/varact.html` 例 9/10:`VARACT[DIMSIZE=500 SET=@A()]`
        // 与 `VARACT[DIMSIZE+=1 SET=@A()]` —— **没有 LET**,原地改数组。
        // 同理 PUSH/POP(例未给,但参数表无 LET 语义需求)。
        if flag(13).is_some() {
            // 手册例 9/10:`DIMSIZE=500`(绝对值)/ `DIMSIZE+=1`(相对值)。
            // 语料实测 **104/104 都是 `+=`**,复合码在窗口 `slot` 高字节
            // (与 `LET` 同一编码:0== 1+= 2-= …)。
            let code = (cmd
                .windows
                .iter()
                .find(|w| w.rec.kind == 13)
                .map(|w| w.rec.slot >> 8)
                .unwrap_or(0)) as u8;
            let operand = num(13);
            let (ty, dims) = self.var_shape(set_var).ok_or_else(|| {
                diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("DIMSIZE 目标未声明 {}", set_var.display()),
                )
            })?;
            if dims.len() != 1 {
                return Err(diag::fatal(
                    code::rt::VAR_SHAPE,
                    Boundary::Script,
                    format!("DIMSIZE 只支持一维数组,目标 {} 维", dims.len()),
                )
                .into());
            }
            let old = dims[0] as i64;
            let new_len = match code {
                0 => operand,
                1 => old + operand,
                2 => old - operand,
                3 => old * operand,
                4 => {
                    if operand == 0 {
                        return Err(diag::fatal(
                            code::rt::EXPR_DIV_ZERO,
                            Boundary::Script,
                            "DIMSIZE /= 0",
                        )
                        .into());
                    }
                    old / operand
                }
                other => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!("DIMSIZE 复合码 {other} 未实现"),
                    )
                    .into())
                }
            };
            self.resize_1d(set_var, ty, new_len.max(0) as usize)?;
            return Ok(StepOutcome::Continue);
        }
        if flag(14).is_some() || flag(15).is_some() {
            let push = flag(14).is_some();
            let pos = num(if push { 14 } else { 15 }).max(0) as usize;
            self.shift_1d(set_var, pos, push)?;
            return Ok(StepOutcome::Continue);
        }
        let target = letv.ok_or_else(|| {
            diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "VARACT 缺 LET(目标变量)",
            )
        })?;
        let src = self.read_var(st, set_var, &set.indices)?;
        let Value::Str(bytes) = src else {
            return Err(diag::fatal(
                code::rt::VAR_TYPE,
                Boundary::Script,
                "VARACT 字符串加工需要字符串源变量",
            )
            .into());
        };
        let out;
        self.varact_bytes = self.varact_bytes.saturating_add(bytes.len() as u64);
        if flag(3).is_some() || flag(2).is_some() {
            let pos = num(4).max(1) as usize;
            let len = num(5).max(0) as usize;
            out = if flag(3).is_some() {
                self.varact_ops[0] = self.varact_ops[0].saturating_add(1);
                copy_chars(&bytes, pos, len)
            } else {
                self.varact_ops[1] = self.varact_ops[1].saturating_add(1);
                cut_chars(&bytes, pos, len)
            };
        } else if flag(7).is_some() {
            self.varact_ops[2] = self.varact_ops[2].saturating_add(1);
            out = ascii_case(&bytes, true);
        } else if flag(8).is_some() {
            self.varact_ops[3] = self.varact_ops[3].saturating_add(1);
            out = fullwidth_case(&bytes, true);
        } else if flag(9).is_some() {
            self.varact_ops[4] = self.varact_ops[4].saturating_add(1);
            out = ascii_case(&bytes, false);
        } else if flag(10).is_some() {
            self.varact_ops[5] = self.varact_ops[5].saturating_add(1);
            out = fullwidth_case(&bytes, false);
        } else if flag(11).is_some() {
            self.varact_ops[6] = self.varact_ops[6].saturating_add(1);
            // 半角→全角:表/算法逐条见 `crate::case_tables`(0x4546aa..)。
            out = crate::case_tables::hantozen(&bytes);
        } else if flag(12).is_some() {
            self.varact_ops[7] = self.varact_ops[7].saturating_add(1);
            // 全角→半角:0x4548cf.. / 0x46b395。
            out = crate::case_tables::zentohan(&bytes);
        } else {
            return Err(diag::unimplemented(
                code::rt::CMD_UNIMPLEMENTED,
                Boundary::Script,
                "VARACT 无已识别的操作项(INIT/G_* 无手册页;DIV-RT-011)",
            )
            .into());
        }
        self.write_target(st, &target, &Value::Str(out))?;
        Ok(StepOutcome::Continue)
    }

    fn read_lvalue(&self, lval: &crate::expr::LValue) -> Result<Value, yu_log::Diag> {
        self.store.read(lval.var, &lval.indices)
    }

    /// `VARACT[INIT + G_*]`:按 `ysv.ybn` 镜像把全局变量整槽重建。
    ///
    /// 算法逐条照旁线 §3.26 ④(0.495 处理体 `0x455b60`):
    /// ① 只收 `kind == 1`(全局);② 按 `(值类型, 组号)` 查掩码,位为 0 则否决;
    /// ③ 组号超出 1..=4 时无否决项 ⇒ 照常恢复;④ 整槽重建(声明 + 类型 + 维数 + 初值)。
    fn restore_globals(&mut self, st: &ExecState, masks: &[u8; 3]) -> Result<(), VmError> {
        let _ = st;
        // 记忆化:同一掩码组合、且自上次恢复以来 `VariableStore` 没有任何
        // 写入/声明/改长度 ⇒ 状态已经等于恢复结果,重复恢复可安全跳过。
        // (启动脚本会在紧循环里对同一组掩码调用数万次。)
        let key =
            usize::from(masks[0]) | (usize::from(masks[1]) << 4) | (usize::from(masks[2]) << 8);
        let generation = self.store.generation();
        if self
            .last_global_restore
            .get(key)
            .copied()
            .flatten()
            .is_some_and(|last| last == generation)
        {
            self.restore_skips = self.restore_skips.saturating_add(1);
            return Ok(());
        }
        let mut rows = 0u64;
        // 启动期已投影好 `(var, ty, dims, init, group, family)`;这里不再
        // 逐次 clone ysv 记录/解码字符串初值。
        for g in &self.globals_restore {
            // 掩码位 = 组号 - 1;组号 0 或 >4 没有否决项。
            if (1..=4).contains(&g.group) {
                let bit = 1u8 << (g.group - 1);
                if masks[g.family] & bit == 0 {
                    continue;
                }
            }
            rows += 1;
            // 整槽重建(引擎 `0x455d8a`);类型/维数不变时复用存储,
            // 可观察状态与重建一致。
            self.store.reset_declared(g.var, g.ty, &g.dims, &g.init);
        }
        self.restore_rows = self.restore_rows.saturating_add(rows);
        if let Some(slot) = self.last_global_restore.get_mut(key) {
            *slot = Some(self.store.generation());
        }
        Ok(())
    }

    /// 改变一维数组长度并保留已有元素(见 [`crate::value::VariableStore::resize_1d`])。
    fn resize_1d(&mut self, var: VarRef, _ty: ElemType, new_len: usize) -> Result<(), VmError> {
        if new_len == 0 {
            // 引擎对 0 长度的处理未取证:保留 1 个空槽(手册例 9/10 均 >=1)。
            self.store.resize_1d(var, 0)?;
            return Ok(());
        }
        self.store.resize_1d(var, new_len)?;
        Ok(())
    }

    fn shift_1d(&mut self, var: VarRef, pos: usize, push: bool) -> Result<(), VmError> {
        self.store.shift_1d(var, pos, push)?;
        Ok(())
    }

    // ---------------------------------------------------------- 数学

    /// `MATH`:手册 `math/math.html`;引擎 §3.20。
    fn op_math(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // LET 目标保留下标:`MATH[… LET=@A(i)]` 写 `A[i]`。语料实况 =
        // `es_First.yst:145` 用 `LOOP[SET=63] MATH[POW=1 … LET=@es.POW2(@_LC-1)]`
        // 填 2 的幂表(DIV-RT-003 除零根因;丢下标会把 63 次写全落在 [0])。
        let mut target: Option<LValue> = None;
        let mut set_raw: Option<Value> = None;
        let mut set: f64 = 0.0;
        let mut set2: f64 = 0.0;
        let mut setstr: Option<Vec<u8>> = None;
        let mut flags: Vec<u8> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => target = Some(self.eval_lvalue_window(st, w)?),
                1 => {
                    let v = self.eval_window(st, w)?;
                    set = v.as_f64().unwrap_or(0.0);
                    set_raw = Some(v);
                }
                2 => set2 = self.eval_window(st, w)?.as_f64().unwrap_or(0.0),
                3 => {
                    if let Value::Str(b) = self.eval_window(st, w)? {
                        // SETSTR = HASH 的字符串输入(旁线 §3.20:`HASH` 对序号 3
                        // 的字符串求 CRC-32);其余数值函数不消费它。
                        setstr = Some(b);
                    }
                }
                slot => {
                    // 旗标按**存在**记:官方判定链测的是窗口在场(每分支形状
                    // = 直接读 SET 载荷,不读旗标值;旁线 §3.20)。此前按
                    // 「值 ≠ 0」会让 `MATH[ABS=0 …]` 之类被静默丢参。
                    let v = self.eval_window(st, w)?;
                    if v.as_int().is_some() {
                        flags.push(slot);
                    }
                }
            }
        }
        let target = target
            .ok_or_else(|| diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "MATH 缺 LET"))?;
        // 官方判定链原序(旁线 §3.20;**与槽位序不同**)—— 首个在场的旗标生效。
        const MATH_CHAIN: [u8; 18] = [
            4, 7, 10, 5, 8, 6, 9, 13, 11, 12, 14, 15, 16, 17, 18, 19, 20, 21,
        ];
        let Some(slot) = MATH_CHAIN.into_iter().find(|s| flags.contains(s)) else {
            // 无函数旗标:官方行为未取证(判定链全部落空);保留内核缺省
            // (写出 SET 值)并留此注释,不另造语义。
            self.store
                .write(target.var, &target.indices, &Value::Float(set))?;
            return Ok(StepOutcome::Continue);
        };
        // 参数名→函数由工程 ysc 给出(参数表顺序即手册函数表)。
        let fname = self.param_name(cmd.opcode, slot).unwrap_or_default();
        // 结果带类型:官方 ABS(整数入)/RND/HASH 走**整数写**
        // (`0x455540` 族),其余走浮点写(`0x4555e4`;旁线 §3.20)。
        let r: Value = match fname.as_str() {
            "SIN" => Value::Float(set.sin()),
            "ASIN" => Value::Float(set.asin()),
            "SINH" => Value::Float(set.sinh()),
            "COS" => Value::Float(set.cos()),
            "ACOS" => Value::Float(set.acos()),
            "COSH" => Value::Float(set.cosh()),
            "TAN" => Value::Float(set.tan()),
            "ATAN" => Value::Float(set.atan()),
            "ATAN2" => Value::Float(set.atan2(set2)),
            "TANH" => Value::Float(set.tanh()),
            "SQRT" => Value::Float(set.sqrt()),
            "ABS" => match set_raw {
                // 旁线 §3.20 `0x448412`:SET 类型为整数 ⇒ 整数解析 +
                // 取绝对值 + 整数写;否则浮点路 + `fabs`。
                Some(Value::Int(i)) => Value::Int(i.abs()),
                _ => Value::Float(set.abs()),
            },
            "EXP" => Value::Float(set.exp()),
            "LOG" => Value::Float(set.ln()),
            "LOG10" => Value::Float(set.log10()),
            "POW" => Value::Float(set.powf(set2)),
            "RND" => {
                // REA `0x448571..0x4485b7`:SET/SET2 取 64 位整数,反向时交换,
                // `rand32 % (max-min+1)` 再加 min;手册例 17 `SET=1 SET2=9`。
                let a = set.trunc() as i64;
                let b = set2.trunc() as i64;
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                let span = (hi as i128 - lo as i128 + 1) as u128;
                let r = if span == 0 {
                    0i128
                } else {
                    i128::from(self.rng.next_u32()) % i128::try_from(span).unwrap_or(i128::MAX)
                };
                Value::Int((lo as i128 + r) as i64)
            }
            "HASH" => {
                // 旁线 §3.20 `0x4484f1 → 0x43fad8`:对 SETSTR 字符串求
                // CRC-32(引擎表 `0x5b41c0` = 标准表)→ 写整数。
                Value::Int(i64::from(crate::hash::crc32(
                    setstr.as_deref().unwrap_or_default(),
                )))
            }
            other => {
                return Err(diag::fatal(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("MATH 未声明参数 `{other}`"),
                )
                .into())
            }
        };
        self.store.write(target.var, &target.indices, &r)?;
        Ok(StepOutcome::Continue)
    }

    // ---------------------------------------------------------- 任务

    /// `TASK`:手册 `control/task.html`。
    ///
    /// **无 ID = 执行者自身**(手册:「タスクＩＤを指定しなかった場合、この命令を
    /// 実行した自身のタスクが対象になります」;REA `0x45074a`:
    /// `MOV EBP, [0x801720]`(当前任务记录)→ 以 `[EBP+0x34]` 为操作对象)。
    fn op_task(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        let mut enabled = true;
        let mut priority: i64 = 1;
        let mut label: Option<crate::tables::Label> = None;
        // 任务局部寄存器初值(TASK 参数序号 4..13=TINT..TINT10 / 14..23=TFLT.. /
        // 24..33=TSTR..;0.495 ycd 实测)。手册 `control/task.html` 未文档化这些
        // 关键字,但官方 ERIS 源码给出完整生产者/消费者对:
        //   生产者 `es_ERIS.yst:524`:`TASK[ID=… #=es.SS.FADE.TASK.MAIN Z=… TINT=@_PINT(1)]`
        //   消费者 `es_ERIS.yst:533`:任务体 `\es.CGTFADEOUT(@_TINT(1))`
        //   (stdlib `es._task` 同型:TINT..TINT4 → `@_TINT(1..4)`)。
        // 引擎侧任务记录 `task+0x3d0`(旁线 §3.16)。
        // 引擎建任务时先把 T* 寄存器逐槽清零(`0x450916..0x450943`:
        // `+0x348/+0x34c`=TFLT、`+0x3d0/+0x3d4`=TINT、`+0x458`=TSTR),
        // 再按 TINT/TFLT/TSTR 参数落位 ⇒ **未传的槽为 0,不继承调用者**。
        let mut tints = [0i64; 16];
        let mut tflts = [0f64; 16];
        let mut tstrs: [Vec<u8>; 16] = std::array::from_fn(|_| Vec::new());
        let mut script_pos: Option<(u32, u32)> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        id = Some(cp932_lossy(&bytes));
                    }
                }
                1 => enabled = self.eval_window(st, w)?.as_int().unwrap_or(1) != 0,
                3 => priority = self.eval_window(st, w)?.as_int().unwrap_or(1),
                slot @ 4..=13 => {
                    tints[(slot - 4) as usize] = self.eval_window(st, w)?.as_int().unwrap_or(0);
                }
                slot @ 14..=23 => {
                    tflts[(slot - 14) as usize] = self.eval_window(st, w)?.as_f64().unwrap_or(0.0);
                }
                slot @ 24..=33 => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        tstrs[(slot - 24) as usize] = bytes;
                    }
                }
                // SCRIPTPOS(手册 `control/task.html` 例 4:位置情報の上書き =
                // 事実上のスクリプト位置ロード)。blob 由 TASKINFO[SCRIPTPOS]
                // 产出,脚本视为不透明数据;本实现 blob = [script_id u32][pc u32]。
                35 => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        if bytes.len() == 8 {
                            script_pos = Some((
                                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                            ));
                        }
                    }
                }
                34 => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        label = Some(self.find_label(&bytes).ok_or_else(|| {
                            diag::fatal(
                                code::rt::LABEL,
                                Boundary::Script,
                                format!("TASK 入口标签不存在:{}", cp932_lossy(&bytes)),
                            )
                        })?);
                    }
                }
                slot => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!(
                            "TASK 参数序号 {slot}(SCRIPTPOS/TEXTPOS/RESTART 等)未实现(DIV-RT-015)"
                        ),
                    )
                    .into())
                }
            }
        }
        // 缺省目标 = 执行者自身(手册;REA 0x45074a 用当前任务记录)。
        let target_id = id.unwrap_or_else(|| task.id.clone());
        match label {
            Some(label) => {
                let seed = TaskSeed {
                    id: target_id,
                    script_id: label.script_id as u32,
                    pc: label.pc,
                    enabled,
                    priority,
                    main: false,
                };
                let pos = script_pos.unwrap_or((seed.script_id, seed.pc));
                let mut fresh = ExecState::new(pos.0, pos.1);
                fresh.task_ints = tints;
                fresh.task_floats = tflts;
                fresh.task_strs = tstrs.clone();
                if seed.id == task.id {
                    // 目标 = 自身:引擎 `TASK` 直接写当前任务记录的 pc/脚本号
                    // (`0x4508bc..0x4508d9`),不是另建一条执行流。这里同样改写
                    // 当前执行位(块/循环/调用栈随新入口重置)。
                    *st = fresh;
                } else {
                    // 同 ID 重配置 = 换入口(引擎复用任务记录并重载脚本表)。
                    self.exec.insert(seed.id.clone(), fresh);
                }
                self.emit(VmEvent::TaskSpawn {
                    frame: self.frame(),
                    id: seed.id.clone(),
                    script: seed.script_id,
                    pc: seed.pc,
                    enabled,
                    main: false,
                });
                self.push_effect(Effect::SpawnTask(seed));
                self.push_effect(Effect::RequestExtraFrame);
            }
            None => {
                // SCRIPTPOS 无 # 形态(手册例 4:对既有任务做「位置ロード」)。
                if let Some((sid, pc)) = script_pos {
                    let target = target_id.clone();
                    if target == task.id {
                        // 目标 = 自身:直接改当前执行位(引擎 `0x450fe0` 走同一记录)。
                        st.script_id = sid;
                        st.pc = pc;
                        st.if_stack.clear();
                        st.loops.clear();
                    } else if let Some(target_st) = self.exec.get_mut(&target) {
                        target_st.script_id = sid;
                        target_st.pc = pc;
                        target_st.if_stack.clear();
                        target_st.loops.clear();
                    }
                }
                self.push_effect(Effect::SetEnabled(target_id, enabled));
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `TASKEND[ID]`:手册 `control/taskend.html`(缺省 = 自身)。
    ///
    /// **立即生效**(REA 0.495 `0x4521dc`):
    /// - 无 `ID`:取 `[0x801720]`(当前任务记录)→ `CALL 0x452364`(链表摘除):
    ///   `0x45237f..0x4523b9` 按 `+8/+0xc` 前后指针从链中解链并 `DEC [pool+0x20]`;
    ///   `0x452223: MOV EAX, 0xffffffff` —— 返回非 0 = 本任务本帧让出,泵立即停。
    ///   因此同一条命令之后的 `LET` 不执行,`TASKINFO EXIST` 立即为 0,
    ///   同 ID 再建从新标签开始(旧执行位已随摘链丢弃)。
    /// - 指定 `ID`:查名字表;命中他人 → 摘链 + 返回 0(继续执行);
    ///   命中自身 → 同无 ID 路径,返回 -1。
    fn op_taskend(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            if w.rec.kind == 0 {
                if let Value::Str(bytes) = self.eval_window(st, w)? {
                    id = Some(cp932_lossy(&bytes));
                }
            }
        }
        let target = id.unwrap_or_else(|| task.id.clone());
        if target == task.id {
            // 自身:摘链由调度器在 Finish 分支完成(节点随帧尾 retain 移除),
            // 执行位在 step_command 的 Finish 路径丢弃。
            return Ok(StepOutcome::Finish);
        }
        self.push_effect(Effect::FinishTask(target));
        Ok(StepOutcome::Continue)
    }

    /// `TASKINFO[ID LET EXIST/E/Z/FILE/LINE/IFNEST/GOSUBNEST/LOOPNEST]`:
    /// 手册 `control/taskinfo.html`。
    ///
    /// 取值来源 = **引擎任务记录**(旁线 §3.2):`+0x20` pc、`+0x3c` 脚本号、
    /// `+0x3e` 块深度(base_a)、`+0x244` 调用栈深度。手册逐条:
    /// `LINE`=当前执行行数、`IFNEST`/`GOSUBNEST`/`LOOPNEST`=三种嵌套数、
    /// `FILE`=当前文件名(这里按脚本号给出,名字表在 `yst_list`)、
    /// `E`/`Z`=启用位/优先级、`EXIST`=任务存在。
    ///
    /// 行号:引擎另有「数据区 B 逐 pc 源码位置」(脚本 `+0x38`,本 crate
    /// `ScriptFile::source_position`),此处直接用它 —— 那是编译产物里
    /// 与 pc 一一对应的真实行号,不是推算。
    fn op_taskinfo(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        let mut target: Option<crate::expr::LValue> = None;
        let mut ops: Vec<(u8, i64)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        id = Some(cp932_lossy(&bytes));
                    }
                }
                1 => target = Some(self.eval_lvalue_window(st, w)?),
                slot => {
                    let v = self.eval_window(st, w)?;
                    if let Some(n) = v.as_int() {
                        ops.push((slot, n));
                    }
                }
            }
        }
        // 缺省 = 自身(手册:「タスクＩＤを指定しなかった場合、この命令を実行した
        // 自身のタスクが対象」;REA 0x4521dc/0x4524xx 同样取当前任务记录)。
        let id = id.unwrap_or_else(|| task.id.clone());
        // 引擎从任务链表取记录;泵期间调度器被 `Vm::tick` 借出,故读同步过来的投影。
        let node = self
            .task_view()
            .iter()
            .find(|n| n.id == id && !n.finished)
            .cloned();
        // 该任务的执行位(自身就在这里;其它任务在 exec 表里)。
        let exec = if id == task.id {
            Some(st.clone())
        } else {
            self.exec_of(&id).cloned()
        };
        let Some((slot, _)) = ops.iter().find(|(_, v)| *v != 0).copied() else {
            return Ok(StepOutcome::Continue);
        };
        let value = match slot {
            2 => Value::Int(node.is_some() as i64),
            3 => Value::Int(node.as_ref().map(|n| n.enabled as i64).unwrap_or(0)),
            5 => Value::Int(node.as_ref().map(|n| n.priority).unwrap_or(0)),
            6 => {
                // FILE:当前脚本文件名(手册)→ 由 `yst_list` 的名字表给出。
                let sid = exec.as_ref().map(|e| e.script_id).unwrap_or(0);
                Value::Int(sid as i64)
            }
            7 => Value::Int(exec.as_ref().map(|e| e.script_id as i64).unwrap_or(-1)),
            8 => {
                // LINE:当前执行行数(数据区 B 的逐 pc 源码位置)。
                let (sid, pc) = exec
                    .as_ref()
                    .map(|e| (e.script_id, e.pc.saturating_sub(1)))
                    .unwrap_or((0, 0));
                let line = self
                    .tables
                    .script(sid)
                    .and_then(|s| s.source_position(pc as usize))
                    .map(|v| v as i64)
                    .unwrap_or(-1);
                Value::Int(line)
            }
            16 => Value::Int(exec.as_ref().map(|e| e.if_stack.len() as i64).unwrap_or(0)),
            17 => Value::Int(
                exec.as_ref()
                    .map(|e| e.frames.len().saturating_sub(1) as i64)
                    .unwrap_or(0),
            ),
            18 => Value::Int(exec.as_ref().map(|e| e.loops.len() as i64).unwrap_or(0)),
            // SCRIPTPOS(手册 `control/taskinfo.html`:現在実行されている
            // スクリプト位置情報)。脚本视为不透明 blob;本实现 = 8 字节
            // [script_id u32 LE][pc u32 LE],与 `TASK[SCRIPTPOS=…]` 配对往返。
            19 => {
                let (sid, pc) = exec.as_ref().map(|e| (e.script_id, e.pc)).unwrap_or((0, 0));
                let mut b = Vec::with_capacity(8);
                b.extend_from_slice(&sid.to_le_bytes());
                b.extend_from_slice(&pc.to_le_bytes());
                Value::Str(b)
            }
            other => {
                return Err(diag::unimplemented(
                    code::rt::CMD_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("TASKINFO 参数序号 {other} 未实现(DIV-RT-015)"),
                )
                .into())
            }
        };
        if let Some(t) = target {
            self.write_target(st, &t, &value)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// `SYSTEMINFO[SCRIPTFILE/SCRIPTNUM/TEXTNUM/EMOTE/NO/LET]`。
    ///
    /// **手册无页**(手册 `system/` 只有 fps/input/mouse;全目录 `grep SYSTEMINFO`
    /// 零命中)。语义来自引擎处理体 v494 `0x450478` 的逐槽反汇编(旧线
    /// `yuris-vm/src/exec/sysinfo.rs` 记录,本实现按同一结论重写):
    ///
    /// | 槽 | 值 |
    /// |---|---|
    /// | `SCRIPTFILE`(0) | `NO`(1 基) → `yst_list` 里的源文件名(越界写空串) |
    /// | `SCRIPTNUM`(1) | 脚本总数 = `yst_list` 条目数 |
    /// | `TEXTNUM`(2) | `NO` → 该脚本的文本数(越界写 0) |
    /// | `EMOTE`(3) | E-mote 运行时标志(宿主能力) |
    /// | `NO`(4) | 1 基下标 |
    /// | `LET`(5) | 写回目标 |
    ///
    /// 三层状态:①手册缺(挂 DIV-RT-011);②ERIS 语料 5 次(全为 `NO`+`LET`);
    /// ③引擎处理体地址级反汇编(见上)。**按军规本应 Unimplemented**,
    /// 但这条是 ERIS 启动链的硬依赖(缺它 → `@mTLine=0` → 除零 → 主线终止),
    /// 且引擎证据完整 —— 故实现并明确登记"缺手册页"这一事实。
    fn op_systeminfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<crate::expr::LValue> = None;
        let mut no: Option<i64> = None;
        let mut want = [false; 4]; // SCRIPTFILE / SCRIPTNUM / TEXTNUM / EMOTE
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                5 => target = Some(self.eval_lvalue_window(st, w)?),
                4 => {
                    no = self.eval_window(st, w)?.as_int();
                }
                0..=3 => {
                    let v = self.eval_window(st, w)?;
                    want[w.rec.kind as usize] = v.as_int().unwrap_or(0) != 0;
                }
                _ => {}
            }
        }
        let count = self.tables().boot.scripts.len() as i64;
        let mut answers: Vec<Value> = Vec::new();
        if want[1] {
            answers.push(Value::Int(count));
        }
        if want[2] {
            // `TEXTNUM`:该脚本的文本条数(`yst_list` 记录第 3 个 u32,
            // 实测与 `TEXT` 条目数一致;引擎同源:处理体查 `0x802b2c[n-1]`)。
            let n = match no {
                Some(n) if n >= 1 && n <= count => self
                    .tables()
                    .boot
                    .scripts
                    .text_count((n - 1) as u16)
                    .map(|c| c as i64)
                    .unwrap_or(0),
                _ => 0, // 越界 → 引擎写 0
            };
            answers.push(Value::Int(n));
        }
        if want[0] {
            let name = match no {
                Some(n) if n >= 1 && n <= count => self
                    .tables()
                    .boot
                    .scripts
                    .path_of((n - 1) as u16)
                    .map(|p| p.to_vec())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            answers.push(Value::Str(name));
        }
        if want[3] {
            answers.push(Value::Int(0));
        }
        if let Some(t) = target {
            for v in &answers {
                self.write_target(st, &t, v)?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `INPUT[KEY/PAD/MOUSE]`:输入源启用/禁用。
    ///
    /// 证据(0.495 REA `0x446564`):
    /// - `KEY` 槽存在 → `DAT_005b4888 = (byte)value`;
    /// - `PAD` 槽存在 → `DAT_008015d4 = (byte)value`;
    /// - `MOUSE` 槽存在 → `DAT_005b488c`,并带一次“重新启用吞掉本次点击”的
    ///   锁存(`DAT_00801860=1` / `DAT_005b4878=1`)。
    ///
    /// 读取侧(`FUN_0041a4b8`)在标志为 0 时**整段跳过**对应源的计数器更新
    /// (状态冻结),不是把按键当成松开。参数名来自工程 ysc,不写死槽号。
    fn op_input(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let vals = self.named_command_values(st, cmd)?;
        let get = |name: &str| {
            vals.iter()
                .rev()
                .find(|(_, n, _)| n == name)
                .map(|(_, _, v)| v.clone())
        };
        let flag = |name: &str| get(name).and_then(|v| v.as_int()).map(|v| v != 0);
        self.input
            .apply_input(flag("KEY"), flag("PAD"), flag("MOUSE"));
        self.emit_domain_from_values("INPUT", Domain::System, &vals);
        Ok(StepOutcome::Continue)
    }

    /// `MOUSE[MODE/TIME/MOVE/CLICK/WHEEL/X/Y/CURSOR/HX/HY/ID]`。
    ///
    /// 证据(0.495 REA `0x44aaa0`):
    /// - MODE 0/1/2 写光标显示状态;TIME 写自动隐藏毫秒;
    /// - MOVE/CLICK/WHEEL 写“自动隐藏后哪些鼠标输入会重新显示光标”的标志;
    /// - X/Y 写 `app+0x5b8/0x5bc` 并 `SetCursorPos`(内核记录 warp 请求);
    /// - CURSOR 0..13 选系统光标;HX/HY/ID 是自定义光标热点与图层 ID。
    ///
    /// 引擎**不**用 MOVE/CLICK/WHEEL 屏蔽计数器(见 `FUN_0041a4b8`:它们只
    /// 出现在光标唤醒条件里),所以按钮判定仍读 `_MOUSE_*` 计数器。
    fn op_mouse(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let vals = self.named_command_values(st, cmd)?;
        let get = |name: &str| {
            vals.iter()
                .rev()
                .find(|(_, n, _)| n == name)
                .map(|(_, _, v)| v.clone())
        };
        let int = |name: &str| get(name).and_then(|v| v.as_int());
        let flag = |name: &str| int(name).map(|v| v != 0);
        let id = match get("ID") {
            Some(Value::Str(bytes)) => Some(bytes),
            _ => None,
        };
        self.input.apply_mouse(
            int("MODE"),
            int("TIME"),
            flag("MOVE"),
            flag("CLICK"),
            flag("WHEEL"),
            int("CURSOR"),
            int("X"),
            int("Y"),
            int("HX"),
            int("HY"),
            id.as_deref(),
        );
        self.emit_domain_from_values("MOUSE", Domain::System, &vals);
        Ok(StepOutcome::Continue)
    }

    /// 按 ysc 参数名求值全部窗口(空窗口跳过)。
    ///
    /// 与 `eval_arg_windows` 的区别:保留参数名,供 INPUT/MOUSE 之类“按名
    /// 消费、跨版本槽号会漂移”的命令使用(项目范式:不写死槽号)。
    fn named_command_values(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Result<Vec<(u8, String, Value)>, VmError> {
        let names: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut out = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .cloned()
                .unwrap_or_default();
            let value = self.eval_window(st, w)?;
            out.push((w.rec.kind, name, value));
        }
        Ok(out)
    }

    fn emit_domain_from_values(
        &mut self,
        name: &str,
        domain: Domain,
        vals: &[(u8, String, Value)],
    ) {
        let args = vals
            .iter()
            .map(|(slot, _, value)| ArgValue::new(*slot, value.clone()))
            .collect();
        self.emit_domain(domain, name.to_string(), args);
    }

    /// `FILEINFO`:文件域查询。**只实现 `EXIST=1 FILE=... LET=...`**。
    ///
    /// 证据(EVID-J3-01):
    /// - 手册 `file/fileinfo.html`:`EXIST = 値(0/1)` 查询文件/文件夹是否存在,
    ///   `LET = 変数名` 接收结果;ver0.276 起可检查封包内文件。
    /// - 官方 0.495 opcode 22 handler `0x44286c`(REA 反汇编):
    ///   `0x44296d` 读 `EXIST` 标志;文件分支 `0x44316c` 依次尝试
    ///   包内/散盘四种存在性策略;`0x443222` 把结果归一为 1/0,
    ///   `0x4432dc` 在不存在时同样写 0;`LET` 目标按整数/浮点槽写回。
    /// - 真实脚本 `es_Button.yst:1761-1765` 用五次 `FILEINFO[EXIST=1]`
    ///   探测 `_off/_over/_on/_onover/_na.png`,再拼不带扩展名的 `CG[FILE=]`。
    ///
    /// 其它查询(FOLDER/SIZE/RELATION/CGFILE/SOUNDFILE)语义未取证,
    /// 保持挂号并只发 `DomainCall`(不猜 0)。
    fn op_fileinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let names: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut file: Option<Vec<u8>> = None;
        let mut folder: Option<Vec<u8>> = None;
        let mut exist = false;
        let mut target: Option<crate::expr::LValue> = None;
        let mut args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match name {
                "LET" => {
                    let lv = self.eval_lvalue_window(st, w)?;
                    let var = self.canonical_var(lv.var);
                    // 事件带上引用身份与当前值(DIV-RT-042;写回仍由下方
                    // write_target 做,读值只是事件事实)。
                    if let Ok(v) = self.read_var(st, var, &lv.indices) {
                        args.push(ArgValue::with_ref(w.rec.kind, v, var));
                    }
                    target = Some(lv);
                }
                "FILE" | "FOLDER" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        if let Value::Str(bytes) = &v {
                            if name == "FILE" {
                                file = Some(bytes.clone());
                            } else {
                                folder = Some(bytes.clone());
                            }
                        }
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "EXIST" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        exist = v.as_f64().map(|n| n != 0.0).unwrap_or(false);
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                _ => {
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
            }
        }
        // 事件流保持与实现前一致(File 域;宿主忽略 FILEINFO 本身)。
        self.emit_domain(Domain::File, "FILEINFO", args);
        if !exist || target.is_none() || folder.is_some() {
            self.report_unimplemented(
                "FILEINFO",
                "只实现 FILE+EXIST+LET(file/fileinfo.html;REA 0x44286c)".to_string(),
            );
            return Ok(StepOutcome::Continue);
        }
        let Some(bytes) = file else {
            self.report_unimplemented("FILEINFO", "EXIST=1 缺 FILE(FOLDER 查询未取证)".to_string());
            return Ok(StepOutcome::Continue);
        };
        let mut present = match yu_vfs::LookupKey::new(&bytes, yu_vfs::Codec::Cp932) {
            Ok(key) => {
                self.src.exists(&key) || self.aux.as_ref().is_some_and(|aux| aux.exists(&key))
            }
            Err(diag) => {
                self.report(diag);
                false
            }
        };
        // 「散盘」策略(REA `0x44316c` 四策略的文件系统支;语料
        // `es.SaveDataCheck`/`es.GSD.INIT` 实测:`FILE=$es.SAVEPATH+名`):
        // 路径以注入的 `$_SAVE_PATH` 为前缀 ⇒ save 根文件系统判存在。
        // 缺它则存档界面的槽存在判定(`\_file.info.exist`)与启动恢复
        // (gsd/seld/kidoku)的 FILEINFO 闸恒 0 —— B1 桌面实测暴露。
        if !present {
            present = self.save_root_fs_exists(&bytes);
        }
        if let Some(target) = target {
            let value = Value::Int(if present { 1 } else { 0 });
            self.write_target(st, &target, &value)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// save 根文件系统存在性(FILEINFO 的散盘支):路径以注入的
    /// `$_SAVE_PATH` 为前缀时,剥出相对名做越界检查后查文件系统。
    /// 注入值 = save 根的显示字符串(正斜杠、尾 `/`,`yu-cli::host_paths`)。
    fn save_root_fs_exists(&self, path: &[u8]) -> bool {
        let Some(root) = self
            .paths
            .save_path
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
        else {
            return false;
        };
        let Some(rel) = path.strip_prefix(root.as_bytes()) else {
            return false;
        };
        if rel.is_empty() {
            return false;
        }
        let Ok(text) = std::str::from_utf8(rel) else {
            return false;
        };
        if text.contains('\\') || text.split('/').any(|c| c == "..") {
            return false;
        }
        std::path::Path::new(&root).join(text).exists()
    }

    // ---------------------------------------------------------- 存档域(B1)

    /// 一个变量的整组快照(SAVE 预约 / LOAD 对拍用)。
    fn var_snapshot(&self, var: VarRef) -> Option<crate::event::VarSnapshot> {
        let slot = self.store.slot(var)?;
        Some(crate::event::VarSnapshot {
            ty: slot.ty,
            dims: slot.dims.iter().map(|&d| d as u32).collect(),
            data: slot.data.clone(),
        })
    }

    /// `(值类型, 组号)` 匹配的全局变量快照,顺序 = 工程变量表(ysv 记录序;
    /// 与 `VARACT[INIT]` 的恢复序同源)。
    fn globals_of_group(&self, ty: ElemType, group: u8) -> Vec<(u16, crate::event::VarSnapshot)> {
        self.globals_restore
            .iter()
            .filter(|g| g.group == group && g.ty == ty)
            .filter_map(|g| {
                let var = g.var;
                self.var_snapshot(var).map(|snap| (var.id, snap))
            })
            .collect()
    }

    /// `SAVE`:手册 `file/save.html`;REA 0.495 处理体 `0x44c45c`(opcode 84,
    /// 分发表 `0x7ace20[84]`;任务书旧锚 `0x44e1c8` 实为 SAVEINFO,勘误)。
    ///
    /// 参数序(官方 ycd):FILE(0)/DNO(1)/SET(2)/ID(3)/IDNO(4)/
    /// G_INT(5)..G_STR4(16)。取证要点:
    /// - `SET=@x`/`SET=@x()` = 变量引用;块内容 = 该变量**当前全部元素**
    ///   (尺寸式 `0x44d5dd`;官方 seld.sd Int[2001]、save50001 标量
    ///   dims=[] 自证);
    /// - **预约即快照**:`SAVE[DNO]` 时刻 `0x44d73d` 序列化当前值(zlib 压缩
    ///   后入预约表),`SAVE[FILE]` 只写文件头/偏移表并流出预约块
    ///   (`0x44cd5a..`);同 DNO 重复预约 = 替换(`0x44c740`);
    /// - `G_<族><组>=1`:按首个在场旗标(官方级联序 G_INT→G_INT2→G_INT3→
    ///   G_INT4→G_FLT→…)定 (类型,组),枚举全局变量表逐变量子块化
    ///   (块类型 2;`0x44ca8a..0x44cc57`);
    /// - `ID=` 图层快照 = H-P4-02(挂账,不阻塞变量保存);
    /// - FILE 缺 `.sd` 补上、含 `.sd` 超 32 字节报错(手册 + REA `0x44c663`)。
    fn op_save(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let names: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut file: Option<String> = None;
        let mut dno: Option<i64> = None;
        let mut set: Option<(LValue, Value)> = None;
        let mut layer_id = false;
        // G_* 旗标按官方级联序的**首个在场**生效(与值无关,DIV-RT-019 同判)。
        let mut group_slot: Option<(ElemType, u8)> = None;
        let mut args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match name {
                "SET" => {
                    let lv = self.eval_lvalue_window(st, w)?;
                    let var = self.canonical_var(lv.var);
                    let v = self.read_var(st, var, &lv.indices)?;
                    args.push(ArgValue::with_ref(w.rec.kind, v.clone(), var));
                    set = Some((lv, v));
                }
                "FILE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        if let Value::Str(bytes) = &v {
                            file = Some(
                                yu_format::Codec::Cp932
                                    .decode(bytes, b"save-file")
                                    .unwrap_or_else(|_| {
                                        String::from_utf8_lossy(bytes).into_owned()
                                    }),
                            );
                        }
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "DNO" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        dno = v.as_int();
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "ID" => {
                    layer_id = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "G_INT" | "G_INT2" | "G_INT3" | "G_INT4" | "G_FLT" | "G_FLT2" | "G_FLT3"
                | "G_FLT4" | "G_STR" | "G_STR2" | "G_STR3" | "G_STR4" => {
                    // 组旗标按官方级联序取**首个在场**(与值无关;DIV-RT-019
                    // 存在性判定同族)。
                    if group_slot.is_none() {
                        if let Ok(v) = self.eval_window(st, w) {
                            group_slot = Some(Self::group_of_name(name));
                            args.push(ArgValue::new(w.rec.kind, v));
                        }
                    }
                }
                _ => {
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
            }
        }
        self.emit_domain(Domain::Save, "SAVE", args);
        if layer_id {
            // 图层快照需要 gfx(CGETSCREEN 类),H-P4-02 交接;官方 SAVE
            // 的 ID 分支与变量保存同函数不同路径,不互相阻塞。
            self.report_unimplemented(
                "SAVE",
                "SAVE[ID] 图层快照未实现(H-P4-02;变量保存不受影响)".to_string(),
            );
        }
        // 预约(DNO 在场;官方 DNO 与 FILE 可同条,先预约后刷盘)。
        if let Some(dno) = dno {
            if !(1..=1024).contains(&dno) {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("SAVE DNO={dno} 越界(手册 file/save.html:1~1024)"),
                ));
                return Ok(StepOutcome::Continue);
            }
            let dno = dno as u32;
            if let Some((ty, group)) = group_slot {
                let vars = self.globals_of_group(ty, group);
                let failure = match self.save_host.as_mut() {
                    Some(host) => host.reserve_group(dno, ty, group, &vars).err(),
                    None => None,
                };
                if let Some(d) = failure {
                    return Err(VmError::from(self.locate(d)));
                }
            } else if let Some((lv, _)) = set {
                let var = self.canonical_var(lv.var);
                let snap = self.var_snapshot(var);
                match snap {
                    Some(snap) => {
                        let failure = match self.save_host.as_mut() {
                            Some(host) => host.reserve(dno, var, &snap).err(),
                            None => None,
                        };
                        if let Some(d) = failure {
                            return Err(VmError::from(self.locate(d)));
                        }
                    }
                    None => {
                        self.report(diag::recoverable(
                            code::rt::VAR_UNDECLARED,
                            Boundary::Script,
                            format!("SAVE[SET] 目标未声明 {}(不猜块内容)", var.display()),
                        ));
                    }
                }
            } else {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "SAVE 缺 SET/G_*(无内容可预约)",
                ));
            }
        }
        // 刷盘(FILE 在场;官方先写头/偏移表再流出预约块)。
        if let Some(file) = file {
            let name = Self::ensure_sd_ext(&file);
            if name.len() > 32 {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("SAVE[FILE] 名字含 .sd 超 32 字节(手册 file/save.html): {file}"),
                ));
                return Ok(StepOutcome::Continue);
            }
            let failure = match self.save_host.as_mut() {
                Some(host) => host.flush(&name).err(),
                None => None,
            };
            if let Some(d) = failure {
                return Err(VmError::from(self.locate(d)));
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `LOAD`:手册 `file/load.html`;REA 0.495 处理体 `0x44708c`(opcode 55)。
    ///
    /// 取证要点:偏移表寻址 = `0xC + DNO*4`(`0x44735a`);块类型 2 逐子块
    /// **按块内 var_id 直接恢复**(`0x447460..0x4474de`,不读 `LET`/`G_*`
    /// 在场位——语料 `LOAD[... G_INT3=1]` 的组旗标被求值但不消费);
    /// 块类型 0 写 `LET` 目标整组(`0x447b3d`);`ID=` 图层恢复 = H-P4-02。
    /// 类型/维数不符官方报错(手册:「サイズが違う場合は、エラー」)。
    fn op_load(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let names: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut file: Option<String> = None;
        let mut dno: Option<i64> = None;
        let mut target: Option<LValue> = None;
        let mut layer_id = false;
        let mut args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match name {
                "LET" => {
                    let lv = self.eval_lvalue_window(st, w)?;
                    let var = self.canonical_var(lv.var);
                    if let Ok(v) = self.read_var(st, var, &lv.indices) {
                        args.push(ArgValue::with_ref(w.rec.kind, v, var));
                    }
                    target = Some(lv);
                }
                "FILE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        if let Value::Str(bytes) = &v {
                            file = Some(
                                yu_format::Codec::Cp932
                                    .decode(bytes, b"load-file")
                                    .unwrap_or_else(|_| {
                                        String::from_utf8_lossy(bytes).into_owned()
                                    }),
                            );
                        }
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "DNO" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        dno = v.as_int();
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "ID" => {
                    layer_id = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                _ => {
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
            }
        }
        self.emit_domain(Domain::Save, "LOAD", args);
        if layer_id {
            self.report_unimplemented(
                "LOAD",
                "LOAD[ID] 图层恢复未实现(H-P4-02;变量恢复不受影响)".to_string(),
            );
            return Ok(StepOutcome::Continue);
        }
        let Some(file) = file else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "LOAD 缺 FILE",
            ));
            return Ok(StepOutcome::Continue);
        };
        let Some(dno) = dno else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "LOAD 缺 DNO",
            ));
            return Ok(StepOutcome::Continue);
        };
        if !(1..=1024).contains(&dno) {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("LOAD DNO={dno} 越界(1..=1024)"),
            ));
            return Ok(StepOutcome::Continue);
        }
        let (dno, target) = (dno as u32, target);
        // 宿主借用只包住容器读取;写回需要 &mut self 的变量域。
        let loaded = match self.save_host.as_mut() {
            Some(host) => host.load_block(&Self::ensure_sd_ext(&file), dno),
            None => return Ok(StepOutcome::Continue),
        };
        let block = match loaded {
            Ok(block) => block,
            Err(d) => {
                // 宿主诊断(文件缺失/容器损坏)原样入流;命令继续
                // (语料用 IF[FILEINFO EXIST] 包裹,缺失属正常路径)。
                self.report(d);
                return Ok(StepOutcome::Continue);
            }
        };
        for item in block.vars {
            // 块类型 2:块自带 var_id;块类型 0:写 LET 目标。
            let var = match item.var {
                Some(var) => self.canonical_var(var),
                None => match &target {
                    Some(lv) => self.canonical_var(lv.var),
                    None => continue, // 官方:无处可写即读后丢弃(0x44760c 路径)
                },
            };
            if let Err(d) = self.restore_snapshot(var, &item.snap) {
                self.report(d);
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// 把一份快照写回变量槽(类型/维数校验;手册 load.html 尺寸不符 = 错误)。
    fn restore_snapshot(
        &mut self,
        var: VarRef,
        snap: &crate::event::VarSnapshot,
    ) -> Result<(), yu_log::Diag> {
        let Some((ty, dims)) = self.var_shape(var) else {
            return Err(diag::recoverable(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!(
                    "LOAD 目标未声明 {}(手册:读档前脚本须先声明同型变量)",
                    var.display()
                ),
            ));
        };
        let dims_match = dims.len() == snap.dims.len()
            && dims.iter().zip(&snap.dims).all(|(&a, &b)| a as u32 == b);
        if ty != snap.ty || !dims_match {
            return Err(diag::recoverable(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!(
                    "LOAD 类型/维数不符:目标 {} = {}{:?},块 = {}{:?}(手册 load.html)",
                    var.display(),
                    ty.as_str(),
                    dims,
                    snap.ty.as_str(),
                    snap.dims
                ),
            ));
        }
        self.store.restore_elements(var, snap.data.clone())
    }

    /// `SAVEEND`:手册 `file/saveend.html`(清预约);官方体 `0x44e1a0`。
    fn op_saveend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let args = self.eval_arg_windows(st, cmd);
        self.emit_domain(Domain::Save, "SAVEEND", args);
        if let Some(host) = self.save_host.as_mut() {
            host.save_end();
        }
        Ok(StepOutcome::Continue)
    }

    /// `G_<族><组>` 参数名 → (值类型, 组号 1..4)。官方 ycd 名(`G_INT`=组 1
    /// … `G_STR4`=组 4);首个在场的旗标生效由调用方保证。
    fn group_of_name(name: &str) -> (ElemType, u8) {
        let (ty, rest) = if let Some(rest) = name.strip_prefix("G_INT") {
            (ElemType::Int, rest)
        } else if let Some(rest) = name.strip_prefix("G_FLT") {
            (ElemType::Float, rest)
        } else {
            (ElemType::Str, name.strip_prefix("G_STR").unwrap_or(""))
        };
        (ty, rest.parse::<u8>().unwrap_or(1))
    }

    /// 存档名缺 `.sd` 时补上(手册:拡張子省略可;REA SAVE `0x44c6cc..`
    /// 检测后拼 `0x8083d8`)。
    fn ensure_sd_ext(name: &str) -> String {
        if name.ends_with(".sd") {
            name.to_string()
        } else {
            format!("{name}.sd")
        }
    }

    /// 解析 `CG[FILE]` 的像素尺寸:按工程 `yscfg` 扩展名序尝试 `.png/.bmp/
    /// .jpg/.gif`,只读图像头。结果(含失败)缓存,避免每帧重读。
    ///
    /// 这是 [`crate::image`] 的唯一调用点;尺寸语义来自引擎 `CGINFO[SX/SY]`
    /// = 已装载纹理尺寸,证据与分层说明见 DIV-GFX-024 / EVID-J3-03。
    fn resolve_asset_size(&mut self, key: &[u8]) -> Option<(u32, u32)> {
        if let Some(hit) = self.asset_size_cache.get(key) {
            return *hit;
        }
        let mut found = None;
        let ext_order = self.tables.boot.config.image_ext_order;
        'probe: for &code in ext_order.iter() {
            let ext: &[u8] = match code {
                0 => break,
                1 => b".png",
                2 => b".bmp",
                3 => b".jpg",
                4 => b".gif",
                // 5 = .psb(E-mote 专用包);本内核不解其头。
                _ => continue,
            };
            let mut path = key.to_vec();
            path.extend_from_slice(ext);
            let Ok(lookup) = yu_vfs::LookupKey::new(&path, yu_vfs::Codec::Cp932) else {
                continue;
            };
            let bytes = match self.src.open(&lookup) {
                Ok(b) => Some(b),
                Err(_) => self.aux.as_ref().and_then(|aux| aux.open(&lookup).ok()),
            };
            if let Some(bytes) = bytes {
                if let Some(size) = crate::image::pixel_size(&bytes) {
                    found = Some(size);
                    break 'probe;
                }
            }
        }
        self.asset_size_cache.insert(key.to_vec(), found);
        found
    }

    /// `MATH3D[INT=@...I() FLT=@...F()]`:ERIS 专用 3D→屏幕投影。
    ///
    /// 证据:官方 0.495 `FUN_00448900`(`0x448900`,sha256 `4f5eefa3…`)。
    /// 输入 `I(1..7)` 与 `F(3..30)`,输出写回 `F(31..34)`:
    /// - `I1` 图层号、`I2` 背景层号、`I4` 父层号、`I5/I6` 视口宽高、`I7` TIP;
    /// - `F3..F5` 相机 XYZ、`F6..F8` 背景 XYZ、`F9/F10` 背景中心、`F11/F12`
    ///   背景 CL、`F13/F14` 本层 XY、`F15` 本层 Z、`F16/F17` 缩放 BX/BY、
    ///   `F18/F19` 本层中心、`F20/F21` MX/MY(百分比)、`F22..F26` 父层、
    ///   `F27` `@DEFZ`、`F28` 相机附加 X;
    /// - 输出 `F31/F32` 屏幕 XY、`F33/F34` 缩放 X/Y。
    ///
    /// 公式按反编译逐项转写(`docs/divergences/DIV-GFX-046` 附录);
    /// 缩放是 `es_SPExec` 判 `SIZE<1` 的唯一来源,未实现时恒为 0,
    /// 全部精灵层会被 `es.CGACT_SIZE` 判为屏幕外(EVID-J3-03)。
    fn op_math3d(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() < 2 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!(
                    "MATH3D 需要 INT+FLT 两个数组窗口,得到 {}",
                    cmd.windows.len()
                ),
            )
            .into());
        }
        let mut int_var = None;
        let mut flt_var = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = self.param_name(cmd.opcode, w.rec.kind).unwrap_or_default();
            let var = self.eval_lvalue_window(st, w)?.var;
            match name.as_str() {
                "INT" => int_var = Some(var),
                "FLT" => flt_var = Some(var),
                _ => {}
            }
        }
        let (Some(int_var), Some(flt_var)) = (int_var, flt_var) else {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "MATH3D 缺 INT/FLT 数组参数",
            )
            .into());
        };
        // 越界读取按官方 `FUN_00449b98` 语义取 0(数组元素不存在 = 0.0)。
        let rd_i = |store: &crate::value::VariableStore, i: i64| -> i64 {
            store
                .read(int_var, &[i])
                .ok()
                .and_then(|v| v.as_int())
                .unwrap_or(0)
        };
        let rd_f = |store: &crate::value::VariableStore, i: i64| -> f64 {
            store
                .read(flt_var, &[i])
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        };
        let i1 = rd_i(&self.store, 1);
        let i2 = rd_i(&self.store, 2);
        let i4 = rd_i(&self.store, 4);
        let wsx = rd_i(&self.store, 5) as f64;
        let wsy = rd_i(&self.store, 6) as f64;
        let tip = rd_i(&self.store, 7);
        let (x, y, zx, zy) = math3d_project(i1, i2, i4, wsx, wsy, tip, &|n| rd_f(&self.store, n));
        let out_ty = self
            .store
            .shape(flt_var)
            .map(|(t, _)| t)
            .unwrap_or(crate::value::ElemType::Float);
        for (idx, value) in [(31i64, x), (32, y), (33, zx), (34, zy)] {
            let stored = if out_ty == crate::value::ElemType::Int {
                Value::Int(value.round() as i64)
            } else {
                Value::Float(value)
            };
            // 官方仅在输出元素存在时写;越界静默(同 `FUN_00448900` 尾部)。
            let _ = self.store.write(flt_var, &[idx], &stored);
        }
        Ok(StepOutcome::Continue)
    }

    /// `CGINFO/FONTINFO/WINDOWINFO`:只读查询,结果写入 `LET`。
    ///
    /// 三层来源:①手册 `cg/cginfo.html`、`font/fontinfo.html`、
    /// `window/windowinfo.html`;②ERIS 125+ 处真实使用;③查询值由
    /// [`crate::event::GraphicsHost`] 在**脚本执行期**同步提供(宿主已按
    /// 同一帧的 DomainCall 更新状态)。未安装宿主/字段未取证 ⇒ 只诊断、
    /// 不写 `LET`(不猜 0)。
    fn op_info(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
        name: &str,
    ) -> Result<StepOutcome, VmError> {
        // 参数名来自工程 ysc 表(opcode → 命令声明),不写死槽号。
        let params: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut targets: Vec<crate::expr::LValue> = Vec::new();
        let mut event_args: Vec<ArgValue> = Vec::new();
        let mut query_args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            if pname == "LET" {
                // 引用身份随事件走(DIV-RT-042/B1);`query` 参数面保持原
                // 契约(只含提问字段,LET 是答案目标);写回仍由 query 结果
                // 经 write_target 完成。
                let lv = self.eval_lvalue_window(st, w)?;
                let var = self.canonical_var(lv.var);
                if let Ok(v) = self.read_var(st, var, &lv.indices) {
                    event_args.push(ArgValue::with_ref(w.rec.kind, v, var));
                }
                targets.push(lv);
                continue;
            }
            if let Ok(v) = self.eval_window(st, w) {
                event_args.push(ArgValue::new(w.rec.kind, v.clone()));
                query_args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        // 事件流仍记录命令事实(trace/golden 需要);宿主会忽略 INFO 本身,
        // 真正的读取走下面的 query。
        let domain = if name == "WINDOWINFO" {
            Domain::Window
        } else {
            Domain::Graphics
        };
        self.emit_domain(domain, name.to_string(), event_args);
        if targets.is_empty() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("{name} 缺 LET 目标"),
            ));
            return Ok(StepOutcome::Continue);
        }
        let Some(mut host) = self.graphics_host.take() else {
            self.report_unimplemented(
                name,
                "未安装 GraphicsHost:INFO 查询无法在脚本执行期读取图形状态".to_string(),
            );
            return Ok(StepOutcome::Continue);
        };
        // 请求的字段名(SX/SY 才走资产元数据回退;其余字段保持原样)。
        let wanted = query_args.iter().find_map(|a| {
            params
                .get(usize::from(a.slot))
                .map(String::as_str)
                .filter(|n| matches!(*n, "SX" | "SY"))
        });
        let result = host.query(name, &query_args);
        let mut value = match result {
            Ok(v) => Some(v),
            Err(d) => {
                // `CGINFO[SX/SY]` 的资源尺寸:gfx 不读文件 ⇒ 向宿主取
                // `MFILE` 键,由本 VM 解析图像头后回填(DIV-GFX-024)。
                //
                // 槽号一律按工程 ysc 参数名解析:该工程表里没有 `MFILE`/`ID`
                // 时不猜序号,按“尺寸不可得”继续(仍走原诊断)。
                let mut resolved = None;
                if name == "CGINFO" && wanted.is_some() {
                    let slot_of =
                        |want: &str| params.iter().position(|p| p == want).map(|i| i as u8);
                    if let (Some(mfile_slot), Some(id_slot)) = (slot_of("MFILE"), slot_of("ID")) {
                        let idno_slot = slot_of("IDNO");
                        let mut ask: Vec<ArgValue> = query_args
                            .iter()
                            .filter(|a| a.slot == id_slot || Some(a.slot) == idno_slot)
                            .cloned()
                            .collect();
                        ask.push(ArgValue::new(mfile_slot, Value::Int(1)));
                        if let Ok(Value::Str(key)) = host.query("CGINFO", &ask) {
                            if !key.is_empty() {
                                if let Some((w, h)) = self.resolve_asset_size(&key) {
                                    host.note_asset_size(&key, w, h);
                                    resolved =
                                        Some(Value::Int(i64::from(if wanted == Some("SX") {
                                            w
                                        } else {
                                            h
                                        })));
                                }
                            }
                        }
                    }
                }
                match resolved {
                    Some(v) => Some(v),
                    None => {
                        self.graphics_host = Some(host);
                        self.report(d);
                        return Ok(StepOutcome::Continue);
                    }
                }
            }
        };
        self.graphics_host = Some(host);
        if let Some(v) = value.take() {
            for t in &targets {
                self.write_target(st, t, &v)?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `SOUNDINFO`:只读查询,结果写入 `LET`。
    ///
    /// 证据(0.495 `0x44f4bc` 原始汇编):
    /// - `LET` 目标由 VM 持左值 → [`crate::event::SoundHost::query`] → 写回;
    /// - `EXIST..PITCH`(槽 1..7)各带独立 `ret`,按固定顺序**首个命中即返回**;
    /// - 这 7 个都不在请求里时,其余字段连续执行、最后写入者胜出;
    /// - 缺层时引擎把 LET 清零(字符串目标写空)。
    fn op_soundinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut targets: Vec<crate::expr::LValue> = Vec::new();
        let mut event_args: Vec<ArgValue> = Vec::new();
        let mut query_args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            if pname == "LET" {
                // 引用身份随事件走(DIV-RT-042/B1);`query` 参数面保持原
                // 契约(LET 是答案目标,不进提问字段)。
                let lv = self.eval_lvalue_window(st, w)?;
                let var = self.canonical_var(lv.var);
                if let Ok(v) = self.read_var(st, var, &lv.indices) {
                    event_args.push(ArgValue::with_ref(w.rec.kind, v, var));
                }
                targets.push(lv);
                continue;
            }
            if let Ok(v) = self.eval_window(st, w) {
                event_args.push(ArgValue::new(w.rec.kind, v.clone()));
                query_args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        self.emit_domain(Domain::Sound, "SOUNDINFO", event_args);
        if targets.is_empty() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "SOUNDINFO 缺 LET 目标".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        }
        let Some(mut host) = self.sound_host.take() else {
            self.report_unimplemented(
                "SOUNDINFO",
                "未安装 SoundHost:声音查询无法在脚本执行期读取层状态".to_string(),
            );
            return Ok(StepOutcome::Continue);
        };
        let result = host.query("SOUNDINFO", &query_args);
        self.sound_host = Some(host);
        match result {
            Ok(v) => {
                for t in &targets {
                    self.write_target(st, t, &v)?;
                }
            }
            Err(d) => self.report(d),
        }
        Ok(StepOutcome::Continue)
    }

    // ---------------------------------------------------------- 文本 / 其它

    /// `TEXT`:手册 `text/text.html`(属性设置;可见文本由 `_`/`WORD` 输出)。
    ///
    /// 三层出处(REA 0.495 `0x453448`,处理次序与写入点逐条对过):
    /// - `ID` = 后续文本自动输出的目标文本层 ID;
    /// - `OUTPUTMODE` = 输出模式字节(`0=手动 / 非 0=自动`,默认 1);
    /// - `CLICKBUTTON1..4` / `SKIPBUTTON1..4` = 按钮码,`0` 写入哨兵 `0x3ff`;
    /// - `WAIT` = 每字符等待毫秒(引擎 `[state+4]`);
    /// - `CLICK=1`:追加 `'C'`;**自动模式返回 -1(本帧让出)**,手动模式继续;
    /// - `RETURN=1` / `PAGE=1` / `CLICKRETURN=1` / `CLICKPAGE=1`:追加
    ///   `'R'` / `'P'` / `'r'` / `'p'`;
    /// - `CLEAR=1`:清空文本与标志;
    /// - `AUTORETURN/AUTOPAGE/AUTOCLICK` = 默认 1 的自动行尾行为。
    ///
    /// 处理次序照引擎:`CLICK` 先于 `RETURN/PAGE/CLICKRETURN/CLICKPAGE` 与
    /// `CLEAR`;自动模式下 `CLICK` 的 -1 会跳过后者(同一条 TEXT 命令里
    /// 同时给 CLICK 与 CLEAR 时,引擎确实不执行 CLEAR)。
    ///
    /// 台账:`COV-M-02`(见 `docs/coverage-matrix.md`)。
    fn op_text(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        use crate::text::flag as tflag;
        // 参数名来自工程 ysc 表(opcode → 命令声明),不写死槽号。
        let names: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut vals: Vec<(String, Value)> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .cloned()
                .unwrap_or_default();
            let v = self.eval_window(st, w)?;
            vals.push((name, v));
        }
        let get = |n: &str| {
            vals.iter()
                .rev()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
        };

        // p0 ID:目标文本层(事件字段是 String;非 UTF-8 保持空并挂号)。
        if let Some(Value::Str(b)) = get("ID") {
            match std::str::from_utf8(&b) {
                Ok(s) => self.text_layer_id = s.to_string(),
                Err(_) => {
                    self.text_layer_id.clear();
                    self.report_unimplemented(
                        "TEXT",
                        "TEXT[ID] 为非 UTF-8 CP932 ID:事件字符串无法承载,保持空".to_string(),
                    );
                }
            }
        }
        // p1 OUTPUTMODE(引擎只存 AL)。
        if let Some(v) = get("OUTPUTMODE") {
            self.text.set_output_mode(v.as_int().unwrap_or(0));
        }
        // p2..p9 CLICKBUTTON1..4 / SKIPBUTTON1..4。
        const CLICK_BUTTONS: [&str; 4] = [
            "CLICKBUTTON",
            "CLICKBUTTON2",
            "CLICKBUTTON3",
            "CLICKBUTTON4",
        ];
        const SKIP_BUTTONS: [&str; 4] = ["SKIPBUTTON", "SKIPBUTTON2", "SKIPBUTTON3", "SKIPBUTTON4"];
        for (i, name) in CLICK_BUTTONS.iter().enumerate() {
            if let Some(v) = get(name) {
                crate::text::TextState::set_button(
                    &mut self.text.click_buttons[i],
                    v.as_int().unwrap_or(0),
                );
            }
        }
        for (i, name) in SKIP_BUTTONS.iter().enumerate() {
            if let Some(v) = get(name) {
                crate::text::TextState::set_button(
                    &mut self.text.skip_buttons[i],
                    v.as_int().unwrap_or(0),
                );
            }
        }
        // p10 WAIT。
        if let Some(v) = get("WAIT") {
            self.text.wait_ms = v.as_int().unwrap_or(0);
        }
        // p15 CLICK:先写 'C';自动模式返回 -1 = 本帧让出(REA 0x45383f..0x45388b)。
        if get("CLICK").and_then(|v| v.as_int()) == Some(1) {
            self.text.push_flag(tflag::CLICK);
            if self.text.output_mode != 0 {
                self.emit(VmEvent::SystemCall {
                    frame: self.frame(),
                    name: "TEXT".to_string(),
                    args: Vec::new(),
                });
                return Ok(StepOutcome::Yield);
            }
        }
        // p11..p14 控制标志(值必须恰为 1)。
        for (name, flag) in [
            ("RETURN", tflag::RETURN),
            ("PAGE", tflag::PAGE),
            ("CLICKRETURN", tflag::CLICK_RETURN),
            ("CLICKPAGE", tflag::CLICK_PAGE),
        ] {
            if get(name).and_then(|v| v.as_int()) == Some(1) {
                self.text.push_flag(flag);
            }
        }
        // p16 CLEAR=1:清空文本与标志(REA 0x453894 → 0x4638e4)。
        if get("CLEAR").and_then(|v| v.as_int()) == Some(1) {
            self.text.clear();
        }
        // p17..p19 AUTO*(任意值原样保存)。
        for name in ["AUTORETURN", "AUTOPAGE", "AUTOCLICK"] {
            if let Some(v) = get(name) {
                let n = v.as_int().unwrap_or(0);
                match name {
                    "AUTORETURN" => self.text.autoreturn = n,
                    "AUTOPAGE" => self.text.autopage = n,
                    _ => self.text.autoclick = n,
                }
            }
        }
        self.emit(VmEvent::SystemCall {
            frame: self.frame(),
            name: "TEXT".to_string(),
            args: Vec::new(),
        });
        Ok(StepOutcome::Continue)
    }

    /// `TEXTINFO`:文本属性查询(手册 `text/textinfo.html`;REA `0x4538cc`)。
    ///
    /// 查询判据是**参数是否出现**(不是值是否为 1);出现多个时按引擎次序
    /// `OUTPUTMODE → CX → CY → CLICK → AUTORETURN → AUTOPAGE → AUTOCLICK`
    /// 取第一个。结果写入 `LET` 目标。`ID`/`SET` 槽引擎不读。
    ///
    /// 台账:`COV-M-02`。
    fn op_textinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let names: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut target: Option<crate::expr::LValue> = None;
        let mut present: Vec<String> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .cloned()
                .unwrap_or_default();
            if name == "LET" {
                target = Some(self.eval_lvalue_window(st, w)?);
            } else if !name.is_empty() {
                // 引擎只检查槽位是否存在(0x4538fa..),不求值查询标志本身。
                present.push(name);
            }
        }
        let Some(target) = target else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "TEXTINFO 缺 LET 目标",
            ));
            return Ok(StepOutcome::Continue);
        };
        for name in [
            "OUTPUTMODE",
            "CX",
            "CY",
            "CLICK",
            "AUTORETURN",
            "AUTOPAGE",
            "AUTOCLICK",
        ] {
            if !present.iter().any(|n| n == name) {
                continue;
            }
            let v = match name {
                "OUTPUTMODE" => i64::from(self.text.output_mode),
                "CX" => self.text.cursor_x,
                "CY" => self.text.cursor_y,
                "CLICK" => i64::from(self.text.click),
                "AUTORETURN" => self.text.autoreturn,
                "AUTOPAGE" => self.text.autopage,
                _ => self.text.autoclick,
            };
            self.write_target(st, &target, &Value::Int(v))?;
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Continue)
    }

    /// `_`(文本输出通道):把式子的值作为文本显示。
    ///
    /// 手册 `basic/basic.html` ●テキスト表示:`_[@BANANA]` / `_[2+3*4]`
    /// ——下划线 + 式,值直接进入文本流。语料 33 处全部是 YSCom 把改行宏
    /// `\r` 展开成的 `_[ $_TEXT(239)+$_TEXT(240) ]`(userscript 演示实测);
    /// `$_TEXT` = 引擎自管显示缓冲(手册 `basic/svariable.html`),运行核
    /// 照常求值即可。撤销 DIV-RT-008 挂账(三层已齐)。
    ///
    /// 台账:`COV-M-03`(每字符写 `'1'/'2'` 到 `_TEXT_FLAG`,REA `0x4567c0`)。
    fn op_text_out(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
            let bytes = match v.cast_to_str()? {
                Value::Str(b) => b,
                _ => Vec::new(),
            };
            self.append_text(&bytes);
            self.emit(VmEvent::Text {
                frame: self.frame(),
                id: self.text_layer_id.clone(),
                bytes,
            });
        }
        Ok(StepOutcome::Continue)
    }

    /// `WORD`:一个文本段(YSCom 把一行剧本文本编译为若干 WORD + 行尾
    /// `RETURNCODE`;行内 `\r` 编译为 `_`)。
    ///
    /// 三层:①手册 `basic/basic.html` ●テキスト表示(全角文本行即显示文本);
    /// ②语料 464 处,载荷全部是 **CP932 原始字节**(无表达式壳,137 文件实测);
    /// ③RxYuris `src/YSTB_TextEditor_V5.cpp` 以 WORD 为文本提取单元、按
    /// YSCM 名字动态取 opcode,且 YSTL `text_count` == 每脚本 WORD 数
    /// (全部 YU-RIS 5.x 商业作品实测通过)。
    ///
    /// 台账:`COV-M-03`(每字符写 `'1'/'2'` 到 `_TEXT_FLAG`,REA `0x458d44`)。
    fn op_word(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let _ = st;
        let Some(w) = cmd.windows.first() else {
            return Err(
                diag::fatal(code::rt::CMD_WINDOWS, Boundary::Script, "WORD 缺文本窗口").into(),
            );
        };
        self.append_text(&w.payload);
        self.emit(VmEvent::Text {
            frame: self.frame(),
            id: self.text_layer_id.clone(),
            bytes: w.payload.clone(),
        });
        Ok(StepOutcome::Continue)
    }

    /// `RETURNCODE`:一行文本的行尾标记(改行)。
    ///
    /// ysc/ycd 双表都声明它**无参数**;唯一窗口的 `kind` 是编译器写入的
    /// **本脚本内文本行计数**(1 起逐行 +1;137 文件实测——0 参数命令的
    /// 窗口 kind 不可能是参数序号,只能是行号语义)。运行核记录事实,
    /// 换行渲染归宿主域。
    ///
    /// **让出点**(任务书 E R3;REA 0.495 `0x44c390`):
    /// - `0x44c451: MOV byte ptr [ESI+9], 1` —— 置 `task+9`(让出标志);
    /// - `0x44c455: MOV EAX, 0xffffffff` —— 处理体返回非 0;
    /// - 泵侧 `0x40442e: MOVSX ECX, byte ptr [ESI+0x9]; TEST; JNZ next_task`
    ///   —— 下一次取任务时该任务直接跳过,本 vsync 链结束于此处;
    /// - 帧内唤醒:`FUN_004046fc` 在输入/超时满足时把 `task+9` 清零。
    ///
    /// 这条**否定**了"文本段(`WORD`/`_`)各自让出"的读法:`WORD`
    /// (`0x458d44`)与 `_`(`0x4567c0`)成功路径都 `XOR EAX,EAX; RET`,
    /// 只有 `RETURNCODE` 置标志并返回 -1。故一行 = `WORD…_[…]…WORD` 全部
    /// 在同一次泵内执行完,行尾才让出(与手册 `basic/task.html`
    /// 「表示テキスト…が来ると…一旦休憩」的行级粒度一致:一行文本执行完才休憩)。
    ///
    /// **行尾标志**(REA `0x44c390`,此前只实现了让出):
    /// - 自动模式(`[state+0x1c] != 0`):`AUTOCLICK` 非 0 → `'C'`,
    ///   `AUTORETURN` 非 0 → `'R'`,`AUTOPAGE` 非 0 → `'P'`;
    /// - 手动模式:`[state+0x36] = 1`(请求清屏);
    /// - 两种模式最后都追加 `'E'`(0x45),再置 `task+9` 让出。
    ///
    /// 台账:`COV-M-02`。
    fn op_returncode(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        use crate::text::flag as tflag;
        let _ = st;
        let line = cmd.windows.first().map(|w| w.rec.kind as u32).unwrap_or(0);
        if self.text.output_mode != 0 {
            if self.text.autoclick != 0 {
                self.text.push_flag(tflag::CLICK);
            }
            if self.text.autoreturn != 0 {
                self.text.push_flag(tflag::RETURN);
            }
            if self.text.autopage != 0 {
                self.text.push_flag(tflag::PAGE);
            }
        } else {
            self.text.clear_pending = true;
        }
        self.text.push_flag(tflag::END);
        self.emit(VmEvent::ReturnCode {
            frame: self.frame(),
            line,
        });
        Ok(StepOutcome::TextYield)
    }

    /// `CLIPACT[TEXT=… SET=…]`:手册 `clip/clipact.html`。
    ///
    /// REA `0x43e2d0`:**两个参数都必须存在**(`TEXT` 只判存在、不看值),
    /// 然后把 `SET` 的字节原样写入 Windows 剪贴板(CF_TEXT)。内核保存
    /// 同一份 CP932 字节镜像并同步发域事件,由宿主写入 OS 剪贴板。
    fn op_clipact(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = self.param_names(cmd.opcode);
        let mut text_present = false;
        let mut set: Option<Vec<u8>> = None;
        let mut args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match pname {
                "TEXT" => {
                    text_present = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "SET" => {
                    let v = self.eval_window(st, w)?;
                    let Value::Str(bytes) = v else {
                        return Err(diag::unimplemented(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            "CLIPACT[SET] 的表达式不是字符串(类型强制未取证;DIV-RT-045)"
                                .to_string(),
                        )
                        .into());
                    };
                    args.push(ArgValue::new(w.rec.kind, Value::Str(bytes.clone())));
                    set = Some(bytes);
                }
                other => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!("CLIPACT 参数 {other} 未在官方处理体中出现;DIV-RT-045"),
                    )
                    .into())
                }
            }
        }
        // REA `0x43e2ec..0x43e31a`:TEXT 与 SET 缺一即返回 0(什么都不做)。
        if !text_present || set.is_none() {
            return Ok(StepOutcome::Continue);
        }
        self.clipboard = set.unwrap_or_default();
        self.emit_domain(Domain::System, "CLIPACT", args);
        Ok(StepOutcome::Continue)
    }

    /// `CLIPINFO[TEXT=1 LET=$S]`:手册 `clip/clipinfo.html`。
    ///
    /// REA `0x43e410`:仅当 `TEXT` 存在时读取剪贴板;逐字符按 CP932 前导
    /// 字节表(`0x5b40c0`)定长拷贝,并**丢弃所有 `< 0x20` 的字节**
    /// (`0x43e4d2..0x43e54d`);结果写入 `LET`(字符串变量)。
    fn op_clipinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = self.param_names(cmd.opcode);
        let mut text_present = false;
        let mut target: Option<crate::expr::LValue> = None;
        let mut args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match pname {
                "TEXT" => {
                    text_present = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "LET" => target = Some(self.eval_lvalue_window(st, w)?),
                other => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!("CLIPINFO 参数 {other} 未在官方处理体中出现;DIV-RT-045"),
                    )
                    .into())
                }
            }
        }
        self.emit_domain(Domain::System, "CLIPINFO", args);
        // TEXT 缺席 ⇒ 官方直接返回 0(不读、不写)。
        if !text_present {
            return Ok(StepOutcome::Continue);
        }
        let Some(target) = target else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "CLIPINFO[TEXT] 缺 LET 目标".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        };
        let filtered: Vec<u8> = self
            .clipboard
            .iter()
            .copied()
            .filter(|b| *b >= 0x20)
            .collect();
        self.write_target(st, &target, &Value::Str(filtered))?;
        Ok(StepOutcome::Continue)
    }

    /// `ERROR[NO=… SET=… COM_CG=… …]`:REA `0x46358c`。
    ///
    /// 官方处理体只有一个可观测副作用:参数槽 2 起(`COM_<命令名>`)的
    /// 值按**命令序号**写进 `[0x689b00 + opcode]` 的「逐命令启用表」;
    /// 引擎初始化时把 128 字节全置 1(`0x462124`)。该表的消费者未定位,
    /// 所以内核只保存、不据此改变分派(DIV-RT-046)。
    fn op_error(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = self.param_names(cmd.opcode);
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            let v = self.eval_window(st, w)?;
            let Some(rest) = pname.strip_prefix("COM_") else {
                // NO / SET:官方处理体不读(DIV-RT-046)。
                continue;
            };
            let Some(opcode) = self.tables().boot.yscm.index_of(rest) else {
                continue;
            };
            let value = v.as_int().unwrap_or(0) as u8;
            if let Some(slot) = self.command_enable.get_mut(opcode) {
                *slot = value;
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// 工程 `ysc.ybn` 的参数名表(槽号 → 名字)。
    fn param_names(&self, opcode: u8) -> Vec<String> {
        self.tables()
            .boot
            .yscm
            .command(opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default()
    }

    /// `DIALOG`:手册 `other/dialog.html` + REA 处理体 `0x43fc74`。
    ///
    /// 官方行为(摘要):非 INPUT 走 `MessageBoxA`(同步模态),INPUT 走自绘
    /// 对话框;`LET` 写「左起第几个按钮」(1 基),`LETSTR` 只在 INPUT 模式
    /// 写输入文本。内核把模态改成「任务挂起 + `Vm::answer_dialog` 回答」,
    /// 完整契约与逐参数证据见 `docs/domain-event-contracts.md` §7。
    fn op_dialog(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params: Vec<String> = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .map(|c| c.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut caption = Vec::new();
        let mut text = Vec::new();
        let mut default_input = Vec::new();
        let mut let_target: Option<crate::expr::LValue> = None;
        let mut letstr_target: Option<crate::expr::LValue> = None;
        let mut okcancel = false;
        let mut yesno = false;
        let mut yesnocancel = false;
        let mut input = false;
        let mut input_limit = 0u32;
        let mut default_button = 0u8;
        let mut icon = crate::dialog::DialogIcon::None;
        let mut unsupported: Option<String> = None;
        let mut event_args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match pname {
                "CAPTION" | "STR" | "DEFSTR" => {
                    let v = self.eval_window(st, w)?;
                    let Value::Str(bytes) = v else {
                        return Err(diag::unimplemented(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!(
                                "DIALOG[{pname}] 的表达式不是字符串(类型强制未取证;DIV-RT-044)"
                            ),
                        )
                        .into());
                    };
                    event_args.push(ArgValue::new(w.rec.kind, Value::Str(bytes.clone())));
                    match pname {
                        "CAPTION" => caption = bytes,
                        "STR" => text = bytes,
                        _ => default_input = bytes,
                    }
                }
                "LET" | "LETSTR" => {
                    let lv = self.eval_lvalue_window(st, w)?;
                    if lv.var.id < self.sys_count {
                        return Err(diag::unimplemented(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            format!(
                                "DIALOG[{pname}] 目标为系统变量 {}(写入语义未取证;DIV-RT-007)",
                                lv.var.display()
                            ),
                        )
                        .into());
                    }
                    if pname == "LET" {
                        let_target = Some(lv);
                    } else {
                        letstr_target = Some(lv);
                    }
                }
                // 存在性判定不看参数值(REA `0x68416d..0x684170` 只测非零
                // 索引;`OKCANCEL=0` 也算存在,与手册「値(0/1)」不同)。
                "OK" => {}
                "OKCANCEL" => {
                    okcancel = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        event_args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "YESNO" => {
                    yesno = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        event_args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "YESNOCANCEL" => {
                    yesnocancel = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        event_args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "INPUT" => {
                    input = true;
                    if let Ok(v) = self.eval_window(st, w) {
                        event_args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "INPUTLIMIT" => {
                    let v = self.eval_window(st, w)?;
                    let n = v.as_int().unwrap_or(0);
                    input_limit = n.max(0).min(i64::from(u32::MAX)) as u32;
                    event_args.push(ArgValue::new(w.rec.kind, v));
                }
                "DEFBUTTON" => {
                    let v = self.eval_window(st, w)?;
                    // REA:1..4 → MB_DEFBUTTON1..4;其它 → 0。
                    default_button = match v.as_int().unwrap_or(0) {
                        1..=4 => v.as_int().unwrap_or(0) as u8,
                        _ => 0,
                    };
                    event_args.push(ArgValue::new(w.rec.kind, v));
                }
                "ICON" => {
                    let v = self.eval_window(st, w)?;
                    icon = crate::dialog::DialogIcon::from_slot(v.as_int().unwrap_or(0));
                    event_args.push(ArgValue::new(w.rec.kind, v));
                }
                other => {
                    // STR2..8 / A / E / CHECK / FILEOPEN / FOLDER / CHECKBOX /
                    // SLIDER / NOWAIT:官方处理体未定位消费点,禁止猜(DIV-RT-044)。
                    unsupported = Some(if other.is_empty() {
                        format!("@{}(工程 ysc 无参数名)", w.rec.kind)
                    } else {
                        other.to_string()
                    });
                }
            }
        }
        // 事件是事实通知(即使参数未取证也记录这次调用);权威状态在
        // `Vm::pending_dialogs`(不可重放)。
        self.emit_domain(Domain::System, "DIALOG", event_args);
        if let Some(p) = unsupported {
            return Err(diag::unimplemented(
                code::rt::CMD_UNIMPLEMENTED,
                Boundary::Script,
                format!("DIALOG 参数 {p} 的语义未取证(不弹窗、不写回;DIV-RT-044)"),
            )
            .into());
        }
        // 回答时才写回 ⇒ 现在就把目标变量校验掉,避免宿主回答后才发现
        // 未声明/类型不符(官方 LET 只接受 int/float 变量,LETSTR 只接受
        // 字符串变量;错误消息 0x18b14)。
        for target in [&let_target, &letstr_target].into_iter().flatten() {
            let var = self.canonical_var(target.var);
            if self.store.shape(var).is_none() {
                return Err(diag::unimplemented(
                    code::rt::VAR_UNDECLARED,
                    Boundary::Script,
                    format!("DIALOG 写回目标未声明 {}", var.display()),
                )
                .into());
            }
        }
        if let Some(target) = &let_target {
            let var = self.canonical_var(target.var);
            if let Some(ElemType::Str) = self.store.shape(var).map(|s| s.0) {
                return Err(diag::fatal(
                    code::rt::VAR_TYPE,
                    Boundary::Script,
                    "DIALOG[LET] 目标必须是数值变量(引擎错误 0x18b14)",
                )
                .into());
            }
        }
        if let Some(target) = &letstr_target {
            let var = self.canonical_var(target.var);
            match self.store.shape(var).map(|s| s.0) {
                Some(ElemType::Str) | None => {}
                Some(_) => {
                    return Err(diag::fatal(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        "DIALOG[LETSTR] 目标必须是字符串变量(引擎错误 0x18b14)",
                    )
                    .into())
                }
            }
        }

        // 按钮组按存在性优先级:YESNOCANCEL > YESNO > OKCANCEL > OK(缺省)。
        // `OK` 槽在官方处理体里从不读取,MB_OK 是缺省。
        let buttons = if input {
            // INPUT 模式走自绘对话框,MessageBox 按钮位不起作用;模板只有 OK。
            crate::dialog::DialogButtons::Ok
        } else if yesnocancel {
            crate::dialog::DialogButtons::YesNoCancel
        } else if yesno {
            crate::dialog::DialogButtons::YesNo
        } else if okcancel {
            crate::dialog::DialogButtons::OkCancel
        } else {
            crate::dialog::DialogButtons::Ok
        };
        // 事件是事实通知;权威状态在 `Vm::pending_dialogs`(不可重放)。
        let id = self.next_dialog_id;
        self.next_dialog_id = self.next_dialog_id.wrapping_add(1).max(1);
        let task = self.cur_task.clone();
        self.dialogs.push(crate::dialog::DialogRequest {
            id,
            task,
            caption,
            text,
            buttons,
            default_button,
            icon,
            input,
            input_limit,
            default_input,
            let_target,
            letstr_target,
        });
        Ok(StepOutcome::Yield)
    }

    /// `LABELINFO[# LET EXIST]`:手册 `other/labelinfo.html`。
    fn op_labelinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let mut target: Option<LValue> = None;
        let mut name: Option<Vec<u8>> = None;
        let mut exist = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => {
                    if let Value::Str(b) = self.eval_window(st, w)? {
                        name = Some(b);
                    }
                }
                // LET 保留下标:与 LET/FILEINFO 同口径(曾丢下标标量化)。
                1 => target = Some(self.eval_lvalue_window(st, w)?),
                2 => exist = self.eval_window(st, w)?.as_int().unwrap_or(0) != 0,
                _ => {}
            }
        }
        if exist {
            if let (Some(name), Some(t)) = (name, target) {
                let found = self.find_label(&name).is_some();
                self.write_target(st, &t, &Value::Int(found as i64))?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    // ---------------------------------------------------------- 辅助

    fn level_param(&mut self, st: &ExecState, cmd: &Cmd) -> Result<usize, VmError> {
        match cmd.windows.first() {
            Some(w) if !w.payload.is_empty() => {
                let v = self.eval_window(st, w)?;
                Ok(v.as_int().unwrap_or(1).max(0) as usize)
            }
            _ => Ok(1),
        }
    }

    fn label_from_window(
        &mut self,
        st: &ExecState,
        cmd: &Cmd,
        index: usize,
    ) -> Result<crate::tables::Label, VmError> {
        let w = cmd
            .windows
            .get(index)
            .ok_or_else(|| windows_error(cmd, "缺少标签窗口"))?;
        let value = self.eval_window(st, w)?;
        let Value::Str(name) = value else {
            return Err(diag::fatal(
                code::rt::VAR_TYPE,
                Boundary::Script,
                format!("标签参数必须是字符串,得到 {}", value.type_name()),
            )
            .into());
        };
        self.find_label(&name).ok_or_else(|| {
            let d = diag::fatal(
                code::rt::LABEL,
                Boundary::Script,
                format!("标签不存在:{}", cp932_lossy(&name)),
            );
            VmError::Fatal(d)
        })
    }

    fn jump_to(
        &mut self,
        st: &mut ExecState,
        to_script: u32,
        to_pc: u32,
        base_a: u8,
        kind: JumpKind,
        from_pc: u32,
    ) -> Result<(), VmError> {
        self.ensure_script(to_script)?;
        let script = self.tables.script(to_script).ok_or_else(|| {
            diag::fatal(
                code::rt::INTERNAL,
                Boundary::Script,
                format!("脚本 {to_script} 刚装载却取不到"),
            )
        })?;
        if to_pc as usize >= script.group_count() {
            return Err(diag::fatal(
                code::rt::LABEL,
                Boundary::Script,
                format!(
                    "跳转目标 s{to_script} pc={to_pc} 超出脚本 {} 组",
                    script.group_count()
                ),
            )
            .into());
        }
        self.emit(VmEvent::Jump {
            frame: self.frame(),
            script: st.script_id,
            pc: from_pc,
            to_pc,
            to_script,
            kind,
        });
        st.script_id = to_script;
        st.pc = to_pc;
        st.base_a = base_a;
        Ok(())
    }
}

// ---------------------------------------------------------------- 纯函数

fn flags_get(flags: &[(u8, i64)], slot: u8) -> Option<i64> {
    flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v)
}

/// VARINFO SEARCH 的针值:`INT=`/`FLT=`/`STR=`(手册例 5/6/7)。
fn array_needle(
    vm: &mut Vm,
    cmd: &Cmd,
    st: &ExecState,
    set: VarRef,
) -> Result<Option<Value>, VmError> {
    let ty = vm.store.shape(set).map(|s| s.0);
    for w in &cmd.windows {
        if w.rec.length == 0 {
            continue;
        }
        let is_needle = matches!(
            (ty, w.rec.kind),
            (Some(ElemType::Str), 19) | (Some(ElemType::Float), 18) | (Some(ElemType::Int), 17)
        );
        if is_needle {
            return Ok(Some(vm.eval_window(st, w)?));
        }
    }
    let _ = cmd;
    Ok(None)
}

fn windows_error(cmd: &Cmd, detail: &str) -> yu_log::Diag {
    diag::fatal(
        code::rt::CMD_WINDOWS,
        Boundary::Script,
        format!("{}: {detail}(实给 {} 个窗口)", cmd.name, cmd.windows.len()),
    )
}

/// 参数名 → **0 基槽位**,带 1..=16 守卫(违者 Fatal,不 panic)。
///
/// 生产侧(`GOSUB[PINTn=]` / `RETURN[RINTn=]`)与消费侧
/// (`@_PINT(n)` / `@_RINT(n)`,`sysvar::param_index` 读 `n-1`)必须指向
/// 同一格:名字里的后缀 N ⇒ 索引 `N-1`。2026-10-25 审查发现 `RETURN`
/// 少了 GOSUB 已有的边界守卫,`RINT0` 会 `usize` 下溢、`RINT17+` 会越界,
/// 故抽成本函数统一口径。
fn param_slot_index(name: &str, slot: u8, command: &str) -> Result<usize, VmError> {
    let n = param_index_of(name).unwrap_or(1);
    if !(1..=16).contains(&n) {
        return Err(diag::fatal(
            code::rt::CMD_WINDOWS,
            Boundary::Script,
            format!(
                "{command} 参数 `{name}`(槽 {slot})编号 {n} 超出 1..16(工程 ysc 表异常;DIV-RT-034)"
            ),
        )
        .into());
    }
    Ok(n - 1)
}

/// 参数名尾部编号:`PINT2` → 2,`PINT` → 1。
pub(crate) fn param_index_of(name: &str) -> Option<usize> {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if digits.is_empty() {
        Some(1)
    } else {
        digits.parse().ok()
    }
}

fn coerce_param(name: &str, v: Value) -> Value {
    if name.starts_with("PFLT") || name.starts_with("RFLT") {
        Value::Float(v.as_f64().unwrap_or(0.0))
    } else if name.starts_with("PSTR") || name.starts_with("RSTR") {
        match v {
            Value::Str(_) => v,
            other => Value::Str(other.to_event_repr().into_bytes()),
        }
    } else if name.starts_with("PINT") || name.starts_with("RINT") {
        Value::Int(
            v.as_int()
                .unwrap_or_else(|| v.as_f64().unwrap_or(0.0) as i64),
        )
    } else {
        v
    }
}

/// 复合赋值码(语料实测 0..8;`LET` 窗口 `slot` 高字节)。
fn compound(code: u8, cur: &Value, rhs: &Value) -> Result<Value, yu_log::Diag> {
    Ok(match code {
        0 => rhs.clone(),
        1 => cur.add(rhs)?,
        2 => cur.sub(rhs)?,
        3 => cur.mul(rhs)?,
        4 => cur.div(rhs)?,
        5 => cur.rem(rhs)?,
        6 => cur.bitand(rhs)?,
        7 => cur.bitor(rhs)?,
        8 => cur.bitxor(rhs)?,
        other => {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("LET 复合赋值码 {other} 超出语料观测范围 0..8"),
            ))
        }
    })
}

/// 声明窗口的目标变量(单条变量指令或数组引用)。
fn target_var(w: &CmdWindow) -> Result<VarRef, yu_log::Diag> {
    let instrs = crate::expr::decode(&w.payload)?;
    let first = instrs
        .iter()
        .find(|i| matches!(i.op, 0x48 | 0x56 | 0x76))
        .ok_or_else(|| {
            diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                "声明窗口不含变量引用",
            )
        })?;
    if first.payload.len() != 3 {
        return Err(diag::fatal(
            code::rt::EXPR_MALFORMED,
            Boundary::Script,
            "变量引用载荷不是 3 字节",
        ));
    }
    Ok(VarRef::new(
        VarSpace::from_prefix(first.payload[0]),
        u16::from_le_bytes([first.payload[1], first.payload[2]]),
    ))
}

/// CP932 字符串的字符数(前导字节吞掉后继)。
pub fn str_char_count(bytes: &[u8]) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        if is_lead(bytes[i]) && i + 1 < bytes.len() {
            i += 2;
        } else {
            i += 1;
        }
        n += 1;
    }
    n
}

fn is_lead(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)
}

/// 按字符位置取子串(1 起算;手册 `VARACT` 例 5)。
///
/// 字符边界统一走 [`crate::cp932_text`](CP932 lead byte 跳步),不在本文件
/// 另写一套 —— 见该模块文档里的片头 LOGO 事故。
fn copy_chars(bytes: &[u8], pos: usize, len: usize) -> Vec<u8> {
    let n = crate::cp932_text::char_count(bytes);
    let start = pos.saturating_sub(1).min(n);
    crate::cp932_text::slice_chars(bytes, start, start + len)
}

/// 挖掉 `[pos, pos+len)` 字符,保留头尾(手册 `VARACT` 例 6)。
fn cut_chars(bytes: &[u8], pos: usize, len: usize) -> Vec<u8> {
    let n = crate::cp932_text::char_count(bytes);
    let start = pos.saturating_sub(1).min(n);
    let end = (start + len).min(n);
    let mut out = crate::cp932_text::slice_chars(bytes, 0, start);
    out.extend_from_slice(&crate::cp932_text::slice_chars(bytes, end, n));
    out
}

/// 半角 ASCII 大小写(`VARACT[UPPER/LOWER]`;手册 `variable/varact.html`
/// 只列半角,引擎 §3.26)。
///
/// **必须按 CP932 lead byte 跳步**:字符串是 CP932 原始字节,双字节字符的
/// 尾字节常落在 ASCII 大写区(`0x41..0x5A`),逐字节转换会把它当半角字母
/// 改写,整字变成别的汉字。实测(2026-10-24,`<game-b>` 片头):
/// `サークル` = `83 54 81 5b 83 4e 83 8b`,逐字节小写后 =
/// `83 74 81 5b 83 6e 83 8b` = **`フーハル`** —— 厂商 LOGO
/// `cg/ev/サークル.jpg`(包内实际存在,293KB)因此查不到,片头整段黑屏。
/// 官方 `VARACT` 处理体按双字节字符推进,不会出现该改写。
fn ascii_case(bytes: &[u8], upper: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let step = crate::cp932_text::char_len_at(bytes, i);
        if step == 2 {
            // 双字节字符原样保留(半角大小写只碰 ASCII 段)。
            out.extend_from_slice(&bytes[i..i + step]);
            i += step;
            continue;
        }
        out.push(if upper {
            bytes[i].to_ascii_uppercase()
        } else {
            bytes[i].to_ascii_lowercase()
        });
        i += 1;
    }
    out
}

/// 半角 + 全角拉丁字母大小写(手册 UPPER2/LOWER2)。
///
/// CP932 全角:Ａ-Ｚ = `0x82 0x60..0x79`,ａ-ｚ = `0x82 0x81..0x9A`。
///
/// 兜底分支同样按 [`crate::cp932_text::char_len_at`] 跳步:非 `0x82` 的双字节
/// 字符(汉字/假名)原样保留,绝不逐字节 ASCII 化 —— 与 `ascii_case` 同一
/// 类事故(见其文档)。
fn fullwidth_case(bytes: &[u8], upper: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == 0x82 {
            let t = bytes[i + 1];
            let mapped = if upper && (0x81..=0x9A).contains(&t) {
                Some(0x82u8)
            } else if !upper && (0x60..=0x79).contains(&t) {
                Some(0x81u8)
            } else {
                None
            };
            if let Some(marker) = mapped {
                let tt = if marker == 0x82 {
                    0x60 + (t - 0x81)
                } else {
                    0x81 + (t - 0x60)
                };
                out.push(0x82);
                out.push(tt);
                i += 2;
                continue;
            }
        }
        let step = crate::cp932_text::char_len_at(bytes, i);
        if step == 2 {
            out.extend_from_slice(&bytes[i..i + step]);
            i += step;
            continue;
        }
        let b = bytes[i];
        out.push(if upper {
            b.to_ascii_uppercase()
        } else {
            b.to_ascii_lowercase()
        });
        i += 1;
    }
    out
}

/// CP932 → 显示文本(lossy;仅事件/诊断)。
fn cp932_lossy(bytes: &[u8]) -> String {
    // 不引入码表:非 ASCII 字节按 `\xNN` 显示,保证确定性与可逆诊断。
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7F).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}

/// 命令名 → 域(仅用于事件归属;命令名来自工程表)。
fn domain_of(name: &str) -> Domain {
    if name.starts_with("CG") {
        Domain::Graphics
    } else if name.starts_with("SOUND")
        || name == "CDDA"
        || name == "MIDI"
        || name == "MIDIEND"
        || name == "MIDIINFO"
    {
        // CDDA 与 MIDI 族同域(B2:官方 0x429fb0 空桩也在声音域;coverage §8)。
        Domain::Sound
    } else if name == "SAVE" || name == "LOAD" || name.starts_with("SAVEINFO") || name == "SAVEEND"
    {
        Domain::Save
    } else if name.starts_with("FILE") {
        Domain::File
    } else if name.starts_with("MENU") {
        Domain::Menu
    } else if name.starts_with("WINDOW") {
        Domain::Window
    } else if name.starts_with("MOVIE") {
        Domain::Movie
    } else if name.starts_with("FLASH") {
        Domain::Flash
    } else if name.starts_with("EMOTE") {
        Domain::Emote
    } else if name.starts_with("SYSTEM") || name == "INPUT" || name == "MOUSE" {
        Domain::System
    } else if name.starts_with("FONT") {
        // 手册 `font/font.html` / `fontinfo.html`(字体渲染域,归宿主 gfx)。
        Domain::Graphics
    } else {
        Domain::Other
    }
}

/// `MATH3D` 的投影核心(纯函数,便于单测)。
///
/// 直接转写官方 `FUN_00448900`(`0x448900`)的 TIP==0/!=0 两个分支:
/// 官方用 x87 double 计算,输出 F31/F32 在写回 INT 目标时取整、写回 FLT
/// 目标时保留原值;此处保留 double,由调用方按目标类型处理。
fn math3d_project(
    lno: i64,
    lbg: i64,
    parent: i64,
    wsx: f64,
    wsy: f64,
    tip: i64,
    f: &dyn Fn(i64) -> f64,
) -> (f64, f64, f64, f64) {
    let cam_x = f(3) + f(28);
    let cam_y = f(4) + f(29);
    let cam_z = f(5) + f(30);
    let def_z = f(27);
    let mx = f(20) / 100.0;
    let my = f(21) / 100.0;
    if tip != 0 {
        // 官方 else 分支:TIP 非 0 时不做透视,只保留图层内偏移/缩放。
        let x = f(13) - f(18) * f(16) / 100.0;
        let y = f(14) - f(19) * f(17) / 100.0;
        return (x, y, f(16), f(17));
    }
    // 背景与父层的透视比例(官方:fVar15/fVar16 = `pow(2.0, (…)/100)`)。
    // `FUN_00469779` 的 9 字节外壳后紧跟 pow 主体(0x469782 `fyl2x`),
    // 两个 4 字节实参 0/0x40000000 = double 2.0,第三个实参是指数。
    let z_bg = ((cam_z + def_z - f(8)) / 100.0).exp2();
    let mut z_layer = ((cam_z + def_z - f(15)) / 100.0).exp2();
    let mut x = ((f(6) - f(9)) - cam_x * mx) * z_bg + f(11) * mx + wsx * 0.5;
    let mut y = f(12) * my + wsy * 0.5 + ((f(7) - f(10)) - cam_y * my) * z_bg;
    if lno != lbg {
        let bx = f(9) * z_bg + x;
        let by = y + f(10) * z_bg;
        if parent == 0 {
            x = bx + (f(13) * mx - f(18)) * z_layer;
            y = by + (f(14) * my - f(19)) * z_layer;
        } else {
            // 父层存在时比例改用父层 Z(官方在同一局部变量上覆写 fVar16)。
            z_layer = ((cam_z + def_z - f(24)) / 100.0).exp2();
            x = bx + (((f(13) + f(22)) * mx - f(18)) - f(25)) * z_layer;
            y = by + (((f(14) + f(23)) * my - f(19)) - f(26)) * z_layer;
        }
    }
    x += f(18) * (100.0 - f(16)) * z_layer / 100.0;
    y += f(19) * (100.0 - f(17)) * z_layer / 100.0;
    (x, y, f(16) * z_layer, f(17) * z_layer)
}

#[cfg(test)]
mod math3d_tests {
    use super::math3d_project;

    fn sample(fill: impl Fn(&mut [f64])) -> Vec<f64> {
        let mut f = vec![0.0; 40];
        f[3] = 0.0;
        f[4] = 0.0;
        f[5] = 0.0;
        f[6] = 0.0;
        f[7] = 0.0;
        f[8] = 200.0;
        f[9] = 400.0;
        f[10] = 300.0;
        f[13] = 0.0;
        f[14] = 0.0;
        f[15] = 200.0;
        f[16] = 100.0;
        f[17] = 100.0;
        f[18] = 0.0;
        f[19] = 0.0;
        f[20] = 100.0;
        f[21] = 100.0;
        f[27] = 200.0;
        fill(&mut f);
        f
    }

    #[test]
    fn background_is_centered_and_unscaled() {
        let f = sample(|_| {});
        let (x, y, zx, zy) = math3d_project(2, 2, 0, 800.0, 600.0, 0, &|n| f[n as usize]);
        assert_eq!((x, y), (0.0, 0.0));
        assert_eq!((zx, zy), (100.0, 100.0));
    }

    #[test]
    fn sprite_uses_layer_offset_center_and_zoom() {
        let f = sample(|f| {
            f[13] = 100.0; // layer X
            f[14] = 50.0; // layer Y
            f[18] = 200.0; // center X
            f[19] = 150.0; // center Y
        });
        let (x, y, zx, zy) = math3d_project(6, 2, 0, 800.0, 600.0, 0, &|n| f[n as usize]);
        // 背景中心 (400,300) + (图层XY - 图层中心)
        assert_eq!((x, y), (300.0, 200.0));
        assert_eq!((zx, zy), (100.0, 100.0));
    }

    #[test]
    fn tip_branch_skips_perspective() {
        let f = sample(|f| {
            f[13] = 10.0;
            f[14] = 20.0;
            f[18] = 200.0;
            f[19] = 150.0;
        });
        let (x, y, zx, zy) = math3d_project(6, 2, 0, 800.0, 600.0, 1, &|n| f[n as usize]);
        assert_eq!((x, y), (10.0 - 200.0, 20.0 - 150.0));
        assert_eq!((zx, zy), (100.0, 100.0));
    }
}
