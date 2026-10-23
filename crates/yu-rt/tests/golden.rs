//! 事件流 golden(时钟 / 输入 / RNG 全注入)。
//!
//! 三个 golden:
//! 1. `logic_slice.jsonl` —— 控制流 + 变量 + 循环 + GOSUB 的纯逻辑切片,
//!    时钟固定、输入为空 ⇒ 事件流逐字节可比对;
//! 2. `input_counter.jsonl` —— 带符号帧计数器随注入输入时间线变化;
//! 3. `error_paths.jsonl` —— 错误路径(坏标签 / 未声明变量 / 未实现命令),
//!    断言 Diag 码与「继续/中止」行为。
//!
//! golden 更新用 `UPDATE_GOLDEN=1 cargo test -p yu-rt --test golden`(评审后提交)。

mod support;

use std::path::{Path, PathBuf};

use support::{ex, with_int_var, GroupSpec, ProjectBuilder};
use yu_rt::input::{InputFrame, InputTimeline};
use yu_rt::json::to_jsonl;
use yu_rt::{BootConfig, Vm};

const OP_END: u8 = 14;
const OP_GOSUB: u8 = 44;
const OP_IF: u8 = 40 + 5; // 45
const OP_IFBLEND: u8 = 46;
const OP_IFEND: u8 = 49;
const OP_INT: u8 = 51;
const OP_LET: u8 = 54;
const OP_LOOP: u8 = 56;
const OP_LOOPEND: u8 = 59;
const OP_RETURN: u8 = 77;
const OP_WAIT: u8 = 102;

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
    if expected != actual {
        // 逐行 diff,便于定位
        let mut diff = String::new();
        for (i, (a, b)) in expected.lines().zip(actual.lines()).enumerate() {
            if a != b {
                diff.push_str(&format!("line {}:\n  golden: {a}\n  actual: {b}\n", i + 1));
                if diff.len() > 4000 {
                    break;
                }
            }
        }
        panic!(
            "golden 不一致: {}\n{}expected {} 行, actual {} 行",
            path.display(),
            diff,
            expected.lines().count(),
            actual.lines().count()
        );
    }
}

/// 纯逻辑切片:声明 → GOSUB→算术→RETURN → 循环 → IF → WAIT。
///
/// 组布局(主线 0..4,循环 5..10,子程序 11..12):
/// ```text
/// 0  INT[@acc=0]
/// 1  GOSUB[#="SUB.DOUBLE" PINT=@acc]
/// 2  LET[@acc = @acc + @_RINT(1)]
/// 3  WAIT[FRAME=1]
/// 4  LOOP[SET=3] -> 10
/// 5    LET[@acc = @acc + @_LC]
/// 6    IF[@acc >= 9] ELSE->8 EXIT->9
/// 7      IFBLEND
/// 8    IFEND
/// 9  LOOPEND
/// 10 END
/// 11 SUB.DOUBLE: LET[@tmp = @_PINT(1)*2]
/// 12             RETURN[RINT=@tmp]
/// ```
fn logic_project() -> ProjectBuilder {
    let mut b = ProjectBuilder::new();
    let acc = with_int_var(&mut b, 0, 0);
    let tmp = with_int_var(&mut b, 1, 0);
    let sys_id = |name: &str, b: &ProjectBuilder| -> u16 {
        b.sysvars
            .iter()
            .position(|(n, _, _)| *n == name)
            .map(|i| i as u16)
            .unwrap_or_else(|| panic!("夹具缺 {name}"))
    };
    let lc = sys_id("_LC", &b);
    let rint = sys_id("_RINT", &b);
    let pint = sys_id("_PINT", &b);

    b.labels = vec![("SYSTEM_START", 0, 0, 0), ("SUB.DOUBLE", 11, 0, 0)];
    b.entry(0, 0);
    let acc_ref = ex::r#ref(0x40, acc.id);
    let acc_get = ex::get(0x40, acc.id);
    b.scripts = vec![vec![
        GroupSpec::new(OP_INT)
            .window(0, acc_ref.clone())
            .window(0, ex::i8_(0)),
        GroupSpec::new(OP_GOSUB)
            .window(0, ex::s("SUB.DOUBLE"))
            .window(1, acc_get.clone()),
        GroupSpec::new(OP_LET).window(0, acc_ref.clone()).window(
            0,
            ex::cat(&[
                acc_get.clone(),
                ex::r#ref(0x40, rint),
                ex::i8_(1),
                ex::aload(),
                ex::op(0x2b),
            ]),
        ),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_LOOP).window(0, ex::i8_(3)).jump(9),
        GroupSpec::new(OP_LET).window(0, acc_ref.clone()).window(
            0,
            ex::cat(&[acc_get.clone(), ex::get(0x40, lc), ex::op(0x2b)]),
        ),
        GroupSpec::new(OP_IF)
            .window(0, ex::cat(&[acc_get.clone(), ex::i8_(9), ex::op(0x5a)]))
            .jump(8)
            .jump(8),
        GroupSpec::new(OP_IFBLEND),
        GroupSpec::new(OP_IFEND),
        GroupSpec::new(OP_LOOPEND),
        GroupSpec::new(OP_END),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, tmp.id))
            .window(
                0,
                ex::cat(&[
                    ex::r#ref(0x40, pint),
                    ex::i8_(1),
                    ex::aload(),
                    ex::i8_(2),
                    ex::op(0x2a),
                ]),
            ),
        GroupSpec::new(OP_RETURN).window(0, ex::get(0x40, tmp.id)),
    ]];
    b
}

#[test]
fn logic_slice_golden() {
    let b = logic_project();
    let acc = b
        .vars
        .first()
        .map(|v| yu_rt::value::VarRef::new(yu_rt::value::VarSpace::At, v.var_id))
        .expect("acc 变量");
    let mut vm = Vm::boot(Box::new(b.build()), BootConfig::default()).expect("boot");
    let timeline = InputTimeline::new();
    // 帧 1:跑到 WAIT 让出;帧 2:继续到 END
    let r1 = vm.tick_at(0, &timeline);
    assert!(r1.errors.is_empty(), "{:?}", r1.errors);
    vm.tick_at(16, &timeline);
    vm.tick_at(32, &timeline);
    let text = to_jsonl(vm.events());
    assert_golden("logic_slice.jsonl", &text);
    // 变量终值也钉住(golden 之外的独立断言):
    // GOSUB 传入 0 → 返回 0;循环加 _LC(1+2+3=6) ⇒ 6
    assert_eq!(
        vm.store().read(acc, &[]).unwrap(),
        yu_rt::value::Value::Int(6),
        "acc = 0 + (1+2+3)"
    );
}

#[test]
fn error_paths_golden() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // 坏标签
        GroupSpec::new(43).window(0, ex::s("NO.SUCH.LABEL")),
        // 未声明变量写
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, 40000))
            .window(0, ex::i8_(1)),
        // 无手册页命令(不计错,只挂号)
        GroupSpec::new(79),
        // 正常赋值
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::i8_(9)),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = Vm::boot(Box::new(b.build()), BootConfig::default()).expect("boot");
    // 每个错误都终结该任务 ⇒ 需要多次重启主线来继续验证后续命令。
    // 这里按引擎未取证的策略(DIV-RT-005)断言:**首个错误结束主线**。
    let r = vm.tick_at(0, &InputTimeline::new());
    let text = to_jsonl(vm.events());
    assert_golden("error_paths.jsonl", &text);
    assert_eq!(r.errors.len(), 1, "首个 Fatal 后主线结束: {:?}", r.errors);
    assert!(r.errors[0].contains("rt.label"));
    assert!(r.main_finished);
}

#[test]
fn input_counter_golden() {
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    let key_a = b
        .sysvars
        .iter()
        .position(|(n, _, _)| *n == "_KEY_A")
        .unwrap() as u16;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // LOOP[SET=5] { LET[@v=@_KEY_A]; WAIT[FRAME=1] } LOOPEND
        GroupSpec::new(OP_LOOP).window(0, ex::i8_(5)).jump(4),
        GroupSpec::new(OP_LET)
            .window(0, ex::r#ref(0x40, v.id))
            .window(0, ex::get(0x40, key_a)),
        GroupSpec::new(OP_WAIT).window(0, ex::i8_(1)),
        GroupSpec::new(OP_LOOPEND),
        GroupSpec::new(OP_END),
    ]];
    let mut timeline = InputTimeline::new();
    timeline.set(1, InputFrame::new(["KEY_A"]));
    timeline.set(2, InputFrame::new(["KEY_A"]));
    timeline.set(3, InputFrame::new(std::iter::empty::<&str>()));
    timeline.set(4, InputFrame::new(std::iter::empty::<&str>()));

    let mut vm = Vm::boot(Box::new(b.build()), BootConfig::default()).expect("boot");
    // 6 帧:循环 5 轮,每轮一帧
    for f in 0..6u32 {
        vm.tick_at(f * 16, &timeline);
    }
    let text = to_jsonl(vm.events());
    assert_golden("input_counter.jsonl", &text);
    // 时间线:帧 1、2 按住(1→2),帧 3、4、5 松开(-1→-2→-3)
    // 循环 5 轮对应帧 1..5,最后一轮读到的 @_KEY_A = -3。
    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        yu_rt::value::Value::Int(-3)
    );
}
