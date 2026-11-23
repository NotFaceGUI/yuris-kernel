//! `PRODUCT`:v555 商业产品钩子(COM 分支)。
//!
//! 由 v555 分支移植并按新命令族布局落位;未取证 COM 逐项挂号,
//! 官方同型落空返回 0 的 COM 保持 no-op。

use super::*;

impl Vm {
    /// `PRODUCT`(v555 商业分支;<v555-4> 处理体 `0x459670`,4241 B)。
    ///
    /// 参数:`COM`(checker)+ `LET/PINT1..10/PFLT1..10/PSTR1..10`。
    /// 官方是**运行期产品钩子**:同一 `COM` 在文件/调试/渲染各域做工具
    /// 动作。已取证分支见 `docs/formats/v555-commercial.md` §7.3/§16.4。
    ///
    /// 本实现只落地**有明确语义且与本内核宿主一致**的分支:
    /// - `COM=29`(0x1D):把模式位 `DAT_00872410`(BSS 初值 0)写入 LET;
    /// - `COM=21/24`(0x15/0x18):按 `PSTR` 目录前缀枚举文件,结果写入
    ///   `_DROPFILE`/`_DROPFILE_NUM`;官方在宿主文件系统/引擎文件表上枚举,
    ///   本内核落到已挂载 VFS(封包 + 散盘),这是 555 场景文件列表的来源;
    /// - `COM=51/52`(0x33/0x34):日志初始化/写 `debuglog.txt`;宿主无该
    ///   调试文件,同型 no-op(Info 一次);
    /// - `COM=80/81`(0x50/0x51):渲染路径标志 `DAT_0087215c` no-op;
    /// - 官方分支里依赖 Win32 文件/对话框/注册表/浏览器、或结果语义尚未
    ///   取证的分支:各 COM **显式挂账一次**(不 fatal,不猜 LET 值);
    /// - 官方处理体也落空返回 0 的 COM(实测 `12/90/1001/20011..14` 等):
    ///   同型 no-op,不挂号(否则会与官方行为相反)。
    pub(crate) fn op_product(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let names: &[String] = &cmd.param_names;
        let mut com: Option<i64> = None;
        let mut target: Option<LValue> = None;
        // COM=21/24 的目录前缀 / COM=12 的源路径;只取第一个 `PSTR`。
        let mut pstr: Option<Vec<u8>> = None;
        // COM=12 的目标路径。
        let mut pstr2: Option<Vec<u8>> = None;
        // COM=22 的 PINT/PINT2/PINT3:`[偏移, 字符串数组槽, 整数数组槽]`。
        let mut pints: [Option<i64>; 3] = [None; 3];
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match name {
                // LET 是输出目标(左值);不求值成右值。
                "LET" => target = Some(self.eval_lvalue_window(st, w)?),
                "COM" => match self.eval_window(st, w) {
                    Ok(v) => com = v.as_int(),
                    Err(d) => self.report(d),
                },
                "PSTR" if pstr.is_none() => match self.eval_window(st, w) {
                    Ok(Value::Str(bytes)) => pstr = Some(bytes),
                    Ok(other) => self.report(diag::recoverable(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        format!("PRODUCT[PSTR] 需要字符串,得到 {}", other.type_name()),
                    )),
                    Err(d) => self.report(d),
                },
                "PSTR2" if pstr2.is_none() => match self.eval_window(st, w) {
                    Ok(Value::Str(bytes)) => pstr2 = Some(bytes),
                    Ok(other) => self.report(diag::recoverable(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        format!("PRODUCT[PSTR2] 需要字符串,得到 {}", other.type_name()),
                    )),
                    Err(d) => self.report(d),
                },
                "PINT" | "PINT2" | "PINT3" => {
                    let index = match name {
                        "PINT" => 0,
                        "PINT2" => 1,
                        _ => 2,
                    };
                    match self.eval_window(st, w) {
                        Ok(v) => pints[index] = v.as_int(),
                        Err(d) => self.report(d),
                    }
                }
                // PINT4../PFLT/PSTR2.. 是输入参数;求值以保持官方「先装载
                // 全部参数」的副作用与错误次序。
                _ => {
                    if let Err(d) = self.eval_window(st, w) {
                        self.report(d);
                    }
                }
            }
        }
        let Some(com) = com else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "PRODUCT 缺 COM",
            ));
            return Ok(StepOutcome::Continue);
        };
        // v555 商业钩子的参数协议只能从调用点还原;每次调用都留一条
        // verbose 记录(默认不刷屏,排查时按 `PRODUCT` 过滤即可)。
        // 证据:`PRODUCT[COM=22]` 的 PINT2/PINT3 在 v555 语料里是
        // **目标变量槽号**(样本 3 实测 2918/2919),不是数组内容。
        self.report(diag::verbose(
            code::rt::PRODUCT_TRACE,
            Boundary::Script,
            format!(
                "PRODUCT[COM={com}] PINT={:?} PINT2={:?} PINT3={:?} PSTR={}B",
                pints[0],
                pints[1],
                pints[2],
                pstr.as_deref().map_or(0, <[u8]>::len)
            ),
        ));
        match com {
            // 0x1D:模式位(BSS 初值 0)写入 LET。
            29 => {
                if let Some(target) = target {
                    self.write_target(st, &target, &Value::Int(0))?;
                }
            }
            // 0x0C:文件复制。官方 2.1MB 族 `FUN_0045a70c` 打开源文件、
            // 建目标、复制、关闭,返回 0/-1。内核复用 FILEACT[COPY] 的宿主
            // 落地路径(save_host 同步消费),把 PSTR/PSTR2 映射成
            // FILE/FILE2/COPY=1;不写 LET。
            12 => {
                let (Some(from), Some(to)) = (pstr.as_deref(), pstr2.as_deref()) else {
                    self.report(diag::recoverable(
                        code::rt::CMD_SLOT,
                        Boundary::Script,
                        "PRODUCT[COM=12] 需要 PSTR(源)与 PSTR2(目标)",
                    ));
                    return Ok(StepOutcome::Continue);
                };
                let mut from = from.to_vec();
                let mut to = to.to_vec();
                for b in &mut from {
                    if *b == b'/' {
                        *b = b'\\';
                    }
                }
                for b in &mut to {
                    if *b == b'/' {
                        *b = b'\\';
                    }
                }
                let yscm = &self.tables().boot.yscm;
                let Some(fileact) = yscm.index_of("FILEACT") else {
                    self.report_unimplemented_variant(
                        "PRODUCT",
                        "COM=12/FILEACT",
                        "工程 ysc 缺 FILEACT,PRODUCT[COM=12] 无法映射文件复制",
                    );
                    return Ok(StepOutcome::Continue);
                };
                let Some(decl) = yscm.command(fileact as u8) else {
                    return Ok(StepOutcome::Continue);
                };
                let slot = |name: &str| {
                    decl.params
                        .iter()
                        .position(|p| p.name == name)
                        .and_then(|i| u8::try_from(i).ok())
                };
                let (Some(file_slot), Some(file2_slot), Some(copy_slot)) =
                    (slot("FILE"), slot("FILE2"), slot("COPY"))
                else {
                    self.report_unimplemented_variant(
                        "PRODUCT",
                        "COM=12/FILEACT-slots",
                        "FILEACT 参数表缺 FILE/FILE2/COPY,PRODUCT[COM=12] 无法映射",
                    );
                    return Ok(StepOutcome::Continue);
                };
                self.emit_domain(
                    Domain::File,
                    "FILEACT",
                    vec![
                        ArgValue::new(file_slot, Value::Str(from)),
                        ArgValue::new(file2_slot, Value::Str(to)),
                        ArgValue::new(copy_slot, Value::Int(1)),
                    ],
                );
            }
            // 0x15/0x18:按 `PSTR` 目录前缀枚举文件 → `_DROPFILE*`。
            // 无 PSTR 时清空(与官方「没有目录可枚举」一致,不猜)。
            21 | 24 => {
                let count = match pstr.as_deref() {
                    Some(prefix) => {
                        let files = self.list_asset_dir(prefix);
                        let n = files.len();
                        if let Some(state) = self.other.as_mut() {
                            state.drop_files = files;
                            state.drop_file_num = n as i32;
                        }
                        n
                    }
                    None => {
                        if let Some(state) = self.other.as_mut() {
                            state.drop_files.clear();
                            state.drop_file_num = 0;
                        }
                        0
                    }
                };
                let label = pstr
                    .as_deref()
                    .map(|p| format!("{:?}", String::from_utf8_lossy(p)))
                    .unwrap_or_else(|| "<缺 PSTR>".to_string());
                self.report(diag::verbose(
                    code::rt::PRODUCT_TRACE,
                    Boundary::Script,
                    format!("PRODUCT[COM={com}] PSTR={label}: 枚举 {count} 个文件"),
                ));
            }
            // 0x16:555 独立剧本的词法器。完整状态机尚未移植;当前落地
            // **首 token 分类**(label lexer 用它区分 `#标签` / `\命令` /
            // `//` 注释 / 空行,三类是 label 表构建的全部输入)。
            // 官方输出契约(b3c02 `0x45a874`):逐 token 调 `FUN_0045baf8`
            // 写 INT 元素 `(type_code, 0)`(高 32 位恒 0,无位置/长度),
            // 文本写 STR 数组;行尾/注释 token 类型 0、文本 "\r";空行
            // 0 token;最后把 token 数写 LET。调用方只读类型码/文本/计数。
            22 => {
                // v555 标签/文本词法器的输入行。只开 verbose(默认不刷屏);
                // 排查重复标签/截断时用它逐行对齐脚本与内核 token。
                if let Some(line) = pstr.as_deref() {
                    let preview = &line[..line.len().min(96)];
                    self.report(diag::verbose(
                        code::rt::PRODUCT_TRACE,
                        Boundary::Script,
                        format!(
                            "PRODUCT[COM=22] line={:?}",
                            String::from_utf8_lossy(preview)
                        ),
                    ));
                }
                let codec = self.script_text_codec(st.script_id);
                // `/* ... */` 块注释跨行:状态随 `other` 存活,下一行继续。
                let mut lex = self
                    .other
                    .as_ref()
                    .map(|s| s.scenario_lex)
                    .unwrap_or_default();
                let tokens = pstr
                    .as_deref()
                    .map(|line| {
                        crate::other::scenario::tokenize_stateful(line, codec, &mut lex)
                    })
                    .unwrap_or_default();
                if let Some(state) = self.other.as_mut() {
                    state.scenario_lex = lex;
                }
                if pstr
                    .as_deref()
                    .is_some_and(|line| line.windows(8).any(|w| w == b"03_01_22"))
                {
                    let dump: Vec<String> = tokens
                        .iter()
                        .map(|t| format!("{:#04x}:{}", t.code, String::from_utf8_lossy(&t.text)))
                        .collect();
                    self.report(diag::verbose(
                        code::rt::PRODUCT_TRACE,
                        Boundary::Script,
                        format!("PRODUCT[COM=22] tokens={}", dump.join(" | ")),
                    ));
                }
                // 官方逐 token 只写 (type_code, 0) + 文本;没有位置/长度
                // 字段(高 32 位恒 0),此前「未输出字段」的挂号不再成立。
                let arrays = self.scenario_token_vars(
                    pints[0].unwrap_or(0),
                    pints[1].unwrap_or(0),
                    pints[2].unwrap_or(0),
                );
                if let Some((str_var, int_var)) = arrays {
                    self.report(diag::verbose(
                        code::rt::PRODUCT_TRACE,
                        Boundary::Script,
                        format!(
                            "PRODUCT[COM=22] 输出 {} {:?} / {} {:?}",
                            str_var.display(),
                            self.var_shape(str_var).map(|(_, d)| d),
                            int_var.display(),
                            self.var_shape(int_var).map(|(_, d)| d)
                        ),
                    ));

                    // 官方 PINT 是行缓冲/位置指针,不是元素下标;token
                    // 文本/码从 0 开始写(消费方按 0..N-1 读)。
                    // PINT 值只进上面的 verbose 记录,不参与寻址。
                    let offset = 0usize;
                    let sdim = self
                        .var_shape(str_var)
                        .and_then(|(_, d)| d.first().copied())
                        .unwrap_or(0);
                    let idim = self
                        .var_shape(int_var)
                        .and_then(|(_, d)| d.first().copied())
                        .unwrap_or(0);
                    // 官方写手在 token 数超过 STR/INT 数组维度时调用
                    // `FUN_0046be30(0x18b14)` 报越界错误;旧实现静默截断
                    // (顶层 1023 上限 + 逐条 `< dim` 跳过),会让标签表
                    // “少几条但不是错误”。这里显式报可恢复诊断,写入仍
                    // 按数组维度截断(越界后的官方流程未取证)。
                    if tokens.len() > sdim || tokens.len() > idim {
                        self.report(diag::recoverable(
                            code::rt::VAR_SHAPE,
                            Boundary::Script,
                            format!(
                                "PRODUCT[COM=22] token 数 {} 超过输出数组维度 \
                                 STR={sdim} INT={idim};官方写手越界报错误 0x18b14",
                                tokens.len()
                            ),
                        ));
                    }
                    for (i, tok) in tokens.iter().enumerate() {
                        if offset + i < sdim {
                            if let Err(d) = self.store_mut().write(
                                str_var,
                                &[(offset + i) as i64],
                                &Value::Str(tok.text.clone()),
                            ) {
                                self.report(d);
                                break;
                            }
                        }
                        if i < idim {
                            if let Err(d) =
                                self.store_mut()
                                    .write(int_var, &[i as i64], &Value::Int(tok.code))
                            {
                                self.report(d);
                                break;
                            }
                        }
                    }
                } else {
                    // 没有输出数组时只写 LET 计数(调用方仍可推进);必须留痕,
                    // 否则标签表会“0 条但不是错误”。
                    self.report_unimplemented_variant(
                        "PRODUCT",
                        "COM=22/vars",
                        "PRODUCT[COM=22] 未定位到 token 输出数组\
                         (PINT2/PINT3 未回填,且 ysv 无唯一 STR[1024]+INT[1024] 对);\
                         文本/类型码未写,只返回 token 数",
                    );
                }
                if let Some(target) = target {
                    self.write_target(st, &target, &Value::Int(tokens.len() as i64))?;
                }
            }
            // 0x17:遍历源字符串数组,把每行长度写入 PINT 目标数组,
            // 非空行数写入 LET。官方处理体 `FUN_0045bcf8`(108B):
            //   do { s = FUN_0044d66c(source, i); n = strlen(s);
            //        FUN_0045bc60(target, i, n); i++; } while (n != 0);
            // 返回非空行数(调用方再写 LET)。
            // 语料证据:<v555-4> `es_scenario_load.yst` s60 pc36:
            //   PRODUCT | COM=23 | PINT=@3004 | PINT2=@3005 | LET=@2881
            // `@3004`/`@3005` 分别持有目标 INT 数组槽与源 STR 数组槽
            // (`@3004=@0b3b`、`@3005=$0b3e`)。旧实现只写
            // `last_text_lines`,漏了 PINT 长度数组。
            23 => {
                let mut count = self.last_text_lines() as i64;
                if let (Some(target_slot), Some(source_slot)) = (pints[0], pints[1]) {
                    if let (Ok(target_slot), Ok(source_slot)) =
                        (u16::try_from(target_slot), u16::try_from(source_slot))
                    {
                        let src = self.store().find_by_id(source_slot, VarSpace::Dollar);
                        let dst = self.store().find_by_id(target_slot, VarSpace::At);
                        if let (Some(src), Some(dst)) = (src, dst) {
                            let src_shape = self.var_shape(src);
                            let dst_shape = self.var_shape(dst);
                            if let (Some((src_ty, src_dims)), Some((dst_ty, dst_dims))) =
                                (src_shape, dst_shape)
                            {
                                if src_ty == ElemType::Str && dst_ty == ElemType::Int {
                                    let src_len = src_dims.first().copied().unwrap_or(0);
                                    let dst_len = dst_dims.first().copied().unwrap_or(0);
                                    let mut n = 0usize;
                                    for i in 0..src_len.min(dst_len) {
                                        let value = self.store().read(src, &[i as i64]);
                                        let Ok(Value::Str(bytes)) = value else {
                                            break;
                                        };
                                        if bytes.is_empty() {
                                            break;
                                        }
                                        if let Err(d) = self.store_mut().write(
                                            dst,
                                            &[i as i64],
                                            &Value::Int(bytes.len() as i64),
                                        ) {
                                            self.report(d);
                                            break;
                                        }
                                        n += 1;
                                    }
                                    count = n as i64;
                                    self.report(diag::verbose(
                                        code::rt::PRODUCT_TRACE,
                                        Boundary::Script,
                                        format!(
                                            "PRODUCT[COM=23] {} 行 → 长度数组 {} + LET",
                                            n,
                                            dst.display()
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                }
                if let Some(target) = target {
                    self.write_target(st, &target, &Value::Int(count))?;
                }
            }
            // 0x22:存档盘剩余字节数。官方 2.1MB 族 `0x45c1d0` =
            // `GetDiskFreeSpaceExA(path, &free, …)`,结果写 LET。
            // 磁盘容量是宿主事实,由 `BootConfig::disk_free_bytes` 注入;
            // 未注入时写 0 并挂号,不猜容量。
            34 => {
                let free = self.disk_free_bytes();
                if let Some(target) = target {
                    let value = free.and_then(|n| i64::try_from(n).ok()).unwrap_or(0);
                    self.write_target(st, &target, &Value::Int(value))?;
                }
                match free {
                    Some(n) => self.report(diag::verbose(
                        code::rt::PRODUCT_TRACE,
                        Boundary::Script,
                        format!("PRODUCT[COM=34] 存档盘剩余 {n}B → LET"),
                    )),
                    None => self.report_aligned_stub(
                        "PRODUCT.COM=34",
                        "PRODUCT[COM=34] GetDiskFreeSpaceExA:宿主未注入磁盘剩余空间,写 0",
                    ),
                }
            }
            // 0x33/0x34:日志初始化 / 写 debuglog.txt。
            51 | 52 => {
                self.report_aligned_stub(
                    "PRODUCT",
                    "COM=51/52 日志初始化/写 debuglog.txt:宿主无该调试文件,同型 no-op",
                );
            }
            // 0x50/0x51:渲染路径(D3D)标志,宿主渲染路径由平台决定。
            80 | 81 => {
                self.report_aligned_stub(
                    "PRODUCT",
                    "COM=80/81 渲染路径标志 DAT_0087215c:宿主渲染路径由平台决定,no-op",
                );
            }
            // 0x64/0x6E:ShowWindow(隐藏/显示)。窗口生命周期归宿主。
            100 | 110 => {
                self.report_aligned_stub(
                    "PRODUCT",
                    "COM=100/110 ShowWindow:窗口生命周期归宿主,no-op",
                );
            }
            // 0x78:ShellExecuteA 打开 IE —— 安全拒绝,不执行外部程序。
            120 => {
                self.report_unimplemented_variant(
                    "PRODUCT",
                    "COM=120",
                    "PRODUCT[COM=120] ShellExecuteA(打开 IE):宿主不执行外部程序(安全策略)",
                );
            }
            // 已取证但依赖 Win32 文件/对话框/注册表、或 LET 结果语义
            // 尚未闭合的分支:显式挂账,不猜值。
            26 | 33 | 36 | 37 | 38 | 10_201 | 10_205 | 21_000 | 21_031 | 0x2776..=0x2779 => {
                let detail = format!(
                    "PRODUCT[COM={com}] 分支已取证但未实现(文件/对话框/注册表工具;\
                     见 docs/formats/v555-commercial.md §7.3)"
                );
                self.report_unimplemented_variant("PRODUCT", &format!("COM={com}"), &detail);
            }
            // 官方处理体同样落空返回 0(实测 12/90/1001/20011..14):同型
            // no-op。列在这里的 COM 不挂号;其余未知 COM 只报一次 Info,
            // 保证新商业版的私有钩子不会“静默消失”。
            90 | 1001 | 20_011..=20_014 => {}
            _ => {
                let key = format!("PRODUCT.COM={com}");
                let detail = format!(
                    "PRODUCT[COM={com}] 未列入已取证分支;官方处理体未知,\
                     按 no-op 对齐(如需实现请补 REA/trace 后加分支)"
                );
                self.report_aligned_stub(&key, &detail);
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `PRODUCT[COM=22]` 的 token 输出数组定位(独立运行面内固定按 555 形态)。
    ///
    /// 先按 `PINT`/`PINT2`/`PINT3` 找 `STR[] + INT[]` 对;v555 商业样本里
    /// PINT2/PINT3 不一定回填(实测 `<v555-2>` 启动链为空),仍需 PINT 侧
    /// 的相邻槽兜底,否则标签表为空、主任务提前 END(窗口闪退)。
    /// 最后的"相邻 1024 数组对"只在 ysv 里唯一时使用。
    ///
    /// **挂账**:PINT 侧的 `+1/+4/+5` 尚无 REA/trace 证据,是让现有商业样本
    /// 跑通的形状启发式;待取证档案见 `DIV-RT-076`,取证前不删。
    fn scenario_token_vars(
        &mut self,
        pint: i64,
        pint2: i64,
        pint3: i64,
    ) -> Option<(VarRef, VarRef)> {
        let records = &self.tables().boot.vars.records;
        let find = |id: u16, ty: u8| {
            records
                .iter()
                .find(|r| r.var_id == id && r.value_type == ty)
                .map(|r| (r.var_id, r.dims.clone()))
        };
        let mut pair: Option<((u16, Vec<u32>), (u16, Vec<u32>))> = None;
        for cand in [pint, pint.saturating_sub(4)] {
            let Ok(sid) = u16::try_from(cand) else {
                continue;
            };
            let Some((s, sdims)) = find(sid, 3) else {
                continue;
            };
            if sdims.as_slice() != [1024] {
                continue;
            }
            for iid in [
                sid.saturating_add(1),
                sid.saturating_add(4),
                sid.saturating_add(5),
            ] {
                if let Some((i, idims)) = find(iid, 1) {
                    if idims.as_slice() == [1024] {
                        pair = Some(((s, sdims.clone()), (i, idims)));
                        break;
                    }
                }
            }
            if pair.is_some() {
                break;
            }
        }
        if pair.is_none() {
            if let (Ok(sid), Ok(iid)) = (u16::try_from(pint2), u16::try_from(pint3)) {
                if let (Some((s, sdims)), Some((i, idims))) = (find(sid, 3), find(iid, 1)) {
                    if sdims.len() == 1 && idims.len() == 1 {
                        pair = Some(((s, sdims), (i, idims)));
                    }
                }
            }
        }
        if pair.is_none() {
            for w in records.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                if a.value_type == 3
                    && a.dims.as_slice() == [1024]
                    && b.value_type == 1
                    && b.dims.as_slice() == [1024]
                {
                    pair = Some(((a.var_id, a.dims.clone()), (b.var_id, b.dims.clone())));
                    break;
                }
            }
        }
        let ((sid, sdims), (iid, idims)) = pair?;
        for (id, ty, dims) in [(sid, 3u8, sdims), (iid, 1u8, idims)] {
            let et = ElemType::from_code(ty)?;
            let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
            self.store_mut().declare_if_absent(
                VarRef::new(VarSpace::for_elem(et), id),
                et,
                &dims,
                &et.default_value(),
            );
        }
        Some((
            VarRef::new(VarSpace::Dollar, sid),
            VarRef::new(VarSpace::At, iid),
        ))
    }
}
