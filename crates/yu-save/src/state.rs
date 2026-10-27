//! `SaveState`:存档/文件域的宿主状态机(B1 存档闭环)。
//!
//! # 真值边界
//!
//! 1. **容器字节级闭环**:`yu-format::yssd` 对官方 0.495 产物 parse →
//!    rebuild **逐字节一致**(269 块全量,`yu-format/tests/yssd_roundtrip.rs`)。
//! 2. **块内容 = 预约时刻的值快照**(B1,REA 取证):`SAVE[DNO]` 处理体
//!    `0x44c45c` → `0x44d73d` 在预约时刻写 btype/strict/var/clen/ulen 并
//!    取当前值压缩入表;`SAVE[FILE]`(`0x44cd5a..`)只写文件头 + 偏移表,
//!    按 DNO 升序流出**预约时**的字节;预约表**不被 flush 清空**
//!    (SAVEEND `0x44e1a0` 才清;语料 es_GSD 每次刷盘后显式 SAVEEND)。
//!    本状态机同型:`reserve` 快照、`flush` 写盘不清、`save_end` 清。
//! 3. `FILEACT`/SAVE 刷盘**默认可写**(2026-10-24 主人令;官方原版已备份
//!    `save-backup-20261024/`);显式构造只读状态机(单测)才拒绝。
//! 4. 事件是**事实记录**;SAVE/LOAD/SAVEEND 的语义经 `yu-rt` 的
//!    `SaveHost` 同步契约(B1 裁 1-A/裁 2-A)进入本状态机,事件流里的
//!    这三条不再重复消费(`apply_event` 只剩 FILEACT/SAVEINFO/FILEINFO)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yu_log::Diag;
use yu_rt::event::{Domain, LoadedBlock, LoadedVar, VarSnapshot, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::{ElemType, Value, VarRef, VarSpace};

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

/// 一次 `SAVE[DNO]` 预约(**值快照**,B1)。
///
/// 形态 = 官方预约表(`0x7fad1c`/`0x7fbe1c` 按 DNO 索引)的内核投影:
/// 块类型 0 = 单变量(整组快照);块类型 2 = 多变量组(逐子块,顺序 =
/// 工程变量表序)。同 DNO 重复预约 = 替换(REA `0x44c740`)。
#[derive(Clone, Debug, PartialEq)]
pub struct ReservedFact {
    /// `DNO` 参数(1..=1024)。
    pub dno: u32,
    /// 块类型(0 = 单变量;2 = 多变量组;与容器头一致)。
    pub btype: u8,
    /// 块头目标变量 id(块类型 2 = 0,子块各带)。
    pub var_id: u16,
    /// 块类型 0 的单变量快照(整组;标量 = 单元素)。
    pub snap: Option<VarSnapshot>,
    /// 块类型 2 的子变量 `(var_id, 快照)`。
    pub subs: Vec<(u16, VarSnapshot)>,
    /// 块类型 1 的图层图像(`SAVE[ID]` 命令时刻像素)。
    pub image: Option<yu_rt::event::CapturedImage>,
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
    /// save 根(唯一的盘上事实来源与写盘根)。
    root: PathBuf,
    /// 是否允许 SAVE 刷盘 / FILEACT 落盘(真值边界 3)。
    writable: bool,
    /// 盘上槽表(按文件名排序;`refresh_slots` 重建)。
    slots: BTreeMap<String, SlotFile>,
    /// 未刷盘预约(DNO → 快照;同 DNO 覆盖)。
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

    /// 消费一条域事件(`Vm::emit` 在**命令时刻**同步喂入;
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
            "SAVE" | "LOAD" | "SAVEEND" => {
                // B1:这三条经 `SaveHost` 同步契约处理(预约/刷盘/拉取都在
                // 命令时刻),事件流里不再重复消费(计数走同步路径)。
            }
            "FILEACT" => {
                self.fileact_calls += 1;
                self.apply_fileact(*frame, &args);
            }
            "SAVEINFO" => {
                // 查询处理体 REA 0x44e1c8 未取证,语料 0 次 ⇒ 整体挂账 B7。
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
                // 查询**已全部由内核实现**(`op_fileinfo`):`EXIST` 走封包/散盘
                // 四策略(REA 0x44286c→0x44316c),`SIZE` 走 `FindFirstFileA`
                // (0x443572),`RELATION` 走 `FindExecutableA`(0x4429e6)。
                // 内核自己会为真正未实现的支(如 `FOLDER`)发带位置的诊断,
                // 宿主这里**不得**再对每条事件无条件报「SIZE/RELATION 未实现」
                // —— 那会产出与事实相反的误报(演出1 实测 626 条)。
                // 事件仍计入域计数,供诊断面板观察调用量。
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

    // ---------------------------------------------------- 同步契约(B1)

    /// `SAVE[DNO=n SET=@x]`:预约一块(块类型 0),**快照当前值**
    /// (REA `0x44d73d`;同 DNO = 替换 `0x44c740`)。
    pub fn reserve(&mut self, dno: u32, var: VarRef, snap: &VarSnapshot) -> Result<(), Diag> {
        self.save_calls += 1;
        if !(1..=1024).contains(&dno) {
            return Err(error::recoverable(
                code::ARG_RANGE,
                format!("SAVE DNO={dno} 越界(1..=1024)"),
            ));
        }
        self.reservations.insert(
            dno,
            ReservedFact {
                dno,
                btype: 0,
                var_id: var.id,
                snap: Some(snap.clone()),
                subs: Vec::new(),
                image: None,
            },
        );
        Ok(())
    }

    /// `SAVE[DNO=n G_<族><组>=1]`:预约一块(块类型 2),逐子块快照
    /// (REA `0x44ca8a..0x44cc57`;顺序 = 工程变量表序)。
    pub fn reserve_group(
        &mut self,
        dno: u32,
        _ty: ElemType,
        _group: u8,
        vars: &[(u16, VarSnapshot)],
    ) -> Result<(), Diag> {
        self.save_calls += 1;
        if !(1..=1024).contains(&dno) {
            return Err(error::recoverable(
                code::ARG_RANGE,
                format!("SAVE DNO={dno} 越界(1..=1024)"),
            ));
        }
        self.reservations.insert(
            dno,
            ReservedFact {
                dno,
                btype: 2,
                var_id: 0,
                snap: None,
                subs: vars.to_vec(),
                image: None,
            },
        );
        Ok(())
    }

    /// `SAVE[DNO=n ID=图层]`:预约一块(块类型 1),内容 = 命令时刻图层
    /// 表面像素(`image_block::encode` 组装段长表 + 两段)。
    pub fn reserve_image(
        &mut self,
        dno: u32,
        image: &yu_rt::event::CapturedImage,
    ) -> Result<(), Diag> {
        self.save_calls += 1;
        if !(1..=1024).contains(&dno) {
            return Err(error::recoverable(
                code::ARG_RANGE,
                format!("SAVE DNO={dno} 越界(1..=1024)"),
            ));
        }
        let expected = image.len_bytes();
        if image.rgba.len() != expected || expected == 0 {
            return Err(error::recoverable(
                code::IMAGE_SHAPE,
                format!(
                    "SAVE[ID] 图像形状非法: {}x{} 需要 {} 字节,实有 {}",
                    image.width,
                    image.height,
                    expected,
                    image.rgba.len()
                ),
            ));
        }
        self.reservations.insert(
            dno,
            ReservedFact {
                dno,
                btype: 1,
                var_id: 0,
                snap: None,
                subs: Vec::new(),
                image: Some(image.clone()),
            },
        );
        Ok(())
    }

    /// `SAVE[FILE="x"]`:刷盘。按 DNO 升序流出**预约时**的快照
    /// (REA `0x44cd5a..0x44ceab`:写头/偏移表 + 逐块写;预约表不清)。
    pub fn flush(&mut self, file: &str) -> Result<(), Diag> {
        if !self.writable {
            return Err(error::recoverable(
                code::WRITE_REFUSED,
                format!("SAVE[FILE={file}] 拒绝:save 根为只读模式(显式构造)"),
            ));
        }
        let mut writer = yu_format::yssd::YssdWriter::default();
        for fact in self.reservations.values() {
            match fact.btype {
                0 => {
                    let Some(snap) = &fact.snap else { continue };
                    let payload = snapshot_payload(snap)?;
                    writer.set_block(
                        fact.dno,
                        yu_format::yssd::encode_block_body(0, 1, fact.var_id, &payload.to_bytes()),
                    );
                }
                2 => {
                    let mut raws = Vec::with_capacity(fact.subs.len());
                    for (var_id, snap) in &fact.subs {
                        let payload = snapshot_payload(snap)?;
                        raws.push((*var_id, payload.to_bytes()));
                    }
                    let subs: Vec<yu_format::yssd::YssdSubBlockRaw<'_>> = raws
                        .iter()
                        .map(|(var_id, raw)| yu_format::yssd::YssdSubBlockRaw {
                            var_id: *var_id,
                            raw,
                        })
                        .collect();
                    writer.set_block(fact.dno, yu_format::yssd::encode_multi_body(&subs));
                }
                1 => {
                    let Some(image) = &fact.image else { continue };
                    let raw = crate::image_block::encode(image);
                    writer.set_block(
                        fact.dno,
                        yu_format::yssd::encode_block_body(1, 1, 0, &raw),
                    );
                }
                other => {
                    let _ = other;
                    return Err(error::recoverable(
                        code::INTERNAL,
                        format!("未知预约块类型 {other}"),
                    ));
                }
            }
        }
        let bytes = writer.build();
        let path = self.confined_path(file)?;
        std::fs::write(&path, &bytes).map_err(|e| {
            error::recoverable(code::FILE_WRITE, format!("SAVE[FILE={file}] 写入失败: {e}"))
        })?;
        self.refresh_slots();
        Ok(())
    }

    /// `SAVEEND[]`:丢弃全部未刷盘预约(REA `0x44e1a0`;官方 flush 不清,
    /// 语料每次刷盘后显式 SAVEEND)。
    pub fn save_end(&mut self) {
        self.saveend_calls += 1;
        self.reservations.clear();
    }

    /// `LOAD[FILE DNO=n]`:读一块恢复数据(容器解析;写回归 VM)。
    ///
    /// 块类型 0 → 单项(`var = None`,写调用方 `LET` 目标);块类型 2 →
    /// 逐子块按块内 var_id 恢复(REA `0x447460..`,不读 `LET`/`G_*`);
    /// 块类型 1 = 配置块,段内语义未逆向 ⇒ 不解释(DIV-RT-012/B13)。
    pub fn load_block(&mut self, file: &str, dno: u32) -> Result<LoadedBlock, Diag> {
        self.load_calls += 1;
        let path = self.confined_path(file)?;
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(error::recoverable(
                    code::FILE_MISSING,
                    format!("LOAD 目标不存在: {file}"),
                ));
            }
            Err(e) => {
                return Err(error::recoverable(
                    code::FILE_READ,
                    format!("LOAD 读取失败: {file}: {e}"),
                ));
            }
        };
        let origin = file.as_bytes();
        let save = yu_format::yssd::YssdSave::parse(&bytes, origin)
            .map_err(|d| error::recoverable(code::CONTAINER, format!("LOAD[FILE={file}] {d}")))?;
        let Some(entry) = save.entry(dno) else {
            return Err(error::recoverable(
                code::FILE_MISSING,
                format!(
                    "LOAD[FILE={file}] DNO={dno} 不在容器(共 {} 块)",
                    save.entries.len()
                ),
            ));
        };
        match entry.btype {
            0 => {
                let raw = entry
                    .decompress(origin)
                    .map_err(|d| error::recoverable(code::CONTAINER, format!("{d}")))?;
                let value = yu_format::yssd::YssdValue::parse(&raw, origin, dno)
                    .map_err(|d| error::recoverable(code::CONTAINER, format!("{d}")))?;
                Ok(LoadedBlock {
                    vars: vec![LoadedVar {
                        var: None,
                        snap: payload_to_snapshot(&value, origin, dno)?,
                    }],
                })
            }
            2 => {
                let subs = yu_format::yssd::parse_multi_payload(&entry.compressed, origin, dno)
                    .map_err(|d| error::recoverable(code::CONTAINER, format!("{d}")))?;
                let mut vars = Vec::with_capacity(subs.len());
                for sub in &subs {
                    let sraw = sub
                        .decompress(origin, dno)
                        .map_err(|d| error::recoverable(code::CONTAINER, format!("{d}")))?;
                    let value = yu_format::yssd::YssdValue::parse(&sraw, origin, dno)
                        .map_err(|d| error::recoverable(code::CONTAINER, format!("{d}")))?;
                    vars.push(LoadedVar {
                        var: Some(VarRef::new(space_for(value.ty), sub.var_id)),
                        snap: payload_to_snapshot(&value, origin, dno)?,
                    });
                }
                Ok(LoadedBlock { vars })
            }
            1 => Err(error::recoverable(
                code::COMMAND_UNIMPLEMENTED,
                format!("LOAD[FILE={file} DNO={dno}] 配置块(btype 1)不解释(DIV-RT-012/B13)"),
            )),
            other => Err(error::recoverable(
                code::CONTAINER,
                format!("LOAD[FILE={file} DNO={dno}] 未知块类型 {other}"),
            )),
        }
    }

    /// `LOAD[FILE DNO=n ID=图层]`:读回图像块(btype 1)。
    ///
    /// 载荷格式见 [`crate::image_block`];读取端同时接受 0.495 与 0.479 的
    /// 表头偏移(以段长自证),官方读取端 `0x4477c0` 校验首 u32 是 `0x640`。
    pub fn load_image(&mut self, file: &str, dno: u32) -> Result<yu_rt::event::CapturedImage, Diag> {
        self.load_calls += 1;
        let path = self.confined_path(file)?;
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(error::recoverable(
                    code::FILE_MISSING,
                    format!("LOAD 目标不存在: {file}"),
                ));
            }
            Err(e) => {
                return Err(error::recoverable(
                    code::FILE_READ,
                    format!("LOAD 读取失败: {file}: {e}"),
                ));
            }
        };
        let origin = file.as_bytes();
        let save = yu_format::yssd::YssdSave::parse(&bytes, origin)
            .map_err(|d| error::recoverable(code::CONTAINER, format!("LOAD[FILE={file}] {d}")))?;
        let Some(entry) = save.entry(dno) else {
            return Err(error::recoverable(
                code::FILE_MISSING,
                format!("LOAD[FILE={file}] DNO={dno} 不在容器"),
            ));
        };
        if entry.btype != 1 {
            return Err(error::recoverable(
                code::CONTAINER,
                format!(
                    "LOAD[FILE={file} DNO={dno}] 不是图像块(btype={})",
                    entry.btype
                ),
            ));
        }
        let raw = entry
            .decompress(origin)
            .map_err(|d| error::recoverable(code::CONTAINER, format!("{d}")))?;
        crate::image_block::decode(&raw).map_err(|detail| {
            error::recoverable(
                code::IMAGE_SHAPE,
                format!("LOAD[FILE={file} DNO={dno}] 图像块解析失败: {detail}"),
            )
        })
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
                    error::info(code::FILE_DELETED, format!("FILEACT[DELETE] 已删 {file}")),
                    frame,
                    "FILEACT",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 幂等删除:后置条件「该文件不存在」已成立。
                // 脚本 `es_SaveLoad.yst:417` 的注释是「上書き保存でなく
                // 新規保存にする」—— 它在**每次保存前**无条件删旧槽,
                // 首次保存时目标本就不存在,属正常路径而非故障。
                self.push_diag(error::at_command(
                    error::verbose(
                        code::FILE_MISSING,
                        format!("FILEACT[DELETE] 目标本就不存在(幂等): {file}"),
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
                        code::FILE_DELETED,
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

    /// 把路径名解析成安全绝对路径:越出 save 根/含 `..` 一律
    /// `sv.path.escape`(安全拒绝;契约文档 §5「宿主必须自己做路径归一化
    /// 与越界检查」)。
    ///
    /// **接受 save 根内的绝对路径**:脚本用 `$es.SAVEPATH = $_SAVE_PATH`
    /// (宿主注入的 save 目录绝对路径)拼文件名,`es_First.yst:30` 的
    /// `FILEACT[CREATE=1 FOLDER=$es.SAVEPATH]` 与 `es_SaveLoad.yst` 的
    /// 删除/复制都直接传这个绝对前缀。旧实现见绝对路径即拒,把合法调用
    /// 误判成越界(演出1 实测 2 条)。真正的安全边界仍是「不得越出 save 根」。
    fn confined_path(&self, name: &str) -> Result<PathBuf, Diag> {
        if name.is_empty() {
            return Err(error::recoverable(code::ARG_MISSING, "路径名为空"));
        }
        let raw = Path::new(name);
        // 绝对路径:只做越界校验(必须在 save 根内)。
        if raw.is_absolute() {
            for comp in raw.components() {
                if matches!(comp, std::path::Component::ParentDir) {
                    return Err(error::recoverable(
                        code::PATH_ESCAPE,
                        format!("路径含 `..`: {name}"),
                    ));
                }
            }
            // 目标可能尚不存在(`FILEACT[CREATE=1 FOLDER=...]` 建目录、
            // 新建存档文件),`canonicalize` 会失败 —— 此时退化为**词法**
            // 前缀校验。
            //
            // 判据只能是 `raw` 等于 save 根、或位于其下;不能反写
            // `root.starts_with(raw)` —— 那会放行 save 根的**祖先**
            // (`D:/.../project-eris` 之于 `.../project-eris/save`),
            // 等于允许脚本从上层目录读删。
            let lexical_ok = raw == self.root.as_path() || raw.starts_with(&self.root);
            let (Ok(root), Ok(resolved)) = (self.root.canonicalize(), raw.canonicalize()) else {
                return if lexical_ok {
                    Ok(raw.to_path_buf())
                } else {
                    Err(error::recoverable(
                        code::PATH_ESCAPE,
                        format!("绝对路径不在 save 根内: {name}"),
                    ))
                };
            };
            return if resolved.starts_with(&root) {
                Ok(resolved)
            } else {
                Err(error::recoverable(
                    code::PATH_ESCAPE,
                    format!("目标解析后越出 save 根: {name}"),
                ))
            };
        }
        if name.contains('\\') {
            return Err(error::recoverable(
                code::PATH_ESCAPE,
                format!("路径非 save 根内相对形式: {name}"),
            ));
        }
        let joined = self.root.join(raw);
        let Ok(resolved) = joined.canonicalize() else {
            // 目标不存在时退化为词法归一化(仍拒绝 `..` 分量)。
            for comp in raw.components() {
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

/// 快照 → YSSD 值载荷(容器 `ty u32 + dims + dlen + data`;布局事实见
/// `yu-format/src/yssd.rs` `YssdValue`)。INT/FLT 每元素 8 字节,STR 逐
/// 元素 `{u32 长 + 字节}`——与官方产物一致(P4 roundtrip 自证)。
fn snapshot_payload(snap: &VarSnapshot) -> Result<yu_format::yssd::YssdValue, Diag> {
    let ty = match snap.ty {
        ElemType::Int => yu_format::yssd::YssdValueType::Int,
        ElemType::Float => yu_format::yssd::YssdValueType::Flt,
        ElemType::Str => yu_format::yssd::YssdValueType::Str,
    };
    let mut data = Vec::new();
    for value in &snap.data {
        match (snap.ty, value) {
            (ElemType::Int, Value::Int(n)) => data.extend_from_slice(&n.to_le_bytes()),
            (ElemType::Int, _) => data.extend_from_slice(&0i64.to_le_bytes()),
            (ElemType::Float, Value::Float(f)) => data.extend_from_slice(&f.to_le_bytes()),
            (ElemType::Float, _) => data.extend_from_slice(&0f64.to_le_bytes()),
            (ElemType::Str, Value::Str(bytes)) => {
                data.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                data.extend_from_slice(bytes);
            }
            (ElemType::Str, _) => data.extend_from_slice(&0u32.to_le_bytes()),
        }
    }
    Ok(yu_format::yssd::YssdValue {
        ty,
        dims: snap.dims.clone(),
        data,
    })
}

/// YSSD 值载荷 → 快照(容器类型 → 变量元素类型;`Value::Str` = CP932 原样)。
fn payload_to_snapshot(
    value: &yu_format::yssd::YssdValue,
    origin: &[u8],
    dno: u32,
) -> Result<VarSnapshot, Diag> {
    let ty = match value.ty {
        yu_format::yssd::YssdValueType::Int => ElemType::Int,
        yu_format::yssd::YssdValueType::Flt => ElemType::Float,
        yu_format::yssd::YssdValueType::Str => ElemType::Str,
    };
    let count = value.element_count() as usize;
    let mut data = Vec::with_capacity(count);
    match ty {
        ElemType::Int | ElemType::Float => {
            for chunk in value.data.chunks_exact(8) {
                if ty == ElemType::Int {
                    data.push(Value::Int(i64::from_le_bytes(
                        chunk.try_into().expect("8B"),
                    )));
                } else {
                    data.push(Value::Float(f64::from_le_bytes(
                        chunk.try_into().expect("8B"),
                    )));
                }
            }
        }
        ElemType::Str => {
            let mut cursor = 0usize;
            for _ in 0..count {
                let Some(head) = value.data.get(cursor..cursor + 4) else {
                    break;
                };
                let len = u32::from_le_bytes(head.try_into().expect("4B")) as usize;
                cursor += 4;
                let Some(bytes) = value.data.get(cursor..cursor + len) else {
                    break;
                };
                data.push(Value::Str(bytes.to_vec()));
                cursor += len;
            }
        }
    }
    if data.len() != count {
        return Err(error::recoverable(
            code::CONTAINER,
            format!(
                "DNO={dno}({}):元素数不符,解出 {}",
                String::from_utf8_lossy(origin),
                count
            ),
        ));
    }
    Ok(VarSnapshot {
        ty,
        dims: value.dims.clone(),
        data,
    })
}

/// 容器值类型 → 变量空间(数值 = `@`,字符串 = `$`;旧式空间由 VM 的
/// `canonical_var` 按 id 声明归一化,DIV-RT-017)。
fn space_for(ty: yu_format::yssd::YssdValueType) -> VarSpace {
    match ty {
        yu_format::yssd::YssdValueType::Int | yu_format::yssd::YssdValueType::Flt => VarSpace::At,
        yu_format::yssd::YssdValueType::Str => VarSpace::Dollar,
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

    fn int_snapshot(vals: &[i64], dims: &[u32]) -> VarSnapshot {
        VarSnapshot {
            ty: ElemType::Int,
            dims: dims.to_vec(),
            data: vals.iter().map(|&v| Value::Int(v)).collect(),
        }
    }

    #[test]
    fn reserve_snapshots_value_and_saveend_clears() {
        let mut state = SaveState::new("target/tmp/sv-unit-1", false);
        state
            .reserve(
                12,
                VarRef::new(VarSpace::At, 0x0a43),
                &int_snapshot(&[7, 9], &[2]),
            )
            .expect("reserve");
        assert_eq!(state.reservations_len(), 1);
        let snap = state.snapshot();
        assert_eq!(snap.reservations[0].dno, 12);
        assert_eq!(snap.reservations[0].btype, 0);
        assert_eq!(snap.reservations[0].var_id, 0x0a43);
        assert_eq!(
            snap.reservations[0].snap.as_ref().expect("snap").data,
            vec![Value::Int(7), Value::Int(9)]
        );
        assert_eq!(snap.save_calls, 1);
        // 同 DNO 重复预约 = 替换(REA 0x44c740)。
        state
            .reserve(12, VarRef::new(VarSpace::At, 1), &int_snapshot(&[1], &[]))
            .expect("reserve again");
        assert_eq!(state.reservations_len(), 1);
        assert_eq!(state.snapshot().reservations[0].var_id, 1);
        state.save_end();
        assert_eq!(state.reservations_len(), 0);
        assert_eq!(state.snapshot().saveend_calls, 1);
    }

    #[test]
    fn flush_is_refused_on_readonly_root() {
        let root = temp_root("sv-unit-2-must-not-exist");
        let mut state = SaveState::new(&root, true);
        state
            .reserve(1, VarRef::new(VarSpace::At, 5), &int_snapshot(&[5], &[]))
            .expect("reserve");
        // 只读观察根:拒绝写(语义保护;引用/值齐备,拒绝原因不再是 DIV-RT-042)。
        state.writable = false;
        let err = state.flush("n1.sd").expect_err("refused");
        assert_eq!(err.code, code::WRITE_REFUSED);
        assert!(!root.join("n1.sd").exists());
    }

    #[test]
    fn flush_writes_official_container_and_load_roundtrips() {
        let root = temp_root("sv-unit-6");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut state = SaveState::new(&root, true);
        // int 数组 + str 标量 + 多变量组:三类各一(DNO 1/2/3)。
        state
            .reserve(
                1,
                VarRef::new(VarSpace::At, 2264),
                &int_snapshot(&[0, 6, 0], &[3]),
            )
            .expect("reserve int");
        let str_snap = VarSnapshot {
            ty: ElemType::Str,
            dims: Vec::new(),
            data: vec![Value::Str("テスト".as_bytes().to_vec())],
        };
        state
            .reserve(2, VarRef::new(VarSpace::Dollar, 1112), &str_snap)
            .expect("reserve str");
        state
            .reserve_group(3, ElemType::Int, 2, &[(1161, int_snapshot(&[1, 2], &[2]))])
            .expect("reserve group");
        state.flush("gsd-test.sd").expect("flush");
        assert!(root.join("gsd-test.sd").exists());
        assert_eq!(state.reservations_len(), 3, "官方 flush 不清预约(REA)");
        // 读回:块类型 0(写 LET 目标)+ 块类型 2(自带 var)。
        let block = state.load_block("gsd-test.sd", 1).expect("load 1");
        assert_eq!(block.vars.len(), 1);
        assert!(block.vars[0].var.is_none());
        assert_eq!(
            block.vars[0].snap.data,
            vec![Value::Int(0), Value::Int(6), Value::Int(0)]
        );
        let block = state.load_block("gsd-test.sd", 3).expect("load 3");
        assert_eq!(block.vars.len(), 1);
        assert_eq!(block.vars[0].var, Some(VarRef::new(VarSpace::At, 1161)));
        assert_eq!(block.vars[0].snap.data, vec![Value::Int(1), Value::Int(2)]);
        // 结构级:官方解析器可读,DNO/var 与预约一致。
        let bytes = std::fs::read(root.join("gsd-test.sd")).unwrap();
        let save = yu_format::yssd::YssdSave::parse(&bytes, b"gsd-test.sd").expect("parse");
        assert_eq!(save.entries.len(), 3);
        assert_eq!(save.entry(1).expect("dno1").var_id, 2264);
        assert_eq!(save.entry(2).expect("dno2").var_id, 1112);
        assert_eq!(save.entry(3).expect("dno3").btype, 2);
        assert_eq!(save.entry(2).expect("dno2").var_id, 1112);
        state.save_end();
        assert_eq!(state.reservations_len(), 0);
    }

    #[test]
    fn load_missing_file_is_reported() {
        let mut state = SaveState::new("target/tmp/sv-unit-7", false);
        let err = state.load_block("none.sd", 1).expect_err("missing");
        assert_eq!(err.code, code::FILE_MISSING);
    }

    /// 官方 golden 结构对拍(真值层级 3;缺语料环境 = 显式报错,禁止空跑):
    /// 官方 `seld.sd` DNO=1 = `@es.SELD`(id 2264)Int[2001],本实现的
    /// load_block 必须读出同型快照。
    #[test]
    fn official_seld_block_reads_as_var_snapshot() {
        let Some(root) = std::env::var_os("YURIS_SAMPLE_ERIS_ROOT") else {
            panic!("YURIS_SAMPLE_ERIS_ROOT 未设置(缺样本环境必须显式报错)");
        };
        let path = Path::new(&root).join("save/seld.sd");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("官方 seld.sd 不可读({e}): {}", path.display()));
        std::fs::create_dir_all(temp_root("sv-golden")).unwrap();
        std::fs::write(temp_root("sv-golden/seld.sd"), &bytes).unwrap();
        let mut state = SaveState::new(temp_root("sv-golden"), false);
        let block = state.load_block("seld.sd", 1).expect("load");
        assert_eq!(block.vars.len(), 1);
        let snap = &block.vars[0].snap;
        assert_eq!(snap.ty, ElemType::Int);
        assert_eq!(snap.dims, vec![2001]);
        assert_eq!(snap.data.len(), 2001);
        // 值域断言(活语料:seld 是选择历史,随游玩推进而变;
        // 2026-10-23 DIV-RT-003 取证时 [1]=6,2026-10-24 主人实测后已更新):
        // [0] 恒 0(占位),[1] = 最近选项号,合法域 0..=2001。
        assert_eq!(snap.data[0], Value::Int(0));
        let Value::Int(sel) = snap.data[1] else {
            panic!("seld[1] 应为 INT");
        };
        assert!((0..=2001).contains(&sel), "seld[1]={sel} 越出选项号域");
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

    /// `SAVE[DNO ID=图层]` → 容器 → `LOAD[ID]` 闭环:块必须是 btype 1,
    /// 载荷段长表 = `[1600][w*h*4]`,`LOAD[ID]` 读回同尺寸同像素。
    #[test]
    fn image_block_roundtrips_through_container() {
        let root = temp_root("sv-unit-image");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut state = SaveState::new(&root, true);
        let image = yu_rt::event::CapturedImage {
            width: 4,
            height: 3,
            rgba: (0..48).map(|i| (i * 5) as u8).collect(),
        };
        state.reserve_image(3, &image).expect("reserve");
        state.flush("thumb.sd").expect("flush");
        // 容器层:块类型 1 + 段长表自证。
        let bytes = std::fs::read(root.join("thumb.sd")).unwrap();
        let save = yu_format::yssd::YssdSave::parse(&bytes, b"thumb.sd").unwrap();
        let entry = save.entry(3).expect("DNO3");
        assert_eq!(entry.btype, 1);
        let raw = entry.decompress(b"thumb.sd").unwrap();
        assert_eq!(&raw[0..4], &1600u32.to_le_bytes());
        assert_eq!(&raw[4..8], &(4u32 * 3 * 4).to_le_bytes());
        assert_eq!(raw.len(), 8 + 1600 + 48);
        // 读取端:同尺寸同像素。
        let back = state.load_image("thumb.sd", 3).expect("load_image");
        assert_eq!(back, image);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 官方 0.495 `save00001.sd` 的 DNO=3 必须解成 `100×75 RGBA`
    /// (真值层级 3;缺样本环境显式报错)。样本缩略图是黑屏,只钉形状。
    #[test]
    fn official_dno3_thumbnail_decodes_as_100x75() {
        let Some(sdk) = std::env::var_os("YURIS_SAMPLE_SDK0495") else {
            panic!("YURIS_SAMPLE_SDK0495 未设置(缺样本环境必须显式报错)");
        };
        let path = Path::new(&sdk).join("data/save/save00001.sd");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("官方 save00001.sd 不可读({e}): {}", path.display()));
        let root = temp_root("sv-official-thumb");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("save00001.sd"), &bytes).unwrap();
        let mut state = SaveState::new(&root, false);
        let image = state.load_image("save00001.sd", 3).expect("DNO3");
        assert_eq!((image.width, image.height), (100, 75));
        assert_eq!(image.rgba.len(), 30000);
        // 该样本是纯黑不透明(RGBA 0,0,0,255)。
        assert_eq!(&image.rgba[0..4], &[0, 0, 0, 255]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 单测专用根:本 crate `target/tmp` 下(系统 TEMP 在本环境不可写)。
    fn temp_root(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(name)
    }
}

/// 路径越界防护:save 根的**祖先**绝不能被当作合法绝对路径。
///
/// 回归点:`FILEACT[CREATE=1 FOLDER=$es.SAVEPATH]` 需要放行 save 根本身
/// (及下层),但词法兜底若写成 `root.starts_with(raw)` 就会连
/// `D:/.../project-eris`(save 的父目录)一起放行 —— 脚本可以借此
/// 在上层目录读删文件。
#[test]
fn confined_path_rejects_save_root_ancestor() {
    // 生产路径的 save 根一定是**绝对**路径(宿主注入 `$_SAVE_PATH`);
    // 这里必须同构构造,否则 `is_absolute()` 为 false 会走相对分支。
    let abs = std::env::current_dir()
        .expect("cwd")
        .join("target/tmp/sv-confine-a/b/save");
    let state = SaveState::new(abs.clone(), false);
    let root = state.root.clone();
    assert!(root.is_absolute(), "测试前置:save 根须为绝对路径");
    let parent = root.parent().expect("save 根有父目录").to_path_buf();

    // save 根本身:放行(CREATE 场景)。
    state
        .confined_path(&root.to_string_lossy())
        .expect("save 根本身应放行");

    // 其下层绝对路径:放行。
    let child = root.join("save30001.sd");
    state
        .confined_path(&child.to_string_lossy())
        .expect("save 根下层应放行");

    // 祖先目录:必须拒绝。
    let err = state
        .confined_path(&parent.to_string_lossy())
        .expect_err("save 根的祖先必须被拒绝");
    assert_eq!(err.code, code::PATH_ESCAPE, "祖先应报 PATH_ESCAPE");
}
