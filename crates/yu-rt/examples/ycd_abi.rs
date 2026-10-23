//! 排查示例:用内核自己的 YcdTable 解析任意版本的 YSCom.ycd,
//! 打印版本/命令数/系统变量数/关键 id,验证老版本布局兼容性。
use yu_rt::ycd::YcdTable;

fn main() {
    for p in std::env::args().skip(1) {
        match std::fs::read(&p)
            .map_err(|e| e.to_string())
            .and_then(|b| YcdTable::parse(&b).map_err(|d| d.detail))
        {
            Ok(t) => {
                let ids: Vec<String> = [
                    "_TIME",
                    "_LC",
                    "_PINT",
                    "_SCREEN_SIZE_X",
                    "_KEY_A",
                    "_KEY_ANY",
                ]
                .iter()
                .map(|n| format!("{n}={:?}", t.sys_var_id(n)))
                .collect();
                println!(
                    "{}: v{} 命令段 {} 条,系统变量 {} 条; {}",
                    p,
                    t.version,
                    t.commands.len(),
                    t.system_variables.len(),
                    ids.join(" ")
                );
            }
            Err(e) => println!("{p}: 解析失败: {e}"),
        }
    }
}
