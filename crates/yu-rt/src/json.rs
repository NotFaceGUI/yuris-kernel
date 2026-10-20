//! 事件流的确定性 JSON 序列化(golden 用)。
//!
//! 不引入 `serde`:字段顺序固定、数值格式固定、字节串按 `\u00XX` 转义,
//! 保证**同一事件流逐字节可复现**(A3),便于 golden diff 与跨平台比对。

use crate::event::{ArgValue, Domain, JumpKind, VmEvent};
use crate::value::Value;

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

/// 浮点序列化:负数零、NaN、Inf 与整数值都给稳定表示。
pub fn fmt_f64(v: f64) -> String {
    if v.is_nan() {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else if v == 0.0 {
        "0.0".to_string()
    } else if v == v.trunc() && v.abs() < 1e16 {
        format!("{v:.1e}")
    } else {
        format!("{v:?}")
    }
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Int(i) => {
            out.push_str("{\"t\":\"int\",\"v\":");
            out.push_str(&i.to_string());
            out.push('}');
        }
        Value::Float(f) => {
            out.push_str("{\"t\":\"flt\",\"v\":");
            out.push_str(&fmt_f64(*f));
            out.push('}');
        }
        Value::Str(b) => {
            out.push_str("{\"t\":\"str\",\"v\":");
            esc_bytes(out, b);
            out.push('}');
        }
    }
}

fn write_args(out: &mut String, args: &[ArgValue]) {
    out.push('[');
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"slot\":");
        out.push_str(&a.slot.to_string());
        out.push_str(",\"value\":");
        write_value(out, &a.value);
        out.push('}');
    }
    out.push(']');
}

/// 一条事件 → 单行 JSON(稳定字段顺序)。
pub fn event_line(ev: &VmEvent) -> String {
    let mut o = String::new();
    o.push_str("{\"frame\":");
    o.push_str(&ev.frame().to_string());
    o.push_str(",\"sub\":");
    esc_str(&mut o, ev.sub());
    o.push_str(",\"kind\":");
    esc_str(&mut o, ev.kind());
    match ev {
        VmEvent::FrameStart { .. } | VmEvent::FrameEnd { .. } => {}
        VmEvent::Command {
            script,
            pc,
            name,
            args,
            ..
        } => {
            o.push_str(",\"script\":");
            o.push_str(&script.to_string());
            o.push_str(",\"pc\":");
            o.push_str(&pc.to_string());
            o.push_str(",\"name\":");
            esc_str(&mut o, name);
            o.push_str(",\"args\":");
            write_args(&mut o, args);
        }
        VmEvent::Jump {
            script,
            pc,
            to_pc,
            to_script,
            kind,
            ..
        } => {
            o.push_str(",\"script\":");
            o.push_str(&script.to_string());
            o.push_str(",\"pc\":");
            o.push_str(&pc.to_string());
            o.push_str(",\"to_script\":");
            o.push_str(&to_script.to_string());
            o.push_str(",\"to_pc\":");
            o.push_str(&to_pc.to_string());
            o.push_str(",\"jump\":");
            esc_str(&mut o, kind_str(*kind));
        }
        VmEvent::TaskSpawn {
            id,
            script,
            pc,
            enabled,
            main,
            ..
        } => {
            o.push_str(",\"id\":");
            esc_str(&mut o, id);
            o.push_str(",\"script\":");
            o.push_str(&script.to_string());
            o.push_str(",\"pc\":");
            o.push_str(&pc.to_string());
            o.push_str(",\"enabled\":");
            o.push_str(if *enabled { "true" } else { "false" });
            o.push_str(",\"main\":");
            o.push_str(if *main { "true" } else { "false" });
        }
        VmEvent::TaskFinish { id, .. } => {
            o.push_str(",\"id\":");
            esc_str(&mut o, id);
        }
        VmEvent::Wait {
            task,
            frames,
            time_ms,
            ..
        } => {
            o.push_str(",\"task\":");
            esc_str(&mut o, task);
            o.push_str(",\"frames\":");
            match frames {
                Some(n) => o.push_str(&n.to_string()),
                None => o.push_str("null"),
            }
            o.push_str(",\"time_ms\":");
            match time_ms {
                Some(n) => o.push_str(&n.to_string()),
                None => o.push_str("null"),
            }
        }
        VmEvent::Text { id, bytes, .. } => {
            o.push_str(",\"id\":");
            esc_str(&mut o, id);
            o.push_str(",\"bytes\":");
            esc_bytes(&mut o, bytes);
        }
        VmEvent::ReturnCode { line, .. } => {
            o.push_str(",\"line\":");
            o.push_str(&line.to_string());
        }
        VmEvent::Fps { fps, skip, .. } => {
            o.push_str(",\"fps\":");
            o.push_str(&fps.to_string());
            o.push_str(",\"skip\":");
            o.push_str(if *skip { "true" } else { "false" });
        }
        VmEvent::SystemCall { name, args, .. } => {
            o.push_str(",\"name\":");
            esc_str(&mut o, name);
            o.push_str(",\"args\":");
            write_args(&mut o, args);
        }
        VmEvent::DomainCall {
            domain, name, args, ..
        } => {
            o.push_str(",\"domain\":");
            esc_str(&mut o, domain_str(*domain));
            o.push_str(",\"name\":");
            esc_str(&mut o, name);
            o.push_str(",\"args\":");
            write_args(&mut o, args);
        }
        VmEvent::End { restart, .. } => {
            o.push_str(",\"restart\":");
            o.push_str(if *restart { "true" } else { "false" });
        }
        VmEvent::Diag { code, detail, .. } => {
            o.push_str(",\"code\":");
            esc_str(&mut o, code);
            o.push_str(",\"detail\":");
            esc_str(&mut o, detail);
        }
    }
    o.push('}');
    o
}

fn kind_str(k: JumpKind) -> &'static str {
    k.as_str()
}

fn domain_str(d: Domain) -> &'static str {
    d.as_str()
}

/// 事件流 → 文本(每行一条,末尾换行;可直接与 golden 文件逐字节比对)。
pub fn to_jsonl(events: &[VmEvent]) -> String {
    let mut out = String::new();
    for ev in events {
        out.push_str(&event_line(ev));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ArgValue, VmEvent};
    use crate::value::Value;

    #[test]
    fn stable_line_for_command() {
        let ev = VmEvent::Command {
            frame: 3,
            script: 7,
            pc: 12,
            name: "LET".to_string(),
            args: vec![
                ArgValue::new(0, Value::Int(1)),
                ArgValue::new(1, Value::Str(b"abc".to_vec())),
            ],
        };
        assert_eq!(
            event_line(&ev),
            "{\"frame\":3,\"sub\":\"vm\",\"kind\":\"command\",\"script\":7,\"pc\":12,\
             \"name\":\"LET\",\"args\":[{\"slot\":0,\"value\":{\"t\":\"int\",\"v\":1}},\
             {\"slot\":1,\"value\":{\"t\":\"str\",\"v\":\"abc\"}}]}"
        );
    }

    #[test]
    fn jsonl_is_newline_terminated() {
        let s = to_jsonl(&[VmEvent::End {
            frame: 0,
            restart: false,
        }]);
        assert!(s.ends_with('\n'));
        assert_eq!(s.matches('\n').count(), 1);
    }
}
