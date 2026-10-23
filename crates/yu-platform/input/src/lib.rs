//! `yu-platform-input` · 宿主侧平台输入翻译(不依赖 winit/浏览器)。
//!
//! # 边界
//!
//! ```text
//! 平台宿主(winit / DOM / Android)
//!   --原始事件(物理键码 / 指针按钮 / 滚轮增量)-->
//! yu-platform-input(本 crate:唯一的键位/光标策略映射)
//!   --引擎按钮名 / 光标形状-->
//! yu-cli::Engine(时间线桥) → yu-rt::input(内核计数器/门控)
//! ```
//!
//! 内核 `yu-rt::input` 的平台无关语义(带符号帧计数器、`INPUT`/`MOUSE`
//! 门控、`_INP`)不在这里;本 crate 只回答“平台事件对应哪个引擎按钮”以及
//! “`MOUSE[MODE/TIME/CURSOR]` 在当前时刻应显示什么光标”。
//!
//! 键名以 `yu-rt` 的按钮码 ABI 表为准(`button_code`);表里没有的键显式
//! 返回 `None`,不猜。

#![forbid(unsafe_code)]

use yu_rt::input::InputSettings;

/// 指针按钮(平台无关)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
    /// 侧键/未知键:引擎 ABI 没有对应 `_MOUSE_*`,显式忽略。
    Other(u16),
}

impl PointerButton {
    /// DOM `PointerEvent.button` / winit `MouseButton` 的常用编号:
    /// 0=左 1=中 2=右。
    pub fn from_index(index: i64) -> Option<Self> {
        match index {
            0 => Some(Self::Left),
            1 => Some(Self::Middle),
            2 => Some(Self::Right),
            3 => Some(Self::Other(3)),
            4 => Some(Self::Other(4)),
            _ => None,
        }
    }

    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::Left => Some("MOUSE_L"),
            Self::Middle => Some("MOUSE_M"),
            Self::Right => Some("MOUSE_R"),
            Self::Other(_) => None,
        }
    }
}

/// 会唤醒自动隐藏光标的鼠标输入类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseActivity {
    Move,
    Click,
    Wheel,
}

/// `MOUSE[MOVE/CLICK/WHEEL]` 是否允许该输入唤醒光标。
pub fn mouse_activity_reactive(settings: &InputSettings, activity: MouseActivity) -> bool {
    match activity {
        MouseActivity::Move => settings.move_reactive,
        MouseActivity::Click => settings.click_reactive,
        MouseActivity::Wheel => settings.wheel_reactive,
    }
}

/// 滚轮方向 → 引擎按钮(`_MOUSE_U/_MOUSE_D`,ycd 码 5/6)。
pub fn wheel_button(delta_y: f64) -> Option<&'static str> {
    if delta_y > 0.0 {
        Some("MOUSE_U")
    } else if delta_y < 0.0 {
        Some("MOUSE_D")
    } else {
        None
    }
}

/// 物理键码 → 引擎按钮名。
///
/// 同时接受:
/// - winit `KeyCode` 的 debug 名(`KeyA` / `Digit1` / `Numpad1` / `Enter`);
/// - DOM `KeyboardEvent.code`(`KeyA` / `Digit1` / `Numpad1` / `Enter`)。
///
/// 两者命名一致;差异只在 winit 的 `NumpadAdd` 等后缀大小写,这里统一
/// 转成 ABI 表的大写形式(`NUMADD`)。未知键返回 `None`(由调用方显式丢弃)。
pub fn key_button(physical_code: &str) -> Option<String> {
    let suffix = if let Some(rest) = physical_code.strip_prefix("Key") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_uppercase() {
            rest.to_string()
        } else {
            return None;
        }
    } else if let Some(rest) = physical_code.strip_prefix("Digit") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
            rest.to_string()
        } else {
            return None;
        }
    } else if let Some(rest) = physical_code.strip_prefix("Numpad") {
        format!("NUM{}", rest.to_ascii_uppercase())
    } else if is_function_key(physical_code) {
        physical_code.to_string()
    } else {
        let named = match physical_code {
            "Enter" => "ENTER",
            "Space" => "SPACE",
            "Escape" => "ESC",
            "Tab" => "TAB",
            "Backspace" => "BS",
            "ShiftLeft" | "ShiftRight" => "SHIFT",
            "ControlLeft" | "ControlRight" => "CTRL",
            "AltLeft" | "AltRight" => "ALT",
            "ArrowUp" => "UP",
            "ArrowDown" => "DOWN",
            "ArrowLeft" => "LEFT",
            "ArrowRight" => "RIGHT",
            "PageUp" => "PAGEUP",
            "PageDown" => "PAGEDOWN",
            "Home" => "HOME",
            "End" => "END",
            "Insert" => "INSERT",
            "Delete" => "DELETE",
            "NumLock" => "NUMLOCK",
            "ScrollLock" => "SCROLL",
            "Help" => "HELP",
            _ => return None,
        };
        named.to_string()
    };
    // ABI 表是“引擎认哪些键”的唯一事实来源;不猜未声明的键。
    yu_rt::input::button_code(&suffix)?;
    Some(format!("KEY_{suffix}"))
}

fn is_function_key(code: &str) -> bool {
    let Some(rest) = code.strip_prefix('F') else {
        return false;
    };
    let Ok(n) = rest.parse::<u8>() else {
        return false;
    };
    (1..=24).contains(&n)
}

/// 平台无关的光标形状;各宿主映射到自己的 API(winit `CursorIcon` / CSS)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Default,
    Text,
    Wait,
    Crosshair,
    VerticalArrows,
    DiagonalDown,
    DiagonalUp,
    HorizontalArrows,
    FourWay,
    NotAllowed,
    Hand,
    Progress,
    Help,
}

/// `MOUSE[CURSOR=0..13]`(手册 `system/mouse.html`)→ 中性形状。
pub fn cursor_shape(index: i64) -> CursorShape {
    match index {
        1 => CursorShape::Text,
        2 => CursorShape::Wait,
        3 => CursorShape::Crosshair,
        4 => CursorShape::VerticalArrows,
        5 => CursorShape::DiagonalDown,
        6 => CursorShape::DiagonalUp,
        7 => CursorShape::HorizontalArrows,
        8 => CursorShape::VerticalArrows,
        9 => CursorShape::FourWay,
        10 => CursorShape::NotAllowed,
        11 => CursorShape::Hand,
        12 => CursorShape::Progress,
        13 => CursorShape::Help,
        _ => CursorShape::Default,
    }
}

/// 宿主时钟(毫秒;由各宿主从单调时钟换算)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorClock {
    pub now_ms: u64,
    pub last_keyboard_ms: u64,
    pub last_mouse_ms: u64,
}

/// 当前是否显示光标。
///
/// - MODE 0 常隐 / 2 常显;
/// - MODE 1:键盘输入后 `TIME` 毫秒隐藏;鼠标输入(按 MOVE/CLICK/WHEEL
///   开关)立即重新显示。手册 `system/mouse.html` 的“自動的に消える”。
pub fn cursor_visible(settings: &InputSettings, clock: CursorClock) -> bool {
    match settings.mouse_mode {
        0 => false,
        2 => true,
        _ => {
            if clock.last_keyboard_ms > clock.last_mouse_ms {
                clock.now_ms.saturating_sub(clock.last_keyboard_ms)
                    < settings.mouse_time_ms.max(0) as u64
            } else {
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_codes_match_abi() {
        assert_eq!(key_button("KeyA").as_deref(), Some("KEY_A"));
        assert_eq!(key_button("KeyZ").as_deref(), Some("KEY_Z"));
        assert_eq!(key_button("Digit1").as_deref(), Some("KEY_1"));
        assert_eq!(key_button("Numpad1").as_deref(), Some("KEY_NUM1"));
        assert_eq!(key_button("NumpadAdd").as_deref(), Some("KEY_NUMADD"));
        assert_eq!(key_button("F1").as_deref(), Some("KEY_F1"));
        assert_eq!(key_button("F24").as_deref(), Some("KEY_F24"));
        assert_eq!(key_button("Enter").as_deref(), Some("KEY_ENTER"));
        assert_eq!(key_button("Escape").as_deref(), Some("KEY_ESC"));
        assert_eq!(key_button("ArrowUp").as_deref(), Some("KEY_UP"));
        assert_eq!(key_button("ControlLeft").as_deref(), Some("KEY_CTRL"));
        assert_eq!(key_button("F25"), None);
        assert_eq!(key_button("KeyAA"), None);
        assert_eq!(key_button("BrowserBack"), None);
    }

    #[test]
    fn pointer_and_wheel_map_to_engine_buttons() {
        assert_eq!(
            PointerButton::from_index(0).unwrap().name(),
            Some("MOUSE_L")
        );
        assert_eq!(
            PointerButton::from_index(1).unwrap().name(),
            Some("MOUSE_M")
        );
        assert_eq!(
            PointerButton::from_index(2).unwrap().name(),
            Some("MOUSE_R")
        );
        assert_eq!(PointerButton::from_index(3).unwrap().name(), None);
        assert_eq!(wheel_button(120.0), Some("MOUSE_U"));
        assert_eq!(wheel_button(-120.0), Some("MOUSE_D"));
        assert_eq!(wheel_button(0.0), None);
    }

    #[test]
    fn cursor_mode_policy_matches_manual() {
        let mut s = InputSettings {
            mouse_mode: 0,
            ..Default::default()
        };
        assert!(!cursor_visible(&s, CursorClock::default()), "MODE 0 常隐");
        s.mouse_mode = 2;
        assert!(cursor_visible(&s, CursorClock::default()), "MODE 2 常显");
        s.mouse_mode = 1;
        s.mouse_time_ms = 2500;
        let clock = CursorClock {
            now_ms: 10_000,
            last_keyboard_ms: 9_000,
            last_mouse_ms: 5_000,
        };
        assert!(cursor_visible(&s, clock), "键盘后 1s < TIME 仍显示");
        let clock = CursorClock {
            now_ms: 12_000,
            last_keyboard_ms: 9_000,
            last_mouse_ms: 5_000,
        };
        assert!(!cursor_visible(&s, clock), "键盘后超过 TIME 隐藏");
        let clock = CursorClock {
            now_ms: 12_000,
            last_keyboard_ms: 9_000,
            last_mouse_ms: 11_000,
        };
        assert!(cursor_visible(&s, clock), "鼠标输入重新显示");
    }
}
