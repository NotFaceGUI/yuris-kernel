//! Phase 0 evidence scanner (dev tool, not shipped in the library).
//!
//! 1. Lists every entry of the reference `data.ypf` through `yu-vfs::YpfSource`
//!    and filters font-like extensions (no byte search).
//! 2. Prints the `FONT` / `FONTINFO` / `TEXT` / `WINDOW` parameter tables from
//!    the compiled `ysc.ybn`.
//! 3. Scans every compiled `yst%05d.ybn` for FONT/FONTINFO/TEXT groups and
//!    decodes literal parameter values (FONT commands only exist after macro
//!    expansion, so the source scripts are not a reliable source).
//!
//! Usage:
//!   cargo run -p yu-font --example scan_evidence -- [ysbin_dir] [data.ypf]
//!
//! Env fallback: YURIS_SAMPLE_ERIS_YSBIN / YURIS_SAMPLE_ERIS_YPF.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::ystb::{self, Segments};
use yu_format::{Codec, CommandTable};

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

fn arg_or_env(arg: Option<String>, key: &str) -> Option<PathBuf> {
    arg.map(PathBuf::from).or_else(|| env_path(key))
}

/// Engine string literal payload: `[terminator][body...][terminator]`, with
/// `\\`, `\n`, `\t` escapes and CP932 lead-byte skipping (same layout as
/// `yu-rt::expr::decode_string_literal`, reimplemented here to keep the
/// scanner independent of the runtime).
fn string_literal(payload: &[u8]) -> Option<Vec<u8>> {
    let term = *payload.first()?;
    let body = payload.get(1..)?;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let b = body[i];
        if b == term {
            return Some(out);
        }
        if b == b'\\' {
            i += 1;
            match *body.get(i)? {
                b'\\' => out.push(b'\\'),
                b'n' => out.push(0x0A),
                b't' => out.push(0x09),
                other => out.push(other),
            }
            i += 1;
            continue;
        }
        out.push(b);
        // CP932 lead byte consumes the trail byte so a terminator byte inside
        // a double-byte character cannot end the literal early.
        if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
            if let Some(&next) = body.get(i + 1) {
                out.push(next);
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    None
}

/// Decode one window payload that consists of literal push instructions only.
fn literal_value(payload: &[u8]) -> Option<String> {
    let mut p = 0usize;
    let mut value: Option<String> = None;
    while p + 3 <= payload.len() {
        let op = payload[p];
        let len = u16::from_le_bytes([payload[p + 1], payload[p + 2]]) as usize;
        let end = p + 3 + len;
        if end > payload.len() {
            return None;
        }
        let body = &payload[p + 3..end];
        let text = match op {
            0x4d => {
                let bytes = string_literal(body)?;
                cp932(&bytes)
            }
            0x42 if !body.is_empty() => (body[0] as i8).to_string(),
            0x49 if body.len() >= 4 => i32::from_le_bytes(body[..4].try_into().ok()?).to_string(),
            0x57 if body.len() >= 2 => i16::from_le_bytes(body[..2].try_into().ok()?).to_string(),
            0x4c if body.len() >= 8 => i64::from_le_bytes(body[..8].try_into().ok()?).to_string(),
            0x46 if body.len() >= 8 => f64::from_le_bytes(body[..8].try_into().ok()?).to_string(),
            _ => return None,
        };
        value = Some(text);
        p = end;
    }
    if p == payload.len() {
        value
    } else {
        None
    }
}

fn cp932(bytes: &[u8]) -> String {
    // Small local decoder: the crate under evidence must not depend on yu-format
    // in production, but this dev example already does; reuse its codec.
    Codec::Cp932.decode(bytes, b"scan").unwrap_or_default()
}

fn scan_ypf(path: &Path) {
    println!("## data.ypf: {}", path.display());
    match yu_vfs::YpfSource::open(path, yu_vfs::Codec::Cp932) {
        Ok(src) => {
            let stats = src.stats();
            println!(
                "entries={} index_start={} name_key=0xFF name_hash_ok={} types={:?} compressed={}",
                stats.parsed_entries,
                stats.index_start,
                stats.name_hash_matches,
                stats.type_counts,
                stats.compressed_entries
            );
            let mut font_like = 0usize;
            let mut ext_hist: BTreeMap<String, usize> = BTreeMap::new();
            for e in src.entries() {
                let name = cp932(&e.name);
                let ext = name
                    .rsplit_once('.')
                    .map(|(_, x)| x.to_ascii_lowercase())
                    .unwrap_or_default();
                *ext_hist.entry(ext.clone()).or_default() += 1;
                if matches!(
                    ext.as_str(),
                    "ttf" | "otf" | "ttc" | "fnt" | "psb" | "fon" | "woff" | "woff2" | "dfont"
                ) {
                    font_like += 1;
                    println!(
                        "FONT-LIKE type={} compressed={} usize={} csize={} name={}",
                        e.file_type, e.compressed, e.uncompressed_size, e.compressed_size, name
                    );
                }
            }
            println!("font_like_entries={font_like}");
            println!("extension histogram (top 25):");
            let mut hist: Vec<_> = ext_hist.into_iter().collect();
            hist.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (ext, n) in hist.into_iter().take(25) {
                println!("  .{ext:<8} {n}");
            }
        }
        Err(d) => println!("YPF OPEN FAILED: {}", d),
    }
}

fn scan_command_table(table: &CommandTable) {
    println!("\n## ysc command table: {} commands", table.commands.len());
    for want in ["FONT", "FONTINFO", "TEXT", "TEXTINFO", "WINDOW"] {
        let Some((op, cmd)) = table
            .commands
            .iter()
            .enumerate()
            .find(|(_, c)| c.name.as_ref() == want)
        else {
            println!("{want}: NOT PRESENT");
            continue;
        };
        let args: Vec<String> = cmd
            .args
            .iter()
            .enumerate()
            .map(|(i, a)| format!("{i}:{}[flag={:#04x} kind={:#04x}]", a.name, a.flag, a.kind))
            .collect();
        println!("{want} opcode={op} argc={} {}", cmd.argc, args.join(" "));
    }
}

fn scan_scripts(dir: &Path) {
    println!("\n## compiled scripts: {}", dir.display());
    let read = |name: &str| std::fs::read(dir.join(name));
    let ysc_bytes = match read("ysc.ybn") {
        Ok(b) => b,
        Err(e) => {
            println!("cannot read ysc.ybn: {e}");
            return;
        }
    };
    let ysc = match CommandTable::parse(&ysc_bytes, b"ysc") {
        Ok(t) => t,
        Err(d) => {
            println!("ysc parse failed: {d}");
            return;
        }
    };
    scan_command_table(&ysc);

    // Source-name map (compiled scripts lose their source path otherwise).
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    if let Ok(bytes) = read("yst_list.ybn") {
        if let Ok(list) = yu_format::ystl::ScriptList::parse(&bytes, b"yst_list") {
            for e in &list.entries {
                names.insert(e.index, cp932(&e.name));
            }
        }
    }

    let mut font_groups = 0usize;
    let mut files_scanned = 0usize;
    let mut per_command: BTreeMap<String, usize> = BTreeMap::new();
    for id in 0u32..1000 {
        let name = format!("yst{id:05}.ybn");
        let Ok(bytes) = read(&name) else { continue };
        let segs = match Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, name.as_bytes()) {
            Ok(s) => s,
            Err(d) => {
                println!("{name}: decrypt failed: {d}");
                continue;
            }
        };
        let script = match ystb::decode(&segs, id, &ysc, Codec::Cp932, name.as_bytes()) {
            Ok(s) => s,
            Err(d) => {
                println!("{name}: decode failed: {d}");
                continue;
            }
        };
        files_scanned += 1;
        for (gi, g) in script.groups.iter().enumerate() {
            let cmd = g.command.as_ref();
            if !matches!(cmd, "FONT" | "FONTINFO" | "TEXT" | "TEXTINFO" | "WINDOW") {
                continue;
            }
            *per_command.entry(cmd.to_string()).or_default() += 1;
            if cmd != "FONT" {
                continue;
            }
            font_groups += 1;
            let def = ysc
                .commands
                .iter()
                .find(|c| c.name.as_ref() == "FONT")
                .map(|c| c.args.as_slice())
                .unwrap_or(&[]);
            println!(
                "FONT {name} src={} group={gi} windows={}",
                names.get(&id).map(String::as_str).unwrap_or("?"),
                g.windows.len()
            );
            for w in &g.windows {
                // WindowRec: +0 kind = parameter index (0-based), +2 slot is
                // the runtime binding slot (yu-rt tables.rs §layout).
                let slot = u16::from_le_bytes([w.bytes[2], w.bytes[3]]) as usize;
                let param = w.bytes[0] as usize;
                let length = u32::from_le_bytes([w.bytes[4], w.bytes[5], w.bytes[6], w.bytes[7]]);
                let offset = u32::from_le_bytes([w.bytes[8], w.bytes[9], w.bytes[10], w.bytes[11]]);
                let start = offset as usize;
                let end = start.checked_add(length as usize);
                let payload = end.and_then(|e| segs.args_data.get(start..e));
                let pname = def
                    .get(param)
                    .map(|a| a.name.to_string())
                    .unwrap_or_else(|| format!("?param{param}"));
                match payload.map(literal_value) {
                    Some(Some(v)) => {
                        println!("  param={param:<2} {pname:<12} = {v}  (slot={slot})")
                    }
                    Some(None) => {
                        println!(
                            "  param={param:<2} {pname:<12} = <non-literal expr>  (slot={slot})"
                        )
                    }
                    None => println!(
                        "  param={param:<2} {pname:<12} = <payload out of range>  (slot={slot})"
                    ),
                }
            }
        }
    }
    println!(
        "\nsummary: files={files_scanned} font_groups={font_groups} groups_by_command={per_command:?}"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ysbin = arg_or_env(args.first().cloned(), "YURIS_SAMPLE_ERIS_YSBIN");
    let ypf = arg_or_env(args.get(1).cloned(), "YURIS_SAMPLE_ERIS_YPF");
    if let Some(p) = ypf {
        scan_ypf(&p);
    } else {
        println!("no YPF path (arg 2 / YURIS_SAMPLE_ERIS_YPF)");
    }
    if let Some(d) = ysbin {
        scan_scripts(&d);
    } else {
        println!("no ysbin dir (arg 1 / YURIS_SAMPLE_ERIS_YSBIN)");
    }
}
