//! `CG[FILE]` 预加载登记:只在图层确实采用该资产时登记,且每个 key 只登记一次。

mod support;

use support::*;

fn cg(table: &yu_gfx::ParamTable, frame: u64, id: &str, file: &str) -> yu_rt::event::VmEvent {
    call(
        frame,
        "CG",
        vec![
            arg(table, "CG", "ID", s(id)),
            arg(table, "CG", "FILE", s(file)),
        ],
    )
}

#[test]
fn cg_file_registers_only_effective_image_layers() {
    let table = table();
    let mut gfx = state();

    // 图层组带 FILE:该 FILE 会被忽略(已有诊断),不得预热。
    gfx.consume_events(&[
        frame_start(1),
        cg(&table, 1, "G/", "bg"),
        frame_end(1),
    ]);
    assert!(gfx.take_preloads().is_empty(), "图层组不预热 FILE");

    // 图像层:登记一次。
    gfx.consume_events(&[frame_start(2), cg(&table, 2, "L", "bg"), frame_end(2)]);
    assert_eq!(gfx.take_preloads(), vec![b"bg".to_vec()]);

    // 同一 key 再次出现:不重复登记(已解码/已上传)。
    gfx.consume_events(&[frame_start(3), cg(&table, 3, "L", "bg"), frame_end(3)]);
    assert!(gfx.take_preloads().is_empty(), "同一资产只登记一次");

    // 类型冲突:对已存在的图层组发图像层 CG,提前返回,不预热。
    gfx.consume_events(&[
        frame_start(4),
        cg(&table, 4, "G", "other"),
        frame_end(4),
    ]);
    assert!(gfx.take_preloads().is_empty(), "冲突分支不预热");

    // 空 FILE 是 1×1 黑图占位,没有资产可预热。
    gfx.consume_events(&[frame_start(5), cg(&table, 5, "L", ""), frame_end(5)]);
    assert!(gfx.take_preloads().is_empty(), "空 FILE 不预热");
}

#[test]
fn new_preloads_are_drained_not_resent() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[frame_start(1), cg(&table, 1, "A", "a"), frame_end(1)]);
    assert_eq!(gfx.take_preloads(), vec![b"a".to_vec()]);
    assert!(gfx.take_preloads().is_empty(), "取走后不得重复返回");

    // 不同 key 各自登记。
    gfx.consume_events(&[frame_start(2), cg(&table, 2, "B", "b"), frame_end(2)]);
    assert_eq!(gfx.take_preloads(), vec![b"b".to_vec()]);
}
