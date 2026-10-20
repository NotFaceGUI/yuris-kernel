//! 拆包示例:`yscfg.ybn` / `ysc.ybn` → 可读文本(本 crate 解析器)。
//!
//! 用法:`unpack_tables <表目录>`(目录内需有 yscfg.ybn 与 ysc.ybn)
//! 输出:`<表目录>/unpack/yscfg.txt`、`<表目录>/unpack/ysc.txt`、
//!       `<表目录>/unpack/yscfg.caption.txt`(标题 CP932 原始字节)

use std::path::Path;

use yu_format::codec::Codec;
use yu_format::{yscf, yscm};

/// UTF-8 BOM(EF BB BF)——Windows 编辑器靠它认编码。
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// 写带 BOM 的 UTF-8 文本。
fn write_utf8_bom(path: &Path, text: &str) {
    let mut bytes = Vec::with_capacity(text.len() + 3);
    bytes.extend_from_slice(BOM);
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(path, bytes).unwrap();
}

fn main() {
    let dir = std::env::args().nth(1).expect("表目录");
    let read = |name: &str| -> Vec<u8> {
        std::fs::read(Path::new(&dir).join(name))
            .unwrap_or_else(|e| panic!("读 {name} 失败: {e}"))
    };
    let out = Path::new(&dir).join("unpack");
    std::fs::create_dir_all(&out).expect("建 unpack 目录");
    let origin = b"unpack_tables";

    // ---- yscfg.ybn:引擎配置 ----
    let cfg = yscf::Yscf::parse(&read("yscfg.ybn"), origin).expect("yscfg 解析");
    let mut s = String::new();
    s.push_str("yscfg.ybn · 引擎配置(解析器:yu-format yscf)\n=\n");
    s.push_str(&format!("width         = {}\n", cfg.width));
    s.push_str(&format!("height        = {}\n", cfg.height));
    s.push_str(&format!("flag_18       = {}\n", cfg.flag_18));
    s.push_str(&format!("image_ext_order    = {:?}\n", cfg.image_ext_order));
    s.push_str(&format!("sound_ext_order    = {:?}\n", cfg.sound_ext_order));
    s.push_str(&format!("flag_28       = {}\n", cfg.flag_28));
    s.push_str(&format!("switches      = {:?}\n", cfg.switches));
    s.push_str(&format!(
        "title         = CP932 {} 字节 = {:02x?}(原样见 yscfg.caption.txt)\n",
        cfg.title.len(),
        cfg.title
    ));
    write_utf8_bom(&out.join("yscfg.txt"), &s);
    std::fs::write(out.join("yscfg.caption.txt"), &cfg.title).unwrap();

    // ---- ysc.ybn:命令表(下标即 opcode)----
    let table = yscm::CommandTable::parse(&read("ysc.ybn"), origin).expect("ysc 解析");
    let mut s = String::new();
    s.push_str("ysc.ybn · 命令表(解析器:yu-format yscm;下标即 opcode)\n");
    s.push_str(&format!("=\ncommands = {}\nmessages = {}\n\n", table.commands.len(), table.messages.len()));
    s.push_str("opcode  命令名              参数(序:名 flag kind)\n");
    s.push_str("------  -------------------- ------------------------------------------------\n");
    for (i, c) in table.commands.iter().enumerate() {
        let params: Vec<String> = c
            .args
            .iter()
            .enumerate()
            .map(|(k, a)| format!("{k}:{}({:02x},{:02x})", a.name, a.flag, a.kind))
            .collect();
        s.push_str(&format!(
            "[{:3}]   {:<20} {}\n",
            i,
            c.name,
            params.join(" ")
        ));
    }
    s.push_str("\n-- 表尾 37 条消息(命令表自带;编码按 CP932 解码预览)--\n");
    for (i, m) in table.messages.iter().enumerate() {
        let text = Codec::Cp932.decode(m, origin).unwrap_or_default();
        s.push_str(&format!("[{i:2}] {text}\n"));
    }
    write_utf8_bom(&out.join("ysc.txt"), &s);

    println!(
        "unpack: yscfg({}x{} title {}B) + ysc({} 条命令 + {} 条消息) → {}",
        cfg.width,
        cfg.height,
        cfg.title.len(),
        table.commands.len(),
        table.messages.len(),
        out.display()
    );
}
