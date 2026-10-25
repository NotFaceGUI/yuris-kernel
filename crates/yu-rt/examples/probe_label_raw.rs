//! 取证探针:按精确名字打印 ysl 里的全部条目(pc/组号/索引),用于核对
//! 「同一名字是否命中多条目、标签表 pc 字段是否被误读」。
//!
//! 用法:`cargo run -p yu-rt --example probe_label_raw -- <game_root> NAME...`
use yu_rt::tables::LabelTable;
use yu_vfs::{Codec, DataSource, LookupKey, YpfSource};

fn main() {
    let root = std::env::args().nth(1).expect("game root");
    let names: Vec<String> = std::env::args().skip(2).collect();
    let pack = YpfSource::open(format!("{root}/data.ypf"), Codec::Cp932).expect("data.ypf");
    let key = LookupKey::new(b"ysbin/ysl.ybn", Codec::Cp932).expect("key");
    let bytes = pack.open(&key).expect("ysl");
    let fmt = yu_format::yslb::YslLabels::parse(&bytes, b"ysbin/ysl.ybn").expect("parse ysl");
    let table = LabelTable::from_format(fmt).expect("labels");
    println!("ysl entries = {}", table.iter().count());
    for e in table.iter() {
        let name = &e.name;
        if names.is_empty() || names.iter().any(|n| n == name) {
            println!(
                "  {name:<28} script={} pc={} base_a={} hash={:08x}",
                e.script_id, e.pc, e.base_a, e.hash
            );
        }
    }
}
