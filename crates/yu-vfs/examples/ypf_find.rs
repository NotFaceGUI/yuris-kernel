fn main() {
    let ypf = std::env::args().nth(1).expect("ypf");
    let pat = std::env::args().nth(2).expect("pattern");
    let src = yu_vfs::YpfSource::open(std::path::Path::new(&ypf), yu_vfs::Codec::Cp932).expect("open");
    let mut n = 0;
    for e in src.entries() {
        let name = String::from_utf8_lossy(&e.name).to_string();
        if name.to_ascii_lowercase().contains(&pat.to_ascii_lowercase()) {
            println!("{} ({}B, type={}, comp={})", name, e.uncompressed_size, e.file_type, e.compressed);
            n += 1;
            if n > 25 { break; }
        }
    }
    println!("matches: {n}");
}
