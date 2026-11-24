//! 取证探针:boot 后跑 N tick,按槽号 dump 运行时变量值。
//!
//! 用法:`dump_runtime_vars <project-root> <ticks> [--codec cp932|gbk|big5] <slot…>`
//!
//! 槽号用 `0x` 前缀按十六进制解析;输出所有匹配槽号的存储项
//! (`@`/`$`/旧式前缀都会命中)。fatal 后仍打印已到达的状态。
//!
//! 商业族(0.500/0.555)会按引擎同一门控安装独立运行面,否则 `PRODUCT`/
//! `TEXT[FILE]` 等命令没有处理器,探针看到的变量状态与真实运行不一致。
use std::path::Path;

use yu_engine::project::Project;
use yu_rt::input::InputTimeline;
use yu_rt::value::Value;
use yu_rt::{BootConfig, Vm};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--codec` / `--answer-dialogs` 是探针参数,先摘出来。
    let mut codec = yu_format::Codec::Gbk;
    if let Some(pos) = args.iter().position(|a| a == "--codec") {
        let value = args.get(pos + 1).cloned().unwrap_or_default();
        args.drain(pos..=pos + 1);
        codec = match value.to_ascii_lowercase().as_str() {
            "cp932" | "sjis" => yu_format::Codec::Cp932,
            "gbk" => yu_format::Codec::Gbk,
            "big5" => yu_format::Codec::Big5,
            other => panic!("--codec 只接受 cp932|gbk|big5,收到 `{other}`"),
        };
    }
    // 缺失/等待宿主回答的 DIALOG 会让主任务永久挂起(启动期更新检查就是
    // 一例);显式给一个按钮号让探针继续。0/缺省 = 不回答。
    let auto_answer: Option<u8> = args
        .iter()
        .position(|a| a == "--answer-dialogs")
        .map(|pos| {
            let value = args.get(pos + 1).cloned().unwrap_or_default();
            args.drain(pos..=pos + 1);
            value
                .parse()
                .expect("--answer-dialogs 需要按钮序号(1=是/2=否)")
        });
    let mut args = args.into_iter();
    let root = args.next().expect("project root");
    let ticks: u32 = args
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let slots: Vec<u16> = args
        .map(|s| {
            s.strip_prefix("0x")
                .map(|h| u16::from_str_radix(h, 16).expect("hex slot"))
                .unwrap_or_else(|| s.parse().expect("slot"))
        })
        .collect();

    let project = Project::discover(Path::new(&root), None).expect("project");
    let key = project.resolve_script_key(None).expect("script key");
    let src = project.open_source().expect("source");
    let aux = project.open_aux().expect("aux");
    let mut vm = Vm::boot_with_aux(
        src,
        Some(aux),
        BootConfig {
            entry_label: "SYSTEM_START".into(),
            script_key: key.bytes,
            codecs: yu_rt::CodecConfig {
                script: codec,
                text: codec,
                name: yu_format::Codec::Cp932,
            },
            ..BootConfig::default()
        },
    )
    .expect("boot");
    // 与引擎同一门控:555 商业族(0.500/0.555)才安装独立运行面。
    if vm.semantics().is_v555_family() {
        vm.install_other_runtime();
    }
    for f in 0..ticks {
        let _ = vm.tick_at(f, &InputTimeline::new());
        if let Some(button) = auto_answer {
            let pending: Vec<(u64, u8, String, String)> = vm
                .pending_dialogs()
                .iter()
                .map(|d| {
                    (
                        d.id,
                        d.buttons.count(),
                        d.caption_display(),
                        d.text_display(),
                    )
                })
                .collect();
            for (id, buttons, caption, text) in pending {
                if f < 3 || f % 60 == 0 {
                    println!(
                        "dialog#{id} buttons={buttons} caption={caption:?} text={:?}",
                        text.chars().take(80).collect::<String>()
                    );
                }
                let choice = button.min(buttons).max(1);
                let _ = vm.answer_dialog(id, choice, None);
            }
        }
    }
    // 任务快照放最前:变量值要结合「当前执行到哪个脚本/pc」才能解释。
    for (task, script, pc) in vm.task_positions() {
        println!("task {task} s{script} pc{pc}");
    }
    for (r, slot) in vm.store().iter() {
        if slots.is_empty() || slots.contains(&r.id) {
            let vals: Vec<String> = slot
                .data
                .iter()
                .take(16)
                .map(|v| match v {
                    Value::Int(i) => i.to_string(),
                    Value::Float(f) => f.to_string(),
                    Value::Str(b) => format!("{:?}", String::from_utf8_lossy(b)),
                })
                .collect();
            // 前 16 项不足以看清稀疏数组(布局表常写 [13]/[14]);再列
            // 前 32 个非零项的下标+值。
            let nonzero: Vec<String> = slot
                .data
                .iter()
                .enumerate()
                .filter(|(_, v)| match v {
                    Value::Int(i) => *i != 0,
                    Value::Float(f) => *f != 0.0,
                    Value::Str(b) => !b.is_empty(),
                })
                .take(32)
                .map(|(i, v)| match v {
                    Value::Int(n) => format!("[{i}]={n}"),
                    Value::Float(f) => format!("[{i}]={f}"),
                    Value::Str(b) => format!("[{i}]={:?}", String::from_utf8_lossy(b)),
                })
                .collect();
            println!(
                "{} id={} ty={} dims={:?} n={} [{}] nonzero: {}",
                r.display(),
                r.id,
                slot.ty.as_str(),
                slot.dims,
                slot.data.len(),
                vals.join(", "),
                nonzero.join(", ")
            );
        }
    }
}
