//! 系统域:CLIPACT/CLIPINFO/REGINFO/ERROR/DIALOG/LABELINFO。
//!
//! 由 `handlers/mod.rs` 的命令族拆分而来;只搬运,函数体逐字不变。

use super::*;

impl Vm {

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
    pub(super) fn op_systeminfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
    pub(super) fn op_input(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let vals = self.named_command_values(st, cmd)?;
        let get = |name: &str| -> Option<&Value> {
            vals.iter()
                .rev()
                .find(|(_, n, _)| *n == name)
                .map(|(_, _, v)| v)
        };
        let flag = |name: &str| get(name).and_then(|v| v.as_int()).map(|v| v != 0);
        self.input
            .apply_input(flag("KEY"), flag("PAD"), flag("MOUSE"));
        self.emit_domain_from_values("INPUT", Domain::System, vals);
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
    pub(super) fn op_mouse(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let vals = self.named_command_values(st, cmd)?;
        let get = |name: &str| -> Option<&Value> {
            vals.iter()
                .rev()
                .find(|(_, n, _)| *n == name)
                .map(|(_, _, v)| v)
        };
        let int = |name: &str| get(name).and_then(|v| v.as_int());
        let flag = |name: &str| int(name).map(|v| v != 0);
        let id = match get("ID") {
            Some(Value::Str(bytes)) => Some(bytes.as_slice()),
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
            id,
        );
        self.emit_domain_from_values("MOUSE", Domain::System, vals);
        Ok(StepOutcome::Continue)
    }

    /// `CLIPACT[TEXT=… SET=…]`:手册 `clip/clipact.html`。
    ///
    /// REA `0x43e2d0`:**两个参数都必须存在**(`TEXT` 只判存在、不看值),
    /// 然后把 `SET` 的字节原样写入 Windows 剪贴板(CF_TEXT)。内核保存
    /// 同一份 CP932 字节镜像并同步发域事件,由宿主写入 OS 剪贴板。
    pub(super) fn op_clipact(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = std::rc::Rc::clone(&cmd.param_names);
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
    pub(super) fn op_clipinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = std::rc::Rc::clone(&cmd.param_names);
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

    /// `REGINFO`:手册 `reg/reginfo.html`;REA `0x44bd30`(2026-11-10)。
    ///
    /// 官方结构:
    /// 1. `KEY` 的 `/` 全部折成 `\`,再按 `HKEY_*\` 前缀定根(七个根);
    ///    前缀不匹配 ⇒ 直接返回 0(**不写 LET**);
    /// 2. `RegOpenKeyExA(KEY_ALL_ACCESS)` + `RegQueryValueExA`(1024 字节缓冲);
    /// 3. 失败(键/值不存在、缓冲不足)⇒ 按目标类型写 0 / 0.0 / 空串;
    /// 4. 成功 ⇒ **只有字符串目标**写值;INT/FLT 目标保持原值
    ///    (`0x44bf0a` 只判 `type==3`;手册例 1 的 DWORD→@A 与处理体不一致,
    ///    按处理体对齐并记入 DIV-RT-045)。
    pub(super) fn op_reginfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = std::rc::Rc::clone(&cmd.param_names);
        let mut key: Option<Vec<u8>> = None;
        let mut name: Option<Vec<u8>> = None;
        let mut target: Option<LValue> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or(""); // silent-ok: 槽号越界 = 无名,按官方未读参数处理
            match pname {
                "KEY" => {
                    if let Value::Str(s) = self.eval_window(st, w)? {
                        key = Some(s);
                    }
                }
                "NAME" => {
                    if let Value::Str(s) = self.eval_window(st, w)? {
                        name = Some(s);
                    }
                }
                "LET" => target = Some(self.eval_lvalue_window(st, w)?),
                // 官方处理体只按固定槽位读 KEY/NAME/LET;额外参数即使存在
                // 也没有分支消费(0x44bd30 全函数只引用这三个槽的标志字节)。
                // 不能报「未实现」——换版本/换工程多出参数时会被误判。
                _other => {}
            }
        }
        // LET 缺席 ⇒ 官方整条命令无写回。
        let Some(target) = target else {
            return Ok(StepOutcome::Continue);
        };
        let ty = self
            .var_shape(target.var)
            .map(|(t, _)| t)
            .unwrap_or(ElemType::Int); // silent-ok: 未声明目标按 INT 默认(官方按槽类型写默认值)
        let default = match ty {
            ElemType::Int => Value::Int(0),
            ElemType::Float => Value::Float(0.0),
            ElemType::Str => Value::Str(Vec::new()),
        };

        // KEY 缺席:官方仍调 `RegOpenKeyExA(0, 0, …)` ⇒ 失败 ⇒ 默认值。
        let Some(mut key) = key else {
            self.write_target(st, &target, &default)?;
            return Ok(StepOutcome::Continue);
        };
        for b in &mut key {
            if *b == b'/' {
                *b = b'\\';
            }
        }
        const HIVES: &[(&[u8], &str)] = &[
            (b"HKEY_CLASSES_ROOT\\", "HKEY_CLASSES_ROOT"),
            (b"HKEY_CURRENT_USER\\", "HKEY_CURRENT_USER"),
            (b"HKEY_LOCAL_MACHINE\\", "HKEY_LOCAL_MACHINE"),
            (b"HKEY_USERS\\", "HKEY_USERS"),
            (b"HKEY_PERFORMANCE_DATA\\", "HKEY_PERFORMANCE_DATA"),
            (b"HKEY_CURRENT_CONFIG\\", "HKEY_CURRENT_CONFIG"),
            (b"HKEY_DYN_DATA\\", "HKEY_DYN_DATA"),
        ];
        let matched = HIVES.iter().find_map(|(prefix, hive)| {
            (key.len() >= prefix.len() && key[..prefix.len()].eq_ignore_ascii_case(prefix))
                .then_some((*hive, prefix.len()))
        });
        // 前缀不匹配:官方直接返回 0,不写 LET。
        let Some((hive, prefix_len)) = matched else {
            return Ok(StepOutcome::Continue);
        };
        let subkey = &key[prefix_len..];
        let value = self
            .registry_host
            .as_mut()
            .and_then(|h| h.read_value(hive, subkey, name.as_deref()));
        match value {
            None => self.write_target(st, &target, &default)?,
            Some(crate::event::RegistryValue::String(bytes)) => {
                if ty == ElemType::Str {
                    self.write_target(st, &target, &Value::Str(bytes))?;
                }
            }
            Some(crate::event::RegistryValue::Dword(raw)) => {
                // 官方把 4 字节原始值写进同一缓冲;字符串目标按 C 串截断。
                if ty == ElemType::Str {
                    let mut bytes = raw.to_le_bytes().to_vec();
                    if let Some(nul) = bytes.iter().position(|&b| b == 0) {
                        bytes.truncate(nul);
                    }
                    self.write_target(st, &target, &Value::Str(bytes))?;
                }
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `ERROR[NO=… SET=… COM_CG=… …]`:REA `0x46358c`。
    ///
    /// 官方处理体只有一个可观测副作用:参数槽 2 起(`COM_<命令名>`)的
    /// 值按**命令序号**写进 `[0x689b00 + opcode]` 的「逐命令启用表」;
    /// 引擎初始化时把 128 字节全置 1(`0x462124`)。该表的消费者未定位,
    /// 所以内核只保存、不据此改变分派(DIV-RT-046)。
    pub(super) fn op_error(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params = std::rc::Rc::clone(&cmd.param_names);
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
    ///
    /// 诊断/工具入口(解释器热路径已改走 `Cmd::param_names` 缓存)。
    #[allow(dead_code)]
    pub(super) fn param_names(&self, opcode: u8) -> Vec<String> {
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
    pub(super) fn op_dialog(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 旧写法每次执行都重建整张 `Vec<String>`(与 `CGINFO` 同一个 bug)。
        let params: &[String] = &cmd.param_names;
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
        // 官方处理体不读取的声明参数:不猜语义、不求值,只记名字。
        let mut ignored_params: Vec<String> = Vec::new();
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
                    // `STR2..8` / `A*` / `E*` / `CHECK*` / `FILEOPEN` /
                    // `FILEEXT` / `FOLDER` / `CHECKBOX` / `SLIDER` / `NOWAIT`:
                    // 官方 0.495 `0x43fc74` + 过程 `0x4403d0`(0.479
                    // `0x439cd8` 同构)对这些槽位**零读取**,也不求值表达式。
                    // 官方行为 = 接受命令并忽略;内核不生成假控件、不写回。
                    ignored_params.push(if other.is_empty() {
                        format!("@{}", w.rec.kind)
                    } else {
                        other.to_string()
                    });
                }
            }
        }
        // 事件是事实通知;权威状态在 `Vm::pending_dialogs`(不可重放)。
        // 被忽略的扩展参数不做求值,因此不会进入 event_args(官方同样不读)。
        self.emit_domain(Domain::System, "DIALOG", event_args);
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
        let codec = self.script_text_codec(st.script_id);
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
            ignored_params,
            codec,
        });
        Ok(StepOutcome::Yield)
    }

    /// `LABELINFO[# LET EXIST]`:手册 `other/labelinfo.html`。
    pub(super) fn op_labelinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
                // 只判存在性:走无克隆的 `find_label_target`(#46)。
                let found = self.find_label_target(&name).is_some();
                self.write_target(st, &t, &Value::Int(found as i64))?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `SYSTEM[SX,SY]`(555 商业族;官方 0.500 处理体 `0x47f2f4`)。
    ///
    /// 官方分支:`SX`/`SY` 同时 >0 时写引擎屏幕尺寸(`DAT_008925c4/c8`
    /// 与显示上下文 `+0xc/+0x10`),并在窗口标志活跃时重建后缓冲。
    /// 本内核只同步 VM 侧事实(`_SCREEN_SIZE_X/Y` 的读取源);后缓冲重建
    /// 归宿主端口(当前未接线,见 `DIV-RT-079`)。`SCREENSAVER`/
    /// `BACKGROUNDEXEC`/`TIMECOUNT*` 是宿主策略/计时,由通用域事件承载。
    ///
    /// 只挂在 `Domain::System` 的通用路径**之前**:之后仍照常发 `DomainCall`,
    /// 事件口径不变;表达式会被求值两次(无副作用,保持改动面最小)。
    pub(super) fn apply_system_screen(&mut self, st: &mut ExecState, cmd: &Cmd) {
        let names: &[String] = &cmd.param_names;
        let mut sx: Option<i64> = None;
        let mut sy: Option<i64> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            if name != "SX" && name != "SY" {
                continue;
            }
            match self.eval_window(st, w) {
                Ok(v) => {
                    if name == "SX" {
                        sx = v.as_int();
                    } else {
                        sy = v.as_int();
                    }
                }
                Err(d) => self.report(d),
            }
        }
        let (Some(sx), Some(sy)) = (sx, sy) else {
            return;
        };
        if sx <= 0 || sy <= 0 {
            return;
        }
        let (width, height) = (sx as u32, sy as u32);
        if self.screen() == (width, height) {
            return;
        }
        self.set_screen(width, height);
        self.report(diag::verbose(
            code::rt::SYSTEM_TRACE,
            Boundary::Script,
            format!("SYSTEM[SX={width} SY={height}]:引擎屏幕尺寸已更新(后缓冲重建归宿主)"),
        ));
    }
}
