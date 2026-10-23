use yu_rt::tables::ScriptList;
fn main() {
    let want: Vec<u32> = std::env::args()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    let ysbin = std::env::var("YURIS_SAMPLE_ERIS_YSBIN").expect("set YURIS_SAMPLE_ERIS_YSBIN");
    let data = std::fs::read(format!("{ysbin}/yst_list.ybn")).unwrap();
    let list = ScriptList::from_format(
        yu_format::ystl::ScriptList::parse(&data, b"ysbin/yst_list.ybn").unwrap(),
    )
    .unwrap();
    for e in &list.entries {
        if want.is_empty() || want.contains(&e.index) {
            println!("{} {}", e.index, String::from_utf8_lossy(&e.path));
        }
    }
}
