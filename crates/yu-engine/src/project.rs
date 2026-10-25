//! 工程数据入口(唯一约定,`AGENTS.md` §2)。
//!
//! - 封包 `<project>/data.ypf` 优先;
//! - 散盘 `<project>/data/` 为备;
//! - 两者都存在时用封包;`save/data` 不是入口,不识别;
//! - 增量包 `update1..9.ypf` 在基础数据之后依序叠加(见 [`Project::update_packs`])。
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

    /// 引擎增量包:`update1.ypf`…`update9.ypf`,按序号递增返回(后挂覆盖先挂)。
    ///
    /// REA:目录扫描按 `.ypf` 后缀命中 `update1..9.ypf` 并按序号登记
    /// (`FUN_0045eda0` / `FUN_0045e3b8`,side notes `format-ypf.md`),
    /// 随后在基础数据之后依序挂载;序号域就是 1..9,`update10.ypf` 不在内。
    /// 位置 = 数据入口所在目录(与 `data.ypf` 同级)。
    pub fn update_packs(&self) -> Vec<PathBuf> {
        let dir = self
            .entry
            .path()
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        (1..=9)
            .map(|n| dir.join(format!("update{n}.ypf")))
            .filter(|path| path.is_file())
            .collect()
    }

    /// 打开主数据源(每次 boot 独立打开;文件句柄不复用)。
    ///
    /// 存在增量包时返回 `基础包 + update1..9` 的叠加栈([`Mount`];
    /// 无增量包时直接返回单一源,保持诊断的 `source_name` 不变)。
    pub fn open_source(&self) -> Result<Box<dyn DataSource>, Failure> {
        let base: Box<dyn DataSource> = match &self.entry {
            DataEntry::Packed(path) => Box::new(open_ypf(path)?),
            DataEntry::Loose(dir) => Box::new(DirSource::open(dir, CODEC).map_err(|d| {
                Failure::runtime(format!("打开 {} 失败: {d}", dir.display()))
            })?),
        };
        let updates = self.update_packs();
        if updates.is_empty() {
            return Ok(base);
        }
        let mut mount = Mount::new(CODEC);
        mount.push(base);
        for path in updates {
            mount.push(Box::new(open_ypf(&path)?));
        }
        Ok(Box::new(mount))
    }

    /// 辅助源:工程根(`system/YSCom/YSCom.ycd` 可能在这里;缺失不是错误)。
    pub fn open_aux(&self) -> Result<Box<dyn DataSource>, Failure> {
        Ok(Box::new(DirSource::open(&self.aux_root, CODEC).map_err(
            |d| Failure::runtime(format!("打开 {} 失败: {d}", self.aux_root.display())),
        )?))
    }

    /// 纹理挂载栈:`基础包 → update1..9 → 散盘`(散盘同名覆盖,
    /// 置于更新包之上 —— 它是开发/覆写通道,必须能压过官方增量内容)。
    pub fn mount(&self) -> Result<Mount, Failure> {
        let mut mount = Mount::new(CODEC);
        if let DataEntry::Packed(path) = &self.entry {
            mount.push(Box::new(open_ypf(path)?));
        }
        for path in self.update_packs() {
            mount.push(Box::new(open_ypf(&path)?));
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

fn open_ypf(path: &Path) -> Result<YpfSource, Failure> {
    YpfSource::open(path, CODEC)
        .map_err(|d| Failure::runtime(format!("打开 {} 失败: {d}", path.display())))
}

pub fn json_escape(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use yu_log::code::vfs;
    use yu_vfs::{LookupKey, ypf::{murmur2, name_len_code}};

    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!("yu-cli-{tag}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 最小 v495 包写入器(仅测试;原样存储,布局同 yu-vfs 测试支持)。
    fn write_pack(path: &Path, entries: &[(&[u8], &[u8])]) {
        let mut index = Vec::new();
        let mut record_starts = Vec::with_capacity(entries.len());
        for (name, _) in entries {
            record_starts.push(index.len());
            index.extend_from_slice(&murmur2(name, 0).to_le_bytes());
            index.push(name_len_code(name.len() as u8, 495));
            index.extend(name.iter().map(|b| !*b));
            index.push(0); // type
            index.push(0); // compressed
            index.extend_from_slice(&0u32.to_le_bytes()); // usize
            index.extend_from_slice(&0u32.to_le_bytes()); // csize
            index.extend_from_slice(&0u32.to_le_bytes()); // dataOffset low
            index.extend_from_slice(&0u32.to_le_bytes()); // dataOffset high
            index.extend_from_slice(&0u32.to_le_bytes()); // payloadHash
        }
        let data_start = 0x20 + index.len();
        let mut data = Vec::new();
        let mut offset = data_start as u64;
        for (i, (name, bytes)) in entries.iter().enumerate() {
            let meta = record_starts[i] + 4 + 1 + name.len();
            index[meta + 2..meta + 6].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            index[meta + 6..meta + 10].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            index[meta + 10..meta + 14].copy_from_slice(&(offset as u32).to_le_bytes());
            index[meta + 18..meta + 22].copy_from_slice(&murmur2(bytes, 0).to_le_bytes());
            data.extend_from_slice(bytes);
            offset += bytes.len() as u64;
        }
        let mut out = Vec::with_capacity(data_start + data.len());
        out.extend_from_slice(b"YPF\0");
        out.extend_from_slice(&495u32.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data_start as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&index);
        out.extend_from_slice(&data);
        std::fs::write(path, out).unwrap();
    }

    fn lookup(source: &dyn DataSource, raw: &[u8]) -> Vec<u8> {
        source
            .open(&LookupKey::new(raw, CODEC).unwrap())
            .unwrap_or_else(|d| panic!("open {:?} failed: {d}", String::from_utf8_lossy(raw)))
    }

    #[test]
    fn update_packs_enumerate_in_numeric_order() {
        let root = temp_root("upd-order");
        std::fs::write(root.join("update3.ypf"), b"placeholder").unwrap();
        std::fs::write(root.join("update1.ypf"), b"placeholder").unwrap();
        std::fs::write(root.join("data.ypf"), b"placeholder").unwrap();
        // 引擎序号域只有 1..9;update10 不入列。
        std::fs::write(root.join("update10.ypf"), b"placeholder").unwrap();
        let project = Project::discover(&root, None).unwrap();
        let names: Vec<String> = project
            .update_packs()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["update1.ypf", "update3.ypf"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_source_layers_updates_over_base_pack() {
        let root = temp_root("upd-open");
        write_pack(
            &root.join("data.ypf"),
            &[(b"a.txt".as_slice(), b"base"), (b"shared.txt", b"base")],
        );
        write_pack(
            &root.join("update1.ypf"),
            &[(b"shared.txt", b"u1"), (b"only1.txt", b"one")],
        );
        write_pack(&root.join("update2.ypf"), &[(b"shared.txt", b"u2")]);

        let project = Project::discover(&root, None).unwrap();
        let source = project.open_source().unwrap();
        assert_eq!(lookup(source.as_ref(), b"a.txt"), b"base");
        // update2 > update1 > 基础包(引擎实测顺序: 基础包 + update1..9)。
        assert_eq!(lookup(source.as_ref(), b"shared.txt"), b"u2");
        assert_eq!(lookup(source.as_ref(), b"only1.txt"), b"one");
        let diag = source
            .open(&LookupKey::new(b"nope.txt", CODEC).unwrap())
            .unwrap_err();
        assert_eq!(diag.code, vfs::MISSING);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mount_puts_updates_above_base_and_below_loose() {
        let root = temp_root("upd-mount");
        write_pack(
            &root.join("data.ypf"),
            &[(b"a.txt".as_slice(), b"base"), (b"shared.txt", b"base")],
        );
        write_pack(
            &root.join("update1.ypf"),
            &[(b"shared.txt", b"u1"), (b"only1.txt", b"one")],
        );
        std::fs::create_dir_all(root.join("data/cg")).unwrap();
        std::fs::write(root.join("data/shared.txt"), b"loose").unwrap();
        std::fs::write(root.join("data/cg/x.png"), b"png").unwrap();

        let project = Project::discover(&root, None).unwrap();
        let mount = project.mount().unwrap();
        // 散盘覆写通道在最上,能压过官方增量内容。
        assert_eq!(mount.open_path(b"shared.txt").unwrap(), b"loose");
        assert_eq!(mount.open_path(b"a.txt").unwrap(), b"base");
        assert_eq!(mount.open_path(b"only1.txt").unwrap(), b"one");
        assert_eq!(mount.open_path(b"cg/x.png").unwrap(), b"png");
        std::fs::remove_dir_all(&root).ok();
    }
}
