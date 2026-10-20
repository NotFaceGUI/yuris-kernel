//! 全量重审 v2:族谱前缀归并后的手册未记载项(字节级)。
fn main() {
    let sdk495 = std::env::var("YURIS_SAMPLE_SDK0495").expect("set YURIS_SAMPLE_SDK0495");
    let sdk479 = std::env::var("YURIS_SAMPLE_SDK0479").expect("set YURIS_SAMPLE_SDK0479");
    let dirs = [
        format!("{sdk495}/マニュアル/YU-RIS/html"),
        format!("{sdk479}/マニュアル/YU-RIS/html"),
    ];
    let mut blobs = Vec::new();
    for dir in &dirs {
        for e in walk(dir) {
            if let Ok(b) = std::fs::read(&e) {
                blobs.push(b);
            }
        }
    }
    // 手册族谱记法(逐族在 svariable.html 明文):
    let families = ["_KEYNUM_", "_KEY_", "_MOUSENUM_", "_MOUSE_", "_CODE_", "_PADNUM_", "_PAD_", "_SERIAL_"];
    let abi = yu_rt::sysvar_abi::SysVarTable::builtin_abi();
    let mut miss = Vec::new();
    for d in &abi.decls {
        let name = d.name.as_str();
        if families.iter().any(|f| name.starts_with(f)) {
            continue; // 族谱已明文(键/鼠标/码/手柄/序列号族)
        }
        let needle = name.trim_start_matches('_').as_bytes();
        let hit = blobs.iter().any(|b| b.windows(needle.len()).any(|w| w == needle));
        if !hit {
            miss.push(name);
        }
    }
    println!("非族谱且手册未命中 {} 项:", miss.len());
    for n in &miss {
        println!("  {n}");
    }
}

fn walk(dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(p.to_str().unwrap()));
            } else if p.extension().map(|x| x == "html").unwrap_or(false) {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    out
}
