//! `yu-cli export`:导出工程文本 / 角色表(跨平台数据导出,不跑引擎)。
//!
//! 输出是翻译流程与运行时文本 hook 的输入:稳定坐标 + UTF-8 的 CSV。
//! 具体布局与编码策略见 [`yu_engine::export`]。

use std::path::PathBuf;

use yu_engine::export::{self, ExportFormat, ExportOptions, TextCodecChoice};
use yu_engine::keys::ScriptKey;
use yu_engine::project::Project;

use crate::args::ArgReader;
use crate::output;
use crate::{CliResult, Failure};

pub fn from_args(tokens: Vec<String>) -> CliResult<()> {
    let mut args = ArgReader::new(tokens);
    let root = args
        .positional(0)
        .ok_or_else(|| Failure::usage("用法: yu-cli export <project-root> [out-dir] [...]"))?;
    let out_dir = args
        .positional_next()
        .map(PathBuf::from)
        .unwrap_or_else(|| export::default_output_dir(std::path::Path::new(&root)));
    let data = args.flag("data");
    let text_codec = match args.flag("text-codec") {
        Some(value) => TextCodecChoice::parse(&value).map_err(Failure::usage)?,
        None => TextCodecChoice::Auto,
    };
    let script_key = match args.flag("script-key") {
        Some(value) if !value.is_empty() => {
            Some(ScriptKey::parse_hex(&value).map_err(Failure::usage)?)
        }
        _ => None,
    };
    let format = match args.flag("format") {
        Some(value) => ExportFormat::parse(&value).map_err(Failure::usage)?,
        None => ExportFormat::Csv,
    };
    let include_literals = !args.switch("no-literals");
    let runtime_characters = args.switch("runtime-characters");
    let runtime_ticks = match args.flag("runtime-ticks") {
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| Failure::usage(format!("--runtime-ticks 需要整数,收到 `{value}`")))?,
        None => 600,
    };
    args.finish()?;

    let project = Project::discover(
        std::path::Path::new(&root),
        data.as_deref().map(std::path::Path::new),
    )?;
    let options = ExportOptions {
        text_codec,
        script_key,
        include_literals,
        format,
        runtime_characters,
        runtime_ticks,
    };
    let report = export::export_project(&project, &out_dir, &options)?;
    if format == ExportFormat::TranslateJson {
        output::line(export::summary_translate(&report));
    } else {
        output::line(export::summary(&report));
    }
    for warning in &report.warnings {
        output::line(format!("[warn] {warning}"));
    }
    for skipped in &report.skipped {
        output::line(format!("[skip] {skipped}"));
    }
    if format == ExportFormat::TranslateJson {
        output::line(format!("→ {}", out_dir.join("translate.json").display()));
    } else {
        output::line(format!("→ {}", out_dir.display()));
    }
    Ok(())
}
