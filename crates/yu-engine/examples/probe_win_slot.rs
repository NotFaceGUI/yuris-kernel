//! 取证探针:打印指定脚本若干 pc 的窗口记录原始字段(kind/flag/slot)。
//!
//! 用法:`probe_win_slot <project-root> <sid> <pc> [<pc>...]`
//! 只走 TableSet 解码;`slot>>8` 是 LET/DIMSIZE 复合赋值码(0.495 语料观测)。

use std::path::Path;

use yu_engine::project::Project;
use yu_rt::tables::TableSet;

fn main() {
    let root = std::env::args().nth(1).expect("project root");
    let sid: u32 = std::env::args().nth(2).expect("sid").parse().expect("sid");
    let mut pcs: Vec<usize> = Vec::new();
    for a in std::env::args().skip(3) {
        let a = a.trim_start_matches("pc");
        if let Ok(n) = a.parse::<usize>() {
            pcs.push(n);
        } else if let Some((s, e)) = a.split_once_once() {
            pcs.extend(s.parse::<usize>().unwrap()..=e.parse::<usize>().unwrap());
        }
    }
    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let mut tables = TableSet::load_from(src.as_ref(), None, key.bytes).expect("tables");
    let script = tables.load_script(src.as_ref(), sid).expect("script").clone();
    for pc in pcs {
        let Some(g) = script.group(pc) else { continue };
        let Some(cmd) = tables.boot.yscm.command(g.opcode) else { continue };
        println!("pc{pc} {}", cmd.name);
        for w in script.group_windows(pc) {
            let pname = cmd
                .params
                .get(w.kind as usize)
                .map(|p| p.name.as_str())
                .unwrap_or("-");
            println!(
                "  kind={}({pname}) flag={:#04x} slot={:#06x} (hi={} lo={}) len={} off={}",
                w.kind,
                w.flag,
                w.slot,
                w.slot >> 8,
                w.slot & 0xff,
                w.length,
                w.offset
            );
        }
    }
}

trait SplitOnce {
    fn split_once_once(&self) -> Option<(&str, &str)>;
}

impl SplitOnce for str {
    fn split_once_once(&self) -> Option<(&str, &str)> {
        let i = self.find('-')?;
        Some((&self[..i], &self[i + 1..]))
    }
}
