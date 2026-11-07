//! `frame()` 投影的**结构**护栏。
//!
//! 背景:菜单/存档打开帧的 render 尖峰(桌面 8–14ms,安卓放大后掉到 20fps)
//! 来自「与本帧无关的整块工作」,而不是图层绘制本身。已收口的两处:
//!
//! 1. `surface_producers()` 每个组离屏 pass 都重算一次 ⇒ 单帧
//!    `组数 × op日志长度`(ERIS 菜单 282 组)。现在由调用方算一次传入。
//! 2. 屏幕快照的生产者查找对每个快照都线性扫 `copy_log` ⇒
//!    `快照数 × 日志长度`(存档界面 10+ 缩略图)。现在按 `CopyId` 建索引。
//!
//! 这里**不做墙钟断言**:CI 机器差异与 debug/test 档的分配器开销都会让
//! 微秒级基线抖动,断言耗时只会制造假红。改为钉住可观察的结构不变量 ——
//! 两处优化各自的语义必要条件。

mod support;

use std::time::Instant;

use support::*;
use yu_gfx::{PassTarget, TextureRef};

/// 建一棵 `groups` 个同层组的树,每组一个可见图像子层。
fn build(groups: usize) -> yu_gfx::GfxState {
    let table = table();
    let mut gfx = state();
    let mut events = vec![frame_start(1)];
    for g in 0..groups {
        let group = format!("GRP{g}");
        let image = format!("GRP{g}/IMG");
        // 组:CG 对以 `/` 结尾的 ID 建组。
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(&format!("{group}/"))),
                arg(&table, "CG", "SX", i(64)),
                arg(&table, "CG", "SY", i(64)),
            ],
        ));
        // 组内图像层:CG[FILE=""] = 1×1 符号面(SX/SY 决定显示尺寸)。
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(&image)),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(64)),
                arg(&table, "CG", "SY", i(64)),
            ],
        ));
    }
    events.push(frame_end(1));
    gfx.consume_events(&events);
    gfx
}

/// 每个组离屏 pass 都必须带着自己的子层 op。
///
/// 这条同时守住「生产层集合被正确传递」:优化 1 把 `surface_producers()`
/// 从 `emit_children_ops` 内部提到调用方,若某条路径忘了传参,该组的 op
/// 会直接消失(画面缺层,不只是变慢)。`emit_children_ops` 的签名现在
/// 强制要求 `produced`,这条测试守住运行时真的传了同样的集合。
#[test]
fn every_group_pass_emits_its_child_layers() {
    let gfx = build(12);
    let frame = gfx.frame();
    let group_passes: Vec<_> = frame
        .passes
        .iter()
        .filter(|p| matches!(&p.target, PassTarget::Offscreen(_)))
        .collect();
    assert!(
        !group_passes.is_empty(),
        "12 个组应产出离屏 pass(否则本用例没测到目标路径)"
    );
    for pass in group_passes {
        assert!(
            !pass.ops.is_empty(),
            "组 pass {:?} 为空:生产层集合没有传达到 emit_children_ops",
            pass.target
        );
    }
    let screen = frame
        .passes
        .iter()
        .find(|p| matches!(p.target, PassTarget::Screen))
        .expect("屏幕 pass");
    assert!(!screen.ops.is_empty(), "屏幕 pass 为空");
}

/// 屏幕快照的生产者仍可被逐条找到(优化 2 的语义必要条件)。
///
/// 优化 2 只把「对每个快照线性扫 `copy_log`」换成按 `CopyId` 建索引,
/// 语义必须完全一致:每个 `screen_snapshots` 条目都能在 `copy_log` 里
/// 找到**恰好一条**同 id 的生产者记录。找不到就说明索引丢了项(该快照
/// 在帧内会退化成后缓冲读回,画面错)。
#[test]
fn every_screen_snapshot_has_its_producer_in_copy_log() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("MAIN/")),
                arg(&table, "CG", "SX", i(320)),
                arg(&table, "CG", "SY", i(240)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("MAIN/BG")),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(320)),
                arg(&table, "CG", "SY", i(240)),
            ],
        ),
        frame_end(1),
    ]);
    gfx.consume_events(&[
        frame_start(2),
        call(
            2,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s(":/W0/")),
                arg(&table, "CGACT", "ID2", s("CGS1")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        call(
            2,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s(":/W0/")),
                arg(&table, "CGACT", "ID2", s("CGS2")),
                arg(&table, "CGACT", "COPY2", i(1)),
            ],
        ),
        frame_end(2),
    ]);
    let frame = gfx.frame();
    assert!(
        !frame.screen_snapshots.is_empty(),
        "本用例应至少产生一个屏幕快照(否则没测到目标路径)"
    );
    for id in frame.screen_snapshots.keys() {
        let producers = gfx.copies().iter().filter(|c| c.id == *id).count();
        assert_eq!(
            producers, 1,
            "屏幕快照 {} 在 copy_log 中应有恰好一条生产者,实际 {producers}",
            id.0
        );
    }
    let _ = TextureRef::None;
}

// ---------------------------------------------------------------- 测量用例

/// 打印 `frame()` 投影成本(组数 / op 日志长度两个维度)。
///
/// **不是断言**,只在人工量化时跑(release 档才有意义):
/// ```text
/// cargo test --release -p yu-gfx --test frame_cost -- --ignored --nocapture
/// ```
///
/// `log_layers` 控制加工项摊在多少个不同层上:真实游戏里动画/演出会对
/// **同一批层**反复加工(层数少、op 多),这决定 `nodes` 查表命中的复用度。
fn measure(groups: usize, log_ops: usize, log_layers: usize, rounds: usize) -> (u128, usize, usize) {
    let table = table();
    let mut gfx = state();
    let mut events = vec![frame_start(1)];
    for g in 0..groups {
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(&format!("G{g}/"))),
                arg(&table, "CG", "SX", i(800)),
                arg(&table, "CG", "SY", i(600)),
            ],
        ));
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(&format!("G{g}/L0"))),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(64)),
                arg(&table, "CG", "SY", i(64)),
            ],
        ));
    }
    // 撑 image_log:先建 `log_layers` 个层,再把 `log_ops` 条加工摊上去。
    //
    // 两个维度都要控住:
    // - 同一层重复加工会被折叠(实测 200 次同层 RECTPAINT 后只剩 2 条),
    //   所以分摊到多个层才能把日志撑长;
    // - 但真实语料的 op/层比很高(演出对同一批层反复加工),所以层数不能
    //   跟着 op 走 —— 否则量到的是「每 op 一个新层」这种不存在的形状。
    let layers_n = log_layers.max(1).min(log_ops.max(1));
    for k in 0..layers_n {
        events.push(call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s(&format!("LOG{k}"))),
                arg(&table, "CG", "FILE", s("")),
                arg(&table, "CG", "SX", i(8)),
                arg(&table, "CG", "SY", i(8)),
            ],
        ));
    }
    // 每层分到的 op 数不同(取模分配),保证 0..layers_n 全部被覆盖。
    for k in 0..log_ops {
        let layer = format!("LOG{}", k % layers_n);
        events.push(call(
            1,
            "CGACT",
            vec![
                arg(&table, "CGACT", "ID", s(&layer)),
                arg(&table, "CGACT", "RECTPAINT", i(255)),
            ],
        ));
    }
    events.push(frame_end(1));
    gfx.consume_events(&events);
    let image_log = gfx.images().len();
    // 同层重复加工会被 `supersede_*` 折叠(RECTPAINT 保留最新一条),
    // 所以日志长度 ≈ 不同层的数量级。这里只要求"确实建起来了",
    // 精确条数打进输出供人读,不做等比断言。
    assert!(
        log_ops == 0 || image_log > 0,
        "测量前提不成立:log_ops={log_ops} 但 image_log 为空 —— 加工项没被记入"
    );
    let _ = gfx.frame(); // 预热
    let mut best = u128::MAX;
    for r in 0..rounds {
        let frame_no = 1000 + r as u64;
        gfx.consume_events(&[frame_start(frame_no), frame_end(frame_no)]);
        let started = Instant::now();
        let frame = gfx.frame();
        let us = started.elapsed().as_micros();
        assert!(!frame.passes.is_empty());
        best = best.min(us);
    }
    (best, groups, image_log)
}

/// 人工量化入口(`--ignored`)。输出两维扫描,用于确认「组数」与
/// 「日志长度」都不再是相乘关系。
#[test]
#[ignore = "人工量化用;release 档跑才有意义"]
fn frame_projection_cost_sweep() {
    println!("== 固定 op 日志(200 项 / 50 层),扫组数 ==");
    for groups in [16usize, 32, 64, 128, 256, 512] {
        let (us, g, log) = measure(groups, 200, 50, 100);
        println!("  groups={g:>4} log={log:>4} -> {us:>7}us");
    }
    println!("== 固定组数(64),扫 op 日志(真实形状:200 层摊开) ==");
    for log_ops in [0usize, 200, 2000, 8000, 16000] {
        let (us, g, log) = measure(64, log_ops, 200, 100);
        println!("  groups={g:>4} log={log:>5} -> {us:>7}us");
    }
    println!("== 固定组数(64)+ 8000 op,扫「op 摊到多少层」(考察 nodes 查表复用) ==");
    for log_layers in [10usize, 100, 1000, 8000] {
        let (us, _g, log) = measure(64, 8000, log_layers, 100);
        println!("  layers={log_layers:>5} log={log:>5} -> {us:>7}us");
    }
}
