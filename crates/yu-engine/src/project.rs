//! 工程数据入口(官方挂载规则,`AGENTS.md` §2)。
//!
//! 引擎的封包发现(`FUN_0045eda0` / `FUN_0045e3b8`,见 `docs/formats/ypf.md` §4):
//! 在工程目录下扫 `*.ypf`,子目录**只递归 `pac/`**;`update1..9.ypf` 登记为
//! 增量层,其余 `.ypf` 是基础包。工程目录有两种形态:
//!
//! - 开发工程:`<project>/data/`(SDK 布局;包在 `data/*.ypf` 与 `data/pac/*.ypf`);
//! - 发行版:`<project>/`(包在 `<project>/*.ypf` 与 `<project>/pac/*.ypf`,
//!   例如商业包的 `pac/bn.ypf`、`pac/cg.ypf` 这种按内容命名的多包)。
//!
//! 旧约定保留:`data.ypf` 只是“根目录只有一个基础包”的特例;`data/` 同时存在
//! 时作为散盘叠层(覆写包内容)。`save/data` 不是入口,不识别。
//!
//! 扫描到的非 YPF 文件跳过并计数,不是错误:实测商业包 `pac/ed01..04.ypf`
//! 就是 MPEG-PS 影片,只靠扩展名分不出来。
//!
//! 读取与执行都在本进程(native):浏览器不接触包字节。

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use yu_vfs::{Codec, DataSource, DirSource, Mount, YpfSource};

use crate::{Failure, EXIT_MISSING};

/// 语料编码(与全仓一致)。
pub const CODEC: Codec = Codec::Cp932;

/// YPF 魔数(`docs/formats/ypf.md` §1)。
const YPF_MAGIC: [u8; 4] = *b"YPF\0";

/// 散盘数据根的识别标记:任一子目录存在即视为数据目录。
///
/// 用于区分“真散盘工程”与“只有几个包的空目录”,避免把发行版根目录
/// (含 exe、存档、备份)整体当资产树索引。
const CONTENT_MARKERS: &[&str] = &[
    "ysbin",
    "cg",
    "cgsys",
    "se",
    "bgm",
    "voice",
    "voicefast",
    "config",
    "save",
    "pac",
];

/// 封包层(一个工程下扫描到的全部 YPF)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackSet {
    /// 基础包(按文件名排序,确定性;后挂覆盖先挂)。
    pub base: Vec<PathBuf>,
    /// 增量层 `update1..9.ypf`(按序号升序)。
    pub updates: Vec<PathBuf>,
    /// 扫描到但魔数不是 `YPF\0` 的文件(如伪装成 `.ypf` 的影片)。
    pub skipped: Vec<PathBuf>,
}

impl PackSet {
    /// 是否一个包都没有(纯散盘工程)。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.updates.is_empty()
    }
}

/// 数据入口形态(主形态;散盘叠层另见 [`Project::loose`])。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataEntry {
    /// 封包层(基础包 + 增量层)。
    Packed(PackSet),
    /// 散盘目录(内容直接位于其下:`ysbin/`、`cg/` …)。
    Loose(PathBuf),
}

impl DataEntry {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            DataEntry::Packed(_) => "packed",
            DataEntry::Loose(_) => "loose",
        }
    }

    /// 主入口路径:第一个基础包(没有则第一个增量包),散盘则是目录。
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            DataEntry::Packed(packs) => packs
                .base
                .first()
                .or_else(|| packs.updates.first())
                .map_or_else(|| Path::new(""), PathBuf::as_path),
            DataEntry::Loose(dir) => dir,
        }
    }
}

/// 已解析的工程根。
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    aux_root: PathBuf,
    /// 数据根:散盘回退与包扫描的共同根(开发工程 = `<root>/data`,发行版 = `<root>`)。
    data_root: PathBuf,
    /// 封包层。
    packs: PackSet,
    /// 散盘叠层(存在时位于包之上;发行版根目录/开发工程 `data/`)。
    loose: Option<PathBuf>,
    /// 运行时合成资产叠层(挂载栈最高优先;`LOAD[ID]` 缩略图)。
    ///
    /// 内容哈希命名 ⇒ 重复写入幂等;所有 `open_source()`/`mount()` 出来的
    /// 数据源共享同一份,渲染器纹理解析与 VM 资产解析看到相同字节。
    overlay: yu_vfs::SharedMem,
}

/// `/api/status` 用的数据入口摘要。
#[derive(Clone, Debug)]
pub struct DatasetInfo {
    pub kind: &'static str,
    pub path: PathBuf,
    pub bytes: u64,
    /// 基础包数。
    pub packs: usize,
    /// 增量层数。
    pub updates: usize,
    /// 非 YPF 跳过数。
    pub skipped: usize,
}

impl DatasetInfo {
    /// 单行摘要(CLI / 宿主 / 服务端日志统一口径)。
    pub fn summary(&self) -> String {
        format!(
            "{}:{} ({} bytes, packs={} updates={} skipped={})",
            self.kind,
            self.path.display(),
            self.bytes,
            self.packs,
            self.updates,
            self.skipped
        )
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"kind\":\"{}\",\"path\":\"{}\",\"bytes\":{},\"packs\":{},\"updates\":{},\"skipped\":{}}}",
            self.kind,
            json_escape(&self.path.to_string_lossy()),
            self.bytes,
            self.packs,
            self.updates,
            self.skipped
        )
    }
}

impl Project {
    /// 按官方挂载规则发现数据入口。`--data` 覆盖时按覆盖路径的形态判定。
    pub fn discover(root: &Path, data_override: Option<&Path>) -> Result<Self, Failure> {
        let overlay = yu_vfs::SharedMem::new(CODEC);

        if let Some(data) = data_override {
            if data.is_file() {
                // 显式包:只挂这一个基础包,增量层从包所在目录扫。
                let dir = parent_dir(data);
                return Self::assemble(root, &dir, false, Some(data), overlay);
            }
            if data.is_dir() {
                return Self::assemble(root, data, false, None, overlay);
            }
            return Err(Failure::new(
                EXIT_MISSING,
                format!("--data 指向的路径不存在:{}", data.display()),
            ));
        }

        if root.is_file() {
            let dir = parent_dir(root);
            return Self::assemble(&dir, &dir, false, Some(root), overlay);
        }
        if !root.is_dir() {
            return Err(Failure::new(
                EXIT_MISSING,
                format!(
                    "工程根不存在:{};期望 <project>/data.ypf、<project>/data/ 或 <project>/pac/*.ypf",
                    root.display()
                ),
            ));
        }

        // 开发工程(SDK 布局)把数据放在 `<root>/data/`;发行版直接用 `<root>`。
        let dev = root.join("data");
        let data_root = if dev.is_dir() {
            dev
        } else {
            root.to_path_buf()
        };
        Self::assemble(root, &data_root, true, None, overlay)
    }

    /// 扫描并装配一个数据根。
    ///
    /// 扫描范围 = 数据根 + 数据根 `/pac`;`scan_project_root` 为真时再并入
    /// 工程根一层(引擎的发现规则是"根 + pac",开发工程的包可能放在工程根)。
    /// 按魔数分流基础包/增量层/跳过。
    fn assemble(
        root: &Path,
        data_root: &Path,
        scan_project_root: bool,
        preferred: Option<&Path>,
        overlay: yu_vfs::SharedMem,
    ) -> Result<Self, Failure> {
        let mut packs = PackSet::default();
        let mut updates: Vec<(u32, PathBuf)> = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();

        let mut roots: Vec<&Path> = vec![data_root];
        if scan_project_root && root != data_root {
            roots.push(root);
        }
        for dir in scan_dirs(&roots) {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_ypf_name(p))
                .collect();
            // 文件名排序:与文件系统枚举顺序无关,保证挂载顺序可复现。
            files.sort();
            for path in files {
                let identity = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if seen.contains(&identity) {
                    continue;
                }
                seen.push(identity);
                if !has_ypf_magic(&path) {
                    packs.skipped.push(path);
                } else if let Some(n) = update_number(&path) {
                    updates.push((n, path));
                } else {
                    packs.base.push(path);
                }
            }
        }

        packs.base.sort();
        if let Some(preferred) = preferred {
            // `--data <pack>` / 位置参数直接给包文件:显式覆盖 = 只挂这一个基础包,
            // 同目录的增量层照常叠加(与旧行为一致)。
            if !has_ypf_magic(preferred) {
                return Err(Failure::new(
                    EXIT_MISSING,
                    format!(
                        "指定的数据包不是 YPF(魔数不符):{}",
                        preferred.display()
                    ),
                ));
            }
            packs.base = vec![preferred.to_path_buf()];
        }
        updates.sort_by_key(|(n, _)| *n);
        packs.updates = updates.into_iter().map(|(_, path)| path).collect();

        let loose = looks_like_data_root(data_root).then(|| data_root.to_path_buf());
        if packs.is_empty() && loose.is_none() {
            return Err(Failure::new(
                EXIT_MISSING,
                format!(
                    "找不到数据入口:{} 下没有 YPF 包,也没有散盘数据目录(ysbin/cg/config/save/pac 等)",
                    data_root.display()
                ),
            ));
        }

        Ok(Self {
            root: root.to_path_buf(),
            // 辅助源(编译器表)在工程根;工程根不存在时退回数据根。
            aux_root: if root.is_dir() {
                root.to_path_buf()
            } else {
                data_root.to_path_buf()
            },
            data_root: data_root.to_path_buf(),
            packs,
            loose,
            overlay,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn aux_root(&self) -> &Path {
        &self.aux_root
    }

    /// 数据根(散盘回退与包扫描的共同根)。
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// 封包层(基础包 / 增量层 / 跳过的非 YPF)。
    pub fn packs(&self) -> &PackSet {
        &self.packs
    }

    /// 散盘叠层目录(不存在则为 `None`)。
    pub fn loose(&self) -> Option<&Path> {
        self.loose.as_deref()
    }

    /// 主入口形态摘要。
    pub fn entry(&self) -> DataEntry {
        if self.packs.is_empty() {
            DataEntry::Loose(
                self.loose
                    .clone()
                    .unwrap_or_else(|| self.data_root.clone()),
            )
        } else {
            DataEntry::Packed(self.packs.clone())
        }
    }

    /// 引擎增量包:`update1.ypf`…`update9.ypf`,按序号递增返回(后挂覆盖先挂)。
    ///
    /// REA:目录扫描按 `.ypf` 后缀命中 `update1..9.ypf` 并按序号登记
    /// (`FUN_0045eda0` / `FUN_0045e3b8`,side notes `format-ypf.md`),
    /// 随后在基础数据之后依序挂载;序号域就是 1..9,`update10.ypf` 不在内
    /// (它是基础包)。
    pub fn update_packs(&self) -> Vec<PathBuf> {
        self.packs.updates.clone()
    }

    /// 打开主数据源(每次 boot 独立打开;文件句柄不复用)。
    ///
    /// 返回 `基础层 + update1..9 + 合成资产叠层` 的叠加栈([`Mount`]);
    /// 基础层 = 全部基础包(没有包时是散盘目录)。叠层是 `LOAD[ID]`
    /// 缩略图的写入点,必须始终在位。
    pub fn open_source(&self) -> Result<Box<dyn DataSource>, Failure> {
        let mut mount = Mount::new(CODEC);
        if self.packs.base.is_empty() {
            if let Some(dir) = &self.loose {
                mount.push(Box::new(DirSource::open(dir, CODEC).map_err(|d| {
                    Failure::runtime(format!("打开 {} 失败: {d}", dir.display()))
                })?));
            }
        } else {
            for path in &self.packs.base {
                mount.push(Box::new(open_ypf(path)?));
            }
        }
        for path in &self.packs.updates {
            mount.push(Box::new(open_ypf(path)?));
        }
        // 合成资产叠层始终置顶:`LOAD[ID]` 的缩略图以内容哈希命名写入,
        // 任何位置读取都拿到同一份字节。
        mount.push(Box::new(self.overlay.clone()));
        Ok(Box::new(mount))
    }

    /// 运行时合成资产叠层(`LOAD[ID]` 缩略图写入点;所有数据源共享)。
    pub fn overlay(&self) -> &yu_vfs::SharedMem {
        &self.overlay
    }

    /// 辅助源:工程根(`system/YSCom/YSCom.ycd` 可能在这里;缺失不是错误)。
    pub fn open_aux(&self) -> Result<Box<dyn DataSource>, Failure> {
        Ok(Box::new(DirSource::open(&self.aux_root, CODEC).map_err(
            |d| Failure::runtime(format!("打开 {} 失败: {d}", self.aux_root.display())),
        )?))
    }

    /// 纹理挂载栈:`基础包 → update1..9 → 散盘 → 合成资产叠层`(后来者
    /// 覆盖;叠层置顶,`LOAD[ID]` 的缩略图不会被同名的官方资产遮住)。
    pub fn mount(&self) -> Result<Mount, Failure> {
        let mut mount = Mount::new(CODEC);
        for path in &self.packs.base {
            mount.push(Box::new(open_ypf(path)?));
        }
        for path in &self.packs.updates {
            mount.push(Box::new(open_ypf(path)?));
        }
        if let Some(dir) = &self.loose {
            if dir.is_dir() {
                mount.push(Box::new(DirSource::open(dir, CODEC).map_err(|d| {
                    Failure::runtime(format!("打开 {} 失败: {d}", dir.display()))
                })?));
            }
        }
        mount.push(Box::new(self.overlay.clone()));
        Ok(mount)
    }

    pub fn describe(&self) -> DatasetInfo {
        let kind = if self.packs.is_empty() {
            "loose"
        } else {
            "packed"
        };
        let path = self
            .packs
            .base
            .first()
            .or_else(|| self.packs.updates.first())
            .cloned()
            .unwrap_or_else(|| self.data_root.clone());
        let bytes = self
            .packs
            .base
            .iter()
            .chain(self.packs.updates.iter())
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        DatasetInfo {
            kind,
            path,
            bytes,
            packs: self.packs.base.len(),
            updates: self.packs.updates.len(),
            skipped: self.packs.skipped.len(),
        }
    }
}

fn parent_dir(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

/// 包扫描目录:每个给定根一层,外加其 `pac/` 子目录(去重)。
fn scan_dirs(roots: &[&Path]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::with_capacity(roots.len() * 2);
    for base in roots {
        for candidate in [base.to_path_buf(), base.join("pac")] {
            if !dirs.iter().any(|d| d == &candidate) {
                dirs.push(candidate);
            }
        }
    }
    dirs
}

fn is_ypf_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ypf"))
}

/// `update1..9.ypf` → 序号;其余(含 `update10.ypf`)是基础包。
///
/// 引擎序号域只有 1..9(`docs/formats/ypf.md` §4);比较不区分大小写,
/// 因为 Windows 文件名本身不区分。
fn update_number(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let digits = name.strip_prefix("update")?.strip_suffix(".ypf")?;
    if digits.len() != 1 {
        return None;
    }
    let n = digits.parse::<u32>().ok()?;
    (1..=9).contains(&n).then_some(n)
}

fn has_ypf_magic(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && magic == YPF_MAGIC
}

fn looks_like_data_root(dir: &Path) -> bool {
    dir.is_dir()
        && CONTENT_MARKERS
            .iter()
            .any(|marker| dir.join(marker).is_dir())
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
    use yu_vfs::{
        ypf::{murmur2, name_len_code},
        LookupKey,
    };

    fn temp_root(tag: &str) -> PathBuf {
        // 缺省落在**仓库内** `target/test-tmp`(本仓既有约定):受限环境
        // (Codex 沙箱等)只允许写工作区,系统 TEMP 会 `PermissionDenied`。
        // `YURIS_TEST_TMP` 仍可覆盖。
        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            });
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!("yu-cli-{tag}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("create_dir_all({}) 失败: {e}", dir.display()));
        dir
    }

    /// 最小 v495 包写入器(仅测试;原样存储,布局同 yu-vfs 测试支持)。
    fn write_pack(path: &Path, entries: &[(&[u8], &[u8])]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
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
        write_pack(&root.join("data.ypf"), &[(b"a.txt".as_slice(), b"base")]);
        write_pack(&root.join("update3.ypf"), &[(b"a.txt", b"u3")]);
        write_pack(&root.join("update1.ypf"), &[(b"a.txt", b"u1")]);
        // 引擎序号域只有 1..9;update10 不入增量层(按基础包处理)。
        write_pack(&root.join("update10.ypf"), &[(b"b.txt", b"u10")]);
        let project = Project::discover(&root, None).unwrap();
        let names: Vec<String> = project
            .update_packs()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["update1.ypf", "update3.ypf"]);
        assert_eq!(
            project.describe().packs,
            2,
            "data.ypf + update10.ypf 都是基础包"
        );
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

    /// 商业发行版的真实布局:`pac/` 下按内容命名的多包 + 增量层 + 伪装成
    /// `.ypf` 的影片(实测 0.481 商业包: `pac/bn.ypf` … `pac/ed01.ypf`)。
    #[test]
    fn pac_directory_multipack_is_discovered_and_non_ypf_is_skipped() {
        let root = temp_root("pac-multi");
        write_pack(&root.join("pac/bn.ypf"), &[(b"ysbin/a.ybn", b"script")]);
        write_pack(&root.join("pac/cg.ypf"), &[(b"cg/b.png", b"png")]);
        write_pack(&root.join("pac/update1.ypf"), &[(b"cg/b.png", b"png-u1")]);
        // MPEG-PS 影片伪装成 .ypf:魔数 00 00 01 BA。
        std::fs::write(root.join("pac/ed01.ypf"), [0x00, 0x00, 0x01, 0xBA, 0x21]).unwrap();

        let project = Project::discover(&root, None).unwrap();
        let packs = project.packs();
        let names: Vec<String> = packs
            .base
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["bn.ypf", "cg.ypf"], "基础包按文件名确定性排序");
        assert_eq!(packs.updates.len(), 1);
        assert_eq!(packs.skipped.len(), 1, "ed01.ypf 非 YPF,跳过并计数");

        let source = project.open_source().unwrap();
        assert_eq!(lookup(source.as_ref(), b"ysbin/a.ybn"), b"script");
        // update1 覆盖同名资产。
        assert_eq!(lookup(source.as_ref(), b"cg/b.png"), b"png-u1");

        let info = project.describe();
        assert_eq!(info.kind, "packed");
        assert_eq!((info.packs, info.updates, info.skipped), (2, 1, 1));
        std::fs::remove_dir_all(&root).ok();
    }

    /// 根目录与 `pac/` 两处的包都要扫到(引擎的扫描范围是根 + pac)。
    #[test]
    fn root_and_pac_packs_are_both_scanned() {
        let root = temp_root("root-pac");
        write_pack(&root.join("data.ypf"), &[(b"a.txt".as_slice(), b"root")]);
        write_pack(&root.join("pac/extra.ypf"), &[(b"b.txt", b"pac")]);

        let project = Project::discover(&root, None).unwrap();
        assert_eq!(project.describe().packs, 2);
        let source = project.open_source().unwrap();
        assert_eq!(lookup(source.as_ref(), b"a.txt"), b"root");
        assert_eq!(lookup(source.as_ref(), b"b.txt"), b"pac");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 纯散盘工程(SDK 布局):`data/` 下没有包,只有目录树。
    #[test]
    fn loose_only_project_uses_data_directory() {
        let root = temp_root("loose-only");
        std::fs::create_dir_all(root.join("data/ysbin")).unwrap();
        std::fs::write(root.join("data/ysbin/yst00000.ybn"), b"loose").unwrap();

        let project = Project::discover(&root, None).unwrap();
        assert_eq!(project.describe().kind, "loose");
        assert_eq!(project.data_root(), root.join("data"));
        let source = project.open_source().unwrap();
        assert_eq!(lookup(source.as_ref(), b"ysbin/yst00000.ybn"), b"loose");
        std::fs::remove_dir_all(&root).ok();
    }

    /// `--data <pack>`:显式覆盖只挂指定的基础包,同目录的增量层照常叠加。
    #[test]
    fn preferred_pack_overrides_base_and_updates_still_layer() {
        let root = temp_root("preferred");
        write_pack(&root.join("aaa.ypf"), &[(b"x.txt".as_slice(), b"aaa")]);
        write_pack(&root.join("zzz.ypf"), &[(b"x.txt", b"zzz")]);
        write_pack(&root.join("update1.ypf"), &[(b"y.txt", b"u1")]);

        let project = Project::discover(&root, Some(&root.join("zzz.ypf"))).unwrap();
        assert_eq!(
            project.describe().path.file_name().unwrap().to_string_lossy(),
            "zzz.ypf"
        );
        assert_eq!(project.describe().packs, 1, "覆盖时只挂指定的基础包");
        let source = project.open_source().unwrap();
        assert_eq!(lookup(source.as_ref(), b"x.txt"), b"zzz");
        assert_eq!(lookup(source.as_ref(), b"y.txt"), b"u1");
        std::fs::remove_dir_all(&root).ok();
    }
}
