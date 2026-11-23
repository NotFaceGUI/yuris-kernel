//! 文本域:TEXT/TEXTINFO/TEXT_OUT/WORD/RETURNCODE。
//!
//! 由 `handlers/mod.rs` 的命令族拆分而来;只搬运,函数体逐字不变。

use super::helpers::*;
use super::*;

impl Vm {

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
    /// `FONT`:把文本状态子集(`+0x20..+0x34`)按参数名写入。
    ///
    /// 槽序来自工程 ysc(`probe_cmd_params` 实测 `0 BNO 1 NO 2 NAME
    /// 3 ANTIALIASING 4 COLOR 5 COLOR2 6 GRADATION 7 ROTATION
    /// 8 DRAWMODE 9 SX 10 SY 11 BOLD`);`NO/NAME/ROTATION` 归字体对象,
    /// 文本状态只保存 `+0x20..+0x34`。
    pub(super) fn apply_text_font(&mut self, cmd: &Cmd, args: &[ArgValue]) {
        let names = &cmd.param_names;
        for a in args {
            let Some(name) = names.get(usize::from(a.slot)).map(String::as_str) else {
                continue;
            };
            let mut v = a.value.as_int().unwrap_or(0); // silent-ok: FONT 值槽非整数按 0(官方 checker 已拒绝)
            // `FONT[SX+=…]`/`[COLOR-=…]`:官方参数算子对当前状态做
            // `current + v`(`-=` 的值已在 compose_window_arg 取负)。
            if a.offset {
                if let Some(current) = self.text.font_param(name) {
                    v = current.wrapping_add(v);
                }
            }
            self.text.apply_font_param(name, v);
        }
    }

    pub(super) fn op_text(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        use crate::text::flag as tflag;
        // 参数名来自工程 ysc 表(opcode → 命令声明),不写死槽号。
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 这里只存槽号 + 值,`get` 返回借用(#82:不再逐次 `Value::clone`)。
        let names: &[String] = &cmd.param_names;
        let mut vals: smallvec::SmallVec<[(u8, Value); 8]> = smallvec::SmallVec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let v = self.eval_window(st, w)?;
            vals.push((w.rec.kind, v));
        }
        let get = |n: &str| -> Option<&Value> {
            let slot = names.iter().position(|name| name == n)? as u8;
            vals.iter()
                .rev()
                .find(|(s, _)| *s == slot)
                .map(|(_, v)| v)
        };

        // p0 ID:目标文本层(CP932 原始字节)。
        // REA 0x453467 求值到字节缓冲后交 0x414559 解析图层:MurmurHash2 +
        // 逐字节比较,不校验 UTF-8;非 ASCII ID 合法(手册 text/text.html ID)。
        // 未命中:官方 0x4538a7 报错并返回 -1,**不更新 state+0x00**(保留
        // 旧层),同一命令后续参数全部跳过。探针未安装(纯 VM 单测)时按
        // 旧行为接受,由宿主判定。
        if let Some(Value::Str(b)) = get("ID") {
            let id = b.clone();
            match self.text_layer_probe.as_mut() {
                Some(probe) => match probe(&id) {
                    Some((w, h)) => {
                        if self.text.layer_id != id {
                            // 官方只换 `state+0x00`;旧层像素留在旧层。
                            self.text.reset_surface_epoch();
                            self.text.layer_id = id;
                        }
                        self.text.set_window(Some((w, h)));
                    }
                    None => {
                        self.report(diag::recoverable(
                            code::rt::TEXT_LAYER,
                            Boundary::Script,
                            format!(
                                "TEXT[ID] 未命中已存在层 {:?};\
                                 官方 FUN_00414559 报错并返回 -1(保留旧层,\
                                 后续参数跳过)",
                                String::from_utf8_lossy(&id)
                            ),
                        ));
                        return Ok(StepOutcome::Yield);
                    }
                },
                None => self.text.layer_id = id,
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
            self.text.push_flag_sized(tflag::CLICK);
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
                // 官方 TEXT 的 RETURN/PAGE/CLICKRETURN/CLICKPAGE 在追加前
                // 写宽/高表(0x4536b5/0x453714/0x453765/0x4537c4)。
                self.text.push_flag_sized(flag);
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
    pub(super) fn op_textinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 旧写法每次执行都重建整张 `Vec<String>`(与 `CGINFO` 同一个 bug)。
        let names: &[String] = &cmd.param_names;
        let mut target: Option<crate::expr::LValue> = None;
        // 只存在场槽号(#82:不再逐窗口 clone 名字)。
        let mut present: smallvec::SmallVec<[u8; 8]> = smallvec::SmallVec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match names.get(usize::from(w.rec.kind)).map(String::as_str) {
                Some("LET") => target = Some(self.eval_lvalue_window(st, w)?),
                Some("") | None => {}
                // 引擎只检查槽位是否存在(0x4538fa..),不求值查询标志本身。
                Some(_) => present.push(w.rec.kind),
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
            let slot = names
                .iter()
                .position(|n| n == name)
                .and_then(|index| u8::try_from(index).ok());
            if !slot.is_some_and(|slot| present.contains(&slot)) {
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
    pub(super) fn op_text_out(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            // `_` 的表达式值已是 `Str` 时直接搬字节(#81)。
            let bytes = into_str_bytes(self.eval_window(st, w)?)?;
            self.append_text_for_script(st.script_id, &bytes);
            self.emit(VmEvent::Text {
                frame: self.frame(),
                id: self.text.layer_id.clone(),
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
    pub(super) fn op_word(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let _ = st;
        let Some(w) = cmd.windows.first() else {
            return Err(
                diag::fatal(code::rt::CMD_WINDOWS, Boundary::Script, "WORD 缺文本窗口").into(),
            );
        };
        self.append_text_for_script(st.script_id, &w.payload);
        self.emit(VmEvent::Text {
            frame: self.frame(),
            id: self.text.layer_id.clone(),
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
    /// **v555 变体**(REA `0x44b920`/`0x44b9e4`/`0x44b824`,三族同型):
    /// 不追加 C/R/P,只写 `'E'`,并在写前**无条件**置清缓冲请求
    /// `state+0x15 = 1`;释放路径 `FUN_004047c4` → `FUN_0046b32c`
    /// 清 `_TEXT`/`_TEXT_FLAG` 与内部写位。由
    /// [`SemanticProfile::returncode`](crate::semantics::SemanticProfile)
    /// 精确门控,旧版行为不变。
    ///
    /// 台账:`COV-M-02`。
    pub(super) fn op_returncode(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        use crate::text::flag as tflag;
        // 引擎 `0x44c390` 在进入文本模式分支前先把当前指令首窗口记录的两个
        // dword 抄进任务记录:`task+0xc` = 首 dword(窗口 `id | type<<16`),
        // `task+0x10` = 次 dword(窗口 `length`)。`TASKINFO[TEXTLINE/NEXTVOICE]`
        // (`0x45272a/0x452708`)随后原样读回(两者都按有符号 dword 扩展)。
        // 事件里的行号用窗口 **u16 id**(低字节 `kind` + 高字节 `flag`),
        // 不能用 u8 `kind` —— 单脚本文本行超过 255 时会截断。
        let (line_id, packed_line, next_voice) = match cmd.windows.first() {
            Some(w) => {
                let id = u32::from(w.rec.kind) | (u32::from(w.rec.flag) << 8);
                (id, id | (u32::from(w.rec.slot) << 16), w.rec.length)
            }
            None => (0, 0, 0),
        };
        st.text_line = i64::from(packed_line as i32);
        st.next_voice = i64::from(next_voice as i32);

        match self.semantics().returncode {
            crate::semantics::ReturnCodeMode::Legacy => {
                if self.text.output_mode != 0 {
                    // 官方 0x44c39x:'C'/'R' 在追加前写宽/高表;'P' 与 'E' 不写。
                    if self.text.autoclick != 0 {
                        self.text.push_flag_sized(tflag::CLICK);
                    }
                    if self.text.autoreturn != 0 {
                        self.text.push_flag_sized(tflag::RETURN);
                    }
                    if self.text.autopage != 0 {
                        self.text.push_flag(tflag::PAGE);
                    }
                } else {
                    self.text.clear_pending = true;
                }
            }
            crate::semantics::ReturnCodeMode::EndOnly => {
                // v555(处理体 0x44b920/0x44b9e4/0x44b824,三族同型):
                // **只**追加 'E',并在追加前无条件置清缓冲请求
                // `state+0x15 = 1`。C/R/P 由旧代 stdlib 的
                // `es.TX.OUTPUT.LINEEND` 在脚本侧插入,引擎不写。
                //
                // 该请求在文本任务下次被调度时由释放路径消费
                // (`FUN_004047c4` → `FUN_0046b32c`:清 `_TEXT`/`_TEXT_FLAG`
                // 与内部写位)。缺这一步会让标志流跨行累加,旧代 stdlib 的
                // 逐字符尺寸循环按 `_TEXT_FLAG` 序号扫描时错位/越界
                // (`テキスト内部エラー` + END)。
                self.text.clear_pending = true;
            }
        }
        self.text.push_flag(tflag::END);
        self.emit(VmEvent::ReturnCode {
            frame: self.frame(),
            line: line_id,
        });
        Ok(StepOutcome::TextYield)
    }
}
