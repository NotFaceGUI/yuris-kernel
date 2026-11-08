//! 打印 ypf 条目名的**原始字节**(CP932 十六进制 + ASCII 可见部分)。
//!
//! 用途:脚本用 `CG[FILE=cg/stand/2kot/2kotd_01]`,而 ypf 里的名字可能是
//! 全角/日文数字或不同分隔符。`String::from_utf8_lossy` 会把差异糊成 `?`,
//! 看不出到底哪里不一致 —— 这里逐字节给出来。
//!
//! 用法: `ypf_names_raw <ypf> <ascii 子串>`
fn main() {
    let ypf = std::env::args().nth(1).expect("ypf");
    let pat = std::env::args().nth(2).expect("pattern").to_ascii_lowercase();
    let src = yu_vfs::YpfSource::open(std::path::Path::new(&ypf), yu_vfs::Codec::Cp932)
        .expect("open");
    let mut n = 0;
    for e in src.entries() {
        let printable = String::from_utf8_lossy(&e.name);
        if !printable.to_ascii_lowercase().contains(&pat) {
            continue;
        }
        let ascii: String = e
            .name
            .iter()
            .map(|b| {
                if (0x20..0x7f).contains(b) {
                    (*b as char).to_string()
                } else {
                    format!("\\x{b:02x}")
                }
            })
            .collect();
        println!("{}  ({}B)  hex={}", ascii, e.name.len(), e.name.iter().map(|b| format!("{b:02x}")).collect::<String>());
        n += 1;
    }
    println!("matches: {n}");
}
