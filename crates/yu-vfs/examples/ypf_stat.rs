//! 统计 ypf 里资源的压缩/大小分布(临时探针)。
fn main() {
    let path = std::env::args().nth(1).expect("ypf path");
    let src = yu_vfs::YpfSource::open(std::path::Path::new(&path), yu_vfs::Codec::Cp932).expect("open ypf");
    let entries = src.entries();
    println!("entries={}", entries.len());
    let (mut comp, mut uncomp, mut comp_bytes, mut uncomp_bytes) = (0usize, 0usize, 0u64, 0u64);
    let (mut img, mut img_uncomp, mut img_bytes) = (0usize, 0usize, 0u64);
    for e in entries {
        if e.compressed { comp += 1; comp_bytes += e.compressed_size as u64; }
        else { uncomp += 1; uncomp_bytes += e.compressed_size as u64; }
        if e.file_type == 2 || e.file_type == 3 {
            img += 1;
            if !e.compressed { img_uncomp += 1; }
            img_bytes += e.uncompressed_size as u64;
        }
    }
    println!("compressed={comp} ({comp_bytes} B)  stored={uncomp} ({uncomp_bytes} B)");
    println!("images(type2/3)={img} 未压缩={img_uncomp} 解压后总={img_bytes} B 平均={}",
        if img > 0 { img_bytes / img as u64 } else { 0 });
}
