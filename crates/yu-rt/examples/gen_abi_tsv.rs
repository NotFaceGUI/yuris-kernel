//! 从一组 `YSCom.ycd` 生成内核 ABI TSV 表。
//!
//! 用法:`gen_abi_tsv <out-dir> <ycd>...`
//!
//! 输出:
//! - `<out>/sysvar-abi-<ver>-<sha8>.tsv`:该 ycd 的系统变量段
//! - 标准输出:每个文件一行摘要(版本/条数/sha8),供人工核对

use std::path::Path;

use yu_rt::sysvar_abi::sha256_hex;
use yu_rt::ycd::YcdTable;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = args.next().expect("out dir");
    let out_dir = Path::new(&out_dir);
    std::fs::create_dir_all(out_dir).expect("create out dir");

    let mut rows: Vec<(u32, String, String, usize)> = Vec::new();
    for path in args {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: 读失败 {e}");
                continue;
            }
        };
        let sha = sha256_hex(&bytes);
        let table = match YcdTable::parse(&bytes) {
            Ok(t) => t,
            Err(d) => {
                eprintln!("{path}: 解析失败 {}", d.detail);
                continue;
            }
        };
        let mut text = String::new();
        text.push_str("# sysvar ABI table (engine ABI, not game data)\n");
        text.push_str(&format!("# source_ycd_version\t{}\n", table.version));
        text.push_str(&format!("# source_ycd_sha256\t{sha}\n"));
        text.push_str(&format!("# count\t{}\n", table.system_variables.len()));
        text.push_str("# columns: id<TAB>name<TAB>value_type(1=int,2=flt,3=str)<TAB>dims(comma)\n");
        for (id, d) in table.system_variables.iter().enumerate() {
            let dims = d
                .dims
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            text.push_str(&format!("{id}\t{}\t{}\t{dims}\n", d.name, d.value_type));
        }
        let name = format!("sysvar-abi-{:04}-{}.tsv", table.version, &sha[..8]);
        std::fs::write(out_dir.join(&name), &text).expect("write tsv");
        println!(
            "{name}\tversion={}\tvars={}\tcommands={}",
            table.version,
            table.system_variables.len(),
            table.commands.len()
        );
        rows.push((table.version, sha, path, table.system_variables.len()));
    }
    rows.sort();
    let mut index = String::from("# version\tsha256\tvars\tsource\n");
    for (ver, sha, path, vars) in rows {
        index.push_str(&format!("{ver}\t{sha}\t{vars}\t{path}\n"));
    }
    std::fs::write(out_dir.join("INDEX.tsv"), index).expect("write index");
}
