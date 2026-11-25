//! 变量域:LET/DECLARE/VARINFO/VARACT/MATH/TASK/TASKINFO 与表达式写回。
//!
//! 由 `handlers/mod.rs` 的命令族拆分而来;只搬运,函数体逐字不变。

use super::helpers::*;
use super::*;

impl Vm {

    // ---------------------------------------------------------- 变量

    /// `LET`:手册 `variable/let.html`;引擎 §3.17(opcode 54)+ §3.10。
    ///
    /// 复合赋值码在窗口 `slot` 高字节(语料实测 0..8;`tag>>24` 同源)。
    pub(super) fn op_let(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() != 2 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!("LET 窗口数 {} != 2", cmd.windows.len()),
            )
            .into());
        }
        let mut lval = self.eval_lvalue_window(st, &cmd.windows[0])?;
        // ERIS 旧式空间按 id 的声明类型归一化;其它未知前缀挂号继续(DIV-RT-017)。
        // 就地改写:此处原本重建一个 LValue 并克隆 indices,而标量写入
        // (99% 的 LET)根本不需要下标表。
        lval.var = self.canonical_var(lval.var);
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
            // 启动期已建 id→按钮码表(#12):命中即按码写入,不再每次
            // `format!("KEY_…")` 再走 113 项 ABI 表线性扫描。`LET` 是单帧
            // 7 万次的热路径。
            if let Some(code) = self.binding().key_code_of_id(lval.var.id) {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                self.input_state_mut()
                    .set_counter_code(i64::from(code), v);
                return Ok(StepOutcome::Continue);
            }
            // 按钮族:按按钮名写同一张帧计数器表。官方 v555 LET
            // `0x4441e5`(id 0x93..0x113)按映射表取下标后调用
            // `FUN_00444750`;该函数按当前物理 down 状态钳制(与
            // `InputState::Counter::write` / 0.495 `FUN_00447014` 同型)。
            // ABI 表里没有的按钮族名(`_KEY_ANY` 等)仍保持求值后不写。
            let button_family = {
                let name = self
                    .sysvars()
                    .get(lval.var.id)
                    .map(|d| d.name.as_str())
                    .unwrap_or("");
                button_family_rest(name).is_some()
            };
            if button_family {
                let button_name = self
                    .sysvars()
                    .get(lval.var.id)
                    .map(|d| d.name.trim_start_matches('_').to_string())
                    .unwrap_or_default();
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.to_int_lossy();
                self.input_state_mut().set_counter(&button_name, v);
                return Ok(StepOutcome::Continue);
            }
            // 其余分支需要完整名字串(数量少,不在热路径):
            // 只有走到这里才 clone。
            let name: String = self
                .sysvars()
                .get(lval.var.id)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            // `@_INP(n)`(手册 `basic/svariable.html` ○ 数组版)= 按引擎
            // 按钮码写**同一张** VK 计数表。REA 0x495 `0x446efc`(id 0x90)
            // → `0x447014`(下标 `n<=0` 忽略;按物理按下状态钳制,与
            // `_KEY_*` 同族)。ERIS `es_Button.yst:3417` 用 `=2` 消费一次
            // 按下、`es_Sprite.yst:851..854` 用 `=11` 限制连发。
            if name == "_INP" {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0); // silent-ok: RHS 非整数按 0(官方同族)
                let code = lval.indices.first().copied().unwrap_or(0); // silent-ok: 缺下标 = 0,setter 忽略
                self.input_state_mut().set_counter_code(code, v);
                return Ok(StepOutcome::Continue);
            }
            // `@_KEYLAST` / `@_MOUSELAST`(手册 ○)= 直接覆盖最近按下码
            // (REA `0x446f52`/`0x446f3c` 写 `DAT_00801908/0x80190c`,
            // 无钳制)。语料写 0 取消 skip(`es_Text.yst:1287`)。
            if name == "_KEYLAST" || name == "_MOUSELAST" {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0); // silent-ok: RHS 非整数按 0(官方同族)
                self.input_state_mut().set_last(name == "_KEYLAST", v);
                return Ok(StepOutcome::Continue);
            }
            // `@_MENUCODE` / `@_WINDOW_EXEC_MAX/MIN/CLOSE/CAPTION` = 状态覆盖
            // (MAX/CLOSE 手册 ○;MIN/CAPTION 手册无页)。MIN 的官方写侧确实
            // 存在(0.495 `0x446fa4` `MOV byte ptr [0x80191c],DL`),
            // 按 REA 落地,`fixed(no-manual)`。
            if name == "_MENUCODE" {
                let rhs = self.eval_window(st, &cmd.windows[1])?;
                let v = rhs.as_int().unwrap_or(0);
                self.set_menu_code(v);
                return Ok(StepOutcome::Continue);
            }
            if name == "_WINDOW_EXEC_MAX"
                || name == "_WINDOW_EXEC_MIN"
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
                let bytes = into_str_bytes(rhs)?;
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
                let bytes = into_str_bytes(rhs)?;
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
            // v555 商业运行面专属:`_DROPFILE_NUM` / `_INT` / `_PINT`。
            // 这些名字只由 `other` 运行面声明;旧版走到这里仍是挂号。
            if self.other.is_some() {
                // 官方 `0x443988` 的 int 分支 id 0x0f:直接把 RHS 低 32 位
                // 写进 `DAT_008724ac`(不按物理状态钳制)。
                if name == "_DROPFILE_NUM" {
                    let rhs = self.eval_window(st, &cmd.windows[1])?;
                    // 官方 0x443988 int 分支 id 0x0f 只写低 32 位
                    // (`DAT_008724ac = uVar16`,32 位存储);值域按 i32 保负,
                    // 与读侧 `i64::from(i32)` 同口径。
                    self.note_lossy_int_rhs("_DROPFILE_NUM", &rhs);
                    let v = rhs.to_int_lossy();
                    if let Some(state) = self.other.as_mut() {
                        state.drop_file_num = v as i32;
                    }
                    return Ok(StepOutcome::Continue);
                }
                // 官方 id 0x32:`_INT(槽号[,元素])` 是动态变量写入。
                // 目标槽必须已声明为 int(否则 `0x1a6c6`),然后按
                // LET 复合赋值码写变量表。
                if name == "_INT" {
                    let (slot, index) = match lval.indices.as_slice() {
                        [slot] => (*slot, 0i64),
                        [slot, index] => (*slot, *index),
                        _ => {
                            self.report(diag::recoverable(
                                code::rt::SYSVAR_WRITE,
                                Boundary::Script,
                                format!(
                                    "_INT 写入需要 1..2 个下标(槽号[,元素]),得到 {} 个",
                                    lval.indices.len()
                                ),
                            ));
                            return Ok(StepOutcome::Continue);
                        }
                    };
                    // 官方片段只证据了目标变量类型检查(0x1a6c6);槽号
                    // 是否有 1000 下界未取证,不做未证实的预检。只保留
                    // ABI 宽度校验(变量 id 是 u16),其余交给变量表:
                    // 未声明走 VAR_UNDECLARED,不静默丢写。
                    if !(0..=i64::from(u16::MAX)).contains(&slot) {
                        self.report(diag::recoverable(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            format!("_INT 目标槽 {slot} 超出变量 id 宽度 0..={}", u16::MAX),
                        ));
                        return Ok(StepOutcome::Continue);
                    }
                    let Some(var) = self.store().find_by_id(slot as u16, VarSpace::At) else {
                        self.report(diag::recoverable(
                            code::rt::VAR_UNDECLARED,
                            Boundary::Script,
                            format!("_INT 目标变量 @{slot} 未声明(官方 0x1a6c6)"),
                        ));
                        return Ok(StepOutcome::Continue);
                    };
                    let Some(shape) = self.store().slot(var) else {
                        self.report(diag::recoverable(
                            code::rt::VAR_UNDECLARED,
                            Boundary::Script,
                            format!("_INT 目标变量 @{slot} 未声明(官方 0x1a6c6)"),
                        ));
                        return Ok(StepOutcome::Continue);
                    };
                    if shape.ty != ElemType::Int {
                        self.report(diag::recoverable(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!("_INT 目标 @{slot} 不是 int(官方 0x1a6c6)"),
                        ));
                        return Ok(StepOutcome::Continue);
                    }
                    let scalar = shape.dims.is_empty();
                    // 官方槽表把标量当 1 元素:无第二下标时元素 0 合法。
                    let write_index: &[i64] = if scalar {
                        if index != 0 {
                            self.report(diag::recoverable(
                                code::rt::VAR_SHAPE,
                                Boundary::Script,
                                format!("_INT 目标 @{slot} 是标量,元素下标 {index} 越界"),
                            ));
                            return Ok(StepOutcome::Continue);
                        }
                        &[]
                    } else {
                        core::slice::from_ref(&index)
                    };
                    let rhs = self.eval_window(st, &cmd.windows[1])?;
                    self.note_lossy_int_rhs("_INT", &rhs);
                    let rhs = Value::Int(rhs.to_int_lossy());
                    let op = (cmd.windows[0].rec.slot >> 8) as u8;
                    let cur = self.store().read(var, write_index).unwrap_or(Value::Int(0));
                    let value = if op == 0 {
                        rhs
                    } else {
                        compound(op, &cur, &rhs)?
                    };
                    self.store.write(var, write_index, &value)?;
                    return Ok(StepOutcome::Continue);
                }
                // 官方 id 0x35:`_PINT(n) = v` 写当前 GOSUB 帧的 int 参数
                // 数组 `+0x10 + (n-1)*8`,同样按 LET 复合赋值码。
                if name == "_PINT" {
                    // 官方缺参形态未取证:不默认 1,显式报可恢复诊断。
                    let Some(&n) = lval.indices.first() else {
                        self.report(diag::recoverable(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            "_PINT 写入缺参数号(官方缺参行为未取证,不猜)",
                        ));
                        return Ok(StepOutcome::Continue);
                    };
                    if !(1..=16).contains(&n) {
                        self.report(diag::recoverable(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            format!("_PINT 参数编号 {n} 超出 1..16"),
                        ));
                        return Ok(StepOutcome::Continue);
                    }
                    if lval.indices.len() > 1 {
                        self.report(diag::recoverable(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            format!(
                                "_PINT 写入只接受 1 个参数号,得到 {} 个下标",
                                lval.indices.len()
                            ),
                        ));
                        return Ok(StepOutcome::Continue);
                    }
                    let idx = (n - 1) as usize;
                    let task = self.current_task().to_string();
                    let frame = self
                        .other
                        .as_ref()
                        .and_then(|s| s.gosub_frames.get(&task))
                        .and_then(|f| f.last());
                    let Some(frame) = frame else {
                        self.report(diag::recoverable(
                            code::rt::SYSVAR_WRITE,
                            Boundary::Script,
                            "_PINT 写入时不在 GOSUB 帧内(官方 0x18b1e)",
                        ));
                        return Ok(StepOutcome::Continue);
                    };
                    let cur = frame.args_int[idx].clone().unwrap_or(Value::Int(0));
                    let rhs = self.eval_window(st, &cmd.windows[1])?;
                    self.note_lossy_int_rhs("_PINT", &rhs);
                    let rhs = Value::Int(rhs.to_int_lossy());
                    let op = (cmd.windows[0].rec.slot >> 8) as u8;
                    let value = if op == 0 {
                        rhs
                    } else {
                        compound(op, &cur, &rhs)?
                    };
                    if let Some(state) = self.other.as_mut() {
                        if let Some(frame) =
                            state.gosub_frames.get_mut(&task).and_then(|f| f.last_mut())
                        {
                            frame.args_int[idx] = Some(value);
                        }
                    }
                    return Ok(StepOutcome::Continue);
                }
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
        if code == 0 {
            // RHS 已是 owned:串槽赋值直接搬字节(#80)。
            self.store.write_owned(lval.var, &lval.indices, rhs)?;
        } else {
            let cur = self.read_lvalue(&lval)?;
            let value = compound(code, &cur, &rhs)?;
            self.store.write(lval.var, &lval.indices, &value)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// `INT/FLT/STR/S_INT/S_FLT/S_STR`:手册 `variable/*.html`。
    ///
    /// 声明窗口 = 变量引用(数组时末条 `0x29` 前是各维长度);
    /// 初始值窗口 = 表达式(缺省 = 类型零值,手册「初期値を省略した場合０」)。
    pub(super) fn op_declare(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
        let dims: smallvec::SmallVec<[usize; 8]> =
            target.iter().map(|&d| d.max(0) as usize).collect();
        let init = match cmd.windows.get(1) {
            Some(w) if !w.payload.is_empty() => {
                // v555: `INT @p = $array[0]` 用作「取字符串数组的槽号」。
                // 官方剧本加载器 (es_scenario_load.yst pc10) 把三份行
                // 数组的槽号存进 INT,再用 `_STR(slot, i)` 合并;若按
                // 字符串值解析会得到 0/指针,merge 直接失效。
                // 只改**字符串数组引用**;数值数组仍按值求。
                let string_array_ref = ty == ElemType::Int
                    && self.semantics().ref_int_param
                        == crate::semantics::RefIntParamMode::SlotId
                    && w.lvalue_plan.is_some();
                if string_array_ref {
                    let lv = self.eval_lvalue_window(st, w)?;
                    let slot_ty = self.store.slot(lv.var).map(|s| s.ty);
                    let is_str_array = slot_ty.is_some_and(|t| matches!(t, ElemType::Str));
                    if !lv.indices.is_empty() && is_str_array {
                        self.report(diag::verbose(
                            code::rt::PARAM_SLOT_TRACE,
                            Boundary::Script,
                            format!(
                                "INT 声明引用→槽号:{} 下标{:?} → id {}",
                                lv.var.display(),
                                lv.indices,
                                lv.var.id
                            ),
                        ));
                        Value::Int(i64::from(lv.var.id))
                    } else {
                        self.eval_window(st, w)?
                    }
                } else {
                    self.eval_window(st, w)?
                }
            }
            _ => ty.default_value(),
        };
        // 复用已有槽存储:同型同维时只重置元素值(可观察状态与 `declare`
        // 一致,见 `VariableStore::reset_declared` 文档);循环里反复声明同一
        // 变量时不再每轮重建两个 `Vec` + 一次哈希重插。
        let decl_target = target_var(&cmd.windows[0])?;
        self.store
            .reset_declared(decl_target, ty, &dims, &init);
        Ok(StepOutcome::Continue)
    }

    /// 声明命令的变量窗口:引用 + 可选各维长度。
    pub(super) fn decl_target(
        &mut self,
        st: &ExecState,
        w: &CmdWindow,
    ) -> Result<smallvec::SmallVec<[i64; 4]>, VmError> {
        // 单条变量指令 = 标量;含 `0x29` = 数组(前导值 = 各维长度)。
        let lv = self.eval_lvalue_window(st, w)?;
        Ok(lv.indices)
    }

    /// `VARINFO`:手册 `variable/varinfo.html`;引擎 §3.21。
    pub(super) fn op_varinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表用 `cmd.param_names`(按调用点缓存一次的 `Rc<[String]>`)。
        // 旧写法每次执行都重新收集 `Vec<String>`:`VARINFO` 单帧可达 2 万次
        // × 20+ 个 `String` 克隆,纯堆分配(与 `op_info` 同一个 bug)。
        let varinfo_names: &[String] = &cmd.param_names;
        let mut target: Option<crate::expr::LValue> = None;
        let mut set: Option<crate::expr::LValue> = None;
        let mut op: Option<(u8, i64)> = None;
        // `UTF8CODE` 在官方 0.495 处理体**不读**(`0x456050..0x456ae4` 对
        // `0x684170` 零引用),只是 ysc 表里的惰性参数:若脚本只给它,
        // 官方是什么都不做。按 `aligned-stub` 对齐并留一条 Info。
        //
        // 但 2025 年本地化发行版(Kagura 系,0.481 定制 ysc)的
        // `data/script/eris/全角文字表示処理.txt` **真的用它**:
        // `VARINFO[UTF8CODE=1, SET=$char, LET=@code]` 取首字符的 UTF-8
        // 字节(脚本拿 0xE2809C/0xEFBC81 这类"打包 UTF-8 字节"做比较,
        // 再映射到 `cgsys/font/font_u22/fnt_s22_nNNN` 字形页)。UTF-8
        // 路线下必须实现,否则所有字符都落到同一占位格。
        //
        // `SJISCODE` 不同:v555 处理体 `0x455200` 的 `0x4554a9` 分支会读
        // `SET` 字符串首字符并写 `LET`(单字节 = 字节值;双字节 =
        // `(lead << 8) | trail`,`0x455b9c` 实证);555 的剧本词法器
        // `es_scenario_lexer.yst` 全靠它做字符分类。因此它是**真查询项**,
        // 不再按惰性参数吞掉。跨版本门控由 `SemanticProfile` 决定
        // (`Inert` = 0.495 及更早;`FirstChar` = v555 起)。
        let sjis_inert = self.semantics().varinfo_sjis == crate::semantics::SjisCodeMode::Inert;
        // UTF8CODE 只在 UTF-8 执行路线启用;CP932/GBK/Big5 保持官方 0.495
        // 的惰性 no-op,不改变既有游戏行为。
        let utf8_active = self.script_text_codec(st.script_id) == yu_format::Codec::Utf8;
        let mut inert_stub: Option<&'static str> = None;
        // 全部数值槽(SEARCH 的针值与 `NO=` 起始下标要用)。
        let mut flags: smallvec::SmallVec<[(u8, i64); 8]> = smallvec::SmallVec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => set = Some(self.eval_lvalue_window(st, w)?),
                1 => target = Some(self.eval_lvalue_window(st, w)?),
                slot => {
                    let v = self.eval_window_ref(st, w)?;
                    if let Some(n) = v.as_int() {
                        flags.push((slot, n));
                        // 「查询项」= 置位的旗标参数,**排除辅助参数**
                        // (`INT`/`FLT`/`STR` 是 SEARCH 的针值,`NO` 是起始
                        // 下标,`NO2` 是 STRFIRST 的结束下标)。按名字排除,
                        // 不按槽号区间 —— 后者的区间在版本间会位移
                        // (`17..=20` 在 v494 上是错的)。
                        let name = varinfo_names.get(usize::from(slot)).map(String::as_str);
                        let is_helper = matches!(name, Some("INT" | "FLT" | "STR" | "NO" | "NO2"))
                            || (name == Some("UTF8CODE") && !utf8_active)
                            || (sjis_inert && name == Some("SJISCODE"));
                        if n != 0 {
                            match name {
                                Some("SJISCODE") if sjis_inert => {
                                    inert_stub = Some("VARINFO.SJISCODE")
                                }
                                Some("UTF8CODE") if !utf8_active => {
                                    inert_stub = Some("VARINFO.UTF8CODE")
                                }
                                // 555:`NO2` 是 `STRFIRST` 的结束下标从属参数;
                                // 单独出现时无可查询项,按对齐 no-op(不 fatal)。
                                Some("NO2") => inert_stub = Some("VARINFO.NO2"),
                                _ => {}
                            }
                        }
                        if op.is_none() && n != 0 && !is_helper {
                            op = Some((slot, n));
                        }
                    }
                }
            }
        }
        let (slot, _) = match op {
            Some(op) => op,
            None => {
                if let Some(name) = inert_stub {
                    let detail = if name == "VARINFO.NO2" {
                        "VARINFO: NO2 是 STRFIRST 的结束下标从属参数,单独出现时\
                         没有查询项,按无操作对齐(v555 REA 0x455200)"
                    } else {
                        "VARINFO: 官方 0.495 处理体不读该槽(0x456050..0x456ae4 对 \
                         0x68416f/0x684170 零引用),按无操作对齐(no-manual;REA)"
                    };
                    self.report_aligned_stub(name, detail);
                    return Ok(StepOutcome::Continue);
                }
                return Err(diag::fatal(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "VARINFO 无查询项(手册:至少给 TYPE/LENGTH/DIMNUM/DIMSIZE*)",
                )
                .into());
            }
        };
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
        // 查询项按**参数名**分派。槽号在版本间会整体位移(`VARINFO` 在
        // v494/0.495 之间就移过位),而参数名稳定;写死数字会让某一边
        // 静默落到错误分支。
        let query = varinfo_names.get(usize::from(slot)).map(String::as_str);
        let mut result_val = match query {
            Some("TYPE") => Value::Int(match shape.0 {
                ElemType::Int => 1,
                ElemType::Float => 2,
                ElemType::Str => 3,
            }),
            Some("DIMNUM") => Value::Int(shape.1.len() as i64),
            // `DIMSIZE` / `DIMSIZE2..8`:名字尾部编号 = 维序号(1 起)。
            Some(name) if name.starts_with("DIMSIZE") => {
                let k = match name.strip_prefix("DIMSIZE") {
                    None | Some("") => 1,
                    Some(digits) => digits.parse::<usize>().map_err(|_| {
                        diag::fatal(
                            code::rt::CMD_SLOT,
                            Boundary::Script,
                            format!("VARINFO 参数名 `{name}` 的维序号不可解析"),
                        )
                    })?,
                };
                match shape.1.get(k - 1) {
                    Some(&d) => Value::Int(d as i64),
                    None => {
                        return Err(diag::fatal(
                            code::rt::VAR_SHAPE,
                            Boundary::Script,
                            format!("VARINFO {name} 超出维数 {}", shape.1.len()),
                        )
                        .into())
                    }
                }
            }
            Some("LENGTH") => {
                // 保留 SET 里写的下标(`$_PSTR(1)` 的 `(1)`);无下标才用 0 号元素。
                let idx: smallvec::SmallVec<[i64; 4]> = if set.indices.is_empty() {
                    smallvec::smallvec![0; shape.1.len()]
                } else {
                    set.indices.clone()
                };
                let v = self.read_var(st, set_var, &idx)?;
                match v {
                    Value::Str(bytes) => Value::Int(str_char_count_with(
                        self.script_text_codec(st.script_id),
                        &bytes,
                    ) as i64),
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
            // `STRTYPE`:REA 0.495 `0x4563b7`(handler `0x456050`)——
            // 取字符串**首字符**的 CP932 字节宽度:单字节(半角)→ 1,
            // lead byte(全角)→ 2(处理体对 class+1 > 1 一律写 2)。
            // 空串:首字节为 0 → 报错 `0x18c4a` 并返回 -1(`0x45674e`);
            // 非字符串 → 同 LENGTH 报类型错误。`ystdlib.yst:489`
            // `_strtype` 的注释与返回值(1=半角/2=全角)与此一致。
            // 手册无页 ⇒ fixed(no-manual)。
            Some("STRTYPE") => {
                let idx: smallvec::SmallVec<[i64; 4]> = if set.indices.is_empty() {
                    smallvec::smallvec![0; shape.1.len()]
                } else {
                    set.indices.clone()
                };
                let v = self.read_var(st, set_var, &idx)?;
                match v {
                    Value::Str(bytes) if bytes.is_empty() => {
                        return Err(diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            "VARINFO STRTYPE: 字符串为空(官方处理体首字节为 0 时报错 \
                             0x18c4a 并返回 -1;REA 0x45674e)",
                        )
                        .into())
                    }
                    Value::Str(bytes) => {
                        Value::Int(if self.script_text_codec(st.script_id).is_lead(bytes[0]) {
                            2
                        } else {
                            1
                        })
                    }
                    other => {
                        return Err(diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!("VARINFO STRTYPE 需要字符串变量,得到 {}", other.type_name()),
                        )
                        .into())
                    }
                }
            }
            // `UTF8CODE`:本地化发行版(2025 Kagura 系)的词法器/字体页
            // 用它取首字符的 UTF-8 字节。语义 = 首字符的字节**大端打包**
            // (`"你"` → 0xE4BDA0),与脚本里的 0xE2809C/0xEFBC81 比较式
            // 一致;空串 → 0。仅 UTF-8 执行路线启用(见上方 `utf8_active`)。
            Some("UTF8CODE") => {
                let idx: smallvec::SmallVec<[i64; 4]> = if set.indices.is_empty() {
                    smallvec::smallvec![0; shape.1.len()]
                } else {
                    set.indices.clone()
                };
                let v = self.read_var(st, set_var, &idx)?;
                match v {
                    Value::Str(bytes) if bytes.is_empty() => Value::Int(0),
                    Value::Str(bytes) => {
                        let width = yu_format::Codec::Utf8
                            .next_char(&bytes, 0)
                            .map_or(1, |(_, width)| width)
                            .min(bytes.len());
                        let mut code: i64 = 0;
                        for byte in &bytes[..width] {
                            code = (code << 8) | i64::from(*byte);
                        }
                        Value::Int(code)
                    }
                    other => {
                        return Err(diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!(
                                "VARINFO UTF8CODE 需要字符串变量,得到 {}",
                                other.type_name()
                            ),
                        )
                        .into())
                    }
                }
            }
            // `SJISCODE`:v555 处理体 `0x455200` 的 `0x4554a9` 分支
            // (0.495 处理体不读,故此前按惰性参数对齐;555 的剧本词法器
            // 实际依赖它)。语义:`SET` 字符串首字符的 Shift-JIS 码——
            // 空串 → 0;单字节 → 字节值;lead byte → `(lead << 8) | trail`
            // (`0x455b9c`: `SHL EAX,0x8` + `MOVZX ECX,[EDX+1]` + `ADD`)。
            //
            // 跨版本门控走 `SemanticProfile`(唯一来源),不再直接比较
            // `yscm.version`:0.495 及更早 `Inert`(无操作),v555 起
            // `FirstChar`(计算)。
            Some("SJISCODE") => {
                if self.semantics().varinfo_sjis == crate::semantics::SjisCodeMode::Inert {
                    self.report_aligned_stub(
                        "VARINFO.SJISCODE",
                        "0.495 处理体不读 SJISCODE 槽(0x456050..0x456ae4 对 \
                         0x68416f/0x684170 零引用),按无操作对齐;v555(ysc 555)\
                         才计算首字符码",
                    );
                    return Ok(StepOutcome::Continue);
                }
                let idx: smallvec::SmallVec<[i64; 4]> = if set.indices.is_empty() {
                    smallvec::smallvec![0; shape.1.len()]
                } else {
                    set.indices.clone()
                };
                let v = self.read_var(st, set_var, &idx)?;
                match v {
                    Value::Str(bytes) if bytes.is_empty() => Value::Int(0),
                    Value::Str(bytes) => {
                        let first = bytes[0];
                        let codec = self.script_text_codec(st.script_id);
                        if codec.is_lead(first) {
                            // 汉化补丁兼容(启动期自检,见
                            // `Vm::detect_legacy_cp932_lexer`):补丁 exe
                            // 把 SJISCODE 改成 CP932 范围内伪码,让仍按
                            // CP932 前导范围编译的剧本词法器把 GBK/Big5
                            // 汉字当成词。
                            if let Some(fake) = self.sjis_lead_remap {
                                Value::Int(i64::from(fake))
                            } else {
                                let trail = bytes.get(1).copied().unwrap_or(0);
                                Value::Int((i64::from(first) << 8) | i64::from(trail))
                            }
                        } else {
                            Value::Int(i64::from(first))
                        }
                    }
                    other => {
                        return Err(diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!(
                                "VARINFO SJISCODE 需要字符串变量,得到 {}",
                                other.type_name()
                            ),
                        )
                        .into())
                    }
                }
            }
            // `SEARCH`:手册 `variable/varinfo.html` 例 5/6/7 ——
            // 从 NO 号元素开始在一维数组里找第一个相等的元素,返回下标,
            // 找不到返回 -1。要搜索的值由 `INT=`/`FLT=`/`STR=` 参数给出。
            Some("SEARCH") => {
                let need = array_needle(&varinfo_names, self, cmd, st, set_var)?;
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
                // `NO=` 槽号同样按参数名解析(写死 20 在 v494 上会取不到)。
                let no_slot = varinfo_no_slot(&varinfo_names);
                let start = no_slot
                    .and_then(|s| flags_get(&flags, s))
                    .unwrap_or(0)
                    .max(0);
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
            // v555 新增 `STRFIRST`(REA 处理体 `0x455200`):
            // 在 `SET` 字符串数组的 `[NO, NO2]` 区间里找**字典序最小**
            // 的元素,返回其下标写入 `LET`。官方比较是 ASCII 大小写不敏感
            // (`FUN_00471840` 的默认分支对 0x41..0x5A 降为小写),循环
            // 更新条件 `current <= best` 在相等时取后者;本实现取首个
            // 最小值(相等时下标较小),两者对调用方(比较两个字符/取
            // 最小项)等价,且更确定。
            //
            // 区间语义由语料自证:`VARINFO[STRFIRST=1, SET=$a,
            // NO=1, NO2=2, LET=@i]` 前后分别写 `$a[1]`、`$a[2]`,调用方
            // 用 `@i` 判断 1/2 ⇒ NO2 是**闭区间终点**。
            Some("STRFIRST") => {
                let (ty, dims) = shape;
                if ty != ElemType::Str || dims.len() != 1 {
                    return Err(diag::fatal(
                        code::rt::VAR_SHAPE,
                        Boundary::Script,
                        format!(
                            "VARINFO STRFIRST 需要字符串一维数组,目标 {} = {}{:?}",
                            set_var.display(),
                            ty.as_str(),
                            dims
                        ),
                    )
                    .into());
                }
                let len = dims[0];
                if len == 0 {
                    Value::Int(0)
                } else {
                    let start = varinfo_slot(&varinfo_names, "NO")
                        .and_then(|s| flags_get(&flags, s))
                        .unwrap_or(0)
                        .max(0) as usize;
                    let end = varinfo_slot(&varinfo_names, "NO2")
                        .and_then(|s| flags_get(&flags, s))
                        .unwrap_or(len as i64 - 1)
                        .max(0) as usize;
                    let end = end.min(len - 1);
                    if start > end || start >= len {
                        Value::Int(0)
                    } else {
                        let mut best = start;
                        let mut best_val = self.read_var(st, set_var, &[best as i64])?;
                        for i in start + 1..=end {
                            let v = self.read_var(st, set_var, &[i as i64])?;
                            let better = match (&v, &best_val) {
                                (Value::Str(a), Value::Str(b)) => str_less_ci(a, b),
                                _ => false,
                            };
                            if better {
                                best = i;
                                best_val = v;
                            }
                        }
                        Value::Int(best as i64)
                    }
                }
            }
            other => {
                return Err(diag::unimplemented(
                    code::rt::CMD_UNIMPLEMENTED,
                    Boundary::Script,
                    format!(
                        "VARINFO 参数 `{}`(槽 {slot})未实现(无手册页/无 REA 证据)",
                        other.unwrap_or("<不在工程 ysc 表>")
                    ),
                )
                .into())
            }
        };
        // 官方 `STRTYPE` 分支按 **LET 当前类型**写回(0x4563e8:INT→整数、
        // FLT→浮点);其余查询项走同一公共出口。内核 `Value` 是动态类型,
        // 这里按目标变量当前类型把整数结果转成浮点,避免把 FLT 目标重标成
        // INT(0.495 处理体逐分支核对)。
        if let (Value::Int(n), Some(t)) = (&result_val, &target) {
            if matches!(self.var_shape(t.var), Some((ElemType::Float, _))) {
                result_val = Value::Float(*n as f64);
            }
        }
        if let Some(t) = target {
            self.write_target(st, &t, &result_val)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// 写回一个左值(保留下标);系统变量目标 ⇒ 挂号(DIV-RT-007)。
    pub(crate) fn write_target(
        &mut self,
        _st: &ExecState,
        target: &crate::expr::LValue,
        v: &Value,
    ) -> Result<(), VmError> {
        let var = self.canonical_var(target.var);
        if var.id < self.sys_count {
            return Err(diag::unimplemented(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "写入系统变量 {}(语义未取证;DIV-RT-007)",
                    var.display()
                ),
            )
            .into());
        }
        self.store.write(var, &target.indices, v)?;
        Ok(())
    }

    /// [`Self::write_target`] 的所有权版:值 owned 时,串槽赋值直接搬字节
    /// (不再 `coerce_to` 克隆一次;#80)。可观察状态完全一致。
    pub(crate) fn write_target_owned(
        &mut self,
        _st: &ExecState,
        target: &crate::expr::LValue,
        v: Value,
    ) -> Result<(), VmError> {
        let var = self.canonical_var(target.var);
        if var.id < self.sys_count {
            return Err(diag::unimplemented(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "写入系统变量 {}(语义未取证;DIV-RT-007)",
                    var.display()
                ),
            )
            .into());
        }
        self.store.write_owned(var, &target.indices, v)?;
        Ok(())
    }

    /// `VARACT`:手册 `variable/varact.html`;引擎 §3.26。
    pub(super) fn op_varact(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        self.varact_calls = self.varact_calls.saturating_add(1);
        if self.prof_commands {
            *self
                .prof_varact_sites
                .entry((self.cur_script, self.cur_pc))
                .or_insert(0) += 1;
        }
        let mut set: Option<crate::expr::LValue> = None;
        let mut letv: Option<crate::expr::LValue> = None;
        let mut flags: smallvec::SmallVec<[(u8, i64); 8]> = smallvec::SmallVec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match w.rec.kind {
                0 => set = Some(self.eval_lvalue_window(st, w)?),
                1 => letv = Some(self.eval_lvalue_window(st, w)?),
                slot => {
                    let v = self.eval_window_ref(st, w)?;
                    if let Some(n) = v.as_int() {
                        flags.push((slot, n));
                    }
                }
            }
        }
        // 参数名 → 槽号的计划在 `fill_cmd` 按站点算好;未命中(理论上只有
        // 手工构造的 `Cmd`)才现算一次,保持与旧路径相同的可观察行为。
        let fallback_plan;
        let plan = match &cmd.varact_plan {
            Some(plan) => plan,
            None => {
                fallback_plan = build_varact_plan(&cmd.param_names, &cmd.windows);
                &fallback_plan
            }
        };
        let present = |slot: u8| flags.iter().any(|(s, _)| *s == slot);
        let flag_slot = |slot: Option<u8>| slot.is_some_and(present);
        let num = |slot: Option<u8>| {
            slot.and_then(|slot| flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v))
                .unwrap_or(0)
        };
        // `INIT`/`G_*` 分支不读 `SET`(引擎 §3.26 ③:`0x454d3d..0x454e00`
        // 一次都没碰 `0x683a60`),因此把「缺 SET → Fatal」的判定推到它们之后。
        let set_missing = set.is_none();
        // 字符串加工 / TYPE 在 `LET` 缺省时写回 `SET`(见下方各分支的
        // `fixed(no-manual)` 注释),所以这里先留住原始左值。
        let set_lvalue = set.clone();
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
        if flag_slot(plan.type_slot) {
            self.varact_type_calls = self.varact_type_calls.saturating_add(1);
            // TYPE:按目标类型转换(手册例 1/2;§3.22)。
            //
            // **LET 可缺省**:侧线 §第七轮第 6 条已取证 —— 引擎 `0x4542ab`
            // 按 `[0x684161]`(LET 是否在场)分两支,**两支都调 `0x43f0d8`**
            // 写结果;LET 缺省时写回 `SET` 指向的变量。手册只示范了带 LET 的
            // 例 1/2,缺省形态无手册页 ⇒ `fixed(no-manual)`。
            let target = letv.as_ref().or(set_lvalue.as_ref()).ok_or_else(|| {
                diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "VARACT 缺 SET/LET")
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
        // `G_*` 族按名字识别(不是按 17..=28 的槽号区间):族 = 前缀,
        // 位号 = 名字尾部数字(`G_INT` ⇒ 0,`G_INT3` ⇒ 3)。
        // 掩码位 = **组号 - 1**(与 `restore_globals` 的 `1 << (group-1)` 对齐)。
        // `G_INT` ⇒ 组 1 ⇒ bit 0;`G_INT2` ⇒ 组 2 ⇒ bit 1 …… `G_INT4` ⇒ bit 3。
        // **必须看「本次指令给了哪些参数」,不能看「表里有没有这些名字」**
        // —— 后者恒真,会把每条 VARACT 都误判进 INIT/G_* 分支并提前返回。
        let any_g = plan.g_slots.iter().any(|(slot, _)| present(*slot));
        if flag_slot(plan.init_slot) || any_g {
            self.varact_init_calls = self.varact_init_calls.saturating_add(1);
            if !flag_slot(plan.init_slot) {
                // INIT **不在场**:引擎判定链 `je 0x454e05`(函数尾)——
                // 什么都不做(旁线 §3.26 ③)。判据是在场,与值无关。
                return Ok(StepOutcome::Continue);
            }
            let mut masks = [0u8; 3]; // INT / FLT / STR 三族的 4 位掩码
            for (slot, mask) in &plan.g_slots {
                if let Some((family, bit)) = mask {
                    if present(*slot) {
                        masks[usize::from(*family)] |= 1 << bit;
                    }
                }
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
        if flag_slot(plan.dimsize_slot) {
            // 手册例 9/10:`DIMSIZE=500`(绝对值)/ `DIMSIZE+=1`(相对值)。
            // 语料实测 **104/104 都是 `+=`**,复合码在窗口 `slot` 高字节
            // (与 `LET` 同一编码:0== 1+= 2-= …),站点级静态值见计划。
            let code = plan.dimsize_code;
            let operand = num(plan.dimsize_slot);
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
        if flag_slot(plan.push_slot) || flag_slot(plan.pop_slot) {
            let push = flag_slot(plan.push_slot);
            let pos = num(if push { plan.push_slot } else { plan.pop_slot }).max(0) as usize;
            self.shift_1d(set_var, pos, push)?;
            return Ok(StepOutcome::Continue);
        }
        // `LET` 可缺省:引擎 `0x4542ab` 按 `[0x684161]`(LET 在场)分两支,
        // **两支都调 `0x43f0d8`**;缺省时结果写回 `SET` 指向的变量。
        // 语料实测:`es_Text.yst:2568` `VARACT[CUT=1 SET=$TX POS=1 LENGTH=@lenA]`
        // (删掉文本开头的名字字符)、`:2586` 同形 `SET=$TX.TASK`、`:2024/:2040`
        // 同形 —— 全部**没有 LET**,都是原地剪切。手册
        // `variable/varact.html` 例 5/6 只示范带 LET 的形态,缺省无手册页
        // ⇒ `fixed(no-manual)`。旧实现此处 Fatal(`rt.cmd.slot`),直接杀掉
        // `es_Text` 任务(用户实测演出 1 触发后全流程终止)。
        let target = letv.or(set_lvalue).ok_or_else(|| {
            diag::fatal(code::rt::CMD_SLOT, Boundary::Script, "VARACT 缺 SET/LET")
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
        let codec = self.script_text_codec(st.script_id);
        self.varact_bytes = self.varact_bytes.saturating_add(bytes.len() as u64);
        if flag_slot(plan.copy_slot) || flag_slot(plan.cut_slot) {
            let pos = num(plan.pos_slot).max(1) as usize;
            let len = num(plan.length_slot).max(0) as usize;
            out = if flag_slot(plan.copy_slot) {
                self.varact_ops[0] = self.varact_ops[0].saturating_add(1);
                copy_chars(&bytes, pos, len, codec)
            } else {
                self.varact_ops[1] = self.varact_ops[1].saturating_add(1);
                cut_chars(&bytes, pos, len, codec)
            };
        } else if flag_slot(plan.upper_slot) {
            self.varact_ops[2] = self.varact_ops[2].saturating_add(1);
            out = ascii_case(&bytes, true, codec);
        } else if flag_slot(plan.upper2_slot) {
            self.varact_ops[3] = self.varact_ops[3].saturating_add(1);
            out = fullwidth_case(&bytes, true, codec);
        } else if flag_slot(plan.lower_slot) {
            self.varact_ops[4] = self.varact_ops[4].saturating_add(1);
            out = ascii_case(&bytes, false, codec);
        } else if flag_slot(plan.lower2_slot) {
            self.varact_ops[5] = self.varact_ops[5].saturating_add(1);
            out = fullwidth_case(&bytes, false, codec);
        } else if flag_slot(plan.hantozen_slot) {
            self.varact_ops[6] = self.varact_ops[6].saturating_add(1);
            // 半角→全角:表/算法逐条见 `crate::case_tables`(0x4546aa..)。
            out = crate::case_tables::hantozen_with(&bytes, codec);
        } else if flag_slot(plan.zentohan_slot) {
            self.varact_ops[7] = self.varact_ops[7].saturating_add(1);
            // 全角→半角:0x4548cf.. / 0x46b395。
            out = crate::case_tables::zentohan_with(&bytes, codec);
        } else {
            return Err(diag::unimplemented(
                code::rt::CMD_UNIMPLEMENTED,
                Boundary::Script,
                "VARACT 无已识别的操作项(INIT/G_* 无手册页;DIV-RT-011)",
            )
            .into());
        }
        self.write_target_owned(st, &target, Value::Str(out))?;
        Ok(StepOutcome::Continue)
    }

    /// `_INT`/`_PINT`/`_DROPFILE_NUM` 的 RHS 强转留痕。
    ///
    /// 官方 v555 LET 处理体按**目标类型**选择求值器:
    /// - int 目标 + 浮点 RHS:`0x420cf4` 把 FPU 舍入控制字置
    ///   `RC=11`(向零截断)后 `fistp qword`,即截断而非四舍五入
    ///   —— 已取证,与 [`Value::to_int_lossy`] 同口径,不再另报;
    /// - int 目标 + 字符串 RHS:求值表里没有独立的“字符串→int”入口,
    ///   官方转换/报错/静默无写入三选一未取证(DIV-RT-077)。当前按
    ///   十进制解析写入,显式留一条可恢复诊断,便于实机对拍时抓到。
    fn note_lossy_int_rhs(&mut self, name: &str, rhs: &Value) {
        if matches!(rhs, Value::Str(_)) {
            self.report(diag::recoverable(
                code::rt::SYSVAR_WRITE,
                Boundary::Script,
                format!(
                    "{name}:字符串 RHS 按十进制解析写入 int(官方 v555 字符串→int \
                     分支未取证,DIV-RT-077)"
                ),
            ));
        }
    }

    pub(super) fn read_lvalue(&self, lval: &crate::expr::LValue) -> Result<Value, yu_log::Diag> {
        self.store.read(lval.var, &lval.indices)
    }

    /// `VARACT[INIT + G_*]`:按 `ysv.ybn` 镜像把全局变量整槽重建。
    ///
    /// 算法逐条照旁线 §3.26 ④(0.495 处理体 `0x455b60`):
    /// ① 只收 `kind == 1`(全局);② 按 `(值类型, 组号)` 查掩码,位为 0 则否决;
    /// ③ 组号超出 1..=4 时无否决项 ⇒ 照常恢复;④ 整槽重建(声明 + 类型 + 维数 + 初值)。
    pub(super) fn restore_globals(&mut self, st: &ExecState, masks: &[u8; 3]) -> Result<(), VmError> {
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
    pub(super) fn resize_1d(&mut self, var: VarRef, _ty: ElemType, new_len: usize) -> Result<(), VmError> {
        if new_len == 0 {
            // 引擎对 0 长度的处理未取证:保留 1 个空槽(手册例 9/10 均 >=1)。
            self.store.resize_1d(var, 0)?;
            return Ok(());
        }
        self.store.resize_1d(var, new_len)?;
        Ok(())
    }

    pub(super) fn shift_1d(&mut self, var: VarRef, pos: usize, push: bool) -> Result<(), VmError> {
        self.store.shift_1d(var, pos, push)?;
        Ok(())
    }

    // ---------------------------------------------------------- 数学

    /// `MATH`:手册 `math/math.html`;引擎 §3.20。
    pub(super) fn op_math(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
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
                    let v = self.eval_window_ref(st, w)?;
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
    pub(super) fn op_task(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let mut id: Option<String> = None;
        let mut enabled = true;
        let mut active = true;
        // E/A 只在参数**存在**时写任务记录(引擎 `0x450bb1/0x450bbf` 均为
        // 「标志非零才写」;缺省由任务构造器给 1)。用存在标志避免
        // `TASK[ID=X Z=…]` 之类的命令把已禁用的任务重新打开。
        let mut enabled_param = false;
        let mut active_param = false;
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
        let names = std::rc::Rc::clone(&cmd.param_names);
        let mut tints = [0i64; 16];
        let mut tflts = [0f64; 16];
        let mut tstrs: [Vec<u8>; 16] = std::array::from_fn(|_| Vec::new());
        // `SCRIPTPOS`:引擎执行位快照串(0.452a54 序列化 / 0x450fe0 装载)。
        let mut script_pos: Option<Vec<u8>> = None;
        // `RESTART=1`:本 vsync 再跑一逻辑帧(REA `[0x801944]`)。
        let mut request_restart = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            // 窗口按**参数名**分派(槽号在版本间会位移,名字稳定)。
            // `TINT`/`TFLT`/`TSTR` 族:族 = 前缀,槽位 = 后缀数字 - 1
            // (`TINT` ⇒ 0、`TINT2` ⇒ 1 …… 与 `_TINT(1..10)` 的 `n-1` 同口径)。
            let pname = names.get(usize::from(w.rec.kind)).map(String::as_str);
            match pname {
                Some("ID") => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        id = Some(cp932_lossy(&bytes));
                    }
                }
                Some("E") => {
                    enabled = self.eval_window(st, w)?.as_int().unwrap_or(1) != 0;
                    enabled_param = true;
                }
                // `A`(引擎 `task+0x15`):0 = 不参与调度。手册 `control/task.html`
                // 无 A 页;REA 0.495 调度器 `0x4043dc..0x4043e6`
                // (`movsx E; movsx A; test A, E; je 跳过`)证得 A 与 E 按位与
                // 后决定是否步进;TASK 处理体 `0x450bbf..0x450bd1` 按参数写它,
                // 构造器 `0x465630/0x465633` 默认 E=A=1。DIV-RT-015。
                Some("A") => {
                    active = self.eval_window(st, w)?.as_int().unwrap_or(1) != 0;
                    active_param = true;
                }
                Some("Z") => priority = self.eval_window(st, w)?.as_int().unwrap_or(1),
                // SCRIPTPOS(手册 `control/task.html` 例 4:位置情報の上書き =
                // 事実上のスクリプト位置ロード)。串格式见 `crate::scriptpos`
                // (0.292/0.466/0.479/0.495/v494 五版线格式同构);这里只收
                // 原始串,循环结束后统一解码。
                Some("SCRIPTPOS") => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        script_pos = Some(bytes);
                    }
                }
                // 入口标签槽:本作与 ERIS 语料的 ysc 里该参数名都是 `#`
                // (实测 v494 `TASK` 表 `#34  # kind=String`)。空串为防御
                // 保留(某些编译器可能写匿名参数名)。
                Some("#" | "") => {
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
                Some(name) if name.starts_with("TINT") => {
                    if let Some(i) = family_index(name, "TINT") {
                        tints[i] = self.eval_window(st, w)?.as_int().unwrap_or(0);
                    }
                }
                Some(name) if name.starts_with("TFLT") => {
                    if let Some(i) = family_index(name, "TFLT") {
                        tflts[i] = self.eval_window(st, w)?.as_f64().unwrap_or(0.0);
                    }
                }
                Some(name) if name.starts_with("TSTR") => {
                    if let Some(i) = family_index(name, "TSTR") {
                        if let Value::Str(bytes) = self.eval_window(st, w)? {
                            tstrs[i] = bytes;
                        }
                    }
                }
                // `RESTART`:请求「本 vsync 再跑一个逻辑帧」。
                //
                // REA 0.495 `TASK` 处理体(`0x4506f4`)在 `0x450c46` 读槽 37
                // 的存在标志 `[0x684185]`;存在 ⇒ `0x450c55
                // mov dword ptr [0x801944], 1`,随后 `0x450d05 call 0x42f7bc`
                // (按 Z 重排任务链)返回。`[0x801944]` 正是内核
                // `Scheduler::extra_frame` 的同一个量(另一处消费者
                // `0x40413d` 在逻辑帧开头清零)。
                //
                // 语料用法正是这个语义:`es_Text.yst:1545` 在 `es.TX.OUTPUT.Z`
                // (行末 2 = 一句文本彻底结束)里 `TASK[RESTART=1]`,让文本任务
                // 在同一次 vsync 内立即重跑主循环。手册 `control/task.html` 只列
                // ID/E/Z/#/SCRIPTPOS,**无此参数页** ⇒ 按 REA + 语料实现并标注
                // (DIV-RT-015)。
                Some("RESTART") => {
                    let on = self.eval_window(st, w)?.as_int().unwrap_or(0) != 0;
                    if on {
                        request_restart = true;
                    }
                }
                // 0.495 TASK 处理体**没有** TEXTPOS(槽 36)分支(只检查
                // `0x684160/161/162/163/182/183/185`);官方忽略该参数。
                // 参数序言仍会求值窗口,这里同型求值后丢弃。
                Some("TEXTPOS") => {
                    let _ = self.eval_window(st, w)?;
                }
                other => {
                    return Err(diag::unimplemented(
                        code::rt::CMD_UNIMPLEMENTED,
                        Boundary::Script,
                        format!(
                            "TASK 参数 `{}`(槽 {})未实现(DIV-RT-015;0.495 处理体无该槽分支)",
                            other.unwrap_or("<不在工程 ysc 表>"),
                            w.rec.kind
                        ),
                    )
                    .into())
                }
            }
        }
        // 缺省目标 = 执行者自身(手册;REA 0x45074a 用当前任务记录)。
        let target_id = id.unwrap_or_else(|| task.id.clone());
        // SCRIPTPOS 串在两条分支里都要用:先统一解码 + 脚本名反查。
        // 旧内核存档(2026-10-26 前)是 8 字节 `[script_id u32 LE][pc u32 LE]`:
        // 这不是引擎串,但为了不让旧存档读档直接 Fatal,按兼容路径还原
        // (无 IF/GOSUB/LOOP 栈与 text_line;script_id 已能定位脚本)。
        let decoded = match script_pos {
            Some(bytes) => Some(self.decode_scriptpos_compat(&bytes)?),
            None => None,
        };
        let mut decoded_pos = decoded.as_ref().map(|(sp, _)| sp.clone());
        let decoded_sid = decoded.as_ref().and_then(|(_, sid)| *sid);
        match label {
            Some(label) => {
                let seed = TaskSeed {
                    id: target_id,
                    script_id: label.script_id as u32,
                    pc: label.pc,
                    enabled,
                    active,
                    priority,
                    main: false,
                };
                let mut fresh = ExecState::new(seed.script_id, seed.pc);
                fresh.task_ints = tints;
                fresh.task_floats = tflts;
                fresh.task_strs = tstrs.clone();
                if seed.id == task.id {
                    // 目标 = 自身:引擎 `TASK` 直接写当前任务记录的 pc/脚本号
                    // (`0x4508bc..0x4508d9`),不是另建一条执行流。这里同样改写
                    // 当前执行位(块/循环/调用栈随新入口重置)。
                    //
                    // 引擎复用任务记录,只写 +0x20/+0x3c/+0x24..+0x38、TINT/
                    // TFLT/TSTR、E/A、SCRIPTPOS/RESTART,**不写 task+0xc/+0x10**
                    // ⇒ 重配置后 TEXTLINE/NEXTVOICE 保留旧值。这里同型继承。
                    fresh.text_line = st.text_line;
                    fresh.next_voice = st.next_voice;
                    if let (Some(sp), Some(sid)) = (decoded_pos.take(), decoded_sid) {
                        // 引擎先落标签再执行 SCRIPTPOS 装载:完整执行位覆盖入口。
                        Self::apply_scriptpos(&mut fresh, sp, sid);
                    }
                    *st = fresh;
                } else {
                    // 同 ID 重配置 = 换入口(引擎复用任务记录并重载脚本表)。
                    if let Some(old) = self.exec.get(&seed.id) {
                        fresh.text_line = old.text_line;
                        fresh.next_voice = old.next_voice;
                    }
                    if let (Some(sp), Some(sid)) = (decoded_pos.take(), decoded_sid) {
                        Self::apply_scriptpos(&mut fresh, sp, sid);
                    }
                    self.exec.insert(seed.id.clone(), Box::new(fresh));
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
                if let (Some(sp), Some(sid)) = (decoded_pos.take(), decoded_sid) {
                    if target_id == task.id {
                        // 目标 = 自身:引擎 `0x450fe0` 写同一任务记录。
                        Self::apply_scriptpos(st, sp, sid);
                    } else if let Some(target_st) = self.exec.get_mut(&target_id) {
                        Self::apply_scriptpos(target_st, sp, sid);
                    } else if self
                        .task_view()
                        .iter()
                        .any(|n| n.id == target_id && !n.finished)
                    {
                        // 目标任务还没有执行位:按解码出的入口新建,再落完整状态。
                        let mut fresh = ExecState::new(sid, sp.pc);
                        Self::apply_scriptpos(&mut fresh, sp, sid);
                        self.exec.insert(target_id.clone(), Box::new(fresh));
                    } else {
                        // 引擎 `0x4507b4`:`test ebx; je` ⇒ 无 ID/无目标任务 = 错误。
                        return Err(diag::fatal(
                            code::rt::TASK,
                            Boundary::Script,
                            format!("TASK SCRIPTPOS: 目标任务 `{target_id}` 不存在"),
                        )
                        .into());
                    }
                }
                // E/A 只写命令里**实际给出**的参数(引擎 `0x450bb1/0x450bbf`
                // 均为存在性守卫);否则 `TASK[ID=X Z=…]` 会把已禁用任务重新打开。
                if enabled_param {
                    self.push_effect(Effect::SetEnabled(target_id.clone(), enabled));
                }
                if active_param {
                    self.push_effect(Effect::SetActive(target_id, active));
                }
            }
        }
        if request_restart {
            self.push_effect(Effect::RequestExtraFrame);
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
    pub(super) fn op_taskend(
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

    /// `TASKINFO[ID LET EXIST/E/A/Z/FILE/FILENO/LINE/FILE0/FILENO0/LINE0/
    /// TEXTLINE/NEXTVOICE/IFNEST/GOSUBNEST/LOOPNEST/SCRIPTPOS]`:
    /// 手册 `control/taskinfo.html` + 引擎处理体(0.495 `0x4523fc` 逐槽;
    /// 0.466/0.479/v494 的存在标志检查顺序与 FILENO0→SCRIPTPOS
    /// fallthrough 与之相同;任务结构偏移随版本不同,字段语义同构)。
    ///
    /// 官方按**参数存在性**短路(不看值是否为 0),分派顺序固定:
    /// `EXIST, E, A, Z, FILE, FILE0, LINE, LINE0, TEXTLINE, NEXTVOICE,
    /// IFNEST, GOSUBNEST, LOOPNEST, FILENO, FILENO0, SCRIPTPOS`。
    /// `GOLABEL/GOSUBLABEL/TEXTPOS` 在 0.495 处理体里没有分支,落到默认
    /// `return 0` 且不写 LET —— 本实现同型对齐,不挂 Unimplemented。
    ///
    /// 取值来源 = **引擎任务记录**(旁线 §3.2):`+0x20` pc、`+0x3c` 脚本号、
    /// `+0x3e` 块深度(base_a)、`+0x244` 调用栈深度;`LINE` 用编译产物的逐 pc
    /// 源码位置(`ScriptFile::source_position`,引擎 `task+0x38[pc]`)。
    ///
    /// `FILE0/FILENO0/LINE0` 读引擎 `task+0x144[0]`(第一层 GOSUB 的返回位:
    /// 调用方脚本/调用点源码行),是任务的**外框位置**;`FILE/FILENO/LINE`
    /// 读当前执行位(跨脚本 GOSUB 后 = 被调方)。无 GOSUB 时前组回退到后组。
    /// `TEXTLINE/NEXTVOICE` 读 `task+0xc/+0x10`,由 `RETURNCODE`(`0x44c390`)
    /// 从窗口记录抄入。
    pub(super) fn op_taskinfo(
        &mut self,
        st: &mut ExecState,
        task: &crate::task::TaskNode,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let names = std::rc::Rc::clone(&cmd.param_names);
        let mut id: Option<String> = None;
        let mut target: Option<crate::expr::LValue> = None;
        // 查询槽按**在场性**收集(引擎 0x4523fc 看窗口存在,不看值;
        // `FILE0=0` 也算请求)。只存槽号(#44):不再逐窗口 `name.to_string()`;
        // 名字在下面按 ORDER 比较时现取(参数名表本身按调用点缓存)。
        let mut present: smallvec::SmallVec<[u8; 16]> = smallvec::SmallVec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            match names.get(usize::from(w.rec.kind)).map(String::as_str) {
                Some("ID") => {
                    if let Value::Str(bytes) = self.eval_window(st, w)? {
                        id = Some(cp932_lossy(&bytes));
                    }
                }
                Some("LET") => target = Some(self.eval_lvalue_window(st, w)?),
                Some(_) => {
                    // 与引擎参数序言一致:所有窗口都求值(未选中的查询也不
                    // 跳过),保持同一处未声明变量/表达式错误面。
                    let _ = self.eval_window(st, w)?;
                    present.push(w.rec.kind);
                }
                None => {
                    let _ = self.eval_window(st, w)?;
                    present.push(w.rec.kind);
                }
            }
        }
        // 缺省 = 自身(手册:「タスクＩＤを指定しなかった場合、この命令を実行した
        // 自身のタスクが対象」;REA 0x4521dc/0x4524xx 同样取当前任务记录)。
        let id = id.unwrap_or_else(|| task.id.clone());
        // 引擎从任务链表取记录;泵期间调度器被 `Vm::tick` 借出,故读同步过来的
        // 投影。只需要 E/A/Z 三个标量,不克隆 `TaskView`(#44)。
        let node_facts = self
            .task_view()
            .iter()
            .find(|n| n.id == id && !n.finished)
            .map(|n| (n.enabled, n.active, n.priority));
        // 引擎 `0x452918`:ID 未命中 ⇒ 按 LET 目标类型写默认值
        // (INT 0 / FLT 0.0 / STR 空串),**不**去读当前任务状态。
        let Some((enabled, active, priority)) = node_facts else {
            if let Some(t) = &target {
                match self.taskinfo_default(t) {
                    Some(v) => self.write_target(st, t, &v)?,
                    // `var_shape` 为 None = LET 目标未声明。旧实现同样走
                    // write_target 报 Fatal;这里保留显式诊断,不静默跳过。
                    None => {
                        self.write_target(st, t, &Value::Int(0))?;
                    }
                }
            }
            return Ok(StepOutcome::Continue);
        };
        // 引擎处理体的固定分派顺序(0.495 `0x4523fc`;0.466/0.479/v494
        // 的存在标志检查顺序相同,不是窗口顺序)。
        const ORDER: [&str; 16] = [
            "EXIST",
            "E",
            "A",
            "Z",
            "FILE",
            "FILE0",
            "LINE",
            "LINE0",
            "TEXTLINE",
            "NEXTVOICE",
            "IFNEST",
            "GOSUBNEST",
            "LOOPNEST",
            "FILENO",
            "FILENO0",
            "SCRIPTPOS",
        ];
        // 在场的槽号 → 参数名 → ORDER 位置,取最靠前的一项(引擎 `find` 语义)。
        let mut best: Option<usize> = None;
        for &slot in &present {
            let Some(name) = names.get(usize::from(slot)).map(String::as_str) else {
                continue;
            };
            if let Some(pos) = ORDER.iter().position(|q| *q == name) {
                best = Some(best.map_or(pos, |cur| cur.min(pos)));
            }
        }
        let Some(query) = best.map(|pos| ORDER[pos]) else {
            // `GOLABEL/GOSUBLABEL/TEXTPOS` 与未知槽:0.495 处理体没有分支,
            // 落到默认 `return 0` 且不写 LET。显式留一条对齐记录(每个槽名
            // 一次),不挂 Unimplemented。
            for &slot in &present {
                match names.get(usize::from(slot)).map(String::as_str) {
                    Some(name) => {
                        if !ORDER.contains(&name) {
                            self.report_aligned_stub(
                                &format!("TASKINFO[{name}]"),
                                "0.495 处理体 0x4523fc 无该查询槽分支;按官方 no-op(不写 LET)",
                            );
                        }
                    }
                    None => {
                        self.report_aligned_stub(
                            &format!("TASKINFO[#{slot}]"),
                            "0.495 处理体 0x4523fc 无该查询槽分支;按官方 no-op(不写 LET)",
                        );
                    }
                }
            }
            return Ok(StepOutcome::Continue);
        };
        let is_self = id == task.id;
        // 借用执行位取出全部标量事实(不克隆 `ExecState` —— 它含 16 元素
        // 寄存器组与三个栈,#44)。缺执行位时全 0(与旧路径同口径)。
        let facts = {
            let exec: Option<&ExecState> = if is_self { Some(&*st) } else { self.exec_of(&id) };
            match exec {
                Some(exec) => TaskinfoFacts {
                    sid: exec.script_id,
                    pc: exec.pc,
                    // 引擎 `0x4524dc/0x4525fa/0x4528b3`:GOSUB 深度 > 0 时读
                    // `task+0x144[0]`(第一层调用的返回位);内核 frames[0] 是基帧,
                    // frames[1] 才是第一层 GOSUB。
                    origin: exec
                        .frames
                        .get(1)
                        .map(|f| (f.return_script, f.return_pc))
                        .unwrap_or((exec.script_id, exec.pc)),
                    text_line: exec.text_line,
                    next_voice: exec.next_voice,
                    if_nest: exec.if_stack.len() as i64,
                    gosub_nest: exec.frames.len().saturating_sub(1) as i64,
                    loop_nest: exec.loops.len() as i64,
                },
                None => TaskinfoFacts::default(),
            }
        };
        let value = match query {
            "EXIST" => Value::Int(1),
            "E" => Value::Int(enabled as i64),
            // A:调度闸门第二字节(引擎 task+0x15;REA 0x4527b0 读)。
            "A" => Value::Int(active as i64),
            "Z" => Value::Int(priority),
            // FILE:当前脚本文件名(手册)→ 引擎 `0x801850[script_no]` 名字表。
            "FILE" => Value::Str(self.script_path_bytes(facts.sid)),
            "FILENO" => Value::Int(i64::from(facts.sid)),
            // LINE:当前执行行数(数据区 B 的逐 pc 源码位置)。
            "LINE" => Value::Int(self.script_line(facts.sid, facts.pc)),
            // 以下三项 = 第一层 GOSUB 的调用点(引擎 `task+0x144[0]`)。
            "FILE0" => Value::Str(self.script_path_bytes(facts.origin.0)),
            "FILENO0" => Value::Int(i64::from(facts.origin.0)),
            "LINE0" => Value::Int(self.script_line(facts.origin.0, facts.origin.1)),
            // task+0xc / task+0x10:最近一次 RETURNCODE 的窗口记录。
            "TEXTLINE" => Value::Int(facts.text_line),
            "NEXTVOICE" => Value::Int(facts.next_voice),
            "IFNEST" => Value::Int(facts.if_nest),
            "GOSUBNEST" => Value::Int(facts.gosub_nest),
            "LOOPNEST" => Value::Int(facts.loop_nest),
            // SCRIPTPOS(手册 `control/taskinfo.html`:現在実行されている
            // スクリプト位置情報)。产官方引擎线格式(见 `crate::scriptpos`;
            // 0.292/0.466/0.479/0.495/v494 五版同构)。
            "SCRIPTPOS" => {
                let exec = if is_self { Some(&*st) } else { self.exec_of(&id) };
                self.scriptpos_value(exec, facts.sid, facts.pc)?
            }
            _ => unreachable!("ORDER 覆盖全部分支"),
        };
        if let Some(t) = &target {
            self.write_taskinfo_value(st, t, query, &value)?;
            // 引擎 `0x4525fa/0x4528fa`:FILENO0 写完不返回,`jmp 0x452628`
            // 继续检查 SCRIPTPOS;两者同时存在时 SCRIPTPOS 二次写入覆盖。
            // 其余分支均在写后直接返回(`0x452673` 等)。
            let scriptpos_present = names
                .iter()
                .position(|n| n == "SCRIPTPOS")
                .and_then(|i| u8::try_from(i).ok())
                .is_some_and(|slot| present.contains(&slot));
            if query == "FILENO0" && scriptpos_present {
                let blob = {
                    let exec = if is_self { Some(&*st) } else { self.exec_of(&id) };
                    self.scriptpos_value(exec, facts.sid, facts.pc)?
                };
                self.write_taskinfo_value(st, t, "SCRIPTPOS", &blob)?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `TASKINFO[SCRIPTPOS]` / `FILENO0+SCRIPTPOS`:把目标任务当前执行位编码成
    /// 官方引擎线格式(线格式跨五版核对,见 [`crate::scriptpos`])。
    pub(super) fn scriptpos_value(
        &self,
        exec: Option<&ExecState>,
        sid: u32,
        pc: u32,
    ) -> Result<Value, VmError> {
        Ok(Value::Str(self.encode_scriptpos(exec, sid, pc)?))
    }

    pub(super) fn encode_scriptpos(
        &self,
        exec: Option<&ExecState>,
        sid: u32,
        pc: u32,
    ) -> Result<Vec<u8>, VmError> {
        let fallback;
        let exec = match exec {
            Some(e) => e,
            None => {
                fallback = ExecState::new(sid, pc);
                &fallback
            }
        };
        let sid = exec.script_id;
        let name = u16::try_from(sid)
            .ok()
            .and_then(|s| self.tables().boot.scripts.path_of(s))
            .map(|p| p.to_vec())
            .ok_or_else(|| -> VmError {
                diag::fatal(
                    code::rt::SCRIPT_POS,
                    Boundary::Script,
                    format!("SCRIPTPOS: 脚本号 {sid} 不在 yst_list 名字表"),
                )
                .into()
            })?;
        let sp = scriptpos::ScriptPos {
            if_entries: exec
                .if_stack
                .iter()
                .map(|f| scriptpos::IfEntry {
                    loop_depth: f.loop_depth as u8,
                    entry_pc: f.entry_pc,
                    end_pc: f.end_pc,
                })
                .collect(),
            gosub_entries: exec
                .frames
                .iter()
                .skip(1)
                .map(|f| scriptpos::GosubEntry {
                    if_depth: f.if_depth as u8,
                    loop_depth: f.loop_depth as u8,
                    return_pc: f.return_pc,
                    return_script: f.return_script as u16,
                })
                .collect(),
            loop_entries: exec
                .loops
                .iter()
                .map(|f| scriptpos::LoopEntry {
                    if_depth: f.if_depth as u8,
                    body_pc: f.body_pc,
                    end_pc: f.end_pc,
                    counter: f.counter,
                    limit: f.limit,
                })
                .collect(),
            text_line: exec.text_line as i32 as u32,
            pc: exec.pc,
            script_name: name,
            tail_byte: 0,
        };
        // 线格式由宿主按**存档容器版本**注入(版本→格式的唯一表在
        // yu_save::profile;0.450–0.479(0.13) = u16 GOSUB 计数,0.480+ = u8)。
        // 写错宽度会让官方读档时脚本名错位(「417」)。
        sp.encode_with_wire(self.scriptpos_wire())
            .map_err(|e| -> VmError {
                diag::fatal(code::rt::SCRIPT_POS, Boundary::Script, e).into()
            })
    }

    pub(super) fn decode_scriptpos(&self, blob: &[u8]) -> Result<scriptpos::ScriptPos, VmError> {
        scriptpos::ScriptPos::decode_for_wire(blob, self.scriptpos_wire())
            .map(|(sp, _)| sp)
            .map_err(|e| -> VmError {
                diag::fatal(code::rt::SCRIPT_POS, Boundary::Script, e).into()
            })
    }

    /// 解码 SCRIPTPOS:优先引擎线格式;旧内核 8 字节 blob 走兼容路径。
    ///
    /// 旧格式(2026-10-26 前)是 `[script_id u32 LE][pc u32 LE]`,没有三栈
    /// 与 text_line;脚本号直接按 yst_list 定位(不依赖脚本名字节)。仅用于
    /// 读旧存档;新存档/官方存档仍走 [`scriptpos::ScriptPos::decode`]。
    pub(super) fn decode_scriptpos_compat(
        &self,
        bytes: &[u8],
    ) -> Result<(scriptpos::ScriptPos, Option<u32>), VmError> {
        if bytes.len() == 8 && bytes != scriptpos::MAGIC.as_slice() {
            let sid = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let pc = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            let known = u16::try_from(sid)
                .ok()
                .and_then(|s| self.tables().boot.scripts.path_of(s))
                .is_some();
            if !known {
                return Err(diag::fatal(
                    code::rt::SCRIPT_POS,
                    Boundary::Script,
                    format!("SCRIPTPOS: 旧内核 blob 的脚本号 {sid} 不在 yst_list 清单"),
                )
                .into());
            }
            return Ok((
                scriptpos::ScriptPos {
                    pc,
                    ..Default::default()
                },
                Some(sid),
            ));
        }
        let sp = self.decode_scriptpos(bytes)?;
        let sid = self.script_id_by_path(&sp.script_name)?;
        Ok((sp, Some(sid)))
    }

    pub(super) fn script_id_by_path(&self, name: &[u8]) -> Result<u32, VmError> {
        self.tables()
            .boot
            .scripts
            .entries
            .iter()
            .find(|e| e.path.as_slice() == name)
            .map(|e| e.index)
            .ok_or_else(|| -> VmError {
                diag::fatal(
                    code::rt::SCRIPT_POS,
                    Boundary::Script,
                    format!("SCRIPTPOS: 脚本名不在 yst_list 清单:{}", cp932_lossy(name)),
                )
                .into()
            })
    }

    /// 把引擎 SCRIPTPOS 解出的执行位写进内核 `ExecState`。
    ///
    /// 不触碰 `next_voice`(引擎装载器不写 `task+0x10`)与 TINT/TFLT/TSTR
    /// (TASK 参数落位,SCRIPTPOS 不序列化)。
    pub(super) fn apply_scriptpos(exec: &mut ExecState, sp: scriptpos::ScriptPos, sid: u32) {
        let base_a = sp.if_entries.len() as u8;
        exec.script_id = sid;
        exec.pc = sp.pc;
        exec.base_a = base_a;
        exec.text_line = i64::from(sp.text_line as i32);
        exec.if_stack = sp
            .if_entries
            .iter()
            .enumerate()
            .map(|(i, e)| IfFrame {
                entry_pc: e.entry_pc,
                end_pc: e.end_pc,
                // 引擎 IF 帧不序列化 base A;基址 = 该帧压入前的 IF 深度(0 基),
                // 与 `IFBREAK` 截断到该帧后的 `task+0x3e` 同口径。
                base_a: i as u8,
                loop_depth: e.loop_depth as usize,
            })
            .collect();
        exec.loops = sp
            .loop_entries
            .iter()
            .map(|e| LoopFrame {
                body_pc: e.body_pc,
                end_pc: e.end_pc,
                limit: e.limit,
                counter: e.counter,
                if_depth: e.if_depth as usize,
                base_a: e.if_depth,
            })
            .collect();
        exec.frames.clear();
        exec.frames.push(GosubFrame::new(sid, sp.pc, base_a));
        for e in &sp.gosub_entries {
            let mut f = GosubFrame::new(e.return_script as u32, e.return_pc, e.if_depth);
            f.if_depth = e.if_depth as usize;
            f.loop_depth = e.loop_depth as usize;
            exec.frames.push(f);
        }
    }

    /// `TASKINFO` 的脚本号 → 源码行(引擎 `task+0x38[pc]`);
    /// pc 是「下一条命令」下标,查表用当前命令下标 `pc-1`。
    pub(super) fn script_line(&self, sid: u32, pc: u32) -> i64 {
        self.tables
            .script(sid)
            .and_then(|s| s.source_position(pc.saturating_sub(1) as usize))
            .map(|v| v as i64)
            .unwrap_or(-1)
    }

    /// `TASKINFO FILE/FILE0`:脚本号 → `yst_list` 路径字节
    /// (引擎 `0x801850[script_no]`;不是 `script_id` 的十进制串)。
    pub(super) fn script_path_bytes(&self, sid: u32) -> Vec<u8> {
        u16::try_from(sid)
            .ok()
            .and_then(|s| self.tables().boot.scripts.path_of(s))
            .map(|p| p.to_vec())
            .unwrap_or_default()
    }

    /// ID 未命中时按 LET 目标类型给出引擎默认值(`0x452918`:INT 0 /
    /// FLT 0.0 / STR 空串;其它类型不写)。
    pub(super) fn taskinfo_default(&self, target: &crate::expr::LValue) -> Option<Value> {
        let (ty, _) = self.var_shape(target.var)?;
        Some(match ty {
            ElemType::Int => Value::Int(0),
            ElemType::Float => Value::Float(0.0),
            ElemType::Str => Value::Str(Vec::new()),
        })
    }

    /// `TASKINFO` 的 LET 类型闸门。
    ///
    /// 引擎写字符串走 `0x43f0d8`(目标必须是 STR,否则不写),写数值走
    /// `0x455540`(目标必须是 INT/FLT,否则不写)。类型不符时只留一条
    /// Recoverable 诊断(按 query + 目标类型去重,避免热循环刷屏),
    /// 不把值硬转进异型变量。
    pub(super) fn write_taskinfo_value(
        &mut self,
        st: &ExecState,
        target: &crate::expr::LValue,
        query: &str,
        value: &Value,
    ) -> Result<(), VmError> {
        // 系统变量写入策略(DIV-RT-007)优先交给 write_target 报挂号。
        if target.var.id < self.sys_count {
            return self.write_target(st, target, value);
        }
        let Some((ty, _)) = self.var_shape(target.var) else {
            return self.write_target(st, target, value);
        };
        let compatible = match value {
            Value::Str(_) => matches!(ty, ElemType::Str),
            Value::Int(_) | Value::Float(_) => matches!(ty, ElemType::Int | ElemType::Float),
        };
        if compatible {
            self.write_target(st, target, value)?;
        } else {
            let key = format!("{query}:{ty:?}");
            if self.taskinfo_type_reported.insert(key) {
                self.report(diag::recoverable(
                    code::rt::VAR_TYPE,
                    Boundary::Script,
                    format!("TASKINFO[{query}] LET 目标类型 {ty:?} 与查询值类型不符;按引擎不写入"),
                ));
            }
        }
        Ok(())
    }
}
