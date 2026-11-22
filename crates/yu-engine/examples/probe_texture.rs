//! 取证探针:用与引擎相同的 TextureStore probe_order 解析资产 key。
//!
//! 用法:`probe_texture <project-root> <key>...`

use std::path::Path;
use yu_engine::project::Project;
use yu_gfx::TextureRef;
use yu_render_core::TextureStore;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let keys: Vec<String> = std::env::args().skip(2).collect();
    let project = Project::discover(Path::new(&root), None).expect("project");
    let order = yu_gfx::probe_order_from_codes(&[1, 2, 3, 4, 5, 6, 0, 0]);
    eprintln!("probe_order={order:?}");
    let mut store = TextureStore::new(
        Box::new(project.mount().expect("mount")),
        yu_engine::project::CODEC,
        order,
    );
    for key in keys {
        match store.resolve_ref(&TextureRef::Asset(key.as_bytes().to_vec())) {
            Ok(t) => println!(
                "OK  {key}: {}x{} {} bytes",
                t.width,
                t.height,
                t.rgba.len()
            ),
            Err(d) => println!("ERR {key}: {d:?}"),
        }
    }
}
