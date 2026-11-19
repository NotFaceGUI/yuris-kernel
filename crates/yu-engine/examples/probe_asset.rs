//! 临时探针:工程 VFS 资产查找(按渲染器候选扩展名)。
//! 用法:`probe_asset <project-root> <key> [key...]`
use std::path::Path;
use yu_engine::project::Project;
use yu_vfs::{Codec, LookupKey};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("project root");
    let project = Project::discover(Path::new(&root), None).expect("project");
    let src = project.open_source().expect("source");
    let aux = project.open_aux().expect("aux");
    for key in args {
        let mut found = false;
        for ext in ["", ".png", ".bmp", ".jpg", ".gif", ".webp", ".psb"] {
            let mut name = key.clone().into_bytes();
            name.extend_from_slice(ext.as_bytes());
            let Ok(lookup) = LookupKey::new(&name, Codec::Cp932) else {
                continue;
            };
            if let Ok(bytes) = src.open(&lookup) {
                println!("FOUND {key}{ext} {} B (src)", bytes.len());
                found = true;
                break;
            }
            if let Ok(bytes) = aux.open(&lookup) {
                println!("FOUND {key}{ext} {} B (aux)", bytes.len());
                found = true;
                break;
            }
        }
        if !found {
            println!("MISS  {key}");
        }
    }
}
