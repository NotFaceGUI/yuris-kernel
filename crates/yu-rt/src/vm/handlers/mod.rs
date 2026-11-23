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
use crate::scriptpos;use crate::task::{Effect, StepOutcome, TaskSeed};
use crate::value::{ElemType, Value, VarRef, VarSpace};
use crate::vm::{
    Cmd, CmdWindow, ExecState, GosubFrame, IfFrame, LoopFrame, VaractPlan, Vm, VmError,
};

/// 引擎嵌套上限(手册 `control/if.html` 64 / `control/loop.html` 64 /
/// `control/gosub.html`「GOSUBのネストは64回まで可能です」)。
const NEST_LIMIT: usize = 64;

/// GOSUB 嵌套上限的引擎判据(REA 0.495 `FUN_004459c4` @0x4459e6..0x4459f3):
/// `MOVSX ESI, byte [task+0x140]`(当前深度)→ `LEA EAX,[ESI+1]`
/// → `CMP EAX, 0x40; JE <报错>`。即**新深度 == 64** 就报错,允许的最大深度
/// 是 63 层调用栈帧;`task+0x140` 初值 0(只有调用帧计数,基帧不在其中)。
/// 故允许的 GOSUB 次数 = 64,对应本实现 `frames.len() == 65`(基帧 + 64 帧)。
pub(crate) const GOSUB_MAX_CALLS: usize = 64;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// `MOVIE`:全屏影片(状态机 + 宿主播放端口;REA `0x44b134`)。
    Movie,
    /// `MOVIEEND`:退出影片模式(REA `0x44b9d8`)。
    Movieend,
    /// `MOVIEINFO`:影片状态/编解码查询,`LET` 写回(REA `0x44ba2c`)。
    Movieinfo,
    /// `YMVINFO`:`YMV` 影片元数据查询(`LET` 同步写回;REA `0x459b98`)。
    Ymvinfo,
    /// `YMV`:影片挂接/换帧(解析容器头推给图形域后照发域事件;REA `0x458ee0`)。
    Ymv,
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
    /// `REGINFO`:Windows 注册表读取(`0x44bd30`;宿主能力 + 官方失败默认值)。
    Reginfo,
    /// 官方处理体只做参数求值、不产生副作用(`PLUG`/`ERRORINFO`)。
    NoopEval,
    /// `CSV`:把 CSV 文件解析成单元格串,顺序写入 LET 的字符串数组
    /// (REA v474 `0x43577c`;命令槽 `0x65ecc0=FILE`/`0x65ecc1=LET`)。
    Csv,
    /// `CSVINFO[SIZE=1]`:解析 CSV 并统计单元格数,写入 LET
    /// (REA v474 `0x435cfc`;槽 `0x65ecc0=FILE`/`0x65ecc1=SIZE`/`0x65ecc2=LET`)。
    Csvinfo,
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
    // `SAVEINFO`(`0x44e1c8`)与 `VAR`(`0x453f88`):0.495 处理体在
    // `FUN_00429fd1` 守卫后直接 `xor eax,eax; pop ecx; ret` —— 不读参数、
    // 不写 LET、无任何副作用。手册无页;不是"未取证"。
    // 证据字节:`44e1e1 33 c0 59 c3`、`453fa1 33 c0 59 c3`(REA 2026-11-10)。
    "SAVEINFO",
    "VAR",
];

/// v555 分发表中**未被覆盖**的槽(默认体 `0x45c63c` 型)。
///
/// 2026-11-21 全量反编译:6 个 555 构建(2.1MB 族 / <v555-pc> / <v555-1> /
/// <v555-2>)的 121 槽表在此集合上完全一致。默认体是
/// `FUN_0046c0cc(0xdbbd2, 1); return -1`,即脚本执行错误,不是空操作。
const DEFAULT_SLOT_COMMANDS_0555: &[&str] = &[
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
    "SYSTEMMODE",
    "COMPILEMODE",
    "RELEASEMODE",
    "PROJECTFOLDER",
];

/// v555 分发表中映射到共享空操作体(`0x423168` 型,`xor eax,eax; ret`)
/// 的命令。与 0.495 的差异:**ALIAS 在 555 是空操作**;
/// `LABEL`/`SAVEINFO`/`VAR` 在 555 是真实处理体(但仅 guard/return 0),
/// 因此不在本清单里。
const NOOP_COMMANDS_0555: &[&str] = &[
    "ALIAS",
    "DEBUGLIST",
    "PREP",
    "RETURNBREAK",
    "S_INT",
    "S_FLT",
    "S_STR",
];

/// v555 分发表里是**真实地址**、但语义等价于 no-op 的三条。
///
/// 2026-11-21 反编译:
/// - `LABEL` `0x4437e8`(12B):`XOR EAX,EAX; RET`;
/// - `SAVEINFO` `0x44dc44`(29B) / `VAR` `0x4532b8`(29B):
///   `FUN_004254cd` 守卫后 `return 0`,正常路径无副作用。
///
/// 它们不在 `0x423168` 共享空操作体里,所以不能放进
/// [`NOOP_COMMANDS_0555`];单列一张“guard/return 0”表。
const GUARD_NOOP_COMMANDS_0555: &[&str] = &["LABEL", "SAVEINFO", "VAR"];

/// 按工程 yscm 建表(名字 → 处理器类别)。表 = 数据解析结果,不是内置命令表。
/// 注意:默认槽/无操作槽是 **0.495 引擎分发表**的属性;命令名与序号仍来自
/// 工程 `ysc.ybn`(DIV-RT-001 的版本隔离不因此改变)。
pub fn build_handler_table(
    yscm: &crate::tables::YscmTable,
    semantics: &crate::semantics::SemanticProfile,
) -> Vec<HandlerKind> {
    // 555 是独立商业分支:默认槽/共享空操作清单来自 555 exe 的分发表,
    // 不能把 0.495 清单当成 555 的充分依据。556/600 没有样本,继续走
    // legacy 清单(不自动继承 555)。门控走语义层,不再比较 yscm.version。
    let v555 = semantics.is_v555();
    let (default_slots, noop_slots) = if v555 {
        (DEFAULT_SLOT_COMMANDS_0555, NOOP_COMMANDS_0555)
    } else {
        (DEFAULT_SLOT_COMMANDS_0495, NOOP_COMMANDS_0495)
    };
    let mut table = Vec::with_capacity(yscm.len());
    for i in 0..yscm.len() {
        // 良性:未声明命令名只影响诊断显示,不参与分派语义。
        let name = yscm.command(i as u8).map(|c| c.name.as_str()).unwrap_or("");
        // 官方槽位判定先于域分类:`SYSTEMMODE` 虽以 SYSTEM 开头,但分发表
        // 未覆盖它 ⇒ 执行到就是脚本执行错误,不能当域事件静默放过。
        let kind = if default_slots.contains(&name) {
            HandlerKind::ScriptError
        } else if noop_slots.contains(&name)
            || (v555 && GUARD_NOOP_COMMANDS_0555.contains(&name))
        {
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
                // REA:只调参数求值器后返回 0(`0x44bd10`/`0x46356c`)。
                "PLUG" | "ERRORINFO" => HandlerKind::NoopEval,
                // CSV 族:**按命令表结构**区分两代引擎,不按版本号:
                // - 0.466:序号 7,但参数表有 `LINE` ⇒ 真实解析器
                //   `0x431970`(REA 2026-11-19;CSV 读取 + 行过滤);
                // - 0.479/0.495:序号 7 且无 `LINE` ⇒ 空桩 `0x43e640`
                //   (只求值参数后 `ret 0`,不读文件、不写 LET);
                // - v474:表比它多一个槽,序号 8 ⇒ 解析器 `0x43577c`。
                // 表序号/参数名由工程 `ysc.ybn` 直接给出;其它序号未见样本,
                // 按解析器一系处理(Likely,注释留档)。
                "CSV" => {
                    // 0.466/v474 是真实解析器;0.479/0.495 同序号 7 无 LINE 时
                    // 是空桩。v555 也是序号 7 且无 LINE,但处理体是真实 CSV
                    // 解析器(`0x43c3a0`,1148B;<v555-1> 变体 1682B)。因此版本
                    // 555 必须走 Csv,不能再套 0.495 的空桩判据。
                    let has_line = yscm
                        .command(i as u8)
                        .is_some_and(|c| c.params.iter().any(|p| p.name == "LINE"));
                    if v555 || has_line || i != 7 {
                        HandlerKind::Csv
                    } else {
                        HandlerKind::NoopEval
                    }
                }
                "CSVINFO" => HandlerKind::Csvinfo,
                "ERROR" => HandlerKind::ErrorTable,
                // 需要宿主:Windows 注册表。
                "REGINFO" => HandlerKind::Reginfo,
                // `YMVINFO` 走与 `CGINFO` 同一条 LET 查询路径(REA `0x459b98`:
                // 读图层上挂好的影片结构,缺层/缺影片时写 0)。
                "YMVINFO" => HandlerKind::Ymvinfo,
                // `YMV`(REA `0x458ee0`)在命令处理体内解析容器并把影片挂到
                // 图层上;内核由 VM 解析容器头后推给图形域(事件契约不变)。
                // `YMVEND`(`0x459a64`)只有释放语义,维持域事件。
                "YMV" => HandlerKind::Ymv,
                "YMVEND" => HandlerKind::Domain(Domain::Movie),
                // `MOVIE` 族(全屏影片,手册 `movie/movie.html`):内核持状态,
                // 解码/呈现经 `MovieHost` 端口交给宿主(官方也是交给系统
                // 媒体栈:MCI + COM,见 `MovieHost` 文档)。
                "MOVIE" => HandlerKind::Movie,
                "MOVIEEND" => HandlerKind::Movieend,
                "MOVIEINFO" => HandlerKind::Movieinfo,
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

/// 图像头探测读取的字节上限。
///
/// PNG/BMP/GIF 只需 ≤26 字节;JPEG 要沿段链走到 SOF,通常几十~几百字节。
/// 取 4 KiB:足够覆盖所有实际文件(含带大 EXIF/ICC 的 JPEG),又远小于
/// 按钮贴图的平均值(实测 359 KB),前缀读仍省 ~99% 的 IO。
const IMAGE_HEADER_PROBE: usize = 4096;

mod control;
mod file_io;
pub(crate) mod helpers;
mod media_info;
mod system;
mod text;
mod variables;

use helpers::{compose_window_arg, domain_of};
/// `vm.rs` 的 `VARACT` 计划缓存入口(原 `handlers::build_varact_plan`)。
pub(crate) use helpers::build_varact_plan;

impl Vm {
    /// 引用型参数槽白名单(DIV-RT-042,B1 裁 1-A):该命令的哪些槽在引擎里
    /// 传**变量地址**(REA SAVE `0x44c45c`:身份经 `0x683a60/0x683c00/
    /// 0x683da0[slot]` 三表;LOAD `0x44708c` 的 LET 同型;FILEINFO
    /// `0x4432dc`/SOUNDINFO `0x44f4bc` 的 LET 同族)。事件在求值结果之外
    /// 保留归一化 `VarRef`;其余命令/槽维持只传值(M-2 形态,普查不变)。
    pub(super) fn reference_slot_names(cmd: &str) -> Option<&'static str> {
        match cmd {
            "SAVE" => Some("SET"),
            "LOAD" | "FILEINFO" | "SOUNDINFO" => Some("LET"),
            _ => None,
        }
    }

    /// 把命令窗口逐条求值成 `(槽号, 值)`。跳转窗口与空窗口跳过;
    /// 求值失败省略该窗口(处理器/诊断会给出真正的失败原因)。
    pub(super) fn eval_arg_windows(&mut self, st: &mut ExecState, cmd: &Cmd) -> Vec<ArgValue> {
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
        let mut args = Vec::with_capacity(cmd.windows.len());
        // 参数增量标志在窗口 +2 的 u16 高字节(`slot`):官方参数序言把
        // `pbVar10[3]` 存进 `0x6835a0[i]`,低字节存进 `0x6830c0[i]` 当
        // 求值器 subtype。语料实测:`CG[… X=…]` 的 slot=1,
        // `CG[… X+=(…)]` 的 slot=257(0x0101);同一 `CG` 的 YSCM 参数
        // 声明恒为 Numeric,不能从 `ParamKind` 区分。`X+=` 的零增量必须
        // 按累加处理,否则会把绝对位置清零(立绘钉在左上)。
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            // 静态「求值必失败」窗口(#40):下面两条路径都只会被丢弃
            // (引用槽求值失败后回退的普通求值同样必失败),直接跳过,
            // 不再逐次构造最终被丢弃的 Diag。
            if w.eval_fails {
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
                // 复合赋值算子 = 窗口 `slot` 的高字节(低字节是求值 subtype):
                // **1 = `+=`,2 = `-=`**。
                //
                // 证据(0.495 官方编译产物实测):
                // - `es_Sprite.yst:8173 CG[… X+=(…)]` → 窗口 `X~1`;
                // - `es_SaveLoad.yst:623 CG[ID=$id A-=8]` → 窗口 `A~2`;
                // - `es_Scenario.yst:244/1319`、`ES_ReplayMode.yst:111/389`
                //   的淡出循环同样是 `A-=`。
                //
                // 消费方(gfx `CG/CGACT`)只认 `offset` = 「加到当前值」,所以
                // `-=` 在这里先取负。曾经把两种算子都当加法:`A-=8` 把
                // 256 一路加到 512(不 clamp)⇒ 全屏截图幕布(CGS999)永远
                // 淡不掉,读档完成后画面停在读档界面上(2026-10-27 实测)。
                let comp = w.rec.slot >> 8;
                let (value, offset) = compose_window_arg(comp, v);
                let mut arg = ArgValue::new(w.rec.kind, value);
                arg.offset = offset;
                args.push(arg);
            }
        }
        args
    }

    /// `HandlerKind::Domain` 的通用域事件路径是否会复用顶部求值结果。
    ///
    /// 三个例外分支在处理器里自己求值/发事件(`INPUT`/`MOUSE`、带宿主的
    /// `SOUNDINFO`、带宿主的 `SAVE`/`LOAD`/`SAVEEND`),`DomainCall` 不由
    /// 通用路径发出 ⇒ 不能复用,保持旧行为。
    pub(super) fn domain_uses_generic_args(&self, kind: HandlerKind, cmd: &Cmd) -> bool {
        let HandlerKind::Domain(domain) = kind else {
            return false;
        };
        match domain {
            Domain::System if matches!(cmd.name.as_str(), "INPUT" | "MOUSE") => false,
            Domain::Sound if cmd.name == "SOUNDINFO" && self.sound_host.is_some() => false,
            Domain::Save
                if self.save_host.is_some()
                    && matches!(cmd.name.as_str(), "SAVE" | "LOAD" | "SAVEEND") =>
            {
                false
            }
            _ => true,
        }
    }

    pub(crate) fn dispatch(
        &mut self,
        st: &mut ExecState,
        task: &mut crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        // 启动期解析的 opcode → 处理器表(见 `build_handler_table`)。
        let kind = self
            .handler_of
            .get(cmd.opcode as usize)
            .copied()
            .unwrap_or(HandlerKind::Unknown);
        // 统一事件:尽量把每个窗口求值结果作为参数记录(失败则省略该窗口,
        // 真正的失败会在处理器里以 Diag 报出)。控制流窗口(跳转目标)是
        // 「length 字段 = 组号」而不是表达式,所以只记录成功求值者。
        // `record_events=false` 时跳过(热路径最大开销;Diag 不受影响)。
        //
        // #38:域命令的 `DomainCall` 与 `Command` 事件用的是**同一批**窗口
        // 求值结果;两次事件之间没有状态变化,因此这里只求值一次:Command
        // 拿一份副本,`DomainCall` 复用原件(旧实现整条命令求值两遍)。
        let reuse_for_domain = self.domain_uses_generic_args(kind, cmd);
        let mut domain_args: Option<Vec<ArgValue>> = None;
        if self.record_events {
            let args = self.eval_arg_windows(st, cmd);
            let for_command = if reuse_for_domain {
                domain_args = Some(args.clone());
                args
            } else {
                args
            };
            self.record_command(st, cmd, for_command);
        } else {
            self.record_command(st, cmd, Vec::new());
        }

        // 独立运行面(精确 555 安装):先拦截商业分支命令;None 交回共享旧路径。
        if let Some(result) = self.other_intercept_command(st, cmd) {
            return result;
        }

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
            HandlerKind::Ymvinfo => self.op_info(st, cmd, "YMVINFO"),
            HandlerKind::Ymv => self.op_ymv(st, cmd),
            HandlerKind::Movie => self.op_movie(st, cmd),
            HandlerKind::Movieend => self.op_movieend(st, cmd),
            HandlerKind::Movieinfo => self.op_movieinfo(st, cmd),
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
            HandlerKind::Reginfo => self.op_reginfo(st, cmd),
            HandlerKind::Csv => self.op_csv(st, cmd),
            HandlerKind::Csvinfo => self.op_csvinfo(st, cmd),
            HandlerKind::NoopEval => {
                // 官方处理体 = 参数求值 + `xor eax,eax; ret`。求值错误的
                // 可恢复诊断照常记录,然后继续。
                for w in &cmd.windows {
                    if w.rec.length == 0 {
                        continue;
                    }
                    // Err 只在诊断已记录后丢弃(eval_window 内部已挂号),
                    // 不二次报告 —— 良性静默(Q1 自查 b 类)。
                    let _ = self.eval_window(st, w);
                }
                Ok(StepOutcome::Continue)
            }
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
                // `FONT` 同时写引擎文本状态 `+0x20..+0x34`(逐字符快照的
                // 来源)。内核 gfx 仍按域事件消费字体;VM 侧只需同步文本
                // 子集,保证 `WORD`/`_` 追加时能按官方快照。
                if domain == Domain::Graphics && cmd.name == "FONT" {
                    let args = domain_args
                        .take()
                        .unwrap_or_else(|| self.eval_arg_windows(st, cmd));
                    self.apply_text_font(cmd, &args);
                    self.emit_domain_borrowed(domain, &cmd.name, args);
                    return Ok(StepOutcome::Continue);
                }
                // 域命令的参数是宿主域状态机唯一能看到的输入;必须带上
                // 求值结果(槽号 = 工程 ysc 表的参数序号)。`record_events`
                // 只控制通用 Command 事件,不影响域事件。
                // 事件记录开启时复用顶部已求值的同一份 args(#38)。
                let args = domain_args
                    .take()
                    .unwrap_or_else(|| self.eval_arg_windows(st, cmd));
                self.emit_domain_borrowed(domain, &cmd.name, args);
                // Graphics/Window 已由同步宿主消费时不再挂号;声音域同理。
                // 菜单域:MENU/MENUEND 由 gfx 的菜单层状态机消费
                // (手册 `menu/menu.html`/`menuend.html`);MENUACT/MENUINFO
                // 尚未接线,仍显式挂号(不按域整体吞掉)。
                let handled = (self.graphics_host.is_some()
                    && matches!(domain, Domain::Graphics | Domain::Window))
                    || (self.graphics_host.is_some()
                        && domain == Domain::Menu
                        && matches!(cmd.name.as_str(), "MENU" | "MENUEND"))
                    || (self.sound_host.is_some()
                        && domain == Domain::Sound
                        && matches!(cmd.name.as_str(), "SOUND" | "SOUNDEND"))
                    // 文件域由 `SaveHost` 消费:实现在 `yu_save::SaveState::
                    // apply_event` 的 match 里,逐命令是
                    // `FILEACT`(落盘/复制/删除)与 `FILEINFO`(事实统计)。
                    // `SaveHost::apply_event` 与 Graphics/Sound 一样在
                    // `Vm::emit` 的**命令时刻**同步调用(官方 FILEACT 就地
                    // 执行);判据是「宿主已安装 **且命令名确实被它处理**」。
                    //
                    // 必须按名收窄:早前写成整个 `Domain::File` 会把
                    // `FOLDER` 之类**真未实现**的查询一起吞掉,那才是掩盖问题。
                    || (self.save_host.is_some()
                        && matches!(cmd.name.as_str(), "FILEACT" | "FILEINFO"));
                if !handled {
                    if matches!(cmd.name.as_str(), "MIDI" | "MIDIEND" | "MIDIINFO" | "CDDA") {
                        // 官方 0.495 处理体是空桩(0x44aa40/0x44aa60/0x44aa80、
                        // CDDA 0x429fb0:仅 FUN_00429fd1 守卫后 return 0),无媒体
                        // 输出;内核同型对齐 —— 域事件照发、不建任何声音状态。
                        // 终态 aligned-stub(声音台账 0-9/N0-10),不计挂号。
                        self.report_aligned_stub(
                            &cmd.name,
                            "官方 0.495 处理体为空桩(0x44aa40/0x44aa60/0x44aa80、\
                             CDDA 0x429fb0),同型对齐:只发域事件,无媒体输出(声音台账 N0-10)",
                        );
                    } else if domain == Domain::System {
                        // `SYSTEM`(`SCREENSAVER`/`BACKGROUNDEXEC`/`SX`/`SY`/
                        // `TIMECOUNT*`)是**宿主窗口/进程策略**:全部参数由
                        // 引擎写进进程级状态供 WndProc 读,没有脚本可见返回。
                        // 域事件契约 §9 把 `TRAY` 等 System 域命令定为宿主边界;
                        // `SYSTEM` 同属此类 —— 内核只发事件,由平台层决定是否
                        // 设置屏保/后台执行策略。不是「未实现」,按归属记录
                        // (Info,每命令名一次),不计挂号表。
                        //
                        // 取证限制:手册 `system/` 目录下**没有** `system.html`
                        // (只有 fps/input/mouse),`BACKGROUNDEXEC` 全手册 0 命中;
                        // 引擎侧逐参数处理体尚未定位(0x41b42c 是屏保窗口创建,
                        // 不是命令处理体)。故此处只声明**归属**,不声称逐参数
                        // 已对齐 —— 参数值照原样进事件载荷,平台层可作为。
                        self.report_aligned_stub(
                            &cmd.name,
                            "SYSTEM 参数为宿主窗口/进程策略(屏保、后台执行、客户区尺寸、\
                             计时器),无脚本可见返回;内核只发域事件(域事件契约 §9)",
                        );
                    } else if domain == Domain::Emote {
                        // E-mote 家族:主人明令暂缓(`deferred(master)`,B9)。
                        // 仍走 Unimplemented 而不是静默,避免下游把「没做」
                        // 误当「做了但没效果」;但文案标明这是**已决定暂缓**,
                        // 不是待查项。
                        self.report_unimplemented(
                            &cmd.name,
                             "E-mote 域:deferred(master)(路线图 B9,主人明令暂缓);\
                             语义属 E-mote 运行时,内核边界外",
                        );
                    } else {
                        // 挂号文案给**可执行的**锚点:域事件契约 §9 是域边界的
                        // 正式定义处(`docs/domain-event-contracts.md`);旧文案
                        // 引「任务书 A」,该文档只覆盖 workspace/yu-log/yu-vfs,
                        // 从来没有域边界章节(2026-10-24 核对)。
                        self.report_unimplemented(
                            &cmd.name,
                            "域命令未接线:语义属宿主域状态机(域边界见 \
                             crates/yu-rt/docs/domain-event-contracts.md §9)",
                        );
                    }
                }
                Ok(StepOutcome::Continue)
            }
            HandlerKind::Unknown => {
                self.report_unimplemented(&cmd.name, "命令无处理器(手册无页或语义未取证)");
                Ok(StepOutcome::Continue)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::YscmTable;
    use yu_format::yscm::{Arg, Command, CommandTable};

    fn synthetic(version: u32) -> YscmTable {
        let mut commands = Vec::with_capacity(121);
        for i in 0..121u8 {
            let name = match i {
                0 => "ALIAS".to_string(),
                7 => "CSV".to_string(),
                117 => "SYSTEMMODE".to_string(),
                _ => format!("X{i}"),
            };
            // v555 CSV 无 LINE 参数(与 0.495 同形);ALIAS/SYSTEMMODE 无参。
            let args: Vec<Arg> = if i == 7 {
                vec![
                    Arg {
                        name: "FILE".into(),
                        flag: 1,
                        kind: 0,
                    },
                    Arg {
                        name: "LET".into(),
                        flag: 0,
                        kind: 0,
                    },
                ]
            } else {
                Vec::new()
            };
            commands.push(Command {
                name: name.into(),
                argc: args.len() as u8,
                args,
            });
        }
        YscmTable::from_format(CommandTable {
            version,
            commands,
            messages: vec![Vec::new(); 37],
            key_table: Box::new([0u8; 256]),
        })
        .expect("synthetic yscm")
    }

    #[test]
    fn v555_routes_alias_and_csv_from_555_dispatch_table() {
        let t = build_handler_table(
            &synthetic(555),
            &crate::semantics::SemanticProfile::from_yscm_version(555),
        );
        assert_eq!(t[0], HandlerKind::Noop, "v555 ALIAS = 0x423168 no-op");
        assert_eq!(t[7], HandlerKind::Csv, "v555 CSV = 真实解析器");
        assert_eq!(
            t[117],
            HandlerKind::ScriptError,
            "v555 SYSTEMMODE 仍是默认错误槽"
        );
    }

    #[test]
    fn legacy_routes_keep_0495_stub_semantics() {
        let t = build_handler_table(
            &synthetic(495),
            &crate::semantics::SemanticProfile::from_yscm_version(495),
        );
        assert_eq!(t[0], HandlerKind::Unknown, "0.495 表没有 ALIAS");
        assert_eq!(t[7], HandlerKind::NoopEval, "0.495 CSV 是空桩");
    }
}
