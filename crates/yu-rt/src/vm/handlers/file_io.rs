//! 文件/存档域:CSV/CSVINFO/FILEINFO/SAVE/LOAD/SAVEEND。
//!
//! 由 `handlers/mod.rs` 的命令族拆分而来;只搬运,函数体逐字不变。

use super::helpers::*;
use super::*;

impl Vm {

    /// `CSV`:把文件解析成单元格串,顺序写入 `LET` 的字符串数组。
    ///
    /// 证据(REA:本地 v474 商业样本,样本名/哈希不入库;命令表初始化器
    /// `FUN_00451f24` 把序号 8 指向 `0x43577c`):
    /// - 处理体先按 `FILE` 打开文件,失败返回 -1;
    /// - 读取原始字节后**追加 CRLF**(`0x435866..0x43587f`),再按
    ///   `,`/CR/LF 切分;引号 `"`/`'` 只保护分隔符(引号本身保留在
    ///   单元格里);行首空格/Tab 跳过(`0x435948..0x435972`);
    /// - 每个单元格经 `0x435c04` 写入 `LET` 字符串数组的连续下标
    ///   (下标从 0 开始,跨行连续;不是二维表)。
    ///
    /// 这不是 `0.495` SDK 命令表序号 7 的 `0x43e640`(那是另一支的
    /// 空桩);v474 的表比 SDK 多一个槽位,序号 8/9 才是 CSV/CSVINFO。
    /// 处理器选择在 [`build_handler_table`] 按**表结构**裁决:表里有
    /// `LINE`(0.466)或序号 != 7(v474)→ 本解析器;否则 `NoopEval`。
    ///
    /// **0.466 `LINE` 语义(REA `0x431970`)**:`LINE` 是 1 基行号过滤,
    /// `<=0`/缺省 = 全部行;每遇 CR/LF 行号 +1,只有目标行的单元格写入
    /// `LET`(下标从 0 重新计),空行在目标行时写一个空串。
    pub(super) fn op_csv(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let names: &[String] = &cmd.param_names;
        let mut file: Option<Vec<u8>> = None;
        let mut target: Option<crate::expr::LValue> = None;
        let mut line: Option<i64> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or(""); // silent-ok: 槽号越界 = 无名(按名判断自然落空)
            match name {
                "LET" => target = Some(self.eval_lvalue_window(st, w)?),
                "FILE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        if let Value::Str(bytes) = v {
                            file = Some(bytes);
                        }
                    }
                }
                "LINE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        line = v.as_int();
                    }
                }
                _ => {
                    let _ = self.eval_window(st, w);
                }
            }
        }
        let Some(target) = target else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "CSV 缺少 LET 参数".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        };
        let Some(path) = file else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "CSV 缺少 FILE 参数".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        };
        let Some(bytes) = self.open_asset(&path) else {
            // 官方 `0x45529d` 打开失败 ⇒ 处理体返回 -1(本帧让出,不终止)。
            self.report(diag::recoverable(
                code::rt::CSV,
                Boundary::Script,
                format!(
                    "CSV 文件 `{}` 不可读(官方 fopen 失败 ⇒ 返回 -1,本帧让出)",
                    String::from_utf8_lossy(&path)
                ),
            ));
            return Ok(StepOutcome::Yield);
        };
        // 0.466 `LINE` 过滤:1 基行号,`<=0`/缺省 = 全部行。
        let cells: Vec<Vec<u8>> = parse_csv_cells_lined(&bytes, self.script_codec)
            .into_iter()
            .filter(|(ln, _)| line.is_none_or(|target| target <= 0 || *ln as i64 == target))
            .map(|(_, cell)| cell)
            .collect();
        self.write_csv_cells(st, &target, cells, "CSV")?;
        Ok(StepOutcome::Continue)
    }

    /// CSV 单元格串 → `LET` 字符串数组(CSV / CSVINFO[DATA] 共用)。
    ///
    /// 官方 `0x435c04` / `0x43f0d8` 在目标数组不够长时会 realloc 扩容;
    /// 本内核的变量域固定维长,不能静默越界。容量不足时停在最后一个可写
    /// 单元格并挂一条可恢复诊断(不 fatal,脚本继续)。官方还要求目标是
    /// **字符串**变量(`0x43f0d8` 检查类型字节 == 3),类型不符时报错。
    pub(super) fn write_csv_cells(
        &mut self,
        st: &ExecState,
        target: &crate::expr::LValue,
        cells: Vec<Vec<u8>>,
        cmd: &str,
    ) -> Result<(), VmError> {
        let var = self.canonical_var(target.var);
        let Some((ty, dims)) = self.var_shape(var) else {
            self.report(diag::recoverable(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!("{cmd} 目标未声明 {}", var.display()),
            ));
            return Ok(());
        };
        if ty != crate::value::ElemType::Str {
            self.report(diag::recoverable(
                code::rt::VAR_TYPE,
                Boundary::Script,
                format!(
                    "{cmd} 目标 {} 不是字符串变量(官方 0x43f0d8 检查类型并报错)",
                    var.display()
                ),
            ));
            return Ok(());
        }
        let capacity = dims.iter().copied().product::<usize>().max(1);
        let total = cells.len();
        // 消费式遍历:单元格字节直接 move 进变量值,省掉逐格 clone。
        for (i, cell) in cells.into_iter().enumerate() {
            if i >= capacity {
                self.report(diag::recoverable(
                    code::rt::CSV,
                    Boundary::Script,
                    format!(
                        "{cmd} 单元格数 {total} 超过 LET 容量 {capacity}(官方会扩容;内核固定维长,余下忽略)"
                    ),
                ));
                break;
            }
            let indices = flat_indices(&dims, i);
            let lv = crate::expr::LValue {
                var,
                indices: indices.into_iter().collect(),
            };
            self.write_target_owned(st, &lv, Value::Str(cell))?;
        }
        Ok(())
    }
    /// `CSVINFO`:统计/读取 CSV,结果写 `LET`。
    ///
    /// 两代处理器按**参数表结构**区分(不看版本号):
    ///
    /// - **0.466**(表里有 `LINE`,REA `0x432050`):
    ///   - `SIZE=1`:单元格计数;`LINE>0` 只数该 1 基行;空行不计;
    ///     文件末行无终止符时**计入**最后一格(EOF 分支 `0x43225a`);
    ///   - `SIZE=0` 且 `LINE` 在场:写**行数**(= CR/LF 序列数 + 1,
    ///     `0x4322f2` 把 `ebp` 写回);
    ///   - 两者都不在场:返回 0。
    /// - **0.479/0.495**(无 `LINE`,REA `0x43e660`):
    ///   - `DATA=1`:把单元格串按顺序写入 `LET` 字符串数组
    ///     (`0x43f0d8`,类型必须为字符串;数组不足时官方 realloc);
    ///   - `SIZE=1`:计数(不追加 CRLF;末行无终止符的最后一格不计,
    ///     与 v474 `0x435cfc` 同);
    ///   - 两者同时在场:先写单元格,再写计数(官方 DATA 分支结束跳回
    ///     SIZE 分支)。
    ///
    /// 打开失败返回 -1(本帧让出)。
    pub(super) fn op_csvinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let names: &[String] = &cmd.param_names;
        // `LINE` 是**表结构**属性(0.466 表恒有),不是本次调用是否给值;
        // 用它决定计数口径,不能只看 `line` 是否为 Some。
        let has_line = names.iter().any(|p| p == "LINE");
        let mut file: Option<Vec<u8>> = None;
        let mut size = false;
        let mut data = false;
        let mut line: Option<i64> = None;
        let mut target: Option<crate::expr::LValue> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or(""); // silent-ok: 槽号越界 = 无名(按名判断自然落空)
            match name {
                "LET" => target = Some(self.eval_lvalue_window(st, w)?),
                "FILE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        if let Value::Str(bytes) = v {
                            file = Some(bytes);
                        }
                    }
                }
                "SIZE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        size = v.as_f64().map(|n| n != 0.0).unwrap_or(false);
                    }
                }
                "DATA" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        data = v.as_f64().map(|n| n != 0.0).unwrap_or(false);
                    }
                }
                "LINE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        line = v.as_int();
                    }
                }
                _ => {
                    let _ = self.eval_window(st, w);
                }
            }
        }
        // 0.466 的纯行数模式:`SIZE=0` + `LINE` 窗口在场。其余无模式 ⇒ 返回 0。
        if !size && !data && !(has_line && line.is_some()) {
            return Ok(StepOutcome::Continue);
        }
        let Some(path) = file else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("CSVINFO 缺少 FILE 参数({})", if data { "DATA" } else { "SIZE" }),
            ));
            return Ok(StepOutcome::Continue);
        };
        let Some(target) = target else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "CSVINFO 缺少 LET 参数".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        };
        let Some(bytes) = self.open_asset(&path) else {
            self.report(diag::recoverable(
                code::rt::CSV,
                Boundary::Script,
                format!(
                    "CSVINFO 文件 `{}` 不可读(官方 fopen 失败 ⇒ 返回 -1,本帧让出)",
                    String::from_utf8_lossy(&path)
                ),
            ));
            return Ok(StepOutcome::Yield);
        };
        if data {
            let cells = parse_csv_cells(&bytes, self.script_codec);
            self.write_csv_cells(st, &target, cells, "CSVINFO[DATA]")?;
        }
        if size {
            let count = if has_line {
                count_csv_cells_0466(&bytes, self.script_codec, line)
            } else {
                count_csv_cells(&bytes, self.script_codec)
            };
            self.write_target(st, &target, &Value::Int(count as i64))?;
        } else if has_line && line.is_some() {
            // 0.466 `CSVINFO[LINE]`(无 SIZE)= 行数。官方把当前行号 `ebp`
            // 写回;行号从 1 起,每遇 CR/LF +1。
            let lines = csv_line_count_0466(&bytes);
            self.write_target(st, &target, &Value::Int(lines as i64))?;
        }
        Ok(StepOutcome::Continue)
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
    pub(super) fn op_fileinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 旧写法每次执行都重建整张 `Vec<String>`(与 `CGINFO` 同一个 bug)。
        let names: &[String] = &cmd.param_names;
        let mut file: Option<Vec<u8>> = None;
        let mut folder: Option<Vec<u8>> = None;
        // `CGFILE`:图像文件名(无扩展名)。BT 按钮宏用它探测
        // `_off/_over/_on/_onover/_na` 是否存在(es_buttonque.yst:341..),
        // 与 `CG[FILE]` 共用图像扩展名探测。
        let mut cgfile: Option<Vec<u8>> = None;
        let mut exist = false;
        let mut size = false;
        let mut relation = false;
        let mut target: Option<crate::expr::LValue> = None;
        let mut args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or(""); // silent-ok: 槽号越界 = 无名(按名判断自然落空)
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
                "CGFILE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        if let Value::Str(bytes) = &v {
                            cgfile = Some(bytes.clone());
                        }
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
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
                "SIZE" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        size = v.as_f64().map(|n| n != 0.0).unwrap_or(false);
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "RELATION" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        relation = v.as_f64().map(|n| n != 0.0).unwrap_or(false);
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
        // CGFILE(0.495 `0x44286c` / v555 `0x43eca8`;BT 按钮宏
        // `es_buttonque.yst` pc341 起逐后缀探测):按 `CG[FILE]` 的扩展名
        // 顺序在 VFS 内查是否存在。官方把当前 CG 基路径(脚本已带
        // `cgsys/` 前缀)与图像扩展名一起拼;这里不猜前缀,只做 exact +
        // yscfg 扩展名序 + 通用 `.webp`。
        //
        // 0.495 与 v555 都读 CGFILE(0.495 `0x44286c` kind 2 → `ebx=3` →
        // `0x442c7f` 拼扩展名),不能按版本门控;`.webp` 是 v555 扩展名表
        // 新增项(`0x878a28`),按主人 2026-11-23 口径作为**通用扩展名能力**
        // 追加,不做 555 判断。
        if exist && target.is_some() {
            if let Some(bytes) = cgfile {
                let present = self.cg_file_exists(&bytes);
                self.report(diag::verbose(
                    code::rt::FILEINFO_TRACE,
                    Boundary::Script,
                    format!(
                        "FILEINFO[CGFILE] {:?} -> {}",
                        String::from_utf8_lossy(&bytes),
                        i32::from(present)
                    ),
                ));
                if let Some(target) = target {
                    self.write_target(st, &target, &Value::Int(i64::from(present)))?;
                }
                return Ok(StepOutcome::Continue);
            }
        }
        // FOLDER(手册例 2;REA `0x4433d6`):官方 `FindFirstFileA` 查散盘目录,
        // 只认 `FILE_ATTRIBUTE_DIRECTORY`;包内文件夹**不可查**(手册注明)。
        // - EXIST ⇒ 1/0;
        // - SIZE ⇒ 官方只支持 FILE(`0x4429a7` 仅 `ebx==2` 走 0x443572),
        //   FOLDER 落 0;
        // - RELATION ⇒ 官方对 FOLDER 写目标类型默认值(0/0.0/空串);
        // - 三者都未给 ⇒ 不写 LET。
        if let Some(folder) = folder {
            let Some(target) = target else {
                return Ok(StepOutcome::Continue);
            };
            if exist {
                let present = self.disk_dir_exists(&folder);
                self.write_target(st, &target, &Value::Int(i64::from(present)))?;
                return Ok(StepOutcome::Continue);
            }
            if size {
                self.write_target(st, &target, &Value::Int(0))?;
                return Ok(StepOutcome::Continue);
            }
            if relation {
                let ty = self
                    .var_shape(target.var)
                    .map(|(t, _)| t)
                    .unwrap_or(ElemType::Int); // silent-ok: 未声明目标按 INT 默认(官方按槽类型写默认值)
                let v = match ty {
                    ElemType::Int => Value::Int(0),
                    ElemType::Float => Value::Float(0.0),
                    ElemType::Str => Value::Str(Vec::new()),
                };
                self.write_target(st, &target, &v)?;
            }
            return Ok(StepOutcome::Continue);
        }
        // RELATION(手册例 4;REA `0x4429e6` → `0x442a20`:
        // `FindExecutableA(NULL, FILE, buf)` → 关联 exe 全路径写入**字符串**
        // LET;引擎按任务 kind 1/2/3 分派 int/float/str 槽)。失败/非 Windows
        // 宿主 = 空串。
        if relation {
            let Some(bytes) = file.clone() else {
                self.report_unimplemented(
                    "FILEINFO",
                    "RELATION=1 缺 FILE(file/fileinfo.html 例 4)",
                );
                return Ok(StepOutcome::Continue);
            };
            if let Some(target) = target.clone() {
                let resolved = self.resolve_file_path(&bytes);
                let assoc = resolved
                    .and_then(|path| self.file_association(&path))
                    .unwrap_or_default();
                self.write_target(st, &target, &Value::Str(assoc))?;
            }
            return Ok(StepOutcome::Continue);
        }
        // SIZE(手册例 3;REA `0x443572`:`FindFirstFileA` → 目录 = 0、
        // 文件 = `nFileSizeLow`(32 位);找不到 = 0)。封包内文件引擎同样
        // 查不到大小(手册注明),内核与官方一致:只查散盘/宿主路径。
        if size {
            let Some(bytes) = file.clone() else {
                self.report_unimplemented("FILEINFO", "SIZE=1 缺 FILE(file/fileinfo.html 例 3)");
                return Ok(StepOutcome::Continue);
            };
            let len = self
                .resolve_file_path(&bytes)
                .and_then(|path| std::fs::metadata(&path).ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .unwrap_or(0);
            if let Some(target) = target {
                self.write_target(st, &target, &Value::Int(len as i64))?;
            }
            return Ok(StepOutcome::Continue);
        }
        // 官方无查询项 / 无 LET / 无 FILE:处理体走到公共出口直接返回 0,
        // 不写任何变量(不是"未实现")。
        if !exist || target.is_none() {
            return Ok(StepOutcome::Continue);
        }
        let Some(bytes) = file else {
            // `EXIST` 但既无 FILE 也无 FOLDER:官方四个分支都不命中,不写。
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

    /// `FILEINFO[CGFILE EXIST]`:图像资产存在性(原 key + yscfg 扩展名序 +
    /// 通用 `.webp`)。
    ///
    /// 两代官方都读 `CGFILE`:0.495 `0x44286c`(kind 2 → `ebx=3` →
    /// `0x442c7f`)与 v555 `0x43eca8`(kind 2 → `FUN_0043f770`)。v555 的
    /// 图像扩展名表在 `.psb` 之后还有 `.webp`(`0x878a28`);`.webp` 按
    /// 通用扩展名能力追加,不做版本门控。
    fn cg_file_exists(&self, bytes: &[u8]) -> bool {
        let exists = |key: &[u8]| {
            yu_vfs::LookupKey::new(key, yu_vfs::Codec::Cp932)
                .ok()
                .is_some_and(|k| {
                    self.src.exists(&k) || self.aux.as_ref().is_some_and(|aux| aux.exists(&k))
                })
        };
        if exists(bytes) {
            return true;
        }
        // 已带扩展名:不再拼(官方候选序列是「原 key + 各扩展名」)。
        if bytes.iter().rev().take(5).any(|b| *b == b'.') {
            return false;
        }
        let order: Vec<u8> = self
            .tables()
            .boot
            .config
            .image_ext_order
            .iter()
            .copied()
            .take_while(|c| *c != 0)
            .collect();
        let order = if order.is_empty() {
            vec![1, 2, 3, 4]
        } else {
            order
        };
        let mut exts: Vec<&'static [u8]> = Vec::new();
        for code in order {
            let ext: &'static [u8] = match code {
                1 => b"png",
                2 => b"bmp",
                3 => b"jpg",
                4 => b"gif",
                5 => b"psb",
                // 未取证码不猜;`.webp` 在下面按通用能力补齐。
                _ => continue,
            };
            exts.push(ext);
        }
        // v555 扩展名表 `.png/.bmp/.jpg/.gif/.psb/.webp` 的最后一项。
        // 0.495 表没有 `.webp`,但旧语料不含该格式,通用追加不改变旧行为。
        if !exts.contains(&b"webp".as_slice()) {
            exts.push(b"webp");
        }
        for ext in exts {
            let mut key = bytes.to_vec();
            key.push(b'.');
            key.extend_from_slice(ext);
            if exists(&key) {
                return true;
            }
        }
        false
    }

    /// save 根文件系统存在性(FILEINFO 的散盘支):路径以注入的
    /// `$_SAVE_PATH` 为前缀时,剥出相对名做越界检查后查文件系统。
    /// 注入值 = save 根的显示字符串(正斜杠、尾 `/`,`yu-cli::host_paths`)。
    pub(super) fn save_root_fs_exists(&self, path: &[u8]) -> bool {
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

    /// 把 `FILE=` 的脚本路径解析成宿主文件系统路径(`SIZE`/`RELATION` 用)。
    ///
    /// 与 `save_root_fs_exists` 同一口径:`$_SAVE_PATH` 前缀 = save 根下相对
    /// 名;其余按封包外相对路径不解析(引擎对封包内文件同样查不到大小,
    /// 手册 `file/fileinfo.html` 例 3 注)。返回 `None` = 不可解析。
    pub(super) fn resolve_file_path(&self, path: &[u8]) -> Option<std::path::PathBuf> {
        let root = self
            .paths
            .save_path
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned())?;
        let rel = path.strip_prefix(root.as_bytes())?;
        if rel.is_empty() {
            return None;
        }
        let text = std::str::from_utf8(rel).ok()?;
        if text.contains('\\') || text.split('/').any(|c| c == "..") {
            return None;
        }
        Some(std::path::Path::new(&root).join(text))
    }

    /// `FILEINFO[FOLDER EXIST]` 的散盘目录检查(REA `0x4433d6`
    /// `FindFirstFileA` + `FILE_ATTRIBUTE_DIRECTORY`)。
    ///
    /// 官方相对路径以进程 CWD(游戏根)解析 ⇒ 这里用 `$_SYSTEM_PATH`
    /// (= 工程根)拼接;绝对路径原样查。`..` 一律拒绝,避免脚本探测根外目录。
    /// 包内文件夹不可查(手册明示),因此不走 VFS。
    pub(super) fn disk_dir_exists(&self, path: &[u8]) -> bool {
        // 脚本字符串是 CP932;官方 A 版 API 按系统 ANSI 码页解释。这里用
        // 工程统一解码器还原,失败才退回 lossy(不让非 UTF-8 路径静默消失)。
        let decoded = yu_format::Codec::Cp932
            .decode(path, path)
            .unwrap_or_else(|_| String::from_utf8_lossy(path).into_owned());
        let text = decoded.replace('\\', "/");
        if text.is_empty() || text.split('/').any(|c| c == "..") {
            return false;
        }
        let candidate = std::path::Path::new(&text);
        if candidate.is_absolute() {
            return candidate.is_dir();
        }
        let Some(root) = self
            .paths
            .system_path
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
        else {
            return false;
        };
        std::path::Path::new(&root)
            .join(text.trim_start_matches('/'))
            .is_dir()
    }

    /// 文件关联 exe 全名(`FILEINFO[RELATION]`;REA `0x442a48`
    /// `FindExecutableA(NULL, file, buf)`)。
    ///
    /// 内核不做 OS 关联查询(纯内核,无 win32 依赖):语料仅 `es_Debug.yst`
    /// 1 处(调试用),B7(宿主能力面)接入真实 `FindExecutableW` 后由宿主
    /// 覆写。当前恒空串 —— 与 `FindExecutableA` 失败时引擎缓冲首字节为 0
    /// 的结果一致,脚本判空走「无关联」分支,行为安全。
    pub(super) fn file_association(&self, _path: &std::path::Path) -> Option<Vec<u8>> {
        Some(Vec::new())
    }

    // ---------------------------------------------------------- 存档域(B1)

    /// 一个变量的整组快照(SAVE 预约 / LOAD 对拍用)。
    ///
    /// 普通变量走变量域槽位。系统变量没有 `VarSlot`:`op_save` 的 SET 分支
    /// 已按引用求值(如 `@_RINT(1)`),那里直接以标量结果成块(官方
    /// DNO 90 = Int 标量、dims=[],见 `op_save`)。
    pub(super) fn var_snapshot(&self, var: VarRef) -> Option<crate::event::VarSnapshot> {
        let slot = self.store.slot(var)?;
        Some(crate::event::VarSnapshot {
            ty: slot.ty,
            dims: slot.dims.iter().map(|&d| d as u32).collect(),
            data: slot.data.clone(),
        })
    }

    /// `SAVE[SET=@x[i][j]]` 的块内容 = **该下标处的元素**(标量块),
    /// 不是整个变量。
    ///
    /// 官方 0.479 存档实证:`es_saveload.yst` 的 DNO=2 目标
    /// `@0x45d[0x8d][1]` 写成 Int 标量 14;变量本身是 Int[256,128]。
    /// 旧实现忽略 `lv.indices`、用 [`Self::var_snapshot`] 快照整组,块变成
    /// Int[256,128] → 官方 LOAD 的严格维数比对失败(`0x440c22..`),
    /// qsave 官方读不了。完整下标 = 标量;无下标 = 整组(原语义);
    /// 部分下标(读子数组)官方语义未取证,返回 `None` 不猜块形状。
    pub(super) fn var_snapshot_at(
        &self,
        var: VarRef,
        indices: &[i64],
        value: &Value,
    ) -> Option<crate::event::VarSnapshot> {
        if indices.is_empty() {
            return self.var_snapshot(var);
        }
        let slot = self.store.slot(var)?;
        if indices.len() != slot.dims.len() {
            return None;
        }
        Some(crate::event::VarSnapshot {
            ty: slot.ty,
            dims: Vec::new(),
            data: vec![value.clone()],
        })
    }

    /// `(值类型, 组号)` 匹配的全局变量快照,顺序 = 工程变量表(ysv 记录序;
    /// 与 `VARACT[INIT]` 的恢复序同源)。
    pub(super) fn globals_of_group(&self, ty: ElemType, group: u8) -> Vec<(u16, crate::event::VarSnapshot)> {
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
    /// - `SET=@x`/`SET=@x()` = 变量引用;无下标 = 该变量**当前全部元素**
    ///   (尺寸式 `0x44d5dd`;官方 seld.sd Int[2001]、save50001 标量
    ///   dims=[] 自证);带完整下标 `@x[i][j]` = **该元素标量块**
    ///   (0.479 官方 save00001 DNO=2:`@0x45d[0x8d][1]` 存 Int 标量 14,
    ///   变量本身 Int[256,128];旧实现存整组导致官方 QLOAD 维数不符);
    /// - **预约即快照**:`SAVE[DNO]` 时刻 `0x44d73d` 序列化当前值(zlib 压缩
    ///   后入预约表),`SAVE[FILE]` 只写文件头/偏移表并流出预约块
    ///   (`0x44cd5a..`);同 DNO 重复预约 = 替换(`0x44c740`);
    /// - `G_<族><组>=1`:按首个在场旗标(官方级联序 G_INT→G_INT2→G_INT3→
    ///   G_INT4→G_FLT→…)定 (类型,组),枚举全局变量表逐变量子块化
    ///   (块类型 2;`0x44ca8a..0x44cc57`);
    /// - `ID=` 图层快照 = H-P4-02(挂账,不阻塞变量保存);
    /// - FILE 缺 `.sd` 补上、含 `.sd` 超 32 字节报错(手册 + REA `0x44c663`)。
    pub(super) fn op_save(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 旧写法每次执行都重建整张 `Vec<String>`(与 `CGINFO` 同一个 bug)。
        let names: &[String] = &cmd.param_names;
        let mut file: Option<String> = None;
        // v555 `SAVE[MEM=n]`:n 是内存存档槽号(不是文件名),与 FILE 互斥
        // (5 个可读样本的 LOAD 计数 FILE+MEM 恰好等于总数)。处理体
        // `0x44b874` 的 MEM 分支优先于 FILE 分支;两者同时给时只写内存。
        let mut mem: Option<i64> = None;
        let mut dno: Option<i64> = None;
        let mut set: Option<(LValue, Value)> = None;
        let mut layer_id = false;
        // `SAVE[DNO=n ID=图层 IDNO=n]` 的目标图层(命令时刻取像素)。
        let mut layer_id_bytes: Option<Vec<u8>> = None;
        let mut layer_idno: Option<i64> = None;
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
                "MEM" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        mem = v.as_int();
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
                        if let Value::Str(bytes) = &v {
                            layer_id_bytes = Some(bytes.clone());
                        }
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "IDNO" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        layer_idno = v.as_int();
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
        // v555 `MEM=99999` 是**参数装载后的独立分支**(REA `0x44b874:142`):
        // 官方立即 `FUN_0044d7a8(); return 0`,不处理 DNO/SET/ID/G_*,
        // 也不刷文件。必须放在预约块之前,避免把同条命令的 SET 误预约。
        if mem == Some(99_999) {
            // 0..499 内存槽打包到 `DAT_0086a03c`(12 字节块头 + 每槽
            // 原始长/压缩长/压缩数据)。脚本可见语义只是「成功返回」;
            // 宿主把该块体留给下一次 `SAVE[FILE]` 写成 DNO=1024。
            let failure = match self.save_host.as_mut() {
                Some(host) => host.pack_mem().err(),
                None => None,
            };
            if let Some(d) = failure {
                return Err(VmError::from(self.locate(d)));
            }
            return Ok(StepOutcome::Continue);
        }
        // 预约(DNO 在场;官方 DNO 与 FILE 可同条,先预约后刷盘)。
        if let Some(dno) = dno {
            // 555 内存槽 DNO 0 基;文件存档按 0.495 手册 1..=1024。
            let dno_min = if mem.is_some() { 0 } else { 1 };
            if !(dno_min..=1024).contains(&dno) {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("SAVE DNO={dno} 越界({dno_min}..=1024)"),
                ));
                return Ok(StepOutcome::Continue);
            }
            let dno = dno as u32;
            // v555 的文件 DNO=1024 归 `SAVE[MEM=99999]` 内存包所有;普通块
            // 预约会与内存包静默同 DNO 覆盖。旧版 1024 仍是合法普通块。
            if mem.is_none() && dno == 1024 && self.semantics().save_mem_pack {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "SAVE DNO=1024 保留给 SAVE[MEM=99999] 的 500 槽内存包;普通块预约被拒绝",
                ));
                return Ok(StepOutcome::Continue);
            }
            if layer_id {
                // `SAVE[ID IDNO]` = 图层快照(块类型 1)。官方在命令处理器里
                // 直接读图层表面(REA `0x4477c0` 读取端);`IDNO` 在场时先把
                // 层名拼成 `ID.N`(REA `0x44c4aa`:追加 `.`(`0x808354`)+
                // `_ltoa` 十进制),与 CG 的 `ID[.IDNO]` 同构。
                // 本内核由图形宿主用渲染器回放当前帧的 COPY/加工命令取像素。
                let composed = layer_id_bytes
                    .as_deref()
                    .map(|id| Self::compose_layer_id(id, layer_idno));
                let captured = match (self.graphics_host.as_mut(), composed.as_deref()) {
                    (Some(host), Some(id)) => match host.capture_layer(id) {
                        Ok(img) => img,
                        Err(d) => {
                            self.report(d);
                            None
                        }
                    },
                    _ => None,
                };
                match captured {
                    Some(image) => {
                        let failure = match self.save_host.as_mut() {
                            Some(host) => host.reserve_image(dno, &image).err(),
                            None => None,
                        };
                        if let Some(d) = failure {
                            return Err(VmError::from(self.locate(d)));
                        }
                    }
                    None => self.report_unimplemented(
                        "SAVE",
                        "SAVE[ID] 图层快照:图形宿主未提供图层像素(H-P4-02)",
                    ),
                }
            } else if let Some((ty, group)) = group_slot {
                let vars = self.globals_of_group(ty, group);
                let failure = match self.save_host.as_mut() {
                    Some(host) => host.reserve_group(dno, ty, group, &vars).err(),
                    None => None,
                };
                if let Some(d) = failure {
                    return Err(VmError::from(self.locate(d)));
                }
            } else if let Some((lv, v)) = set {
                let var = self.canonical_var(lv.var);
                // 普通变量 = 整组槽位快照;系统变量(`@_RINT(1)` 等)没有
                // VarSlot,但 SET 窗口已在命令时刻按引用求值 —— 官方把该值
                // 写成**标量块**(DNO 90 实测:type=Int、dims=[]、data=[0])。
                let snap = self.var_snapshot_at(var, &lv.indices, &v).or_else(|| {
                    (var.id < self.sys_count).then(|| {
                        let ty = match &v {
                            Value::Int(_) => ElemType::Int,
                            Value::Float(_) => ElemType::Float,
                            Value::Str(_) => ElemType::Str,
                        };
                        crate::event::VarSnapshot {
                            ty,
                            dims: Vec::new(),
                            data: vec![v],
                        }
                    })
                });
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
            } else if !layer_id {
                // 只有 ID 的块(缩略图)合法;它的未实现在上面的
                // `SAVE[ID] 图层快照未实现` 已挂号,不再误报缺 SET/G_*。
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "SAVE 缺 SET/G_*(无内容可预约)",
                ));
            }
        }
        // v555 `SAVE[MEM]`:MEM 分支优先于 FILE(REA `0x44b874`:
        // MEM 在场时写完内存槽就 `return 0`,不再走文件路径)。
        if mem.is_some() && file.is_some() {
            self.report(diag::verbose(
                code::rt::MEM_TRACE,
                Boundary::Script,
                "SAVE 同时给 FILE 与 MEM:MEM 优先(官方 0x44b874),FILE 被忽略",
            ));
        }
        if let Some(slot) = mem {
            if !(0..=i64::from(u32::MAX)).contains(&slot) {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("SAVE MEM={slot} 非法(内存槽号必须非负 32 位整数)"),
                ));
                return Ok(StepOutcome::Continue);
            }
            let slot = slot as u32;
            let failure = match self.save_host.as_mut() {
                Some(host) => host.save_mem(slot).err(),
                None => None,
            };
            if let Some(d) = failure {
                return Err(VmError::from(self.locate(d)));
            }
            return Ok(StepOutcome::Continue);
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
    pub(super) fn op_load(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 旧写法每次执行都重建整张 `Vec<String>`(与 `CGINFO` 同一个 bug)。
        let names: &[String] = &cmd.param_names;
        let mut file: Option<String> = None;
        // v555 `LOAD[MEM=n]`:n 是内存存档槽号,与 FILE 互斥;MEM 在场时
        // 官方 `0x4447c8` 只走内存来源(文件参数不求值也不打开)。
        let mut mem: Option<i64> = None;
        let mut dno: Option<i64> = None;
        let mut target: Option<LValue> = None;
        let mut layer_id = false;
        let mut layer_id_bytes: Option<Vec<u8>> = None;
        let mut layer_idno: Option<i64> = None;
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
                "MEM" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        mem = v.as_int();
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
                        if let Value::Str(bytes) = &v {
                            layer_id_bytes = Some(bytes.clone());
                        }
                        args.push(ArgValue::new(w.rec.kind, v));
                    }
                }
                "IDNO" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        layer_idno = v.as_int();
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
        if mem.is_some() && file.is_some() {
            self.report(diag::verbose(
                code::rt::MEM_TRACE,
                Boundary::Script,
                "LOAD 同时给 FILE 与 MEM:MEM 优先(官方 0x4447c8),FILE 被忽略",
            ));
        }
        if layer_id {
            // `LOAD[DNO=n ID=图层 IDNO=n]` = 图像块(btype 1):块内像素装回
            // 图层表面。`IDNO` 在场时层名 = `ID.N`(REA `0x4470d1`,与
            // SAVE 同构,见 `compose_layer_id`)。
            let composed = layer_id_bytes
                .as_deref()
                .map(|id| Self::compose_layer_id(id, layer_idno));
            // 旧版路径(无 MEM)保持 c1549eb 的完整参数检查与文案。
            if mem.is_none() && (file.is_none() || dno.is_none() || composed.is_none()) {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "LOAD[ID] 缺 FILE/DNO/ID",
                ));
                return Ok(StepOutcome::Continue);
            }
            let (Some(dno), Some(id)) = (dno, composed.as_deref()) else {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "LOAD[ID] 缺 DNO/ID",
                ));
                return Ok(StepOutcome::Continue);
            };
            // 555 内存槽的 DNO 是 0 基(实测 `DNO=@base+0`;处理体只判
            // `>0x400`),文件存档仍按 0.495 手册的 1..=1024。
            let dno_min = if mem.is_some() { 0 } else { 1 };
            if !(dno_min..=1024).contains(&dno) {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("LOAD DNO={dno} 越界({dno_min}..=1024)"),
                ));
                return Ok(StepOutcome::Continue);
            }
            // MEM 优先于 FILE(REA `0x4447c8`:MEM 在场不再走文件来源)。
            let mem_slot = match mem {
                Some(slot) if (0..=i64::from(u32::MAX)).contains(&slot) => Some(slot as u32),
                Some(slot) => {
                    self.report(diag::recoverable(
                        code::rt::CMD_SLOT,
                        Boundary::Script,
                        format!("LOAD MEM={slot} 非法(内存槽号必须非负 32 位整数)"),
                    ));
                    return Ok(StepOutcome::Continue);
                }
                None => None,
            };
            if mem_slot == Some(99_999) {
                // 官方 LOAD 处理体没有 99999 特殊分支(只有 SAVE 的
                // `0x44b874` → `FUN_0044d7a8` 打包);99999 不是合法内存槽。
                // 不猜官方 OOB 行为:显式拒绝,不读写任何状态。
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "LOAD[MEM=99999]:官方 LOAD 无 99999 分支(SAVE 才有 0..499 \
                     打包);拒绝执行,不猜",
                ));
                return Ok(StepOutcome::Continue);
            }
            let loaded = match self.save_host.as_mut() {
                Some(host) => match mem_slot {
                    Some(slot) => host.load_image_mem(slot, dno as u32),
                    None => match file.as_deref() {
                        Some(file) => host.load_image(&Self::ensure_sd_ext(file), dno as u32),
                        None => {
                            self.report(diag::recoverable(
                                code::rt::CMD_SLOT,
                                Boundary::Script,
                                "LOAD[ID] 缺 FILE/MEM",
                            ));
                            return Ok(StepOutcome::Continue);
                        }
                    },
                },
                None => return Ok(StepOutcome::Continue),
            };
            match loaded {
                Ok(image) => {
                    let applied = match self.graphics_host.as_mut() {
                        Some(host) => host.restore_layer(id, &image),
                        None => Ok(false),
                    };
                    match applied {
                        Ok(true) => {}
                        Ok(false) => self.report_unimplemented(
                            "LOAD",
                            "LOAD[ID] 图层恢复:图形宿主未接受图层像素(H-P4-02)",
                        ),
                        Err(d) => self.report(d),
                    }
                }
                Err(d) => self.report(d),
            }
            return Ok(StepOutcome::Continue);
        }
        // 旧版路径(无 MEM)保持 c1549eb 的 FILE 前置检查与文案。
        if mem.is_none() && file.is_none() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "LOAD 缺 FILE",
            ));
            return Ok(StepOutcome::Continue);
        }
        let Some(dno) = dno else {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "LOAD 缺 DNO",
            ));
            return Ok(StepOutcome::Continue);
        };
        // 555 内存槽 DNO 0 基;文件存档按 0.495 手册 1..=1024。
        let dno_min = if mem.is_some() { 0 } else { 1 };
        if !(dno_min..=1024).contains(&dno) {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("LOAD DNO={dno} 越界({dno_min}..=1024)"),
            ));
            return Ok(StepOutcome::Continue);
        }
        // MEM 优先于 FILE(REA `0x4447c8`);两者都缺才是参数错误。
        let mem_slot = match mem {
            Some(slot) if (0..=i64::from(u32::MAX)).contains(&slot) => Some(slot as u32),
            Some(slot) => {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    format!("LOAD MEM={slot} 非法(内存槽号必须非负 32 位整数)"),
                ));
                return Ok(StepOutcome::Continue);
            }
            None => None,
        };
        if mem_slot == Some(99_999) {
            // 同图像分支:官方 LOAD 无 99999 分支,拒绝而不猜 OOB。
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "LOAD[MEM=99999]:官方 LOAD 无 99999 分支(SAVE 才有 0..499 \
                 打包);拒绝执行,不猜",
            ));
            return Ok(StepOutcome::Continue);
        }
        // v555 `LOAD[FILE DNO=1024]` 是**文件分支的独立形态**:官方
        // `0x4447c8` 读到 DNO=1024 后跳 `FUN_004453f4`,按 12 字节头 +
        // 500 槽打包格式恢复内存槽,并忽略 `LET`。不能落到通用单块读取。
        if self.semantics().save_mem_pack && mem_slot.is_none() && dno == 1024 {
            let Some(file) = file.as_deref() else {
                self.report(diag::recoverable(
                    code::rt::CMD_SLOT,
                    Boundary::Script,
                    "LOAD[FILE DNO=1024] 缺 FILE",
                ));
                return Ok(StepOutcome::Continue);
            };
            let result = match self.save_host.as_mut() {
                Some(host) => host.load_mem_pack(&Self::ensure_sd_ext(file)),
                None => Ok(()),
            };
            if let Err(d) = result {
                self.report(d);
            }
            return Ok(StepOutcome::Continue);
        }
        let (dno, target) = (dno as u32, target);
        // 宿主借用只包住容器读取;写回需要 &mut self 的变量域。
        let loaded = match self.save_host.as_mut() {
            Some(host) => match mem_slot {
                Some(slot) => host.load_block_mem(slot, dno),
                None => {
                    let Some(file) = file else {
                        self.report(diag::recoverable(
                            code::rt::CMD_SLOT,
                            Boundary::Script,
                            "LOAD 缺 FILE/MEM(无存档来源)",
                        ));
                        return Ok(StepOutcome::Continue);
                    };
                    host.load_block(&Self::ensure_sd_ext(&file), dno)
                }
            },
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
            // 块类型 2:块自带 var_id,恢复**整个变量**;块类型 0:写 LET
            // 目标——目标可带下标(`LOAD[LET=@x[i][j]]` 只写该元素,官方
            // 0.479 save00001 DNO=2 的标量块就是这种目标)。
            let (var, indices) = match item.var {
                Some(var) => (self.canonical_var(var), Vec::new()),
                None => match &target {
                    Some(lv) => (self.canonical_var(lv.var), lv.indices.to_vec()),
                    None => continue, // 官方:无处可写即读后丢弃(0x44760c 路径)
                },
            };
            if let Err(d) = self.restore_snapshot(var, &indices, &item.snap) {
                self.report(d);
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// 把一份快照写回变量槽(类型/维数校验;手册 load.html 尺寸不符 = 错误)。
    ///
    /// `indices` 非空 = `LOAD[... LET=@x[i][j]]`:官方只写该**元素**,
    /// 块必须是标量同型;空 = 块类型 0 写整组 / 块类型 2 按 var_id 写整组。
    pub(super) fn restore_snapshot(
        &mut self,
        var: VarRef,
        indices: &[i64],
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
        if !indices.is_empty() {
            // 部分下标(恢复子数组)官方语义未取证:显式挂账,不猜。
            if indices.len() != dims.len() {
                return Err(diag::recoverable(
                    code::rt::VAR_SHAPE,
                    Boundary::Script,
                    format!(
                        "LOAD 目标 {} 给了 {} 个下标,变量 {} 维(部分下标恢复未取证)",
                        var.display(),
                        indices.len(),
                        dims.len()
                    ),
                ));
            }
            if ty != snap.ty || !snap.dims.is_empty() || snap.data.len() != 1 {
                return Err(diag::recoverable(
                    code::rt::VAR_SHAPE,
                    Boundary::Script,
                    format!(
                        "LOAD 元素块类型/形状不符:目标 {} = {}(下标 {indices:?}),\
                         块 = {}{:?}[{} 个元素](手册 load.html)",
                        var.display(),
                        ty.as_str(),
                        snap.ty.as_str(),
                        snap.dims,
                        snap.data.len()
                    ),
                ));
            }
            return self.store.write(var, indices, &snap.data[0]);
        }
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
    pub(super) fn op_saveend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let args = self.eval_arg_windows(st, cmd);
        self.emit_domain(Domain::Save, "SAVEEND", args);
        if let Some(host) = self.save_host.as_mut() {
            host.save_end();
        }
        Ok(StepOutcome::Continue)
    }

    /// `ID` + `IDNO` → 官方层名 `ID.N`。
    ///
    /// 证据(0.495 REA):`SAVE 0x44c4aa..0x44c51c` 与 `LOAD 0x4470d1..0x447145`
    /// 在 `IDNO` 槽在场时把 `ID` 串拷到临时缓冲,追加常量 `"."`(`0x808354`),
    /// 再以 64 位 `_ltoa(…, 10)` 追加十进制值;随后交给路径规范化
    /// `0x4141e4` / 层查找 `0x414559`。`IDNO` 缺省时层名就是 `ID` 本身。
    /// 与 `CG`/`CGEND`/`CGINFO` 的 `ID[.IDNO]` 规则一致。
    pub(super) fn compose_layer_id(id: &[u8], idno: Option<i64>) -> Vec<u8> {
        match idno {
            Some(n) => {
                let mut out = Vec::with_capacity(id.len() + 1 + 20);
                out.extend_from_slice(id);
                out.push(b'.');
                out.extend_from_slice(n.to_string().as_bytes());
                out
            }
            None => id.to_vec(),
        }
    }

    /// `G_<族><组>` 参数名 → (值类型, 组号 1..4)。官方 ycd 名(`G_INT`=组 1
    /// … `G_STR4`=组 4);首个在场的旗标生效由调用方保证。
    pub(super) fn group_of_name(name: &str) -> (ElemType, u8) {
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
    pub(super) fn ensure_sd_ext(name: &str) -> String {
        if name.ends_with(".sd") {
            name.to_string()
        } else {
            format!("{name}.sd")
        }
    }
}
