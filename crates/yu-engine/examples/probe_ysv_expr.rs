//! 取证探针:列出 `ysv.ybn` 里**串初值不是单条字面量**的记录。
//!
//! 用法:`probe_ysv_expr <project-root> [slot…]`
//!
//! 判定:把初值字节按 YSTB 表达式层解码,若指令序列不是「恰好一条 `0x4d`
//! 字面量」,就打印槽位/种类/维数/字节与解码结果 —— 这类记录需要运行核
//! 求值,而不是按字面存字节。
//!
//! 给了槽位参数时只打印这些槽位(不分类,连 `Init::None` 与空串一起列出),
//! 用于「某个变量的初值到底是什么形态」的定点核对。
use yu_engine::project::Project;
use yu_rt::expr;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let project = Project::discover(std::path::Path::new(&root), None).expect("project");
    let src = project.open_source().expect("data source");
    let bytes = src
        .open(&yu_vfs::LookupKey::new(b"ysbin/ysv.ybn", yu_vfs::Codec::Cp932).expect("key"))
        .expect("ysv.ybn");
    let decls = yu_format::ysvr::VarDecls::parse(&bytes, b"ysbin/ysv.ybn").expect("parse ysv");

    let want: Vec<u16> = std::env::args()
        .skip(2)
        .filter_map(|v| v.parse().ok())
        .collect();

    let mut literal = 0u32;
    let mut expr = 0u32;
    let mut none = 0u32;
    let mut empty = 0u32;
    for d in &decls.decls {
        if !want.is_empty() && !want.contains(&d.slot) {
            continue;
        }
        let yu_format::Init::Str(raw) = &d.init else {
            if !want.is_empty() {
                println!(
                    "slot={} kind={} group={} script={} ty={} dims={:?} init={:?}",
                    d.slot, d.kind, d.flag, d.file_id, d.ty, d.dims, d.init
                );
            }
            if matches!(d.init, yu_format::Init::None) {
                none += 1;
            }
            continue;
        };
        let instrs = expr::decode(raw).ok();
        if raw.is_empty() {
            if !want.is_empty() {
                println!(
                    "slot={} kind={} group={} script={} ty={} dims={:?} init=<empty str>",
                    d.slot, d.kind, d.flag, d.file_id, d.ty, d.dims
                );
            }
            empty += 1;
            continue;
        }
        let is_literal = matches!(&instrs, Some(v) if v.len() == 1 && v[0].op == 0x4d);
        if is_literal {
            literal += 1;
            continue;
        }
        expr += 1;
        let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        println!(
            "slot={} kind={} group={} script={} dims={:?} len={}",
            d.slot,
            d.kind,
            d.flag,
            d.file_id,
            d.dims,
            raw.len()
        );
        println!("  hex {hex}");
        match instrs {
            Some(v) => {
                for ins in &v {
                    println!("  op {:#04x} payload {:02x?}", ins.op, ins.payload);
                }
            }
            None => println!("  decode error"),
        }
    }
    println!("str literal={literal} expr={expr} empty={empty} 非串={none}");
}
