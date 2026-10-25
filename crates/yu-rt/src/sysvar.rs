//! 系统变量读取(按**名字**分派,id 全部来自工程 `YSCom.ycd`)。
//!
//! # 纪律
//!
//! 每个已实现分支必须能指出:手册页 + 语料使用 + 引擎取证。
//! **没有手册页的系统变量一律 `Unimplemented`**,即使语料里高频使用
//! (0.495 实测:`_DXMODE`/`_CHR`/`_VARTYPE` 三类在手册 HTML 里 0 命中,
//! 见 `crates/yu-rt/docs/divergences/DIV-RT-007.md`)。
//!
//! # 名称 → id(工程数据,不是常量)
//!
//! id 从 `tables.boot.sysvars` 的顺序而来(有 ycd 时来自 ycd,商业包来自
//! 引擎 ABI 数据表,两者都由 `ysv.ybn` 逐项校验)。实测 0.495:
//! `_TIME`=0、`_OS_TIME`=1、`_PAI`=10、`_DEBUGMODE`=11、`_LC`=48、
//! `_PINT`=53、`_PFLT`=54、`_PSTR`=55、`_PFLG`=56、`_RINT`=60、
//! `_SCREEN_SIZE_X`=115、`_MOUSE_X`=133、`_INP`=144、`_KEY_A`=167。

use yu_log::Boundary;

use crate::code;
use crate::diag;
use crate::value::{Value, VarRef};
use crate::vm::{ExecState, Vm};

/// 读取系统变量。
///
/// `var.id` 由编译产物直接给出;`indices` 是脚本写的下标(空 = 无下标)。
pub fn read(vm: &Vm, st: &ExecState, var: VarRef, indices: &[i64]) -> Result<Value, yu_log::Diag> {
    let Some(decl) = vm.sysvars().get(var.id) else {
        return Err(diag::fatal(
            code::rt::SYSVAR_UNIMPLEMENTED,
            Boundary::Script,
            format!("系统变量 id {} 超出系统变量 ABI 表范围", var.id),
        ));
    };
    let name = decl.name.as_str();
    match name {
        // ---- 循环计数器 --------------------------------------------------
        // 手册 `control/loop.html`:「システム変数 @_LC が、ループカウンタとして
        // 利用できます。5 回のループとした場合、1 周する度に 1～5 まで変化」。
        // 引擎 §3.13:LOOP 帧 +0x10 = 已执行轮数、置 1、LOOPEND 每轮 +1。
        // 语料:18,521 次(全系统变量第一位)。
        "_LC" => Ok(Value::Int(st.loops.last().map(|l| l.counter).unwrap_or(0))),

        // ---- GOSUB 参数 / RETURN 返回值 -----------------------------------
        // 手册 `basic/svariable.html` + `control/gosub.html`:
        // `@_PINT(n)/@_PFLT(n)/$_PSTR(n)` = 从跳转源收到的参数;
        // `@_PFLG(n)` = 该编号是否传过;`@_RINT(n)…` = RETURN 带回的值。
        // 引擎 §3.16:系统变量 0x35/0x36/0x37/0x38(组 A)与 0x3C/0x3D/0x3E(组 B)。
        // 语料:_PINT 12,380 / _PFLT 2,752 / _PSTR 4,101 / _RINT 7,302 / _RFLT 1,065 /
        //       _RSTR 2,668 / _PFLG 30。
        "_PINT" | "_PFLT" | "_PSTR" => {
            let n = param_index(indices)?;
            let frame = st.frames.last().ok_or_else(|| {
                diag::fatal(
                    code::rt::INTERNAL,
                    Boundary::Script,
                    "无调用帧却读 _PINT/_PFLT/_PSTR",
                )
            })?;
            match name {
                "_PINT" => Ok(frame.args[n].clone().unwrap_or(Value::Int(0))),
                "_PFLT" => Ok(Value::Float(
                    frame.args[n]
                        .as_ref()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                )),
                _ => Ok(frame.args[n]
                    .clone()
                    .unwrap_or_else(|| Value::Str(Vec::new()))),
            }
        }
        "_PFLG" => {
            let n = param_index(indices)?;
            Ok(Value::Int(
                st.frames.last().map(|f| f.arg_flags[n] as i64).unwrap_or(0),
            ))
        }
        "_RINT" | "_RFLT" | "_RSTR" => {
            let n = param_index(indices)?;
            let frame = st.frames.last().ok_or_else(|| {
                diag::fatal(
                    code::rt::INTERNAL,
                    Boundary::Script,
                    "无调用帧却读 _RINT/_RFLT/_RSTR",
                )
            })?;
            match name {
                "_RINT" => Ok(frame.ret[n].clone().unwrap_or(Value::Int(0))),
                "_RFLT" => Ok(Value::Float(
                    frame.ret[n]
                        .as_ref()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                )),
                _ => Ok(frame.ret[n]
                    .clone()
                    .unwrap_or_else(|| Value::Str(Vec::new()))),
            }
        }
        "_RFLG" => {
            let n = param_index(indices)?;
            Ok(Value::Int(
                st.frames.last().map(|f| f.ret_flags[n] as i64).unwrap_or(0),
            ))
        }

        // ---- 任务局部寄存器 ----------------------------------------------
        // **两版 SDK 手册(0.479/0.495)均无 `_TINT` 页**——旧注释误引
        // svariable.html,已勘误。语义出处 = 官方 ERIS 源码的生产者/消费者对
        // (`es_ERIS.yst:524` TASK[TINT=…] → `:533` `@_TINT(1)`;stdlib
        // `es._task` 同型)+ 引擎任务记录 `task+0x3d0`(旁线 §3.16)。
        // 写入侧 = `TASK[TINT..]`(见 `op_task`)。
        "_TINT" | "_TFLT" | "_TSTR" => {
            let n = param_index(indices)?;
            match name {
                "_TINT" => Ok(Value::Int(st.task_ints[n])),
                "_TFLT" => Ok(Value::Float(st.task_floats[n])),
                _ => Ok(Value::Str(st.task_strs[n].clone())),
            }
        }

        // ---- 系统字符构造 ------------------------------------------------
        // `$_CHR(n)`:把一个数值当作**单字节**取回(CP932 原始字节,不做
        // 码位到 CP932 的编码)。REA 依据:0.495 的字符串系统变量取值走
        // `0x446b93` 分派,`_CHR` 一支按字节写回缓冲;语料用法把它当
        // "构造控制字符"用 —— `es_Text.yst:878` 用
        // `$_CHR(0xEF)+$_CHR(0xF3)` 拼出文本流里的 `C` 控制码,而编译器把
        // 剧本里的 `\r` 展开成 `$_TEXT(239)+$_TEXT(240)`(字节 0xEF 0xF0)。
        // 因此返回**原始字节**才与 `_TEXT` 里的控制码逐字节相等;
        // 若按 Unicode 码位再编码成 CP932(0xEF ⇒ 双字节),比较恒不成立,
        // 文本控制符('C'/'R'/'P'/'E')将全部识别失败(DIV-RT-007,
        // 手册无页 ⇒ 本条按语料用法+REA 定语义)。
        "_CHR" => {
            let [v] = indices else {
                return Err(diag::unimplemented(
                    code::rt::SYSVAR_UNIMPLEMENTED,
                    Boundary::Script,
                    "_CHR 需要 1 个下标(字节值);手册无页(DIV-RT-007)".to_string(),
                ));
            };
            Ok(Value::Str(vec![(*v & 0xff) as u8]))
        }

        // ---- 帧率/帧号 ----------------------------------------------------
        // 手册 `basic/svariable.html` 未列 `_SYSTEM_FRAME`;旁线未给语义
        // ⇒ 不实现(DIV-RT-007)。
        "_SYSTEM_FRAME" | "_SYSTEM_TASKFRAME" | "_SYSTEM_TIME" => Err(undoc(name)),

        // ---- 时间(手册 `basic/svariable.html` 全表) ---------------------
        // `@_TIME(0,k)` 当前日期时间 / `@_TIME(1,k)` 启动时刻;k: 0年1月2日
        // 3星期4时5分6秒7毫秒。**时钟由宿主注入**(A3);纪元基准是注入时钟的
        // 起点(`BootConfig::start_ms` 的基准原点),不是读墙钟(DIV-RT-009)。
        // 语料:336 次。
        "_TIME" => {
            let (row, col) = match indices {
                [r, c] => (*r, *c),
                _ => {
                    return Err(diag::fatal(
                        code::rt::SYSVAR_UNIMPLEMENTED,
                        Boundary::Script,
                        "_TIME 需要两个下标(行=0当前/1启动,列=0..7)",
                    ))
                }
            };
            let base = if row == 0 { vm.now_ms() } else { vm.start_ms() };
            time_field(base, col)
        }
        // `@_OS_TIME(0)` = OS 启动以来的毫秒;`(1)` = YU-RIS 启动以来的毫秒。
        // 手册同上。语料 1,463 次(实测下标恒为 1 → 引擎 uptime)。
        // (0) 需要宿主 OS uptime,未注入 ⇒ Unimplemented。
        "_OS_TIME" => {
            let idx = indices.first().copied().unwrap_or(0);
            match idx {
                1 => Ok(Value::Int(vm.now_ms().wrapping_sub(vm.start_ms()) as i64)),
                0 => Err(diag::unimplemented(
                    code::rt::SYSVAR_UNIMPLEMENTED,
                    Boundary::Script,
                    "_OS_TIME(0) 需要宿主注入的 OS uptime;内核不读墙钟(A3/DIV-RT-009)",
                )),
                other => Err(diag::fatal(
                    code::rt::SYSVAR_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("_OS_TIME 下标 {other} 超出手册范围(0..1)"),
                )),
            }
        }

        // ---- 常量 ---------------------------------------------------------
        // 手册 `basic/svariable.html`:「@_PAI = πの値(3.1415926535～)」。
        // 引擎常量(数学),不是游戏数据 ⇒ 允许进代码。
        "_PAI" => Ok(Value::Float(std::f64::consts::PI)),
        // 手册:`@_DEBUGMODE` = デバッグモードか否か(1/0)。真值来自工程
        // `projectconfig.txt` 的 `SYSTEMMODE[DEBUGMODE=…]`,由宿主装载后注入。
        // 语料 570 次。
        "_DEBUGMODE" => Ok(Value::Int(vm.debug_mode() as i64)),
        // 手册:`@_SYSTEM_VERSION` = YU-RIS 版本 ×1000。真值来自工程
        // `ysc.ybn`/`yscfg.ybn` 的 version 字段(同一工程内一致)。
        "_SYSTEM_VERSION" => Ok(Value::Int(vm.engine_version() as i64 * 1000)),
        // 手册:`@_SCREEN_SIZE_X/Y` = 当前画面分辨率;真值 = 工程 `yscfg.ybn`
        // 的 width/height(u32;R12 起直接采用 yu_format::yscf;引擎
        // `FUN_0045dbb0` 按 dword 读;与 `WINDOWINFO NO=0 SX/SY` 同源)。
        // 语料 0 次(ERIS 用自算布局变量)。
        "_SCREEN_SIZE_X" => Ok(Value::Int(vm.screen().0 as i64)),
        "_SCREEN_SIZE_Y" => Ok(Value::Int(vm.screen().1 as i64)),
        // 手册 `basic/svariable.html`:`_MEDIA_READONLY`(0/1)/`_OS_SOUND`(0/1)。
        // 两者都是**宿主环境属性**,由 `BootConfig` 注入(A3:内核不探测外部环境)。
        // 语料:`es_System.yst` 的用户可配置开关 `@T(17,04)/@T(17,05)` 决定是否检查。
        "_MEDIA_READONLY" => Ok(Value::Int(vm.media_readonly() as i64)),
        "_OS_SOUND" => Ok(Value::Int(vm.os_sound() as i64)),
        // 手册:`@_SCREEN_COLOR` = 現在の色数。宿主显示层注入(A3:
        // 内核不读真实显示;缺省 32bpp 由 `BootConfig::screen_color` 给)。
        "_SCREEN_COLOR" => Ok(Value::Int(vm.screen_color() as i64)),
        // 手册无页;语料用法证明它是「DX 缓冲图层可用性」的宿主能力开关
        // (`es_first.yst`:IF[@_DXMODE] { CG[ID=DXBUF …] })。由宿主注入。
        "_DXMODE" => Ok(Value::Int(vm.dx_mode() as i64)),
        // 手册:`@_ARGV(n)` 命令行参数 / `@_ARGC` 个数。由宿主注入。
        "_ARGC" => Ok(Value::Int(vm.argv().len() as i64)),
        "_ARGV" => {
            let n = indices.first().copied().unwrap_or(0);
            if n < 0 {
                return Ok(Value::Str(Vec::new()));
            }
            Ok(Value::Str(
                vm.argv().get(n as usize).cloned().unwrap_or_default(),
            ))
        }
        // 路径类:$ _SYSTEM_PATH / $_SAVE_PATH / $_APPDATA_PATH / $_PERSONAL_PATH
        // 手册有页(`basic/svariable.html`);**`$_PROJECT_PATH` 手册无页**
        // (字节级核对 0.479/0.495 两版,2026-10-19)——按同类宿主注入处理,
        // 引用出处见各自分支。宿主路径未注入 ⇒ 显式 Unimplemented。
        "_SYSTEM_PATH" | "_SAVE_PATH" | "_APPDATA_PATH" | "_PERSONAL_PATH" | "_PROJECT_PATH" => {
            let p = vm.host_paths();
            let v = match name {
                "_SYSTEM_PATH" => &p.system_path,
                "_SAVE_PATH" => &p.save_path,
                "_APPDATA_PATH" => &p.appdata_path,
                "_PERSONAL_PATH" => &p.personal_path,
                _ => &p.project_path,
            };
            match v {
                Some(bytes) => Ok(Value::Str(bytes.clone())),
                None => Err(diag::unimplemented(
                    code::rt::SYSVAR_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("{name}:宿主未注入路径(A2:内核不猜文件系统布局;DIV-RT-010)"),
                )),
            }
        }

        // ---- 输入(手册 `basic/input.html`) -------------------------------
        // 按键/鼠标 = **带符号帧计数器**(本 crate `input` 模块)。
        _ if name.starts_with("_KEY_") || name.starts_with("_MOUSE_") => {
            read_button(vm, name, indices)
        }
        // 手册:`@_KEYNUM_～ / @_MOUSENUM_～` = 押された回数。
        _ if name.starts_with("_KEYNUM_") || name.starts_with("_MOUSENUM_") => {
            let button = match name.strip_prefix("_KEYNUM_") {
                Some(rest) => format!("KEY_{rest}"),
                None => format!("MOUSE_{}", name.strip_prefix("_MOUSENUM_").unwrap_or("")),
            };
            Ok(Value::Int(vm.input().count(&button)))
        }
        // 手册:`@_INP(n)` 数组版;n = 按钮码(`@_INP(@_CODE_Z)` ≡ `@_KEY_Z`,
        // 手册 ※3)。REA:同一张 VK 计数表(`FUN_00427350` case 0x90,
        // `n<=0 → 0`)。码→按钮见 `input::button_of_code`;手柄码无注入 ⇒ 0。
        "_INP" => {
            let idx = indices.first().copied().unwrap_or(0);
            vm.input().inp(idx).map(Value::Int)
        }
        // `_KEYLAST` / `_MOUSELAST`:最近一次按下的按钮码。
        //
        // REA 0.495 `FUN_00427350` case 0x91 读 `DAT_00801908`、case 0x92 读
        // `DAT_0080190c`;写入点是窗口过程 `FUN_0045a3b4`(WM_KEYDOWN →
        // `DAT_00801908`;WM_LBUTTONDOWN/RBUTTONDOWN/MBUTTONDOWN →
        // `DAT_0080190c` = 1/2/4)。脚本用写 0 取消 skip(ERIS `es_Text` 实证)。
        "_KEYLAST" => Ok(Value::Int(vm.input().keylast())),
        "_MOUSELAST" => Ok(Value::Int(vm.input().mouselast())),
        // 手册 `basic/svariable.html`:@_CODE_〜 = 各键的按钮码。
        // 键盘/鼠标段 = 引擎按钮码 ABI(REA:`FUN_00427350` case 0x19f..0x21e
        // 经运行期字节表 → VK 计数表下标;值层证据见 `input` 模块文档);
        // 手柄段是纯算术(case 0x21f/0x223:`384 + n*36 + …`)。手册 ○ 可赋值
        // = 键位重映射,写入走 `InputState::set_code`。
        _ if name.starts_with("_CODE_") => {
            let base = name.strip_prefix("_CODE_").unwrap_or("");
            match (base, indices) {
                ("PAD_UP", [n]) | ("PAD_DOWN", [n]) | ("PAD_LEFT", [n]) | ("PAD_RIGHT", [n]) => {
                    let dir = match base {
                        "PAD_UP" => 0,
                        "PAD_DOWN" => 1,
                        "PAD_LEFT" => 2,
                        _ => 3,
                    };
                    Ok(Value::Int(crate::input::pad_dir_code(dir, *n)))
                }
                ("PAD_BUTTON", [n, m]) => Ok(Value::Int(crate::input::pad_button_code(*n, *m))),
                (_, []) => {
                    let default = vm.binding().code_of_id(var.id).ok_or_else(|| {
                        diag::fatal(
                            code::rt::SYSVAR_UNIMPLEMENTED,
                            Boundary::Script,
                            format!("{name}:工程 ycd 声明了该码变量,但引擎按钮码表无此名"),
                        )
                    })?;
                    Ok(Value::Int(vm.input().code_value(var.id, default)))
                }
                _ => Err(diag::fatal(
                    code::rt::SYSVAR_UNIMPLEMENTED,
                    Boundary::Script,
                    format!("{name}:下标形态不符(标量或 [n]/[n,m])"),
                )),
            }
        }
        // 手册:`@_TEXT` / `_TEXT_FLAG` / `_TEXT_NUM` = 当前显示文本内容/标志/字数。
        // 文本层状态机在 `TEXT`/`_` 处理器里维护;`_` 语义未取证(DIV-RT-008)⇒
        // 内容恒为空,按手册「空文本」返回空串,不猜其它字段。
        // 手册 `basic/svariable.html`:`$_TEXT` = 現在表示されているテキストの内容。
        // 缓冲由 `WORD`/`_` 追加、`$_TEXT` 写入替换(见 handlers)。
        "_TEXT" => Ok(Value::Str(vm.current_text())),
        // 手册:`$_TEXT_FLAG` = 半角全角情報や改行改頁クリック待ち情報(有页)。
        // 三层已齐(REA 0.495 `0x446b93` id 0x7b → `[state+0x3c]`;字节表见
        // `crate::text::flag`),返回与 `$_TEXT` 并行的控制字节流。
        // 台账:`COV-M-01`。
        "_TEXT_FLAG" => Ok(Value::Str(vm.text_flags())),
        // `_TEXT_NUM`:手册无页;引擎 int getter `0x4276e3` 读装载期全局
        // `0x80174c`,该值在 `ysbin/yst.ybn` 装载时由头部 text_total 写入
        // (`0x45d73f`)。内核用 `yst_list` 逐脚本 text_count 求和(ERIS 396,
        // 与 yst.ybn 头部一致;DIV-RT-048)。
        "_TEXT_NUM" => Ok(Value::Int(vm.text_total())),
        // 手册:`○@_MENUCODE` = 最後に選択されたメニュー項目の CODE 値。
        // 状态由菜单域宿主选中时推入([`Vm::set_menu_code`]);○ 可赋值。
        "_MENUCODE" => Ok(Value::Int(vm.menu_code())),
        // 手册:`○@_WINDOW_EXEC_MAX/CLOSE` = 最大化/閉じるボタン押下状態(0/1)。
        // 宿主窗口事件推入;○ 可赋值。`_WINDOW_EXEC_MIN`/`_WINDOW_INIT_POS`
        // 手册无页 ⇒ 走 undoc。
        "_WINDOW_EXEC_MAX" | "_WINDOW_EXEC_CLOSE" | "_WINDOW_EXEC_CAPTION" => Ok(Value::Int(
            vm.window_exec(name.rsplit('_').next().unwrap_or("")),
        )),
        // `_WINDOW_INIT_POS`:手册无页;引擎 int getter `0x4276f3` 读
        // `[0x80133c]`,该字节由 `yscfg.dat` 第 10 项写入(`0x460b74` 路径),
        // 由宿主注入(缺省 0;DIV-RT-047)。
        "_WINDOW_INIT_POS" => Ok(Value::Int(vm.window_init_pos())),

        // ---- 其余:手册无页或无注入来源 -----------------------------------
        _ => Err(undoc(name)),
    }
}

fn read_button(vm: &Vm, name: &str, indices: &[i64]) -> Result<Value, yu_log::Diag> {
    // `_MOUSE_X/Y` 是坐标(标量),单独处理。
    match name {
        "_MOUSE_X" => return Ok(Value::Int(vm.input().cursor().0)),
        "_MOUSE_Y" => return Ok(Value::Int(vm.input().cursor().1)),
        // `_MOUSE_X0..3`/`_MOUSE_Y0..3`:手册无页;REA 0.495 `FUN_00427350`
        // case 0x85..0x8e = 4 个窗口状态槽(槽表 `DAT_008019c0..cc`)的客户区
        // 光标坐标(每槽 HWND@+0x5b0、X@+0x5b8、Y@+0x5bc);case 0x85/0x86
        // 同槽 ⇒ `_MOUSE_X` ≡ `_MOUSE_X0`。每帧更新 `FUN_00403ee0`:槽无
        // 窗口(HWND=0)⇒ 坐标写 0,不报错(2026-10-24 政策:按 REA 实现并
        // 标注)。内核 headless 只有注入光标(槽 0)⇒ 槽 1..3 恒 0,与官方
        // 单窗口场景一致;debug UI(ERIS s66)读 `_MOUSE_X2` 由此不再 Fatal
        // (DIV-L-001 黑帧根因)。
        "_MOUSE_X0" => return Ok(Value::Int(vm.input().cursor().0)),
        "_MOUSE_Y0" => return Ok(Value::Int(vm.input().cursor().1)),
        "_MOUSE_X1" | "_MOUSE_X2" | "_MOUSE_X3" | "_MOUSE_Y1" | "_MOUSE_Y2" | "_MOUSE_Y3" => {
            return Ok(Value::Int(0))
        }
        "_KEY_ANY" => return Ok(Value::Int(vm.input().any_counter())),
        _ => {}
    }
    let button = name.trim_start_matches('_');
    // 手柄钮:码 = `384 + n*36 + …`(REA case 0x114/0x118)。手柄无宿主注入
    // ⇒ 计数器恒 0(从未按过,与「未出现的键不参与」同一约定)。
    if let Some(rest) = button.strip_prefix("PAD_") {
        let is_num = name.starts_with("_PADNUM_");
        return match (rest, indices) {
            ("UP", [n]) | ("DOWN", [n]) | ("LEFT", [n]) | ("RIGHT", [n]) => {
                let dir = match rest {
                    "UP" => 0,
                    "DOWN" => 1,
                    "LEFT" => 2,
                    _ => 3,
                };
                let code = crate::input::pad_dir_code(dir, *n);
                Ok(Value::Int(vm.input().pad_counter(code, is_num)))
            }
            ("BUTTON", [n, m]) => {
                let code = crate::input::pad_button_code(*n, *m);
                Ok(Value::Int(vm.input().pad_counter(code, is_num)))
            }
            _ => Err(diag::fatal(
                code::rt::SYSVAR_UNIMPLEMENTED,
                Boundary::Script,
                format!("{name}:下标形态不符(手柄需要 [n] 或 [n,m])"),
            )),
        };
    }
    if !indices.is_empty() {
        // `_KEY_A` 等是标量;带下标属未取证形态。
        return Err(diag::fatal(
            code::rt::SYSVAR_UNIMPLEMENTED,
            Boundary::Script,
            format!("{name} 是标量,却给了 {} 个下标", indices.len()),
        ));
    }
    let id = vm.binding().key_id(button).ok_or_else(|| {
        diag::fatal(
            code::rt::SYSVAR_UNIMPLEMENTED,
            Boundary::Script,
            format!("{name}:工程 ycd 未声明该按钮(输入绑定缺失)"),
        )
    })?;
    let _ = id;
    Ok(Value::Int(vm.input().counter(button)))
}

fn param_index(indices: &[i64]) -> Result<usize, yu_log::Diag> {
    let n = indices.first().copied().unwrap_or(1);
    if !(1..=16).contains(&n) {
        return Err(diag::fatal(
            code::rt::SYSVAR_UNIMPLEMENTED,
            Boundary::Script,
            format!("参数编号 {n} 超出 1..16(手册口径)"),
        ));
    }
    Ok((n - 1) as usize)
}

/// 注入时钟 → 日期字段。纪元基准 = 注入毫秒的原点(1970-01-01T00:00:00Z,
/// 固定 UTC,不含本地时区;官方用系统本地时间 ⇒ DIV-RT-009)。
fn time_field(ms: u32, col: i64) -> Result<Value, yu_log::Diag> {
    let secs = (ms as i64) / 1000;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d, wd) = civil_from_days(days);
    let value = match col {
        0 => y,
        1 => m,
        2 => d,
        3 => wd,
        4 => rem / 3600,
        5 => (rem % 3600) / 60,
        6 => rem % 60,
        7 => (ms as i64) % 1000,
        other => {
            return Err(diag::fatal(
                code::rt::SYSVAR_UNIMPLEMENTED,
                Boundary::Script,
                format!("_TIME 列 {other} 超出手册范围(0..7)"),
            ))
        }
    };
    Ok(Value::Int(value))
}

/// Howard Hinnant 的 `civil_from_days`(公历;固定 UTC)。
fn civil_from_days(z: i64) -> (i64, i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    // 1970-01-01 = 星期四(4)。手册:`3 = 曜日`,取值未给 ⇒ 用 0=日..6=土
    // 的确定性约定并在事件里可见(DIV-RT-009)。
    let wd = (days_since_epoch(z) + 4).rem_euclid(7);
    (y, m, d, wd)
}

fn days_since_epoch(z: i64) -> i64 {
    z - 719_468
}

/// 系统变量在工程 ycd 里存在但**手册无页** ⇒ 按任务书军规不实现。
fn undoc(name: &str) -> yu_log::Diag {
    diag::unimplemented(
        code::rt::SYSVAR_UNIMPLEMENTED,
        Boundary::Script,
        format!(
            "{name}:工程 ycd 声明了该变量,但官方手册无对应页(三层出处缺①)\
             ⇒ 不实现(DIV-RT-007)"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1, 4));
        assert_eq!(civil_from_days(1), (1970, 1, 2, 5));
    }

    #[test]
    fn time_field_columns_are_bounded() {
        assert!(time_field(0, 0).is_ok());
        assert!(time_field(0, 8).is_err());
    }
}
