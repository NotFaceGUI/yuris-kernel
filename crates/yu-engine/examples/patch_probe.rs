//! 只读探针:打印文本编码路线解析结果。
//!
//! 用法:`patch_probe <project-root> [auto|original|gbk|tunnel]`

use std::path::Path;

use yu_engine::patch::{PatchKind, PatchSelection};
use yu_engine::project::Project;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let kind = match std::env::args().nth(2) {
        Some(value) => PatchKind::parse(&value).expect("patch kind"),
        None => PatchKind::Auto,
    };
    let project = Project::discover(Path::new(&root), None).expect("project");
    let profile = PatchSelection::new(kind).profile(&project).expect("profile");
    println!("{}", profile.summary());
    println!("evidence={:?}", profile.evidence);
}
