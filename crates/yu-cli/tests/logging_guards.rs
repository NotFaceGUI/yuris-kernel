//! Q1 R5 · 日志/静默三条守卫(进提交前手跑清单,与 `case --all` 同级):
//!
//! 1. `src/` 零终端宏:任何 crate 的 `src/` 不许直接 `println!/print!/
//!    `eprintln!/dbg!`(K2 §0.1 既有纪律;终端输出只经 `yu_engine::output`
//!    或 `yu_log::console`);
//! 2. stdout 无 ANSI:ANSI 转义字面量只允许出现在 `yu-log/src/console.rs`
//!    (唯一着色点;`output::line/raw` 这两个 stdout 出口无色无前缀,机器
//!    可读产品输出永不经日志通道);
//! 3. `silent_fallback_audit`(todo-silent-handling S3 的扫描测试):
//!    `yu-rt`/`yu-gfx` 生产代码的 `unwrap_or/​_default` 必须逐处标注
//!    (「缺省合法(出处)」或「类型不符,应挂号」),未标注数 = 本文件
//!    白名单快照 —— 只许减少不许新增;新增一处即测试红,逼着当场分类。
//!
//! 白名单按「文件 → 未标注数」锁计数(行号会漂移,计数不会);S3 排期里
//! 的逐条分类(合法缺省 vs 类型不符)把数字清零后删对应白名单项即可。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-cli -> workspace root")
        .to_path_buf()
}

/// 收集 `crates/*/src` 下全部 .rs(排除 examples/ 与 tests/ 目录)。
fn src_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let crates = workspace().join("crates");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&crates)
        .expect("crates/ 可读")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let src = dir.join("src");
        if !src.is_dir() {
            continue;
        }
        for entry in walk_rs(&src) {
            let rel = entry.strip_prefix(&src).unwrap();
            if rel.starts_with("examples") || rel.starts_with("tests") {
                continue;
            }
            out.push(entry);
        }
    }
    out
}

fn walk_rs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir).expect("目录可读").flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                out.push(path);
            }
        }
    }
    out
}

/// 去掉行注释与块注释文本,避免文档/说明里的示例字样触发守卫。
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_line = false;
    let mut in_block = false;
    let mut in_string = false;
    let mut prev_escape = false;
    while let Some(c) = chars.next() {
        if in_line {
            if c == '\n' {
                in_line = false;
                out.push(c);
            }
            continue;
        }
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if in_string {
            out.push(c);
            if prev_escape {
                prev_escape = false;
            } else if c == '\\' {
                prev_escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block = true;
            }
            _ => out.push(c),
        }
    }
    out
}

#[test]
fn src_has_no_direct_terminal_macros() {
    let mut offenders = Vec::new();
    for path in src_files() {
        let code = strip_comments(&std::fs::read_to_string(&path).expect("源码可读"));
        for (needle, name) in [
            ("println!(", "println!"),
            ("print!(", "print!"),
            ("eprintln!(", "eprintln!"),
            ("eprint!(", "eprint!"),
            ("dbg!(", "dbg!"),
        ] {
            if code.contains(needle) {
                offenders.push(format!("{} 含 {}", path.display(), name));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "src/ 出现直写终端宏(纪律 K2 §0.1 / AGENTS §4):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn ansi_escapes_only_in_console_module() {
    let allowlist_prefix = "crates/yu-log/src/console.rs";
    let mut offenders = Vec::new();
    for path in src_files() {
        let display = path
            .strip_prefix(workspace())
            .expect("in workspace")
            .to_string_lossy()
            .replace('\\', "/");
        if display == allowlist_prefix {
            continue;
        }
        let code = strip_comments(&std::fs::read_to_string(&path).expect("源码可读"));
        if code.contains("\\x1b") || code.contains("\\u{1b}") || code.contains('\u{1b}') {
            offenders.push(display);
        }
    }
    assert!(
        offenders.is_empty(),
        "ANSI 转义只允许出现在 {allowlist_prefix}(stdout 出口必须无色):\n{}",
        offenders.join("\n")
    );
}

/// S3 白名单:文件 → 未标注 `unwrap_or` 数(2026-10-25 快照;只许减少)。
fn silent_fallback_whitelist() -> BTreeMap<&'static str, usize> {
    BTreeMap::from([
        ("crates/yu-rt/src/expr.rs", 1),
        ("crates/yu-rt/src/input.rs", 9),
        ("crates/yu-rt/src/sysvar.rs", 15),
        ("crates/yu-rt/src/sysvar_abi.rs", 2),
        ("crates/yu-rt/src/task.rs", 3),
        ("crates/yu-rt/src/value.rs", 3),
        ("crates/yu-rt/src/vm.rs", 4),
        ("crates/yu-rt/src/ycd.rs", 1),
        ("crates/yu-rt/src/vm/handlers.rs", 101),
        ("crates/yu-gfx/src/op.rs", 2),
        ("crates/yu-gfx/src/state.rs", 18),
    ])
}

#[test]
fn silent_fallback_audit() {
    let marker = |line: &str| line.contains("// 缺省") || line.contains("// silent-ok") || line.contains("// 良性");
    let call = |code: &str| code.contains(".unwrap_or(") || code.contains(".unwrap_or_default(");
    let mut actual: BTreeMap<String, usize> = BTreeMap::new();
    for path in src_files() {
        let rel = path
            .strip_prefix(workspace())
            .expect("in workspace")
            .to_string_lossy()
            .replace('\\', "/");
        if !(rel.starts_with("crates/yu-rt/src/") || rel.starts_with("crates/yu-gfx/src/")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("源码可读");
        let lines: Vec<&str> = text.lines().collect();
        let mut unann = 0usize;
        for (i, line) in lines.iter().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            if call(code) {
                let prev = if i > 0 { lines[i - 1] } else { "" };
                if !(marker(line) || marker(prev)) {
                    unann += 1;
                }
            }
        }
        if unann > 0 {
            actual.insert(rel, unann);
        }
    }
    let whitelist = silent_fallback_whitelist();
    let mut problems = Vec::new();
    for (file, count) in &actual {
        match whitelist.get(file.as_str()) {
            Some(allowed) if allowed >= count => {}
            Some(allowed) => {
                problems.push(format!("{file}: 未标注 {count} > 白名单 {allowed}(新增未分类 unwrap_or)"));
            }
            None => {
                problems.push(format!("{file}: 白名单无此项但有 {count} 处未标注 unwrap_or"));
            }
        }
    }
    for (file, allowed) in &whitelist {
        let had = actual.get(*file).copied().unwrap_or(0);
        if had < *allowed {
            problems.push(format!(
                "{file}: 未标注 {had} < 白名单 {allowed}(S3 分类推进了?请把白名单同步下调)"
            ));
        }
    }
    assert!(problems.is_empty(), "silent_fallback_audit:\n{}", problems.join("\n"));
}
