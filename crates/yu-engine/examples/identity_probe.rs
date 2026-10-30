//! 取证探针:游戏身份(游戏名 / YU-RIS 版本 / 内核版本 / 窗口图标)。
//!
//! 用法:
//!
//! ```text
//! cargo run -p yu-engine --example identity_probe -- <工程根> [out.png]
//! ```
//!
//! 只走 `yu_engine::identity` 这条宿主会给窗口用的路径:标题/版本来自
//! `ysbin/yscfg.ybn` 与 `ysbin/ysc.ybn`(数据自证),图标来自工程根 exe 的
//! `RT_GROUP_ICON` 资源。给 `out.png` 时把解出的图标写成 PNG 便于肉眼对照
//! 资源管理器里的 exe 图标。

use std::path::Path;

use yu_engine::identity::{Identity, load_icon};
use yu_engine::project::Project;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("用法: identity_probe <工程根> [out.png] [data 目录]");
        std::process::exit(2);
    };
    let out = args.next();
    let data = args.next();
    let project = Project::discover(
        Path::new(&root),
        data.as_deref().map(Path::new),
    )
    .expect("project");
    println!("project root: {}", project.root().display());

    // 标题/版本直接读表(与 boot 同源,免去整条 VM 启动)。
    let source = project.open_source().expect("data source");
    let key = |path: &[u8]| yu_vfs::LookupKey::new(path, yu_vfs::Codec::Cp932).expect("key");
    let cfg = source.open(&key(b"ysbin/yscfg.ybn")).expect("yscfg");
    let cfg = yu_format::yscf::Yscf::parse(&cfg, b"ysbin/yscfg.ybn").expect("yscfg parse");
    let yscm = source.open(&key(b"ysbin/ysc.ybn")).expect("ysc");
    let yscm_version = u32::from_le_bytes(yscm[4..8].try_into().unwrap());

    let identity = Identity::from_boot(&project, &cfg.title, yscm_version, true);
    println!("{}", identity.summary());
    println!("window title: {}", identity.window_title());
    println!("json: {}", identity.to_json());

    match &identity.icon {
        Some(icon) => {
            println!("icon: {}x{} ({} bytes RGBA)", icon.width, icon.height, icon.rgba.len());
            if let Some(path) = out {
                match write_png(Path::new(&path), icon.width, icon.height, &icon.rgba) {
                    Ok(()) => println!("wrote {path}"),
                    Err(error) => println!("PNG 写出失败: {error}"),
                }
            }
        }
        None => println!("icon: 未解出(缺 exe / 只有 PNG 压缩图标)"),
    }

    // 对照:`load_icon` 直调(证明 exe 扫描按大小降序、跳过非 PE)。
    let direct = load_icon(project.root());
    println!(
        "load_icon direct: {}",
        match &direct {
            Some(icon) => format!("{}x{}", icon.width, icon.height),
            None => "none".to_string(),
        }
    );
}

/// 最小 PNG 写出(无压缩=stored deflate 太重;这里用 `miniz_oxide` 无依赖
/// 替代:直接写未压缩 zlib 流)。
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0); // filter: none
        let start = y * width as usize * 4;
        raw.extend_from_slice(&rgba[start..start + width as usize * 4]);
    }
    let idat = zlib_stored(&raw);

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit RGBA
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).map_err(|e| e.to_string())
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// zlib 容器 + stored(无压缩)deflate 块。
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut at = 0;
    while at < data.len() {
        let take = (data.len() - at).min(0xFFFF);
        let last = at + take == data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes());
        out.extend_from_slice(&data[at..at + take]);
        at += take;
    }
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = if value & 1 != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
        }
        *slot = value;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc = table[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}
