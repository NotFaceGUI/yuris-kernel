//! 按 `(sub, kind, frame, ordinal)` 对齐两份 trace,打印首分歧报告。
//!
//! 用法示例(官方屏幕级 vs 内核屏幕级):
//! ```text
//! cargo run -p yu-trace --example diff_traces -- \
//!   --expected target/tmp/official_screen.jsonl \
//!   --actual target/tmp/kernel_screen.jsonl \
//!   --mode cross --header codec-sample --subs frame \
//!   --kinds frame_description --context 2
//! ```
//!
//! 退出码:0 = 零分歧;1 = 有内容分歧;2 = 参数/协议错误。

use std::process::ExitCode;

use yu_trace::{
    diff_aligned, error, AlignedDiffOptions, DiffFailure, HeaderCompare, Kind, Projection, Sub,
};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|item| item == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(expected) = arg(&args, "--expected") else {
        eprintln!("missing --expected <trace.jsonl>");
        return ExitCode::from(2);
    };
    let Some(actual) = arg(&args, "--actual") else {
        eprintln!("missing --actual <trace.jsonl>");
        return ExitCode::from(2);
    };
    let expected_text = match std::fs::read_to_string(&expected) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {expected}: {error}");
            return ExitCode::from(2);
        }
    };
    let actual_text = match std::fs::read_to_string(&actual) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {actual}: {error}");
            return ExitCode::from(2);
        }
    };

    let mode = arg(&args, "--mode").unwrap_or_else(|| "cross".to_string());
    let mut options = match mode.as_str() {
        "same" => AlignedDiffOptions::same_producer(),
        "cross" => AlignedDiffOptions::cross_producer(),
        other => {
            eprintln!("unknown --mode {other:?} (cross|same)");
            return ExitCode::from(2);
        }
    };
    if let Some(header) = arg(&args, "--header") {
        options.header = match header.as_str() {
            "ignore" => HeaderCompare::Ignore,
            "codec-sample" => HeaderCompare::CodecSample,
            "semantic" => HeaderCompare::Semantic,
            "strict" => HeaderCompare::Strict,
            other => {
                eprintln!("unknown --header {other:?}");
                return ExitCode::from(2);
            }
        };
    }
    if let Some(subs) = arg(&args, "--subs") {
        let mut parsed = Vec::new();
        for name in parse_list(&subs) {
            match Sub::parse(&name) {
                Some(sub) => parsed.push(sub),
                None => {
                    eprintln!("unknown sub {name:?}");
                    return ExitCode::from(2);
                }
            }
        }
        options.projection = Projection {
            subs: parsed,
            ..options.projection
        };
    }
    if let Some(kinds) = arg(&args, "--kinds") {
        let mut parsed = Vec::new();
        for name in parse_list(&kinds) {
            match Kind::parse(&name) {
                Some(kind) => parsed.push(kind),
                None => {
                    eprintln!("unknown kind {name:?}");
                    return ExitCode::from(2);
                }
            }
        }
        options.projection = Projection {
            kinds: parsed,
            ..options.projection
        };
    }
    if let Some(first) = arg(&args, "--frame-first") {
        let first = first.parse::<u64>().expect("--frame-first must be u64");
        let last = arg(&args, "--frame-last")
            .map(|text| text.parse::<u64>().expect("--frame-last must be u64"))
            .unwrap_or(u64::MAX);
        options.projection = options.projection.frames(first, last);
    }
    if let Some(fields) = arg(&args, "--payload-fields") {
        let fields = parse_list(&fields);
        options.payload_fields = fields;
    }
    if let Some(context) = arg(&args, "--context") {
        options.context = context.parse::<usize>().expect("--context must be usize");
    }

    match diff_aligned(&expected_text, &actual_text, &options) {
        Ok(()) => {
            println!("OK: zero divergence");
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print_failure(&failure);
            if failure.code() == error::code::DIFF {
                ExitCode::from(1)
            } else {
                ExitCode::from(2)
            }
        }
    }
}

fn print_failure(failure: &DiffFailure) {
    println!("DIVERGENCE code={}", failure.code());
    if let Some(report) = failure.aligned_report() {
        println!("key={}", report.key);
        println!(
            "expected_line={}",
            report
                .expected_line
                .map(|line| line.to_string())
                .unwrap_or_else(|| "missing".to_string())
        );
        println!(
            "actual_line={}",
            report
                .actual_line
                .map(|line| line.to_string())
                .unwrap_or_else(|| "missing".to_string())
        );
        println!("path={}", if report.path.is_empty() { "/" } else { &report.path });
        println!("expected={}", report.expected);
        println!("actual={}", report.actual);
        for line in &report.context {
            println!("context={line}");
        }
    } else if let Some(report) = failure.report() {
        println!("line={}", report.line);
        println!("path={}", if report.path.is_empty() { "/" } else { &report.path });
        println!("expected={}", report.expected);
        println!("actual={}", report.actual);
    } else {
        println!("detail={}", failure.diag);
    }
}
