//! `SaveState`:存档/文件域的宿主状态机(事实记录 + 盘上事实)。
//!
//! # 真值边界(本轮的硬约束,读后再用)
//!
//! 1. **容器字节级是闭环的**:`yu-format::yssd` 对官方 0.495 产物
//!    parse → rebuild **逐字节一致**(269 块全量,见
//!    `yu-format/tests/yssd_roundtrip.rs`)。读侧(SLOT 表/LOAD 解析)
//!    因此是忠实的。
//! 2. **写侧暂被契约挡住**:`SAVE[SET=@X]` / `LOAD[LET=@Y]` 在引擎里传
//!    **变量引用**,而 `DomainCall` 事件层只剩求值结果(契约缺口
//!    DIV-RT-042,`yu-rt/docs/domain-event-contracts.md` §6.1)。块的
//!    `var_id` 与整组数组内容拿不到 ⇒ 任何“从事件重建块”都是猜。
//!    本状态机对 `SAVE[FILE]` 刷盘**显式拒绝并挂号**
//!    (`sv.write.refused`),直到引用身份关闭;绝不写半忠实的存档。
//! 3. **FILEACT 只在显式可写根下落盘**:`DELETE/COPY` 会改宿主文件系统;
//!    只有构造时给了显式 save 根(用例/服务注入 `target/tmp` 之类)才
//!    允许,默认只读观察(保护语料工程目录)。
//! 4. 事件是**事实记录**,不是可重放的命令对象(契约文档 §6 的前提)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yu_log::Diag;
use yu_rt::event::{Domain, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;

use crate::error::{self, code};

/// 一个存档槽文件(`save/*.sd`)的盘上事实。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotFile {
    /// 文件名(save 根内相对;CP932 解码后)。
    pub name: String,
    /// 字节数。
    pub size: u64,
    /// 块数(容器头 `+0x08`)。
    pub blocks: u32,
    /// DNO → 块变量 id(`0 = 配置/多变量等非单变量块`)。
    pub dnos: BTreeMap<u32, u16>,
}

/// 一次 `SAVE` 预约的事实记录(未刷盘;见模块文档真值边界 2)。
#[derive(Clone, Debug, PartialEq)]
pub struct ReservedFact {
    /// `DNO` 参数(1..=1024)。
    pub dno: u32,
    /// `SET` 的**求值结果**(注意:不是变量内容的事实,见 DIV-RT-042)。
    pub value: Value,
    /// 事件帧号。
    pub frame: u64,
}

/// 槽位表的确定性快照(诊断/用例摘要用;不含时间戳等易变事实)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveSnapshot {
    /// 盘上槽文件(按文件名排序)。
    pub slots: Vec<SlotFile>,
    /// 当前未刷盘的 SAVE 预约(按 DNO 排序)。
    pub reservations: Vec<ReservedFact>,
    /// 累计事件计数。
    pub save_calls: u64,
    pub load_calls: u64,
    pub saveend_calls: u64,
    pub fileact_calls: u64,
}

/// 存档/文件域宿主状态。
pub struct SaveState {
    /// save 根(唯一的盘上事实来源与(未来)写盘根)。
    root: PathBuf,
    /// 是否允许 FILEACT 落盘(真值边界 3)。
    writable: bool,
    /// 盘上槽表(按文件名排序;`refresh_slots` 重建)。
    slots: BTreeMap<String, SlotFile>,
    /// 未刷盘预约(DNO → 事实)。
    reservations: BTreeMap<u32, ReservedFact>,
    /// 诊断(容量上限 [`MAX_DIAGS`],超限丢最旧并记账)。
    diags: std::collections::VecDeque<Diag>,
    diags_dropped: u64,
    save_calls: u64,
    load_calls: u64,
    saveend_calls: u64,
    fileact_calls: u64,
}

/// 诊断上限(与 `Engine` 侧 sound 诊断同量级)。
pub const MAX_DIAGS: usize = 8192;

impl SaveState {
    /// 构造。`writable = false` 时 FILEACT 的删除/复制被拒绝并挂号。
    pub fn new(root: impl Into<PathBuf>, writable: bool) -> Self {
        Self {
            root: root.into(),
            writable,
            slots: BTreeMap::new(),
            reservations: BTreeMap::new(),
            diags: std::collections::VecDeque::new(),
            diags_dropped: 0,
            save_calls: 0,
            load_calls: 0,
            saveend_calls: 0,
            fileact_calls: 0,
        }
    }

    /// save 根。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// FILEACT 是否允许落盘。
    pub fn writable(&self) -> bool {
        self.writable
    }

    /// 当前盘上槽表(`refresh_slots` 的缓存视图)。
    pub fn slots(&self) -> impl Iterator<Item = &SlotFile> {
        self.slots.values()
    }

    /// 当前未刷盘预约数。
    pub fn reservations_len(&self) -> usize {
        self.reservations.len()
    }

    /// 确定性快照(用例摘要/诊断)。
    #[must_use]
    pub fn snapshot(&self) -> SaveSnapshot {
        SaveSnapshot {
            slots: self.slots.values().cloned().collect(),
            reservations: self.reservations.values().cloned().collect(),
            save_calls: self.save_calls,
            load_calls: self.load_calls,
            saveend_calls: self.saveend_calls,
            fileact_calls: self.fileact_calls,
        }
    }

    /// 取走诊断(最旧在前;调用方负责消费)。
    pub fn take_diags(&mut self) -> Vec<Diag> {
        std::mem::take(&mut self.diags).into_iter().collect()
    }

    /// 累计被丢弃的诊断数(超限记账)。
    pub fn diags_dropped(&self) -> u64 {
        self.diags_dropped
    }

    fn push_diag(&mut self, diag: Diag) {
        if self.diags.len() >= MAX_DIAGS {
            self.diags.pop_front();
            self.diags_dropped += 1;
        }
        self.diags.push_back(diag);
    }

    /// 重新扫描 save 根,重建盘上槽表。每个 `.sd` 只读头部与偏移表
    /// (不逐块解压;大存档也不贵)。解析失败的文件挂号并跳过,
    /// 不影响其余槽。
    pub fn refresh_slots(&mut self) {
        self.slots.clear();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("sd") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                self.push_diag(error::recoverable(
                    code::FILE_READ,
                    format!("槽文件不可读: {name}"),
                ));
                continue;
            };
            match yu_format::yssd::YssdSave::parse(&bytes, name.as_bytes()) {
                Ok(save) => {
                    let mut dnos = BTreeMap::new();
                    for entry in &save.entries {
                        // 配置/多变量块没有单一目标变量 ⇒ 记 0。
                        let var = if entry.btype == 0 { entry.var_id } else { 0 };
                        dnos.insert(entry.dno, var);
                    }
                    self.slots.insert(
                        name.to_string(),
                        SlotFile {
                            name: name.to_string(),
                            size: bytes.len() as u64,
                            blocks: save.entries.len() as u32,
                            dnos,
                        },
                    );
                }
                Err(diag) => {
                    self.push_diag(error::recoverable(
                        code::CONTAINER,
                        format!("槽文件解析失败: {name}: {diag}"),
                    ));
                }
            }
        }
    }

    /// 消费一条域事件(调用方从 `Vm::take_events()` 的 drain 里喂入;
    /// 只关心 `Domain::Save | Domain::File`)。参数名经工程 `yscm` 取。
    pub fn apply_event(&mut self, ev: &VmEvent, yscm: &YscmTable) {
        let VmEvent::DomainCall {
            frame,
            domain: Domain::Save | Domain::File,
            name,
            ..
        } = ev
        else {
            return;
        };
        let args: Vec<(String, &Value)> = ev
            .domain_args(yscm)
            .map(|list| {
                list.into_iter()
                    .map(|a| (a.name.into_owned(), a.value))
                    .collect()
            })
            .unwrap_or_default();
        let command = name.as_str();
        match command {
            "SAVE" => {
                self.save_calls += 1;
                self.apply_save(*frame, &args);
            }
            "LOAD" => {
                self.load_calls += 1;
                self.apply_load(*frame, command, &args);
            }
            "SAVEEND" => {
                self.saveend_calls += 1;
                let n = self.reservations.len();
                self.reservations.clear();
                if n > 0 {
                    self.push_diag(error::info(
                        code::COMMAND_UNIMPLEMENTED,
                        format!("SAVEEND 丢弃 {n} 条未刷盘预约(手册 file/saveend.html)"),
                    ));
                }
            }
            "FILEACT" => {
                self.fileact_calls += 1;
                self.apply_fileact(*frame, &args);
            }
            "SAVEINFO" => {
                // 查询处理体 REA 0x44e1c8 未取证,语料 0 次;LET 写回同时
                // 受 DIV-RT-042 阻塞 ⇒ 整体挂账,不猜字段。
                self.push_diag(error::at_command(
                    error::unimplemented(
                        code::COMMAND_UNIMPLEMENTED,
                        "SAVEINFO 查询未取证(REA 0x44e1c8;语料 0 次);挂账".to_string(),
                    ),
                    *frame,
                    command,
                ));
            }
            "FILEINFO" => {
                // FILEINFO[FILE+EXIST+LET] 已由内核实现(J3,REA 0x44286c);
                // 宿主不双写。SIZE/RELATION 的 LET 写回受 DIV-RT-042 阻塞。
                self.push_diag(error::at_command(
                    error::unimplemented(
                        code::COMMAND_UNIMPLEMENTED,
                        "FILEINFO SIZE/RELATION 未实现(DIV-RT-042:LET 引用身份;EXIST 已由内核实现)"
                            .to_string(),
                    ),
                    *frame,
                    command,
                ));
            }
            other => {
                self.push_diag(error::at_command(
                    error::unimplemented(
                        code::COMMAND_UNIMPLEMENTED,
                        format!("存档/文件域命令 {other} 未接线"),
                    ),
                    *frame,
                    other,
                ));
            }
        }
    }

    /// `SAVE`:无 `FILE` = 预约;有 `FILE` = 刷盘(当前拒绝,见真值边界 2)。
    fn apply_save(&mut self, frame: u64, args: &[(String, &Value)]) {
        let mut file: Option<String> = None;
        let mut dno: Option<u32> = None;
        let mut set: Option<&Value> = None;
        let mut layer_id = false;
        for (name, value) in args {
            match name.as_str() {
                "FILE" => {
                    if let Value::Str(bytes) = value {
                        file = Some(decode_name(bytes));
                    } else {
                        self.push_diag(error::at_command(
                            error::recoverable(code::ARG_TYPE, "SAVE[FILE] 需要 STR"),
                            frame,
                            "SAVE",
                        ));
                    }
                }
                "DNO" => {
                    dno = value.as_int().map(|n| n as u32);
                }
                "SET" => set = Some(value),
                "ID" => layer_id = true,
                _ => {}
            }
        }
        if layer_id {
            // 图层快照需要 gfx(CGETSCREEN 类),H-P4-02 交接 P2/集成轮。
            self.push_diag(error::at_command(
                error::unimplemented(
                    code::COMMAND_UNIMPLEMENTED,
                    "SAVE[ID] 图层快照未实现(H-P4-02 → P2/集成轮)".to_string(),
                ),
                frame,
                "SAVE",
            ));
        }
        let Some(dno) = dno else {
            self.push_diag(error::at_command(
                error::recoverable(code::ARG_MISSING, "SAVE 缺 DNO"),
                frame,
                "SAVE",
            ));
            return;
        };
        if !(1..=1024).contains(&dno) {
            self.push_diag(error::at_command(
                error::recoverable(code::ARG_RANGE, format!("SAVE DNO={dno} 越界(1..=1024)")),
                frame,
                "SAVE",
            ));
            return;
        }
        match file {
            None => {
                // 预约:只记事实(SET 的求值结果;变量身份未进事件)。
                let value = set.cloned().unwrap_or(Value::Int(0));
                self.reservations
                    .insert(dno, ReservedFact { dno, value, frame });
            }
            Some(file) => {
                // 刷盘:契约缺口(DIV-RT-042)未关闭前拒绝写,不猜。
                self.push_diag(error::at_command(
                    error::unimplemented(
                        code::WRITE_REFUSED,
                        format!(
                            "SAVE[FILE={file} DNO={dno}] 刷盘拒绝:SET/LET 变量引用身份在事件层丢失 \
                             (DIV-RT-042),写不出与官方一致的块;容器写路径已就绪 \
                             (yu-format::yssd 逐字节 roundtrip)"
                        ),
                    ),
                    frame,
                    "SAVE",
                ));
            }
        }
    }

    /// `LOAD`:读侧(容器解析 + 槽表)可行;`LET` 写回被 DIV-RT-042 挡住。
    fn apply_load(&mut self, frame: u64, command: &str, args: &[(String, &Value)]) {
        let mut file: Option<String> = None;
        let mut dno: Option<u32> = None;
        for (name, value) in args {
            match name.as_str() {
                "FILE" => {
                    if let Value::Str(bytes) = value {
                        file = Some(decode_name(bytes));
                    }
                }
                "DNO" => dno = value.as_int().map(|n| n as u32),
                "LET" | "ID" => {
                    self.push_diag(error::at_command(
                        error::unimplemented(
                            code::COMMAND_UNIMPLEMENTED,
                            "LOAD[LET/ID] 未实现(DIV-RT-042:LET 引用;ID 图层恢复 → H-P4-02)"
                                .to_string(),
                        ),
                        frame,
                        command,
                    ));
                }
                _ => {}
            }
        }
        let Some(file) = file.or_else(|| {
            dno.filter(|_| false).map(|_| String::new()) // DNO 只指槽内块号,不指文件
        }) else {
            self.push_diag(error::at_command(
                error::recoverable(code::ARG_MISSING, "LOAD 缺 FILE"),
                frame,
                command,
            ));
            return;
        };
        let path = match self.confined_path(&file) {
            Ok(path) => path,
            Err(diag) => {
                self.push_diag(error::at_command(diag, frame, command));
                return;
            }
        };
        match std::fs::read(&path) {
            Ok(bytes) => match yu_format::yssd::YssdSave::parse(&bytes, file.as_bytes()) {
                Ok(save) => {
                    self.push_diag(error::at_command(
                        error::info(
                            code::COMMAND_UNIMPLEMENTED,
                            format!(
                                "LOAD[FILE={file}] 容器已读:{}/{} 块;值写回挂账(DIV-RT-042)",
                                save.entries.len(),
                                file
                            ),
                        ),
                        frame,
                        command,
                    ));
                }
                Err(diag) => {
                    self.push_diag(error::at_command(
                        error::recoverable(code::CONTAINER, format!("LOAD[FILE={file}] {diag}")),
                        frame,
                        command,
                    ));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.push_diag(error::at_command(
                    error::recoverable(code::FILE_MISSING, format!("LOAD 目标不存在: {file}")),
                    frame,
                    command,
                ));
            }
            Err(e) => {
                self.push_diag(error::at_command(
                    error::recoverable(code::FILE_READ, format!("LOAD 读取失败: {file}: {e}")),
                    frame,
                    command,
                ));
            }
        }
    }

    /// `FILEACT`:手册 `file/fileact.html`。已实现 COPY / DELETE(FILE 存档
    /// 或 FOLDER 空文件夹);EXEC 拒绝;图层导出(BMP/PNG/JPG+ID)挂账
    /// H-P4-02。
    fn apply_fileact(&mut self, frame: u64, args: &[(String, &Value)]) {
        let mut file: Option<String> = None;
        let mut file2: Option<String> = None;
        let mut folder: Option<String> = None;
        let mut copy = false;
        let mut delete = false;
        let mut exec = false;
        let mut layer_export = false;
        for (name, value) in args {
            let flag = |v: &Value| v.as_int().map(|n| n != 0).unwrap_or(false);
            match name.as_str() {
                "FILE" => {
                    if let Value::Str(bytes) = value {
                        file = Some(decode_name(bytes));
                    }
                }
                "FILE2" => {
                    if let Value::Str(bytes) = value {
                        file2 = Some(decode_name(bytes));
                    }
                }
                "FOLDER" => {
                    if let Value::Str(bytes) = value {
                        folder = Some(decode_name(bytes));
                    }
                }
                "COPY" => copy = flag(value),
                "DELETE" => delete = flag(value),
                "EXEC" => {
                    exec = flag(value);
                }
                "ID" | "BMP" | "PNG" | "JPG" => layer_export = layer_export || flag(value),
                "PARAM" | "SET" => {}
                _ => {}
            }
        }
        if layer_export {
            self.push_diag(error::at_command(
                error::unimplemented(
                    code::COMMAND_UNIMPLEMENTED,
                    "FILEACT 图层导出(ID/BMP/PNG/JPG)未实现(H-P4-02 → P2/集成轮)".to_string(),
                ),
                frame,
                "FILEACT",
            ));
        }
        if exec {
            self.push_diag(error::at_command(
                error::unimplemented(
                    code::EXEC_REFUSED,
                    "FILEACT[EXEC] 拒绝:宿主不执行外部程序(无引擎语义取证 + 安全策略)".to_string(),
                ),
                frame,
                "FILEACT",
            ));
        }
        if copy {
            self.fileact_copy(frame, file.as_deref(), file2.as_deref());
        }
        if delete {
            if let Some(folder) = folder.filter(|_| file.is_none()) {
                self.fileact_delete_dir(frame, &folder);
            } else {
                self.fileact_delete_file(frame, file.as_deref());
            }
        }
    }

    fn fileact_copy(&mut self, frame: u64, file: Option<&str>, file2: Option<&str>) {
        let (Some(from), Some(to)) = (file, file2) else {
            self.push_diag(error::at_command(
                error::recoverable(code::ARG_MISSING, "FILEACT[COPY] 需要 FILE 与 FILE2"),
                frame,
                "FILEACT",
            ));
            return;
        };
        if !self.writable {
            self.push_diag(error::at_command(
                error::recoverable(
                    code::WRITE_REFUSED,
                    format!("FILEACT[COPY {from} -> {to}] 拒绝:当前 save 根为只读观察模式"),
                ),
                frame,
                "FILEACT",
            ));
            return;
        }
        let (src, dst) = match (self.confined_path(from), self.confined_path(to)) {
            (Ok(s), Ok(d)) => (s, d),
            (Err(d), _) | (_, Err(d)) => {
                self.push_diag(error::at_command(d, frame, "FILEACT"));
                return;
            }
        };
        match std::fs::copy(&src, &dst) {
            Ok(n) => {
                self.refresh_slots();
                self.push_diag(error::at_command(
                    error::info(
                        code::COMMAND_UNIMPLEMENTED,
                        format!("FILEACT[COPY] {from} -> {to}({n}B)"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.push_diag(error::at_command(
                    error::recoverable(
                        code::FILE_MISSING,
                        format!("FILEACT[COPY] 源不存在: {from}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) => {
                self.push_diag(error::at_command(
                    error::recoverable(
                        code::FILE_WRITE,
                        format!("FILEACT[COPY] 失败: {from} -> {to}: {e}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
        }
    }

    fn fileact_delete_file(&mut self, frame: u64, file: Option<&str>) {
        let Some(file) = file else {
            self.push_diag(error::at_command(
                error::recoverable(code::ARG_MISSING, "FILEACT[DELETE] 缺 FILE"),
                frame,
                "FILEACT",
            ));
            return;
        };
        if !self.writable {
            self.push_diag(error::at_command(
                error::recoverable(
                    code::WRITE_REFUSED,
                    format!("FILEACT[DELETE {file}] 拒绝:当前 save 根为只读观察模式"),
                ),
                frame,
                "FILEACT",
            ));
            return;
        }
        let path = match self.confined_path(file) {
            Ok(path) => path,
            Err(diag) => {
                self.push_diag(error::at_command(diag, frame, "FILEACT"));
                return;
            }
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.refresh_slots();
                self.push_diag(error::at_command(
                    error::info(
                        code::COMMAND_UNIMPLEMENTED,
                        format!("FILEACT[DELETE] 已删 {file}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.push_diag(error::at_command(
                    error::recoverable(
                        code::FILE_MISSING,
                        format!("FILEACT[DELETE] 目标不存在: {file}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) => {
                self.push_diag(error::at_command(
                    error::recoverable(
                        code::FILE_WRITE,
                        format!("FILEACT[DELETE] 失败: {file}: {e}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
        }
    }

    fn fileact_delete_dir(&mut self, frame: u64, folder: &str) {
        if !self.writable {
            self.push_diag(error::at_command(
                error::recoverable(
                    code::WRITE_REFUSED,
                    format!("FILEACT[DELETE FOLDER {folder}] 拒绝:当前 save 根为只读观察模式"),
                ),
                frame,
                "FILEACT",
            ));
            return;
        }
        let path = match self.confined_path(folder) {
            Ok(path) => path,
            Err(diag) => {
                self.push_diag(error::at_command(diag, frame, "FILEACT"));
                return;
            }
        };
        // 手册:只删**空**文件夹;非空由 remove_dir 原生失败,不递归。
        match std::fs::remove_dir(&path) {
            Ok(()) => {
                self.push_diag(error::at_command(
                    error::info(
                        code::COMMAND_UNIMPLEMENTED,
                        format!("FILEACT[DELETE] 已删空文件夹 {folder}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.push_diag(error::at_command(
                    error::recoverable(
                        code::FILE_MISSING,
                        format!("FILEACT[DELETE] 文件夹不存在: {folder}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) => {
                self.push_diag(error::at_command(
                    error::recoverable(
                        code::FILE_WRITE,
                        format!("FILEACT[DELETE] 文件夹失败(仅空文件夹): {folder}: {e}"),
                    ),
                    frame,
                    "FILEACT",
                ));
            }
        }
    }

    /// 把 save 根内相对名解析成安全绝对路径:`..`/绝对路径/越界一律
    /// `sv.path.escape`(安全拒绝;契约文档 §5「宿主必须自己做路径归一化
    /// 与越界检查」)。
    fn confined_path(&self, name: &str) -> Result<PathBuf, Diag> {
        if name.is_empty() {
            return Err(error::recoverable(code::ARG_MISSING, "路径名为空"));
        }
        let rel = Path::new(name);
        if rel.is_absolute() || name.contains('\\') {
            return Err(error::recoverable(
                code::PATH_ESCAPE,
                format!("路径非 save 根内相对形式: {name}"),
            ));
        }
        let joined = self.root.join(rel);
        let Ok(resolved) = joined.canonicalize() else {
            // 目标不存在时退化为词法归一化(仍拒绝 `..` 分量)。
            for comp in rel.components() {
                if matches!(comp, std::path::Component::ParentDir) {
                    return Err(error::recoverable(
                        code::PATH_ESCAPE,
                        format!("路径含 `..`: {name}"),
                    ));
                }
            }
            return Ok(joined);
        };
        let Ok(root) = self.root.canonicalize() else {
            return Err(error::recoverable(
                code::PATH_ESCAPE,
                "save 根不存在(无法核界)",
            ));
        };
        if resolved.starts_with(&root) {
            Ok(resolved)
        } else {
            Err(error::recoverable(
                code::PATH_ESCAPE,
                format!("目标解析后越出 save 根: {name}"),
            ))
        }
    }
}

/// CP932 文件名解码(事件里的 `Value::Str` 字节;失败按 lossy,不影响
/// 后续路径拒绝)。
fn decode_name(bytes: &[u8]) -> String {
    match yu_format::Codec::Cp932.decode(bytes, b"save-name") {
        Ok(text) => text,
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yu_rt::event::ArgValue;

    /// 合成 yscm:SAVE(DNO,FILE,SET)/ LOAD(FILE,LET)/ SAVEEND / FILEACT。
    fn synthetic_yscm() -> YscmTable {
        use yu_format::yscm::{Arg, Command, CommandTable, KEY_TABLE_BYTES};
        let command = |name: &str, args: &[&str]| Command {
            name: name.to_string().into(),
            argc: args.len() as u8,
            args: args
                .iter()
                .map(|a| Arg {
                    name: (*a).to_string().into(),
                    flag: 0,
                    kind: 0,
                })
                .collect(),
        };
        let table = CommandTable {
            version: 495,
            commands: vec![
                command("SAVE", &["DNO", "FILE", "SET", "ID"]),
                command("LOAD", &["FILE", "LET", "ID", "DNO"]),
                command("SAVEEND", &[]),
                command(
                    "FILEACT",
                    &["FILE", "FILE2", "FOLDER", "COPY", "DELETE", "EXEC"],
                ),
            ],
            messages: Vec::new(),
            key_table: Box::new([0u8; KEY_TABLE_BYTES]),
        };
        YscmTable::from_format(table).expect("synthetic yscm")
    }

    fn domain_call(name: &str, args: &[(u8, &str, Value)]) -> VmEvent {
        use yu_rt::event::Domain;
        VmEvent::DomainCall {
            frame: 7,
            domain: Domain::Save,
            name: name.to_string(),
            args: args
                .iter()
                .map(|(slot, _, value)| ArgValue::new(*slot, value.clone()))
                .collect(),
            script: None,
            pc: None,
        }
    }

    #[test]
    fn save_without_file_reserves_fact_and_saveend_clears() {
        let yscm = synthetic_yscm();
        let mut state = SaveState::new("target/tmp/sv-unit-1", false);
        // 槽号按合成 yscm 的 SAVE 参数序:DNO=0 / FILE=1 / SET=2 / ID=3。
        state.apply_event(
            &domain_call(
                "SAVE",
                &[(0, "DNO", Value::Int(12)), (2, "SET", Value::Int(2))],
            ),
            &yscm,
        );
        assert_eq!(state.reservations_len(), 1);
        let snap = state.snapshot();
        assert_eq!(snap.reservations[0].dno, 12);
        assert_eq!(snap.reservations[0].value, Value::Int(2));
        assert_eq!(snap.save_calls, 1);
        state.apply_event(&domain_call("SAVEEND", &[]), &yscm);
        assert_eq!(state.reservations_len(), 0);
        assert_eq!(state.snapshot().saveend_calls, 1);
    }

    #[test]
    fn save_with_file_is_refused_not_written() {
        let yscm = synthetic_yscm();
        let root = temp_root("sv-unit-2-must-not-exist");
        let mut state = SaveState::new(&root, true);
        state.apply_event(
            &domain_call(
                "SAVE",
                &[
                    (0, "DNO", Value::Int(1)),
                    (1, "FILE", Value::Str(b"n1.sd".to_vec())),
                    (2, "SET", Value::Int(5)),
                ],
            ),
            &yscm,
        );
        // 拒绝写:文件不出现;挂号 WRITE_REFUSED。
        assert!(!root.join("n1.sd").exists());
        let diags = state.take_diags();
        assert!(diags.iter().any(|d| d.code == code::WRITE_REFUSED));
    }

    #[test]
    fn dno_out_of_range_is_reported() {
        let yscm = synthetic_yscm();
        let mut state = SaveState::new("target/tmp/sv-unit-3", false);
        state.apply_event(&domain_call("SAVE", &[(0, "DNO", Value::Int(0))]), &yscm);
        state.apply_event(&domain_call("SAVE", &[(0, "DNO", Value::Int(1025))]), &yscm);
        let diags = state.take_diags();
        assert_eq!(
            diags.iter().filter(|d| d.code == code::ARG_RANGE).count(),
            2
        );
        assert_eq!(state.reservations_len(), 0);
    }

    #[test]
    fn refresh_slots_reads_container_facts() {
        // 用 yu-format 的写路径造一个最小官方形态容器(合成,不依赖语料)。
        let payload = yu_format::yssd::YssdValue {
            ty: yu_format::yssd::YssdValueType::Int,
            dims: Vec::new(),
            data: 5i64.to_le_bytes().to_vec(),
        };
        let mut writer = yu_format::yssd::YssdWriter::default();
        writer.set_block(
            1,
            yu_format::yssd::encode_block_body(0, 1, 0x0a43, &payload.to_bytes()),
        );
        let bytes = writer.build();
        let root = temp_root("sv-unit-4");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("t.sd"), &bytes).unwrap();

        let mut state = SaveState::new(&root, false);
        state.refresh_slots();
        let slots: Vec<_> = state.slots().collect();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "t.sd");
        assert_eq!(slots[0].blocks, 1);
        assert_eq!(slots[0].dnos.get(&1), Some(&0x0a43));
    }

    #[test]
    fn confined_path_rejects_escape() {
        let state = SaveState::new("target/tmp/sv-unit-5", false);
        assert_eq!(
            state.confined_path("../outside.sd").unwrap_err().code,
            code::PATH_ESCAPE
        );
        assert_eq!(
            state.confined_path("C:\\abs.sd").unwrap_err().code,
            code::PATH_ESCAPE
        );
        assert_eq!(state.confined_path("").unwrap_err().code, code::ARG_MISSING);
    }

    /// 单测专用根:本 crate `target/tmp` 下(系统 TEMP 在本环境不可写)。
    fn temp_root(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(name)
    }
}
