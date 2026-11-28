//! 真实数据验收 —— 官方 YSPac 0.495 语料。
//!
//! 样本由环境变量提供:
//! - `YURIS_SAMPLE_ERIS_YPF`: 官方 YSPac 0.495 打出的 `data.ypf`
//!   (见 `docs/eris-corpus.md`:1,407 条,31,501,753 字节)。
//! - `YURIS_SAMPLE_SDK0495`: 官方 SDK 0.495 的 `システム` 散盘目录。
//! - `YURIS_SAMPLE_SDK0479`: 官方 SDK 0.479 的 `システム` 散盘目录
//!   (1,424 个文件,其中 85 个日文名,DIV-003 折回验收)。
//!
//! 样本缺失时**显式打印 skip 并累计计数**,不静默通过。
//! 断言只读取索引与少量按偏移定位的负载,不整包载入。

use flate2::read::ZlibDecoder;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use yu_log::code::vfs;
use yu_vfs::{Codec, DataSource, DirSource, LookupKey, YpfDialect, YpfSource};

static SKIP_COUNT: AtomicUsize = AtomicUsize::new(0);

/// 缺样本必须显式可见:设 `YURIS_REQUIRE_SAMPLES=1` 时改为硬失败,
/// 否则打印 `SKIP[count=N]`(禁止静默 ok)。
fn skip(reason: &str) {
    if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
        panic!("YURIS_REQUIRE_SAMPLES=1 but sample unavailable: {reason}");
    }
    let total = SKIP_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
    eprintln!("SKIP[count={total}] {reason}");
}

fn sample_ypf() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_ERIS_YPF") else {
        skip("YURIS_SAMPLE_ERIS_YPF is not set (official YSPac corpus)");
        return None;
    };
    let path = PathBuf::from(path);
    if path.is_file() {
        Some(path)
    } else {
        skip(&format!(
            "YURIS_SAMPLE_ERIS_YPF={} does not exist",
            path.display()
        ));
        None
    }
}

fn key(raw: &[u8]) -> LookupKey {
    LookupKey::new(raw, Codec::Cp932).unwrap()
}

fn raw_slice(path: &Path, offset: u64, len: u32) -> Vec<u8> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    let mut bytes = vec![0u8; len as usize];
    file.read_exact(&mut bytes).unwrap();
    bytes
}

fn image_magic_ok(bytes: &[u8]) -> bool {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.starts_with(&PNG)
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
        || bytes.starts_with(b"BM")
        || bytes.starts_with(b"GIF8")
}

/// 官方 0.495 包的核心回归。
#[test]
fn official_pack_walks_and_closes() {
    let Some(path) = sample_ypf() else {
        return;
    };
    eprintln!("REAL SAMPLE: {}", path.display());
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    let header = source.header();
    let stats = source.stats();

    eprintln!(
        "header: version={} entries={} dataStart={:#x} fileLen={}",
        header.version, header.entry_count, header.data_start, header.file_len
    );
    eprintln!(
        "index: start={:#x} key={:#04x} meta={} bytes={} consumed={} closed={} nameHash={}/{} compressed={} types={:?}",
        stats.index_start,
        stats.name_key,
        stats.meta_len,
        stats.index_bytes,
        stats.consumed_bytes,
        stats.closed,
        stats.name_hash_matches,
        stats.parsed_entries,
        stats.compressed_entries,
        stats.type_counts
    );
    eprintln!("dialect: {}", source.dialect().as_str());

    // 1) 行走恰好到数据区起点。
    assert_eq!(
        source.dialect(),
        YpfDialect::HashedLen { index_start: 0x20, name_key: 0xFF },
        "official 0.495 pack uses indexStart 0x20 and name key 0xFF"
    );
    assert_eq!(stats.meta_len, 0x16, "v495 meta is 22 bytes");
    assert_eq!(stats.declared_entries, header.entry_count);
    assert_eq!(stats.parsed_entries, header.entry_count as usize);
    assert_eq!(source.entries().len(), header.entry_count as usize);
    assert!(stats.closed, "index walk must end exactly at dataStart");
    assert_eq!(stats.consumed_bytes, stats.index_bytes);
    // 2) 名字哈希全命中。
    assert_eq!(
        stats.name_hash_matches,
        stats.parsed_entries,
        "name MurmurHash2 must match for every entry"
    );
    // 3) 每条偏移/长度都在数据区内,且 type/compressed 自洽。
    for entry in source.entries() {
        assert!(!entry.name.is_empty());
        assert!(entry.name_hash_ok);
        assert!(
            (entry.data_offset) >= header.data_start as u64,
            "{}: dataOffset {:#x} before dataStart",
            String::from_utf8_lossy(&entry.name),
            entry.data_offset
        );
        assert!(
            entry.data_offset + entry.compressed_size as u64 <= header.file_len,
            "{}: payload exceeds file",
            String::from_utf8_lossy(&entry.name)
        );
        if entry.compressed {
            assert!(entry.compressed_size <= entry.uncompressed_size);
        } else {
            assert_eq!(entry.compressed_size, entry.uncompressed_size);
        }
    }

    // 4) zlib 条目的解压长度 == usize,且与独立 inflate 逐字节一致。
    let mut zlib_checked = 0usize;
    for entry in source.entries().iter().filter(|e| e.compressed) {
        let bytes = source.open(&entry.normalized).unwrap();
        assert_eq!(bytes.len(), entry.uncompressed_size as usize);
        let raw = raw_slice(&path, entry.data_offset, entry.compressed_size);
        let mut independent = Vec::new();
        ZlibDecoder::new(&raw[..])
            .read_to_end(&mut independent)
            .expect("independent zlib inflate");
        assert_eq!(independent, bytes);
        zlib_checked += 1;
        if zlib_checked >= 24 {
            break;
        }
    }
    eprintln!("zlib entries verified: {zlib_checked}");

    // 5) 全部 raw 条目:只读首 8 字节按扩展名校验魔数(不整包载入)。
    let mut raw_checked = 0usize;
    let mut image_checked = 0usize;
    let mut unknown_ext = 0usize;
    for entry in source.entries().iter().filter(|e| !e.compressed) {
        let head = raw_slice(
            &path,
            entry.data_offset,
            entry.compressed_size.min(8),
        );
        let lower = entry.name.to_ascii_lowercase();
        if lower.ends_with(b".png") {
            assert!(head.starts_with(&[0x89, b'P', b'N', b'G']), "{}", String::from_utf8_lossy(&entry.name));
            image_checked += 1;
        } else if lower.ends_with(b".jpg") || lower.ends_with(b".jpeg") {
            assert!(head.starts_with(&[0xff, 0xd8, 0xff]), "{}", String::from_utf8_lossy(&entry.name));
            image_checked += 1;
        } else if lower.ends_with(b".ogg") {
            assert!(head.starts_with(b"OggS"), "{}", String::from_utf8_lossy(&entry.name));
        } else if lower.ends_with(b".wav") {
            assert!(head.starts_with(b"RIFF"), "{}", String::from_utf8_lossy(&entry.name));
        } else if lower.ends_with(b".ybn") {
            // 四种元数据表容器都可能原样存储(YSTB/YSCM/YSCF/YSLB/YSTL/YSVR/YSTD)。
            assert_eq!(&head[0..2], b"YS", "{}", String::from_utf8_lossy(&entry.name));
        } else {
            unknown_ext += 1;
        }
        raw_checked += 1;
    }
    eprintln!(
        "raw entries verified: {raw_checked} (image magic ok: {image_checked}, other ext: {unknown_ext})"
    );
    assert!(image_checked > 0, "at least one raw image must match its magic");

    // 6) 抽样完整读取 raw 条目,确认长度与内容一致。
    for entry in source.entries().iter().filter(|e| !e.compressed).take(8) {
        let bytes = source.open(&entry.normalized).unwrap();
        assert_eq!(bytes.len(), entry.uncompressed_size as usize);
        assert!(image_magic_ok(&bytes), "{}", String::from_utf8_lossy(&entry.name));
    }
}

/// 验收点名项: `open("ysbin\\ysc.ybn")` 以 `YSCM` 开头、版本 495。
#[test]
fn official_pack_ysc_is_yscm_495() {
    let Some(path) = sample_ypf() else {
        return;
    };
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();

    // 只有 ASCII 大小写折叠 + 分隔符归一,没有额外猜测。
    for raw in [
        br"ysbin\ysc.ybn".as_slice(),
        b"YSBIN\\YSC.YBN",
        b"ysbin/ysc.ybn",
    ] {
        let bytes = source.open(&key(raw)).unwrap();
        assert_eq!(&bytes[0..4], b"YSCM", "magic for {:?}", raw);
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        eprintln!(
            "ysc via {:?}: {} bytes, magic YSCM, version {}",
            String::from_utf8_lossy(raw),
            bytes.len(),
            version
        );
        assert_eq!(version, 495);
    }

    let entry = source
        .entries()
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(br"ysbin\ysc.ybn"))
        .unwrap();
    assert!(entry.compressed);
    assert_eq!(
        entry.uncompressed_size as usize,
        source.open(&entry.normalized).unwrap().len()
    );
}

/// 缺失条目与非目录的错误码必须显式。
#[test]
fn official_pack_error_codes() {
    let Some(path) = sample_ypf() else {
        return;
    };
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    assert_eq!(
        source.open(&key(b"no/such/entry.bin")).unwrap_err().code,
        vfs::MISSING
    );
    assert_eq!(
        source.list(&key(br"ysbin\ysc.ybn")).unwrap_err().code,
        vfs::NOT_DIR
    );
}

fn sdk_0495_dir() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_SDK0495") else {
        skip("YURIS_SAMPLE_SDK0495 is not set (0.495 loose-dir check)");
        return None;
    };
    let path = PathBuf::from(path);
    if !path.is_dir() {
        skip(&format!(
            "YURIS_SAMPLE_SDK0495={} is not a directory",
            path.display()
        ));
        return None;
    }
    // 0.495 SDK 盘的顶层全是日文名;散盘检查实际对象是 `システム/`
    // (data/、system/、yscfg.dat、yu-ris.exe)。根即该系统目录时直接用。
    let system = path.join("システム");
    if system.is_dir() && path.file_name().is_none_or(|n| n != "システム") {
        Some(system)
    } else {
        Some(path)
    }
}

fn sdk_0479_dir() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_SDK0479") else {
        skip("YURIS_SAMPLE_SDK0479 is not set (0.479 loose-dir check)");
        return None;
    };
    let path = PathBuf::from(path);
    if !path.is_dir() {
        skip(&format!(
            "YURIS_SAMPLE_SDK0479={} is not a directory",
            path.display()
        ));
        return None;
    }
    let system = path.join("システム");
    if system.is_dir() && path.file_name().is_none_or(|n| n != "システム") {
        Some(system)
    } else {
        Some(path)
    }
}

fn find_first_ascii_file(source: &DirSource, dir: &LookupKey, depth: usize) -> Option<LookupKey> {
    if depth > 8 {
        return None;
    }
    for entry in source.list(dir).ok()? {
        if entry.name.iter().any(|&b| b >= 0x80) {
            continue;
        }
        let child = match dir.file_name() {
            None => LookupKey::new(&entry.name, Codec::Cp932).ok()?,
            Some(_) => {
                let mut raw = dir.as_bytes().to_vec();
                raw.push(b'/');
                raw.extend_from_slice(&entry.name);
                LookupKey::new(&raw, Codec::Cp932).ok()?
            }
        };
        match entry.kind {
            yu_vfs::EntryKind::File => return Some(child),
            yu_vfs::EntryKind::Dir => {
                if let Some(found) = find_first_ascii_file(source, &child, depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// 跨版本散盘目录: 大小写折叠与显式跳过非 ASCII 名。
#[test]
fn sdk_0495_loose_directory_works_with_case_folding() {
    let Some(root) = sdk_0495_dir() else {
        return;
    };
    let source = DirSource::open(&root, Codec::Cp932).unwrap();
    eprintln!(
        "0.495 DirSource: files={} skipped={}",
        source.file_count(),
        source.skipped_count()
    );
    assert!(source.file_count() > 0);
    let file = find_first_ascii_file(&source, &LookupKey::root(), 0)
        .expect("0.495 SDK dir must contain an ASCII-named file");
    let bytes = source.open(&file).unwrap();
    let mut upper = file.as_bytes().to_vec();
    upper.make_ascii_uppercase();
    assert!(source.exists(&key(&upper)));
    assert_eq!(source.open(&key(&upper)).unwrap(), bytes);
    eprintln!("loose sample: {} bytes={}", file.display(), bytes.len());
}

/// DIV-003 验收: 0.479 SDK 散盘 1,424 个文件全部索引,0 skipped。
/// 其中 85 个日文名经 yu-format CP932 码表折回字节键(引擎在 Windows 上
/// 按 Unicode 名访问散盘文件;折回即恢复其 CP932 字节路径语义)。
#[test]
fn sdk_0479_loose_directory_indexes_every_file_including_japanese_names() {
    let Some(root) = sdk_0479_dir() else {
        return;
    };
    let source = DirSource::open(&root, Codec::Cp932).unwrap();
    eprintln!(
        "0.479 DirSource: files={} skipped={}",
        source.file_count(),
        source.skipped_count()
    );
    assert_eq!(
        source.file_count(),
        1_424,
        "官方 0.479 SDK システム 目录文件数"
    );
    assert_eq!(
        source.skipped_count(),
        0,
        "DIV-003: 非 ASCII 名必须全部折回,不得跳过"
    );

    // 日文名锚点: data/cgsys/extra/bgmmode/tip_再生時間.png
    // (CP932 独立 oracle,Python cp932: 再=0x8DC4 生=0x90B6 時=0x8E9E 間=0x8A94)。
    let raw = b"data/cgsys/extra/bgmmode/tip_\x8D\xC4\x90\xB6\x8E\x9E\x8A\xD4.png";
    let tip = key(raw);
    assert!(source.exists(&tip), "折回后的日文名必须命中: {}", tip.display());
    let bytes = source.open(&tip).unwrap();
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G']),
        "tip_再生時間.png 必须按 PNG 魔数读出"
    );
    eprintln!("japanese name ok: {} bytes={}", tip.display(), bytes.len());
}

// ---------------------------------------------------------------------------
// 0.255 官方样本包(参考仓库公开的 `example-files` 中的 v255 包)
// ---------------------------------------------------------------------------

fn sample_v255_ypf() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_V2_YPF") else {
        skip("YURIS_SAMPLE_V2_YPF is not set (official 0.255 sample pack)");
        return None;
    };
    let path = PathBuf::from(path);
    if path.is_file() {
        Some(path)
    } else {
        skip(&format!(
            "YURIS_SAMPLE_V2_YPF={} does not exist",
            path.display()
        ));
        None
    }
}

/// 0.255 包:103 条、`+0x0C` 是**索引块字节数**(相对)而不是绝对 dataStart;
/// 名字 Adler32 + 负载 Adler32 + `V004` 长度表(`<0x100` 交换表)。
///
/// 这直接否证了既有计划里「<265 无哈希」的假设:官方 0.255 样本 103/103
/// 条名字哈希都是 Adler32(见 PROGRESS 成果 L-1 的勘误)。
#[test]
fn v255_pack_walks_and_closes() {
    let Some(path) = sample_v255_ypf() else {
        return;
    };
    eprintln!("REAL SAMPLE V255 YPF: {}", path.display());
    let source = YpfSource::open(&path, Codec::Cp932).unwrap();
    let header = source.header();
    assert_eq!(header.version, 255, "包版本");
    assert_eq!(header.entry_count, 103, "条目数");
    assert_eq!(header.data_start, 0x1083, "索引块结束/首负载偏移");
    let stats = source.stats();
    assert!(stats.closed, "索引必须闭合");
    assert_eq!(stats.parsed_entries, 103);
    assert_eq!(stats.name_hash_matches, 103, "全量名字哈希命中");
    assert_eq!(stats.name_key, 0xFF, "名字 XOR key");
    // V000 与 V004 在 0x13↔0x0D 这一对上一致;两份都是结构合法候选
    // (候选顺序先 V000),不把"第一个能解析的"当成唯一事实。
    assert!(
        matches!(
            stats.profile.len_map,
            yu_vfs::ypf::LenMap::V000 | yu_vfs::ypf::LenMap::V004
        ),
        "0.255 包必须命中 V000/V004 长度表族"
    );
    assert_eq!(stats.profile.name_hash, yu_vfs::ypf::HashAlg::Adler32);
    assert_eq!(stats.profile.data_hash, yu_vfs::ypf::HashAlg::Adler32);
    assert_eq!(stats.compressed_entries, 103, "全部条目为 zlib 负载");
    assert!(
        matches!(source.dialect(), YpfDialect::Other { .. }),
        "0.255 不属于已命名的 0.495/v473 方言"
    );
    assert_eq!(source.entries().len(), 103);
    for entry in source.entries() {
        assert!(
            entry.name.starts_with(b"ysbin\\"),
            r"条目名必须以 ysbin\ 开头: {:?}",
            String::from_utf8_lossy(&entry.name)
        );
        assert!(entry.name_hash_ok, "名字哈希必须逐条命中");
    }

    // 至少一个条目与参考解包逐字节一致(0.255 语料目录本身来自该包)。
    let ysc = source.open(&key(b"ysbin/ysc.ybn")).unwrap();
    assert_eq!(&ysc[..4], b"YSCM");
    let cfg = source.open(&key(b"ysbin/yscfg.ybn")).unwrap();
    assert_eq!(&cfg[..4], b"YSCF");
    assert_eq!(cfg.len(), 62, "0.255 的 YSCF 短布局(62 字节)");
    if let Some(dir) = std::env::var_os("YURIS_SAMPLE_V2_255").map(PathBuf::from) {
        if dir.is_dir() {
            assert_eq!(
                ysc,
                std::fs::read(dir.join("ysc.ybn")).unwrap(),
                "ysc.ybn 必须与参考解包逐字节一致"
            );
            assert_eq!(
                cfg,
                std::fs::read(dir.join("yscfg.ybn")).unwrap(),
                "yscfg.ybn 必须与参考解包逐字节一致"
            );
        }
    }
}

/// 0.290 商业重打包包的索引块末尾有 2 字节 0 对齐填充(1940 条走完后
/// `pos = 0x1aeec`,头部声明 `0x1aeee`)。零填充不构成伪条目,必须接受。
#[test]
fn v290_repack_index_zero_padding_is_accepted() {
    let Some(path) = std::env::var_os("YURIS_SAMPLE_V2_290B_CGSYS") else {
        skip("YURIS_SAMPLE_V2_290B_CGSYS is not set (0.290 repack cgsys.ypf)");
        return;
    };
    let path = PathBuf::from(path);
    if !path.is_file() {
        skip(&format!(
            "YURIS_SAMPLE_V2_290B_CGSYS={} does not exist",
            path.display()
        ));
        return;
    }
    let source = YpfSource::open(&path, Codec::Cp932).expect("open 0.290 repack pack");
    let header = source.header();
    let stats = source.stats();
    assert_eq!(
        stats.parsed_entries, header.entry_count as usize,
        "条目数必须走满"
    );
    assert!(
        source.entries().iter().all(|e| e.name_hash_ok),
        "零填充不得影响名字哈希校验"
    );
    assert!(!source.entries().is_empty());
}

fn sample_pack(env: &str, what: &str) -> Option<PathBuf> {
    let Some(path) = std::env::var_os(env) else {
        skip(&format!("{env} is not set ({what})"));
        return None;
    };
    let path = PathBuf::from(path);
    if path.is_file() {
        Some(path)
    } else {
        skip(&format!("{env}={} does not exist", path.display()));
        None
    }
}

/// 0.263 商业包:YSCF 是 `V263`(标题长 +0x3C,标题 +0x3E,98 字节)。
#[test]
fn v263_pack_yscfg_layout() {
    let Some(path) = sample_pack("YURIS_SAMPLE_V2_263_PACK", "0.263 pac/bn.ypf") else {
        return;
    };
    let source = YpfSource::open(&path, Codec::Cp932).expect("open 0.263 pack");
    let bytes = source
        .open(&key(b"ysbin/yscfg.ybn"))
        .expect("ysbin/yscfg.ybn");
    let cfg = yu_format::yscf::Yscf::parse(&bytes, b"0.263/yscfg.ybn").expect("parse");
    assert_eq!(cfg.layout, yu_format::yscf::YscfLayout::V263);
    assert_eq!((cfg.width, cfg.height), (800, 600));
    assert_eq!(cfg.title.len(), 36, "title={:02x?}", cfg.title);
}

/// 0.289 商业包:YSCF 是 `V292`(标题 +0x42),1024x768。
#[test]
fn v289_pack_yscfg_layout() {
    let Some(path) = sample_pack("YURIS_SAMPLE_V2_289_PACK", "0.289 pac/ysbin.ypf") else {
        return;
    };
    let source = YpfSource::open(&path, Codec::Cp932).expect("open 0.289 pack");
    let bytes = source
        .open(&key(b"ysbin/yscfg.ybn"))
        .expect("ysbin/yscfg.ybn");
    let cfg = yu_format::yscf::Yscf::parse(&bytes, b"0.289/yscfg.ybn").expect("parse");
    assert_eq!(cfg.layout, yu_format::yscf::YscfLayout::V292);
    assert_eq!((cfg.width, cfg.height), (1024, 768));
}

/// 0.290 商业包:YSCF 是 `Old`(宽 +0x10/高 +0x14,标题 +0x42)。
#[test]
fn v290_pack_yscfg_layout() {
    let Some(path) = sample_pack("YURIS_SAMPLE_V2_290_PACK", "0.290 pac/bn.ypf") else {
        return;
    };
    let source = YpfSource::open(&path, Codec::Cp932).expect("open 0.290 pack");
    let bytes = source
        .open(&key(b"ysbin/yscfg.ybn"))
        .expect("ysbin/yscfg.ybn");
    let cfg = yu_format::yscf::Yscf::parse(&bytes, b"0.290/yscfg.ybn").expect("parse");
    assert_eq!(cfg.layout, yu_format::yscf::YscfLayout::Old);
    assert_eq!((cfg.width, cfg.height), (800, 600));
}

/// 0.290 `<merichri>`:`productlib` 脚本请求 `cgsys/logo/logo_R`(大写 R),
/// 而 `cgsys.ypf` 里只有 `logo_r.png`。官方 Windows 引擎大小写不敏感命中;
/// 内核旧实现精确比较 ⇒ LOGO.BUF 空、标题 logo 中间缺一块(2026-11-28 实机)。
#[test]
fn v290_logo_assets_resolve_case_insensitively() {
    let Some(path) = sample_pack("YURIS_SAMPLE_V2_290_CGSYS", "0.290 pac/cgsys.ypf") else {
        return;
    };
    let source = YpfSource::open(&path, Codec::Cp932).expect("open 0.290 cgsys.ypf");
    let lower = LookupKey::new(b"cgsys/logo/logo_r.png", Codec::Cp932).expect("lower key");
    assert!(source.exists(&lower), "包内小写名应存在");
    let upper = LookupKey::new(b"cgsys/logo/logo_R.png", Codec::Cp932).expect("upper key");
    assert!(
        source.exists(&upper),
        "官方 Windows 语义:文件名大小写不敏感,logo_R 必须命中 logo_r"
    );
    assert_eq!(
        source.open_prefix(&upper, 8).expect("read prefix").len(),
        8,
        "大小写回退必须能真正读出负载"
    );
}
