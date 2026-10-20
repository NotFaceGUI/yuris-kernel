//! 真实脚本事件流 golden(时钟/输入全注入)。
//!
//! # 语料
//!
//! 官方 YSCom 0.495 编译的 ERIS 工程,取三个**真实标签**作入口:
//!
//! | 标签 | 脚本 | 组 | 源语义 |
//! |---|---|---|---|
//! | `ES.EV.DEFINE` | s76 | 0..2 | `@T(24,01)=800` → `RETURN[]` |
//! | `ES.MENU.DEFINE` | s93 | 0..2 | `@T(20,01)=200` → `RETURN[]` |
//! | `ES.SE.DEFINE` | s94 | 0..2 | `@T(26,01)=0` → `RETURN[]` |
//!
//! 三个标签的编译产物形态完全一致:LET(数组左值 + 立即数)+ RETURN + END,
//! 是"纯逻辑段"的天然切片(不触碰 CG/声音/窗口)。
//!
//! 入口由 `BootConfig::entry_label` 指定 —— 走的是**生产启动链**
//! (表装载 → 入口标签 → 主线任务 → 脚本装载)。
//!
//! golden 更新:`UPDATE_GOLDEN=1 cargo test -p yu-rt --test corpus_golden`

use std::path::{Path, PathBuf};

use yu_rt::input::InputTimeline;
use yu_rt::json::to_jsonl;
use yu_rt::value::{Value, VarRef, VarSpace};
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

/// 返回**数据根**(`.../project-eris/data`,含 `ysbin/`)。
fn corpus_root() -> Option<PathBuf> {
    let p = std::env::var_os("YURIS_SAMPLE_ERIS_YSBIN")?;
    let ysbin = PathBuf::from(p);
    if !ysbin.is_dir() {
        return None;
    }
    let data = ysbin.parent()?.to_path_buf();
    let project = data.parent()?;
    // 编译器表在工程根;语料 A5 要求它存在(缺则跳过而不是假通过)。
    if project.join("system/YSCom/YSCom.ycd").is_file() {
        Some(data)
    } else {
        None
    }
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).expect("写 golden");
        eprintln!("UPDATED {}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "读 golden {} 失败: {e}(先跑 UPDATE_GOLDEN=1)",
            path.display()
        )
    });
    assert_eq!(expected, actual, "golden 不一致: {}", path.display());
}

/// `@T(24,01)=800` 一类:全局数组 `@1110`(dims [128,128])的元素写入。
///
/// YSVR 实测记录:`(kind=1, group=1, script=0, var_id=1110, type=1,
/// dims=[128,128], init=0)` —— id 来自工程表,不是常量。
fn run_entry(label: &str) -> Option<Vm> {
    let data = corpus_root()?;
    // 语料根 = <project>/data(编译产物 ysbin/*);辅助根 = <project>(system/YSCom)。
    let project = data.parent()?;
    let src = DirSource::open(&data, Codec::Cp932).expect("DirSource(data)");
    let aux = DirSource::open(project, Codec::Cp932).expect("DirSource(project)");
    let cfg = BootConfig {
        entry_label: label.to_string(),
        start_ms: 0,
        fps: 60,
        ..BootConfig::default()
    };
    Some(Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg).expect("boot"))
}

fn array_var(vm: &Vm, name_hint: &str) -> VarRef {
    // 用 YSVR 表定位"那个二维全局数组":kind=1、type=1、dims=[128,128]。
    // 名字在运行期不存在(引擎只保存 id),所以按表结构定位并断言唯一。
    let candidates: Vec<_> = vm
        .tables()
        .boot
        .vars
        .records
        .iter()
        .filter(|r| r.kind == 1 && r.value_type == 1 && r.dims == vec![128, 128])
        .collect();
    assert_eq!(candidates.len(), 1, "{name_hint}: 期望唯一 [128,128] 全局数组");
    VarRef::new(VarSpace::At, candidates[0].var_id)
}

macro_rules! real_slice {
    ($test:ident, $label:expr, $golden:expr, $idx:expr, $want:expr) => {
        #[test]
        fn $test() {
            let Some(mut vm) = run_entry($label) else {
                eprintln!("SKIP[count=1] YURIS_SAMPLE_ERIS_YSBIN 不可用,跳过真实语料 golden");
                return;
            };
            let timeline = InputTimeline::new();
            // 该切片是纯逻辑:一帧内 LET → RETURN(基帧)→ 任务结束。
            let report = vm.tick_at(0, &timeline);
            assert!(report.errors.is_empty(), "{:?}", report.errors);

            let array = array_var(&vm, $label);
            let (i, j) = $idx;
            assert_eq!(
                vm.store().read(array, &[i, j]).unwrap(),
                Value::Int($want),
                "{}: @1110[{i},{j}]",
                $label
            );

            let text = to_jsonl(vm.events());
            assert_golden($golden, &text);

            // 事件流必须包含真实命令名(来自工程 ysc 表)与跳转。
            assert!(text.contains("\"name\":\"LET\""), "{text}");
            assert!(text.contains("\"kind\":\"task_finish\""), "{text}");
        }
    };
}

real_slice!(
    real_slice_ev_define,
    "ES.EV.DEFINE",
    "corpus_ev_define.jsonl",
    (24, 1),
    800
);

real_slice!(
    real_slice_menu_define,
    "ES.MENU.DEFINE",
    "corpus_menu_define.jsonl",
    (20, 1),
    200
);

real_slice!(
    real_slice_se_define,
    "ES.SE.DEFINE",
    "corpus_se_define.jsonl",
    (26, 1),
    0
);

/// 交叉核对:三个切片写的是**同一个**数组的不同行(源脚本都是 `@T(row, col)`)。
#[test]
fn three_slices_share_one_table_array() {
    let Some(mut vm) = run_entry("ES.EV.DEFINE") else {
        eprintln!("SKIP[count=1] 语料不可用");
        return;
    };
    vm.tick_at(0, &InputTimeline::new());
    let array = array_var(&vm, "cross-check");
    // 其余两行在本次运行中未被写(独立进程/独立 VM),仍为初值 0。
    assert_eq!(vm.store().read(array, &[20, 1]).unwrap(), Value::Int(0));
    assert_eq!(vm.store().read(array, &[26, 1]).unwrap(), Value::Int(0));
    // 元数据来自工程 ycd:BLOCK 与 id 的对应可由 ysv 记录复核。
    let rec = vm
        .tables()
        .boot
        .vars
        .records
        .iter()
        .find(|r| r.var_id == array.id)
        .expect("ysv 记录");
    assert_eq!(rec.dims, vec![128, 128]);
    assert_eq!(rec.group, 1, "恢复组号 1(G_INT 族)");
}
