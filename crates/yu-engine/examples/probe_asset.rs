//! 取证探针:按工程数据入口逐个查资产是否存在(封包/散盘同一口径)。
//!
//! 用法:`cargo run -p yu-engine --example probe_asset -- <project-root> <path>...`
use yu_engine::project::Project;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let src = project.open_source().expect("data source");
    for path in std::env::args().skip(2) {
        let key = match yu_vfs::LookupKey::new(path.as_bytes(), yu_vfs::Codec::Cp932) {
            Ok(k) => k,
            Err(d) => {
                println!("{path:<44} key-error {d:?}");
                continue;
            }
        };
        match src.open(&key) {
            Ok(bytes) => println!("{path:<44} HIT {} bytes", bytes.len()),
            Err(_) => println!("{path:<44} MISS"),
        }
    }
}
