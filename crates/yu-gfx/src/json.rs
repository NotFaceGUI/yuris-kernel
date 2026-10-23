//! 稳定 JSON 序列化。
//!
//! 不引入 `serde`:字段顺序固定、浮点格式固定、字节串按 `\u00XX` 转义。
//! **同一 `CompositorFrame` 两次序列化必须逐字节一致**(A3);所有集合都用
//! `Vec`/`BTreeMap`,不存在散列表迭代序。
//!
//! 两种输出:
//! - [`to_json`][]:完整契约(含全部图层状态快照);
//! - [`to_display_json`]:显示列表(省略不可见层快照,用于 ERIS 首屏 golden)。

use crate::frame::{Clear, CompositorFrame, TextureFilter, Viewport};
use crate::layer::{
    BlendMode, EngineValue, ImageFormat, Layer, LayerId, LayerKind, LayerStatus, RectI, SizeU32,
    StageArg, StageOp, TextureFactorRule, TextureRef, Transform, Transition,
};
use crate::op::{CopyRegion, CopySource, CopyTiming, ImageAction, Op, OpStatus};
use crate::pass::{Pass, PassTarget};

fn esc_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn esc_bytes(out: &mut String, b: &[u8]) {
    out.push('"');
    for &x in b {
        match x {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            0x20..=0x7E => out.push(x as char),
            _ => out.push_str(&format!("\\u{:04x}", x)),
        }
    }
    out.push('"');
}

/// 稳定、合法 JSON 的浮点格式。
///
/// - `NaN/±Inf` 不是合法 JSON → 输出 `null`(值不可表示,不做数字谎报);
/// - 整数值 `128.0` 输出 `128.0`(不是 `1.3e2`);
/// - `-0.0` 保留符号;
/// - 其余用 Rust 的最短往返格式。
fn fmt_f64(v: f64) -> String {
    if !v.is_finite() {
        return "null".to_string();
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    if v.fract() == 0.0 && v.abs() < 1e16 {
        format!("{v:.1}")
    } else {
        format!("{v:?}")
    }
}

fn wid(out: &mut String, v: &LayerId) {
    esc_bytes(out, v.as_bytes());
}

fn engine_value(out: &mut String, v: &EngineValue) {
    match v {
        EngineValue::Int(i) => {
            out.push_str("{\"t\":\"int\",\"v\":");
            out.push_str(&i.to_string());
            out.push('}');
        }
        EngineValue::Float(f) => {
            out.push_str("{\"t\":\"flt\",\"v\":");
            out.push_str(&fmt_f64(*f));
            out.push('}');
        }
        EngineValue::Str(b) => {
            out.push_str("{\"t\":\"str\",\"v\":");
            esc_bytes(out, b);
            out.push('}');
        }
    }
}

fn viewport(out: &mut String, v: Viewport) {
    out.push_str("{\"width\":");
    out.push_str(&v.width.to_string());
    out.push_str(",\"height\":");
    out.push_str(&v.height.to_string());
    out.push('}');
}

fn clear(out: &mut String, c: &Clear) {
    out.push_str("{\"rgba\":[");
    for (i, ch) in c.rgba.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&ch.to_string());
    }
    out.push_str("],\"source\":");
    esc_str(
        out,
        match c.source {
            crate::frame::ClearSource::EngineDefaultBlack => "engine_default_black",
            crate::frame::ClearSource::Explicit => "explicit",
        },
    );
    out.push('}');
}

fn filter(out: &mut String, f: &TextureFilter) {
    out.push_str("{\"mag\":");
    esc_str(out, f.mag.as_str());
    out.push_str(",\"min\":");
    esc_str(out, f.min.as_str());
    out.push_str(",\"mip\":");
    esc_str(out, f.mip.as_str());
    out.push_str(",\"source\":");
    esc_str(out, f.source.as_str());
    out.push('}');
}

fn image_format(out: &mut String, f: ImageFormat) {
    out.push_str("{\"name\":");
    esc_str(out, f.as_str());
    out.push_str(",\"code\":");
    out.push_str(
        &match f {
            ImageFormat::Png => 1,
            ImageFormat::Bmp => 2,
            ImageFormat::Jpg => 3,
            ImageFormat::Gif => 4,
            ImageFormat::Psb => 5,
            ImageFormat::Other(c) => c,
        }
        .to_string(),
    );
    out.push('}');
}

fn texture(out: &mut String, t: &TextureRef) {
    match t {
        TextureRef::Asset(path) => {
            out.push_str("{\"kind\":\"asset\",\"path\":");
            esc_bytes(out, path);
            out.push('}');
        }
        TextureRef::SolidBlack1x1 => out.push_str("{\"kind\":\"solid_black_1x1\"}"),
        TextureRef::Offscreen(id) => {
            out.push_str("{\"kind\":\"offscreen\",\"id\":");
            wid(out, id);
            out.push('}');
        }
        TextureRef::Snapshot(id) => {
            out.push_str("{\"kind\":\"snapshot\",\"id\":");
            out.push_str(&id.0.to_string());
            out.push('}');
        }
        TextureRef::None => out.push_str("{\"kind\":\"none\"}"),
    }
}

fn size(out: &mut String, s: SizeU32) {
    out.push_str("{\"width\":");
    out.push_str(&s.width.to_string());
    out.push_str(",\"height\":");
    out.push_str(&s.height.to_string());
    out.push('}');
}

fn rect(out: &mut String, r: &RectI) {
    out.push_str("{\"x\":");
    out.push_str(&r.x.to_string());
    out.push_str(",\"y\":");
    out.push_str(&r.y.to_string());
    out.push_str(",\"width\":");
    out.push_str(&r.width.to_string());
    out.push_str(",\"height\":");
    out.push_str(&r.height.to_string());
    out.push('}');
}

fn transform(out: &mut String, t: &Transform) {
    out.push_str("{\"x\":");
    out.push_str(&t.x.to_string());
    out.push_str(",\"y\":");
    out.push_str(&t.y.to_string());
    out.push_str(",\"filter\":[");
    for (i, (k, v)) in t.filter.values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        esc_str(out, k.name());
        out.push_str(",\"value\":");
        engine_value(out, v);
        out.push('}');
    }
    out.push_str("]}");
}

fn transition(out: &mut String, t: &Transition) {
    out.push_str("{\"t\":");
    out.push_str(&t.t.to_string());
    out.push_str(",\"ta\":");
    out.push_str(&t.ta.to_string());
    out.push_str(",\"tid\":");
    match &t.tid {
        Some(id) => wid(out, id),
        None => out.push_str("null"),
    }
    out.push_str(",\"tid_raw\":");
    match &t.tid_raw {
        Some(b) => esc_bytes(out, b),
        None => out.push_str("null"),
    }
    out.push_str(",\"fade\":");
    match t.fade_value() {
        Some(v) => out.push_str(&fmt_f64(v)),
        None => out.push_str("null"),
    }
    out.push('}');
}

fn blend(out: &mut String, b: BlendMode) {
    out.push_str("{\"code\":");
    out.push_str(&b.code().to_string());
    out.push_str(",\"name\":");
    esc_str(out, b.name());
    out.push_str(",\"confidence\":");
    esc_str(out, b.confidence().as_str());
    out.push_str(",\"path\":");
    esc_str(out, if b.needs_shader() { "shader" } else { "fixed" });
    out.push('}');
}

fn layer_status(out: &mut String, s: LayerStatus) {
    match s {
        LayerStatus::Executable => out.push_str("{\"executable\":true}"),
        LayerStatus::Unimplemented(why) => {
            out.push_str("{\"executable\":false,\"reason\":");
            esc_str(out, why);
            out.push('}');
        }
    }
}

pub fn layer(out: &mut String, l: &Layer) {
    out.push_str("{\"id\":");
    wid(out, &l.id);
    out.push_str(",\"seq\":");
    out.push_str(&l.seq.to_string());
    out.push_str(",\"z\":");
    out.push_str(&l.z.to_string());
    out.push_str(",\"alpha\":");
    out.push_str(&l.alpha.to_string());
    out.push_str(",\"blend\":");
    blend(out, l.blend);
    out.push_str(",\"visible\":");
    out.push_str(if l.visible { "true" } else { "false" });
    out.push_str(",\"kind\":");
    match &l.kind {
        LayerKind::Image { size: image_size } => {
            out.push_str("{\"kind\":\"image\",\"size\":");
            match image_size {
                Some(s) => size(out, *s),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        LayerKind::Group { size: group_size } => {
            out.push_str("{\"kind\":\"group\",\"size\":");
            match group_size {
                Some(s) => size(out, *s),
                None => out.push_str("null"),
            }
            out.push('}');
        }
    }
    out.push_str(",\"texture\":");
    texture(out, &l.texture);
    out.push_str(",\"parts\":[");
    for (i, p) in l.parts.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"texture\":");
        texture(out, &p.texture);
        out.push_str(",\"raw\":[");
        for (j, v) in p.raw.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&v.to_string());
        }
        out.push_str("]}");
    }
    out.push_str("],\"transform\":");
    transform(out, &l.transform);
    out.push_str(",\"mask\":");
    match &l.mask {
        Some(m) => wid(out, m),
        None => out.push_str("null"),
    }
    out.push_str(",\"transition\":");
    match &l.transition {
        Some(t) => transition(out, t),
        None => out.push_str("null"),
    }
    out.push_str(",\"clip\":");
    match &l.clip {
        Some(r) => rect(out, r),
        None => out.push_str("null"),
    }
    out.push_str(",\"status\":");
    layer_status(out, l.status);
    out.push_str(",\"extra\":[");
    for (i, (k, v)) in l.extra.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        esc_str(out, k);
        out.push_str(",\"value\":");
        engine_value(out, v);
        out.push('}');
    }
    out.push_str("]}");
}

fn op_status(out: &mut String, s: OpStatus) {
    match s {
        OpStatus::Executable => out.push_str("{\"executable\":true}"),
        OpStatus::Unimplemented(why) => {
            out.push_str("{\"executable\":false,\"reason\":");
            esc_str(out, why);
            out.push('}');
        }
    }
}

fn copy_region(out: &mut String, r: &CopyRegion) {
    match r {
        CopyRegion::Full => out.push_str("{\"kind\":\"full\"}"),
        CopyRegion::Partial { x, y, x2, y2 } => {
            out.push_str("{\"kind\":\"partial\",\"x\":");
            opt_i64(out, *x);
            out.push_str(",\"y\":");
            opt_i64(out, *y);
            out.push_str(",\"x2\":");
            opt_i64(out, *x2);
            out.push_str(",\"y2\":");
            opt_i64(out, *y2);
            out.push('}');
        }
    }
}

fn opt_i64(out: &mut String, v: Option<i64>) {
    match v {
        Some(n) => out.push_str(&n.to_string()),
        None => out.push_str("null"),
    }
}

fn image_action(out: &mut String, a: &ImageAction) {
    esc_str(out, a.name());
}

pub fn op(out: &mut String, o: &Op) {
    match o {
        Op::Layer(l) => {
            out.push_str("{\"op\":\"layer\",\"layer\":");
            layer(out, l);
            out.push('}');
        }
        Op::Group(g) => {
            out.push_str("{\"op\":\"group\",\"offscreen\":");
            wid(out, &g.offscreen);
            out.push_str(",\"layer\":");
            layer(out, &g.layer);
            out.push('}');
        }
        Op::Transition(t) => {
            out.push_str("{\"op\":\"transition\",\"from\":");
            texture(out, &t.from);
            out.push_str(",\"to\":");
            match &t.to {
                Some(x) => texture(out, x),
                None => out.push_str("null"),
            }
            out.push_str(",\"fade\":");
            out.push_str(&fmt_f64(t.fade));
            out.push_str(",\"tid\":");
            match &t.tid {
                Some(x) => wid(out, x),
                None => out.push_str("null"),
            }
            out.push_str(",\"layer\":");
            layer(out, &t.layer);
            out.push('}');
        }
        Op::Mask(m) => {
            out.push_str("{\"op\":\"mask\",\"mask\":");
            wid(out, &m.mask);
            out.push_str(",\"layer\":");
            layer(out, &m.layer);
            out.push('}');
        }
        Op::Copy(c) => {
            out.push_str("{\"op\":\"copy\",\"copy\":");
            out.push_str(&c.id.0.to_string());
            out.push_str(",\"kind\":");
            esc_str(out, c.kind.name());
            out.push_str(",\"source\":");
            match &c.source {
                CopySource::Layer(x) => {
                    out.push_str("{\"kind\":\"layer\",\"id\":");
                    wid(out, x);
                    out.push('}');
                }
                CopySource::Group(x) => {
                    out.push_str("{\"kind\":\"group\",\"id\":");
                    wid(out, x);
                    out.push('}');
                }
            }
            out.push_str(",\"dest\":");
            wid(out, &c.dest);
            out.push_str(",\"region\":");
            copy_region(out, &c.region);
            out.push_str(",\"timing\":");
            esc_str(
                out,
                match c.timing {
                    CopyTiming::CommandCurrent => "command_current",
                },
            );
            out.push_str(",\"status\":");
            op_status(out, c.status);
            out.push('}');
        }
        Op::Image(i) => {
            out.push_str("{\"op\":\"image\",\"source\":");
            wid(out, &i.source);
            out.push_str(",\"target\":");
            match &i.target {
                Some(t) => wid(out, t),
                None => out.push_str("null"),
            }
            out.push_str(",\"action\":");
            image_action(out, &i.action);
            out.push_str(",\"set\":");
            match &i.set {
                Some(v) => engine_value(out, v),
                None => out.push_str("null"),
            }
            out.push_str(",\"set2\":");
            match &i.set2 {
                Some(v) => engine_value(out, v),
                None => out.push_str("null"),
            }
            out.push_str(",\"setstr\":");
            match &i.set_str {
                Some(v) => esc_bytes(out, v),
                None => out.push_str("null"),
            }
            out.push_str(",\"sx\":");
            opt_i64(out, i.sx);
            out.push_str(",\"sy\":");
            opt_i64(out, i.sy);
            out.push_str(",\"x\":");
            opt_i64(out, i.x);
            out.push_str(",\"y\":");
            opt_i64(out, i.y);
            out.push_str(",\"x2\":");
            opt_i64(out, i.x2);
            out.push_str(",\"y2\":");
            opt_i64(out, i.y2);
            out.push_str(",\"status\":");
            op_status(out, i.status);
            out.push('}');
        }
    }
}

fn pass_target(out: &mut String, t: &PassTarget) {
    match t {
        PassTarget::Screen => out.push_str("{\"kind\":\"screen\"}"),
        PassTarget::Offscreen(id) => {
            out.push_str("{\"kind\":\"offscreen\",\"id\":");
            wid(out, id);
            out.push('}');
        }
        PassTarget::Texture(id) => {
            out.push_str("{\"kind\":\"texture\",\"id\":");
            wid(out, id);
            out.push('}');
        }
    }
}

pub fn pass(out: &mut String, p: &Pass) {
    out.push_str("{\"target\":");
    pass_target(out, &p.target);
    out.push_str(",\"clear\":");
    match &p.clear {
        Some(c) => clear(out, c),
        None => out.push_str("null"),
    }
    out.push_str(",\"ops\":[");
    for (i, o) in p.ops.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        op(out, o);
    }
    out.push_str("]}");
}

fn text(out: &mut String, t: &crate::text::TextRun) {
    out.push_str("{\"id\":");
    wid(out, &t.id);
    out.push_str(",\"bytes\":");
    esc_bytes(out, &t.bytes);
    out.push_str(",\"style\":{\"name\":");
    match &t.style.name {
        Some(n) => esc_bytes(out, n),
        None => out.push_str("null"),
    }
    out.push_str(",\"font_no\":");
    match t.style.font_no {
        Some(v) => out.push_str(&v.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"font_bno\":");
    match t.style.font_bno {
        Some(v) => out.push_str(&v.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"color\":");
    out.push_str(&t.style.color.to_string());
    out.push_str(",\"color2\":");
    match t.style.color2 {
        Some(c) => out.push_str(&c.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"size_x\":");
    out.push_str(&t.style.size_x.to_string());
    out.push_str(",\"size_y\":");
    out.push_str(&t.style.size_y.to_string());
    out.push_str(",\"bold\":");
    out.push_str(if t.style.bold { "true" } else { "false" });
    out.push_str(",\"antialias\":");
    out.push_str(if t.style.antialias { "true" } else { "false" });
    out.push_str(",\"draw_mode\":");
    out.push_str(&t.style.draw_mode.to_string());
    out.push_str(",\"gradation\":");
    out.push_str(&t.style.gradation.to_string());
    out.push_str(",\"rotation\":");
    out.push_str(&t.style.rotation.to_string());
    out.push_str(",\"line_spacing\":");
    match t.style.line_spacing {
        Some(v) => out.push_str(&v.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"stroke\":");
    match &t.style.stroke {
        Some(s) => {
            out.push_str("{\"color\":");
            out.push_str(&s.color.to_string());
            out.push_str(",\"width\":");
            out.push_str(&s.width.to_string());
            out.push('}');
        }
        None => out.push_str("null"),
    }
    out.push_str("},\"origin\":");
    match t.origin {
        Some((x, y)) => {
            out.push('[');
            out.push_str(&x.to_string());
            out.push(',');
            out.push_str(&y.to_string());
            out.push(']');
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"atlas\":");
    match &t.atlas {
        Some(a) => {
            out.push_str("{\"key\":");
            esc_bytes(out, &a.key);
            out.push_str(",\"version\":");
            out.push_str(&a.version.to_string());
            out.push('}');
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"frame\":");
    out.push_str(&t.frame.to_string());
    out.push_str(",\"flags\":");
    match &t.flags {
        Some(f) => esc_bytes(out, f),
        None => out.push_str("null"),
    }
    out.push_str(",\"reveal\":");
    match t.reveal {
        Some(n) => out.push_str(&n.to_string()),
        None => out.push_str("null"),
    }
    out.push('}');
}

/// 文本排版/`CGACT[TEXT]` 绘制项。空时不输出,保持无文本帧的 golden 稳定。
fn text_frame_extras(out: &mut String, f: &crate::frame::CompositorFrame) {
    if let Some(l) = &f.text_layout {
        out.push_str(",\"text_layout\":{\"target\":");
        match &l.target {
            Some(t) => wid(out, t),
            None => out.push_str("null"),
        }
        out.push_str(",\"origin\":[");
        out.push_str(&l.origin.0.to_string());
        out.push(',');
        out.push_str(&l.origin.1.to_string());
        out.push_str("],\"style\":{\"color\":");
        out.push_str(&l.style.color.to_string());
        out.push_str(",\"size_x\":");
        out.push_str(&l.style.size_x.to_string());
        out.push_str(",\"size_y\":");
        out.push_str(&l.style.size_y.to_string());
        out.push_str(",\"bold\":");
        out.push_str(if l.style.bold { "true" } else { "false" });
        out.push_str("},\"line_height\":");
        out.push_str(&l.line_height.to_string());
        out.push_str(",\"chars\":");
        out.push_str(&l.chars.to_string());
        out.push_str(",\"reveal\":");
        match l.reveal {
            Some(n) => out.push_str(&n.to_string()),
            None => out.push_str("null"),
        }
        out.push_str(",\"flags_source\":\"text_flag\",\"glyphs\":[");
        for (i, g) in l.glyphs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"bytes\":");
            esc_bytes(out, &g.bytes);
            out.push_str(",\"full\":");
            out.push_str(if g.full { "true" } else { "false" });
            out.push_str(",\"x\":");
            out.push_str(&g.x.to_string());
            out.push_str(",\"y\":");
            out.push_str(&g.y.to_string());
            out.push('}');
        }
        out.push_str("]}");
    }
    if !f.text_draws.is_empty() {
        out.push_str(",\"text_draws\":[");
        for (i, d) in f.text_draws.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"target\":");
            wid(out, &d.target);
            out.push_str(",\"bytes\":");
            esc_bytes(out, &d.bytes);
            out.push_str(",\"x\":");
            out.push_str(&d.x.to_string());
            out.push_str(",\"y\":");
            out.push_str(&d.y.to_string());
            out.push_str(",\"set\":");
            out.push_str(&d.set.to_string());
            out.push_str(",\"bno\":");
            match d.bno {
                Some(v) => out.push_str(&v.to_string()),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        out.push(']');
    }
}

fn diagnostics(out: &mut String, ds: &[crate::error::FrameDiag]) {
    out.push('[');
    for (i, d) in ds.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":");
        esc_str(out, &d.code);
        out.push_str(",\"severity\":");
        esc_str(out, d.severity);
        out.push_str(",\"command\":");
        esc_str(out, &d.command);
        out.push_str(",\"detail\":");
        esc_str(out, &d.detail);
        out.push_str(",\"count\":");
        out.push_str(&d.count.to_string());
        out.push('}');
    }
    out.push(']');
}

fn window_state(out: &mut String, w: &crate::frame::WindowState) {
    out.push_str("{\"number\":");
    out.push_str(&w.number.to_string());
    out.push_str(",\"layer_set\":");
    match &w.layer_set {
        Some(id) => wid(out, id),
        None => out.push_str("null"),
    }
    out.push_str(",\"visible\":");
    out.push_str(if w.visible { "true" } else { "false" });
    out.push_str(",\"alpha\":");
    out.push_str(&w.alpha.to_string());
    out.push_str(",\"x\":");
    out.push_str(&w.x.to_string());
    out.push_str(",\"y\":");
    out.push_str(&w.y.to_string());
    out.push_str(",\"size\":");
    match w.size {
        Some(s) => size(out, s),
        None => out.push_str("null"),
    }
    out.push_str(",\"z\":");
    out.push_str(&w.z.to_string());
    out.push('}');
}

fn header(out: &mut String, f: &CompositorFrame) {
    out.push_str("{\"frame\":");
    out.push_str(&f.frame.to_string());
    out.push_str(",\"viewport\":");
    viewport(out, f.viewport);
    out.push_str(",\"dx_mode\":");
    out.push_str(&f.dx_mode.to_string());
    out.push_str(",\"clear\":");
    clear(out, &f.clear);
    out.push_str(",\"color_space\":");
    esc_str(out, f.color_space.as_str());
    out.push_str(",\"filter\":");
    filter(out, &f.filter);
    out.push_str(",\"image_probe_order\":[");
    for (i, fmt) in f.image_probe_order.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        image_format(out, *fmt);
    }
    out.push_str("],\"windows\":[");
    for (i, w) in f.windows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        window_state(out, w);
    }
    out.push(']');
}

/// 完整契约 JSON(单行)。
pub fn to_json(f: &CompositorFrame) -> String {
    let mut out = String::new();
    header(&mut out, f);
    out.push_str(",\"layers\":[");
    for (i, l) in f.layers.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        layer(&mut out, l);
    }
    out.push_str("],\"passes\":[");
    for (i, p) in f.passes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        pass(&mut out, p);
    }
    out.push_str("],\"text\":[");
    for (i, t) in f.text.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        text(&mut out, t);
    }
    text_frame_extras(&mut out, f);
    out.push_str("],\"diagnostics\":");
    diagnostics(&mut out, &f.diagnostics);
    out.push('}');
    out
}

/// 显示列表 JSON(单行;省略完整图层快照,用于 golden)。
pub fn to_display_json(f: &CompositorFrame) -> String {
    let mut out = String::new();
    header(&mut out, f);
    out.push_str(",\"layer_count\":");
    out.push_str(&f.layers.len().to_string());
    out.push_str(",\"passes\":[");
    for (i, p) in f.passes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        pass(&mut out, p);
    }
    out.push_str("],\"text\":[");
    for (i, t) in f.text.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        text(&mut out, t);
    }
    text_frame_extras(&mut out, f);
    out.push_str("],\"diagnostics\":");
    diagnostics(&mut out, &f.diagnostics);
    out.push('}');
    out
}

/// JSONL(单行 + 换行;多个 frame 可直接拼接)。
pub fn to_jsonl(frames: &[CompositorFrame]) -> String {
    let mut out = String::new();
    for f in frames {
        out.push_str(&to_json(f));
        out.push('\n');
    }
    out
}

/// 混合模式状态表 JSON(后端初始化用;不随每帧重复)。
pub fn blend_table_json() -> String {
    let mut out = String::new();
    out.push('[');
    for (i, m) in BlendMode::ALL.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":");
        out.push_str(&m.code().to_string());
        out.push_str(",\"name\":");
        esc_str(&mut out, m.name());
        out.push_str(",\"confidence\":");
        esc_str(&mut out, m.confidence().as_str());
        out.push_str(",\"fixed\":");
        match m.fixed_state() {
            Some(s) => fixed_state(&mut out, &s),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push(']');
    out
}

fn stage_arg(out: &mut String, a: Option<StageArg>) {
    match a {
        Some(a) => {
            out.push_str("{\"code\":");
            out.push_str(&a.d3d_code().to_string());
            out.push_str(",\"name\":");
            esc_str(
                &mut *out,
                match a {
                    StageArg::Diffuse => "diffuse",
                    StageArg::Current => "current",
                    StageArg::Texture => "texture",
                    StageArg::TFactor => "tfactor",
                },
            );
            out.push('}');
        }
        None => out.push_str("null"),
    }
}

fn stage_op(out: &mut String, op: StageOp) {
    out.push_str("{\"code\":");
    out.push_str(&op.d3d_code().to_string());
    out.push_str(",\"name\":");
    esc_str(
        out,
        match op {
            StageOp::Disable => "disable",
            StageOp::SelectArg1 => "select_arg1",
            StageOp::SelectArg2 => "select_arg2",
            StageOp::Modulate => "modulate",
            StageOp::ModulateAlphaAddColor => "modulate_alpha_add_color",
        },
    );
    out.push('}');
}

fn blend_factor(out: &mut String, b: crate::layer::BlendFactor) {
    out.push_str("{\"code\":");
    out.push_str(&b.d3d_code().to_string());
    out.push_str(",\"name\":");
    esc_str(
        out,
        match b {
            crate::layer::BlendFactor::Zero => "zero",
            crate::layer::BlendFactor::One => "one",
            crate::layer::BlendFactor::SrcColor => "src_color",
            crate::layer::BlendFactor::InvSrcColor => "inv_src_color",
            crate::layer::BlendFactor::SrcAlpha => "src_alpha",
            crate::layer::BlendFactor::InvSrcAlpha => "inv_src_alpha",
            crate::layer::BlendFactor::DestColor => "dest_color",
            crate::layer::BlendFactor::InvDestColor => "inv_dest_color",
        },
    );
    out.push('}');
}

fn blend_op(out: &mut String, op: crate::layer::BlendOp) {
    out.push_str("{\"code\":");
    out.push_str(&op.d3d_code().to_string());
    out.push_str(",\"name\":");
    esc_str(
        out,
        match op {
            crate::layer::BlendOp::Add => "add",
            crate::layer::BlendOp::Subtract => "subtract",
            crate::layer::BlendOp::RevSubtract => "rev_subtract",
            crate::layer::BlendOp::Min => "min",
            crate::layer::BlendOp::Max => "max",
        },
    );
    out.push('}');
}

fn fixed_state(out: &mut String, s: &crate::layer::FixedPipelineState) {
    out.push_str("{\"stages\":[");
    for (i, st) in s.stages.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"color_op\":");
        stage_op(out, st.color_op);
        out.push_str(",\"color_arg1\":");
        stage_arg(out, st.color_arg1);
        out.push_str(",\"color_arg2\":");
        stage_arg(out, st.color_arg2);
        out.push_str(",\"alpha_op\":");
        stage_op(out, st.alpha_op);
        out.push_str(",\"alpha_arg1\":");
        stage_arg(out, st.alpha_arg1);
        out.push_str(",\"alpha_arg2\":");
        stage_arg(out, st.alpha_arg2);
        out.push('}');
    }
    out.push_str("],\"texture_factor\":");
    esc_str(
        out,
        match s.texture_factor {
            TextureFactorRule::AlphaGray => "alpha_gray",
            TextureFactorRule::OpaqueWhite => "opaque_white",
        },
    );
    out.push_str(",\"src_blend\":");
    blend_factor(out, s.src_blend);
    out.push_str(",\"dst_blend\":");
    blend_factor(out, s.dst_blend);
    out.push_str(",\"blend_op\":");
    blend_op(out, s.blend_op);
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{CompositorFrame, Viewport};
    use crate::layer::{Layer, LayerId, LayerKind, LayerStatus, TextureRef};

    fn sample() -> CompositorFrame {
        let mut f = CompositorFrame::new(3, Viewport::new(800, 600));
        f.layers.push(Layer {
            id: LayerId::new(b":/W0/ABC".to_vec()),
            seq: 1,
            z: 10,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Image { size: None },
            texture: TextureRef::Asset(b"cg/bg".to_vec()),
            parts: Vec::new(),
            transform: Transform::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            texture_status: LayerStatus::Executable,
            param_status: LayerStatus::Executable,
            extra: Default::default(),
        });
        f
    }

    #[test]
    fn deterministic_two_runs() {
        let a = to_json(&sample());
        let b = to_json(&sample());
        assert_eq!(a, b);
        let c = to_display_json(&sample());
        let d = to_display_json(&sample());
        assert_eq!(c, d);
    }

    #[test]
    fn blend_table_is_stable_and_has_19_modes() {
        let s = blend_table_json();
        assert_eq!(s, blend_table_json());
        assert!(s.matches("\"code\":").count() >= 19);
    }

    #[test]
    fn float_format_is_valid_json_and_keeps_integer_values() {
        assert_eq!(fmt_f64(128.0), "128.0");
        assert_eq!(fmt_f64(0.0), "0.0");
        assert_eq!(fmt_f64(-0.0), "-0.0");
        assert_eq!(fmt_f64(1.5), "1.5");
        // NaN/Inf 不是合法 JSON 数字 ⇒ 显式 null,不写 "NaN"/"Infinity"。
        assert_eq!(fmt_f64(f64::NAN), "null");
        assert_eq!(fmt_f64(f64::INFINITY), "null");
        assert_eq!(fmt_f64(f64::NEG_INFINITY), "null");
        assert_eq!(fmt_f64(1e20), "1e20");
    }
}
