//! 从 ypf 里按名字导出单个文件的原始字节(取证用)。
fn main() {
    let ypf = std::env::args().nth(1).expect("ypf");
    let want = std::env::args().nth(2).expect("entry name");
    let out = std::env::args().nth(3).expect("out path");
    let src = yu_vfs::YpfSource::open(std::path::Path::new(&ypf), yu_vfs::Codec::Cp932).expect("open");
    let key = yu_vfs::LookupKey::new(want.as_bytes(), yu_vfs::Codec::Cp932).expect("key");
    let bytes = yu_vfs::DataSource::open(&src, &key).expect("entry");
    std::fs::write(&out, &bytes).expect("write");
    println!("{} bytes -> {}", bytes.len(), out);
}
