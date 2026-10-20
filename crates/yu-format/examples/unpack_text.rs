//! 文本提取示例(RxYuris `YSTB_TextEditor_V5` 同法,纯本 crate):
//! 1. `ysc` 命令表解出后按**命令名**找 `WORD`(零硬编码);
//! 2. 逐个 `ystNNNNN.ybn`:`Segments::decrypt`(默认键)→ `decode` 得到
//!    带命令名的组;`WORD` 组的首窗口载荷即文本段(原始 CP932);
//! 3. 经 [`Codec::Cp932`] 解码输出 **UTF-8** 文本(条目间空行,RxYuris
//!    格式),每脚本一个 `.txt`,另写 `_index.txt` 汇总。
//!
//! 内置交叉核对(2026-10-20 实证):`yst_list.text_count` = **RETURNCODE 数**
//! (= 文本行数;行内改行宏会把一行拆成多个 WORD 段),不是 WORD 段数。示例按此
//! 口径核对:RETURNCODE 数 ↔ `text_count`(两条独立解析路径互证)。
//!
//! 用法:`unpack_text <数据目录> <输出目录>`
//! (数据目录 = 含 ysc.ybn / yst_list.ybn / ystNNNNN.ybn 的 ysbin)

use std::path::Path;

use yu_format::codec::Codec;
use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::{ystb, yscm, ystl};

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
    let dir = std::env::args().nth(1).expect("数据目录");
    let out = std::env::args().nth(2).expect("输出目录");
    let dir_path = Path::new(&dir);
    let out_path = Path::new(&out);
    std::fs::create_dir_all(out_path).expect("建输出目录");
    let origin = b"unpack_text";

    let read = |name: &str| -> Vec<u8> {
        std::fs::read(dir_path.join(name)).unwrap_or_else(|e| panic!("读 {name} 失败: {e}"))
    };

    let table = yscm::CommandTable::parse(&read("ysc.ybn"), origin).expect("ysc 解析");
    let list = ystl::ScriptList::parse(&read("yst_list.ybn"), origin).expect("yst_list 解析");

    let mut entries: Vec<_> = std::fs::read_dir(dir_path)
        .expect("列目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.len() == 12
                && n.starts_with("yst")
                && n.ends_with(".ybn")
                && n[3..8].bytes().all(|b| b.is_ascii_digit())
        })
        .collect();
    entries.sort();

    let mut index = String::new();
    index.push_str("script  path(yst_list)\tWORD 段数\tRETURNCODE 行数\t清单 text_count\n");
    let (mut total, mut scripts, mut mismatch) = (0usize, 0usize, 0usize);

    for name in &entries {
        let sid: u32 = name[3..8].parse().unwrap();
        let data = std::fs::read(dir_path.join(name)).unwrap();
        let segs = ystb::Segments::decrypt(&data, &DEFAULT_KEY_BYTES, origin)
            .unwrap_or_else(|e| panic!("{name} 解密失败: {e}"));
        let script = ystb::decode(&segs, sid, &table, Codec::Cp932, origin)
            .unwrap_or_else(|e| panic!("{name} 解码失败: {e}"));

        // WORD 组的首窗口 = 文本段(原始 CP932);载荷 = 参数数据区切片。
        let mut body = String::new();
        let (mut count, mut retcode) = (0usize, 0usize);
        for g in &script.groups {
            if g.command.as_ref() == "RETURNCODE" {
                retcode += 1;
            }
            if g.command.as_ref() != "WORD" {
                continue;
            }
            let Some(w) = g.windows.first() else { continue };
            let len = u32::from_le_bytes(w.bytes[4..8].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(w.bytes[8..12].try_into().unwrap()) as usize;
            if len == 0 {
                continue;
            }
            let Some(text_bytes) = segs.args_data.get(off..off + len) else {
                panic!("{name}: WORD 载荷 {off}+{len} 越出参数数据区 {} 字节", segs.args_data.len());
            };
            let text = Codec::Cp932.decode(text_bytes, origin).unwrap_or_default();
            body.push_str(&text);
            body.push_str("\r\n\r\n");
            count += 1;
        }

        let entry = list.entries.iter().find(|e| e.index == sid);
        let rel = entry
            .map(|e| Codec::Cp932.decode(&e.name, origin).unwrap_or_default())
            .unwrap_or_else(|| "?".into());
        let listed = entry.map(|e| e.text_count as usize).unwrap_or(0);
        if listed != retcode {
            mismatch += 1;
        }
        index.push_str(&format!(
            "{sid:>6}  {rel}\tWORD={count}\tRETURNCODE={retcode}\ttext_count={listed}\n"
        ));
        total += count;
        scripts += 1;
        // UTF-8 BOM:Windows 编辑器(记事本/VS Code/Notepad++)靠它认编码,
        // 否则中文系统会按 ANSI(GBK)或 SJIS 猜成乱码。
        write_utf8_bom(&out_path.join(name).with_extension("txt"), &body);
    }
    write_utf8_bom(&out_path.join("_index.txt"), &index);
    println!(
        "unpack(text): scripts={scripts} WORD={total} 行数↔text_count 不一致 {mismatch} 个 → {}/(UTF-8)",
        out_path.display()
    );
}
