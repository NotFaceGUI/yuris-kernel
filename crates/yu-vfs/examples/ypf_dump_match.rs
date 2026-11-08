//! 把 ypf 里名字含某子串的条目全部导出到目录(取证用)。
//!
//! 用法: `ypf_dump_match <ypf> <pattern> <out_dir>`
//!
//! 条目名是 CP932 原字节,命令行无法直接传,所以只能按 ASCII 子串匹配。
fn main() {
    let ypf = std::env::args().nth(1).expect("ypf");
    let pat = std::env::args().nth(2).expect("pattern").to_ascii_lowercase();
    let out_dir = std::env::args().nth(3).expect("out dir");
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    let src = yu_vfs::YpfSource::open(std::path::Path::new(&ypf), yu_vfs::Codec::Cp932)
        .expect("open");
    let mut n = 0;
    for e in src.entries() {
        let name = String::from_utf8_lossy(&e.name).to_string();
        if !name.to_ascii_lowercase().contains(&pat) {
            continue;
        }
        let key = match yu_vfs::LookupKey::new(&e.name, yu_vfs::Codec::Cp932) {
            Ok(key) => key,
            Err(error) => {
                eprintln!("skip {name}: {error}");
                continue;
            }
        };
        match yu_vfs::DataSource::open(&src, &key) {
            Ok(bytes) => {
                // 文件名用序号,避免 CP932 乱码落到文件系统。
                let path = std::path::Path::new(&out_dir).join(format!("{n:02}.png"));
                if let Err(error) = std::fs::write(&path, &bytes) {
                    eprintln!("write {}: {error}", path.display());
                    continue;
                }
                println!("{n:02}.png <- {name} ({}B)", bytes.len());
                n += 1;
            }
            Err(error) => eprintln!("open {name}: {error}"),
        }
    }
    println!("dumped {n}");
}
