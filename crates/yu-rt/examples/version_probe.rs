//! 示例:游戏引擎版本判别 —— **全部来自封包/散盘内的表头数据**,
//! 不靠 exe 文件名、不靠内置版本表(A5:数据自证)。
//!
//! # 用法
//!
//! ```text
//! cargo run -p yu-rt --example version_probe -- <游戏根目录|data.ypf> [辅助根]
//! ```
//!
//! 散盘工程传 `…/data`(ysbin 的上级);封包游戏直接传 `data.ypf` ——
//! **不需要解包到磁盘**,[`yu_vfs::YpfSource`] 按归一键直读包内条目。
//!
//! # 判别原理
//!
//! `.ybn` 家族容器头 = `+0x00` 魔数 + `+0x04` u32 版本。七张元数据表各自
//! 申报版本,交叉一致即为引擎版本(ERIS 全表 495 = YU-RIS 0.495;
//! <game-b> 全表 494 = 0.494)。脚本(`yst%05d.ybn`)头部明文同样带版本,
//! 不需要解密即可读;解密一段脚本则进一步证明密钥与布局自洽。
//!
//! 命令表 `ysbin/ysc.ybn`(YSCM)就在封包内 —— **哪怕是封包,命令/参数名
//! 也来自工程数据,与版本无关**;运行核对版本号不做任何分支(A5)。
//! 版本号在本内核只有一个用途:进 `table_provenance` 供审计。

use yu_format::container::read_header;
use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::Codec as TextCodec;
use yu_format::Segments;
use yu_vfs::{DataSource, DirSource, LookupKey, YpfSource};

fn key(path: &[u8]) -> LookupKey {
    LookupKey::new(path, yu_vfs::Codec::Cp932).expect("归一键")
}

fn main() {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("用法: version_probe <游戏根目录|data.ypf> [辅助根]");
        std::process::exit(2);
    };
    let path = std::path::Path::new(&arg);

    // ① 打开数据源:封包(pack)或散盘目录,上层无感(A1)。
    let ypf;
    let dir;
    let src: Box<dyn DataSource> = if path.is_file() {
        ypf = YpfSource::open(path, yu_vfs::Codec::Cp932).expect("打开 ypf 封包");
        let h = ypf.header();
        println!(
            "封包: {} (ypf 包版本 {}, 方言 {}, {} 条目)",
            path.display(),
            h.version,
            ypf.dialect().as_str(),
            ypf.stats().parsed_entries,
        );
        Box::new(ypf)
    } else {
        dir = DirSource::open(path, yu_vfs::Codec::Cp932).expect("打开散盘目录");
        println!("散盘: {} ({})", path.display(), dir.source_name());
        Box::new(dir)
    };

    // ② 七张元数据表:魔数 + 版本 + 计数,全部逐表申报。
    //    (word08 的语义随表不同:命令数/标签数/记录数/清单条数…)
    let tables: [(&[u8], &[u8; 4], &str); 7] = [
        (b"ysbin/ysc.ybn", b"YSCM", "命令数"),
        (b"ysbin/ysl.ybn", b"YSLB", "标签数"),
        (b"ysbin/ysv.ybn", b"YSVR", "变量记录数"),
        (b"ysbin/yst.ybn", b"YSTD", "槽位基数"),
        (b"ysbin/yst_list.ybn", b"YSTL", "清单条数"),
        (b"ysbin/yscfg.ybn", b"YSCF", "-"),
        (b"ysbin/yse.ybn", b"YSER", "消息条数"),
    ];
    let mut versions = Vec::new();
    for (path, magic, what) in tables {
        let Ok(bytes) = src.open(&key(path)) else {
            println!("  {:<22} 缺失", String::from_utf8_lossy(path));
            continue;
        };
        let h = read_header(&bytes, path, magic).expect("容器头");
        // YSVR 的条数是 +0x08 的 u16(高位属于后续字段);其余表为 u32。
        let count = if magic == b"YSVR" {
            (h.word08 & 0xFFFF).to_string()
        } else {
            h.word08.to_string()
        };
        println!(
            "  {:<22} {:4} v{}  {what}={count}",
            String::from_utf8_lossy(path),
            String::from_utf8_lossy(magic),
            h.version
        );
        versions.push((path, h.version));
    }

    // ③ 脚本:头部明文即带版本(不解密可读);再解密一份证明密钥自洽。
    let scripts: Vec<_> = src
        .list(&key(b"ysbin"))
        .expect("列 ysbin")
        .into_iter()
        .filter(|e| {
            e.name.len() == 12
                && e.name.starts_with(b"yst")
                && e.name.ends_with(b".ybn")
                && e.name[3..8].iter().all(|b| b.is_ascii_digit())
        })
        .collect();
    println!("  ysbin 脚本文件: {} 个", scripts.len());
    if let Some(first) = scripts.first() {
        let name: Vec<u8> = [b"ysbin/".as_slice(), first.name.as_slice()].concat();
        let bytes = src.open(&key(&name)).expect("脚本");
        let header_version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        match Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, &name) {
            Ok(segs) => println!(
                "  {:<22} v{}(解密自洽:count={})",
                String::from_utf8_lossy(&name),
                header_version,
                segs.count
            ),
            Err(d) => println!(
                "  {:<22} v{}(头部明文可读;解密失败: {})",
                String::from_utf8_lossy(&name),
                header_version,
                d.detail
            ),
        }
    }

    // ④ 工程设置:分辨率/标题(数据自证)。
    if let Ok(bytes) = src.open(&key(b"ysbin/yscfg.ybn")) {
        if let Ok(cfg) = yu_format::yscf::Yscf::parse(&bytes, b"ysbin/yscfg.ybn") {
            let title = TextCodec::Cp932.decode(&cfg.title, b"yscfg").unwrap_or_default();
            println!(
                "  ysbin/yscfg.ybn        画面 {}x{},标题 {:?}",
                cfg.width, cfg.height, title
            );
        }
    }

    // ⑤ 编译器定义(可选):封包游戏常不带 → 运行核用内嵌 ABI 兜底并发
    //    `table.fallback` 日志(见 `Vm::boot`);命令表不受影响。
    let ycd = key(b"system/YSCom/YSCom.ycd");
    println!(
        "  system/YSCom/YSCom.ycd {}",
        if src.exists(&ycd) { "存在" } else { "缺失(系统变量走内嵌 ABI 兜底,已用 ysv 校验)" }
    );

    // ⑥ 判别:七表交叉一致的版本即引擎版本。
    let distinct: std::collections::BTreeSet<u32> =
        versions.iter().map(|(_, v)| *v).collect();
    if distinct.len() == 1 {
        println!(
            "判别:七表版本交叉一致 = {},即 YU-RIS 引擎 0.{}(数据自证,与 exe 名无关)",
            distinct.iter().next().unwrap(),
            distinct.iter().next().unwrap()
        );
    } else {
        println!("判别:版本不一致!{versions:?}(需人工核查,内核不按版本分支)");
    }
}
