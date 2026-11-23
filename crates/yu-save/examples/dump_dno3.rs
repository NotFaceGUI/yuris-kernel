use std::path::Path;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let bytes = std::fs::read(path).expect("read");
        let save = yu_format::yssd::YssdSave::parse(&bytes, b"dump").expect("parse");
        let entry = save.entry(3).expect("dno3");
        let raw = entry.decompress(b"dno3").expect("decompress");
        assert!(raw.len() >= 8, "DNO=3 载荷过短: {}", raw.len());
        let a_len = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize;
        let b_len = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
        println!(
            "{}\n  btype={} strict={} var={} clen={} ulen={} raw={} A={} B={}",
            arg,
            entry.btype,
            entry.strict,
            entry.var_id,
            entry.compressed.len(),
            entry.ulen,
            raw.len(),
            a_len,
            b_len
        );
        assert!(8 + a_len <= raw.len(), "A 段越界: 8+{a_len} > {}", raw.len());
        let a = &raw[8..8 + a_len];
        for off in (0..a.len()).step_by(4) {
            let v = u32::from_le_bytes(a[off..off + 4].try_into().unwrap());
            if v != 0 {
                println!("  {off:04x}: {v:08x}");
            }
        }
    }
}
