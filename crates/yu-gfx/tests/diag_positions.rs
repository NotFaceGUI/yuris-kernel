//! S1(todo-silent-handling):FrameDiag 必须带发生位置。
//!
//! 验收原文:「复现 f86,报告应出现 `gfx.imageop.region.partial @ s66 pc6028`
//! (而非只有层名)」——本文件用合成事件锁住机制:`DomainCall` 的
//! `(script, pc)` 必须在诊断聚合后仍然可见,且 frame JSON 位置段只在
//! 有位置时出现(旧 golden 逐字节不变)。

mod support;

use support::{call, call_at, frame_start, state};
use yu_gfx::FrameDiag;

fn first_diag(gfx: &yu_gfx::GfxState) -> FrameDiag {
    let diags = gfx.diagnostics();
    assert_eq!(diags.len(), 1, "恰好一条聚合诊断: {diags:?}");
    diags[0].clone()
}

#[test]
fn domain_call_position_survives_aggregation() {
    let mut gfx = state();
    gfx.on_event(&frame_start(86));
    // CGACT 无 ID ⇒ `gfx.arg.missing`(与 <game-a> 片头同型的聚合路径)。
    gfx.on_event(&call_at(86, 66, 6028, "CGACT", vec![]));
    let diag = first_diag(&gfx);
    assert_eq!(diag.code, "gfx.arg.missing");
    assert_eq!(diag.count, 1);
    assert_eq!(diag.at.len(), 1);
    assert_eq!(diag.at[0].script, "s66");
    assert_eq!(diag.at[0].pc, 6028);
    assert_eq!(diag.at[0].cmd.as_deref(), Some("CGACT"));
    assert_eq!(diag.at_display(), "s66 pc6028");
}

#[test]
fn same_detail_from_two_pcs_keeps_both_positions() {
    let mut gfx = state();
    gfx.on_event(&frame_start(86));
    // 同 detail 文案的两个不同 pc:聚合 count=2,位置不合并(S1 原痛点)。
    gfx.on_event(&call_at(86, 66, 6028, "CGACT", vec![]));
    gfx.on_event(&call_at(86, 66, 9001, "CGACT", vec![]));
    let diag = first_diag(&gfx);
    assert_eq!(diag.count, 2);
    assert_eq!(diag.at_display(), "s66 pc6028,s66 pc9001");
}

#[test]
fn frame_json_writes_at_only_when_present() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    // 无位置路径(旧 golden 口径):JSON 不出现 "at" 键。
    gfx.on_event(&call(0, "CGACT", vec![]));
    let json = yu_gfx::json::to_json(&gfx.frame());
    assert!(
        !json.contains("\"at\""),
        "无位置诊断不得改变旧 JSON 形状: {json}"
    );
    assert!(json.contains("\"count\":1"));

    gfx.on_event(&frame_start(1));
    gfx.on_event(&call_at(1, 66, 6028, "CGACT", vec![]));
    let json = yu_gfx::json::to_json(&gfx.frame());
    assert!(
        json.contains("\"at\":[{\"script\":\"s66\",\"pc\":6028,\"cmd\":\"CGACT\"}]"),
        "{json}"
    );
}

#[test]
fn positions_beyond_cap_fold_into_at_more() {
    let mut gfx = state();
    gfx.on_event(&frame_start(0));
    for pc in 0..12u32 {
        gfx.on_event(&call_at(0, 66, pc * 10, "CGACT", vec![]));
    }
    let diag = first_diag(&gfx);
    assert_eq!(diag.count, 12);
    assert_eq!(diag.at.len(), yu_gfx::MAX_FRAME_AT);
    assert_eq!(diag.at_more, 4, "超出上限的不同位置进显式计数");
    assert!(diag.at_display().ends_with(",+4"), "{}", diag.at_display());
}
