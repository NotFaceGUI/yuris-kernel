use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("save path");
    let dnos: Vec<u32> = std::env::args()
        .skip(2)
        .map(|s| s.parse().expect("dno"))
        .collect();
    let path = Path::new(&path);
    let root = path.parent().expect("parent").to_path_buf();
    let file = path.file_name().expect("file").to_string_lossy().into_owned();
    let mut state = yu_save::SaveState::new(root, false);
    for dno in dnos {
        match state.load_block(&file, dno) {
            Ok(block) => {
                println!("DNO {dno}: {} vars", block.vars.len());
                for var in block.vars.iter().take(8) {
                    println!(
                        "  var={:?} ty={:?} dims={:?} data={:?}",
                        var.var,
                        var.snap.ty,
                        var.snap.dims,
                        var.snap.data.iter().take(16).collect::<Vec<_>>()
                    );
                }
            }
            Err(e) => println!("DNO {dno}: ERR {} {}", e.code, e.detail),
        }
    }
}
