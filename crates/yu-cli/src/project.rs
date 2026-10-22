//! 工程数据入口(唯一约定,`AGENTS.md` §2)。
//!
//! - 封包 `<project>/data.ypf` 优先;
//! - 散盘 `<project>/data/` 为备;
//! - 两者都存在时用封包;`save/data` 不是入口,不识别。
//!
//! 读取与执行都在本进程(native):浏览器不接触包字节。

use std::path::{Path, PathBuf};

use yu_vfs::{Codec, DataSource, DirSource, Mount, YpfSource};

use crate::{Failure, EXIT_MISSING};

/// 语料编码(与全仓一致)。
pub const CODEC: Codec = Codec::Cp932;

/// 数据入口形态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataEntry {
    /// 封包文件。
    Packed(PathBuf),
    /// 散盘目录(内容直接位于其下:`ysbin/`、`cg/` …)。
    Loose(PathBuf),
}

impl DataEntry {
    pub fn kind(&self) -> &'static str {
        match self {
            DataEntry::Packed(_) => "packed",
            DataEntry::Loose(_) => "loose",
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            DataEntry::Packed(p) => p,
            DataEntry::Loose(p) => p,
        }
    }
}

/// 已解析的工程根。
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    aux_root: PathBuf,
    entry: DataEntry,
}

/// `/api/status` 用的数据入口摘要。
#[derive(Clone, Debug)]
pub struct DatasetInfo {
    pub kind: &'static str,
    pub path: PathBuf,
    pub bytes: u64,
}

impl DatasetInfo {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"kind\":\"{}\",\"path\":\"{}\",\"bytes\":{}}}",
            self.kind,
            json_escape(&self.path.to_string_lossy()),
            self.bytes
        )
    }
}

impl Project {
    /// 按唯一约定发现数据入口。`--data` 覆盖时按覆盖路径的形态判定。
    pub fn discover(root: &Path, data_override: Option<&Path>) -> Result<Self, Failure> {
        if let Some(data) = data_override {
            let entry = entry_for(data)?;
            let aux_root = data.parent().unwrap_or(Path::new(".")).to_path_buf();
            return Ok(Self {
                root: root.to_path_buf(),
                aux_root,
                entry,
            });
        }
        if root.is_file() {
            let parent = root.parent().unwrap_or(Path::new(".")).to_path_buf();
            return Ok(Self {
                root: parent.clone(),
                aux_root: parent,
                entry: DataEntry::Packed(root.to_path_buf()),
            });
        }
        if !root.is_dir() {
            return Err(Failure::new(
                EXIT_MISSING,
                format!(
                    "工程根不存在:{};期望 <project>/data.ypf 或 <project>/data/",
                    root.display()
                ),
            ));
        }
        let packed = root.join("data.ypf");
        if packed.is_file() {
            return Ok(Self {
                root: root.to_path_buf(),
                aux_root: root.to_path_buf(),
                entry: DataEntry::Packed(packed),
            });
        }
        let loose = root.join("data");
        if loose.is_dir() {
            return Ok(Self {
                root: root.to_path_buf(),
                aux_root: root.to_path_buf(),
                entry: DataEntry::Loose(loose),
            });
        }
        Err(Failure::new(
            EXIT_MISSING,
            format!(
                "找不到数据入口:{} 与 {} 都不存在(数据入口唯一约定:data.ypf 优先、data/ 为备)",
                packed.display(),
                loose.display()
            ),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn aux_root(&self) -> &Path {
        &self.aux_root
    }

    pub fn entry(&self) -> &DataEntry {
        &self.entry
    }

    /// 打开主数据源(每次 boot 独立打开;文件句柄不复用)。
    pub fn open_source(&self) -> Result<Box<dyn DataSource>, Failure> {
        match &self.entry {
            DataEntry::Packed(path) => Ok(Box::new(YpfSource::open(path, CODEC).map_err(
                |d| Failure::runtime(format!("打开 {} 失败: {d}", path.display())),
            )?)),
            DataEntry::Loose(dir) => Ok(Box::new(
                DirSource::open(dir, CODEC)
                    .map_err(|d| Failure::runtime(format!("打开 {} 失败: {d}", dir.display())))?,
            )),
        }
    }

    /// 辅助源:工程根(`system/YSCom/YSCom.ycd` 可能在这里;缺失不是错误)。
    pub fn open_aux(&self) -> Result<Box<dyn DataSource>, Failure> {
        Ok(Box::new(
            DirSource::open(&self.aux_root, CODEC).map_err(|d| {
                Failure::runtime(format!("打开 {} 失败: {d}", self.aux_root.display()))
            })?,
        ))
    }

    /// 纹理挂载栈:封包在下、散盘在上(散盘同名覆盖)。
    pub fn mount(&self) -> Result<Mount, Failure> {
        let mut mount = Mount::new(CODEC);
        if let DataEntry::Packed(path) = &self.entry {
            mount.push(Box::new(YpfSource::open(path, CODEC).map_err(|d| {
                Failure::runtime(format!("打开 {} 失败: {d}", path.display()))
            })?));
        }
        let loose_dir = self.loose_dir();
        if let Some(dir) = loose_dir {
            if dir.is_dir() {
                mount.push(Box::new(DirSource::open(&dir, CODEC).map_err(|d| {
                    Failure::runtime(format!("打开 {} 失败: {d}", dir.display()))
                })?));
            }
        }
        Ok(mount)
    }

    fn loose_dir(&self) -> Option<PathBuf> {
        match &self.entry {
            DataEntry::Packed(_) => {
                let candidate = self.root.join("data");
                candidate.is_dir().then_some(candidate)
            }
            DataEntry::Loose(dir) => Some(dir.clone()),
        }
    }

    pub fn describe(&self) -> DatasetInfo {
        let path = self.entry.path().to_path_buf();
        let bytes = if path.is_file() {
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        DatasetInfo {
            kind: self.entry.kind(),
            path,
            bytes,
        }
    }
}

fn entry_for(data: &Path) -> Result<DataEntry, Failure> {
    if data.is_file() {
        Ok(DataEntry::Packed(data.to_path_buf()))
    } else if data.is_dir() {
        Ok(DataEntry::Loose(data.to_path_buf()))
    } else {
        Err(Failure::new(
            EXIT_MISSING,
            format!("--data 指向的路径不存在:{}", data.display()),
        ))
    }
}

pub(crate) fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
