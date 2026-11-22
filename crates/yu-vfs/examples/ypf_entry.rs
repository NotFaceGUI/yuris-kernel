//! 临时取证:打印 YPF 条目的 offset/size/hash/profile。
fn main() {
    let path = std::env::args().nth(1).expect("ypf path");
    let needle = std::env::args().nth(2).unwrap_or_default().to_ascii_lowercase();
    let src = yu_vfs::YpfSource::open(std::path::Path::new(&path), yu_vfs::Codec::Cp932)
        .expect("open ypf");
    println!("profile: {}", src.profile().describe());
    println!("entries: {}", src.entries().len());
    for e in src.entries() {
        let name = String::from_utf8_lossy(&e.name).replace('\\', "/");
        if needle.is_empty() || name.to_ascii_lowercase().contains(&needle) {
            println!(
                "{name} flag/file_type={} compressed={} usize={} csize={} off={:#x} ph={:#010x} alg={} name_ok={}",
                e.file_type,
                e.compressed,
                e.uncompressed_size,
                e.compressed_size,
                e.data_offset,
                e.payload_hash,
                e.data_hash_alg.as_str(),
                e.name_hash_ok
            );
        }
    }
}
