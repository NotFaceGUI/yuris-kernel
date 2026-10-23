//! 宿主侧场景脚本:按**逻辑帧**注入输入与窗口操作。
//!
//! 用途:让 P1 的 7 个症状剧本在真实桌面宿主(winit + wgpu + egui)上
//! **可复现**,而不是靠人工记忆点击坐标。格式与 `cases/*.json` 同形,
//! 额外支持 `pulse` / `action` 字段:
//!
//! ```json
//! {
//!   "name": "symptom-ingame-config",
//!   "frames": 520,
//!   "inputs": [
//!     { "frame": 300, "button": "MOUSE_L", "x": 217, "y": 281 },
//!     { "frame": 440, "button": "MOUSE_L", "x": 475, "y": 583 }
//!   ],
//!   "actions": [
//!     { "frame": 460, "action": "resize", "width": 1024, "height": 768 },
//!     { "frame": 480, "action": "minimize" },
//!     { "frame": 500, "action": "restore" },
//!     { "frame": 510, "action": "focus_lost" },
//!     { "frame": 600, "action": "close" }
//!   ]
//! }
//! ```
//!
//! `pulse` 是滚轮一格(`_MOUSE_U/_MOUSE_D`),不产生抬起沿。
//!
//! 帧号是**引擎逻辑帧**(`Engine::frame()`),不是宿主迭代数:输入在
//! `engine.frame() + 1 >= frame` 时注入,保证该帧的 tick 能看到。
//! 脚本只驱动宿主已有的公开 API(`set_input` / `pulse_input` /
//! `request_window_close` / winit 窗口方法),不碰引擎语义。

use std::path::Path;

use yu_cli::json::{self, Value};

/// 脚本里的一条动作。
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptAction {
    Press {
        button: String,
        x: i64,
        y: i64,
    },
    Release {
        button: String,
    },
    Pulse {
        button: String,
    },
    Resize {
        width: u32,
        height: u32,
    },
    Minimize,
    Restore,
    /// 宿主模拟失焦路径(`WindowEvent::Focused(false)` 的行为:释放所有键)。
    FocusLost,
    /// 走窗口关闭请求路径(与 `WindowEvent::CloseRequested` 同一处理)。
    Close,
}

/// 带帧号的动作。
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledAction {
    pub frame: u64,
    pub action: ScriptAction,
}

/// 场景脚本执行器。
#[derive(Debug)]
pub struct ScriptRunner {
    pub name: String,
    pub frames: Option<u32>,
    actions: Vec<ScheduledAction>,
    cursor: usize,
}

impl ScriptRunner {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("读脚本 {} 失败: {e}", path.display()))?;
        let value =
            json::parse(&text).map_err(|e| format!("解析脚本 {} 失败: {e}", path.display()))?;
        Self::from_value(&value, path)
    }

    fn from_value(value: &Value, path: &Path) -> Result<Self, String> {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "script".to_string());
        let frames = value
            .get("frames")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let mut actions = Vec::new();
        if let Some(list) = value.get("inputs").and_then(Value::as_array) {
            for item in list {
                let frame = item
                    .get("frame")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "inputs[].frame 缺失".to_string())?;
                let raw = item
                    .get("button")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "inputs[].button 缺失".to_string())?;
                let button = if raw.eq_ignore_ascii_case("CLICK") {
                    "MOUSE_L".to_string()
                } else {
                    raw.to_string()
                };
                let x = item.get("x").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                let y = item.get("y").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                actions.push(ScheduledAction {
                    frame: frame.max(1),
                    action: ScriptAction::Press {
                        button: button.clone(),
                        x,
                        y,
                    },
                });
                actions.push(ScheduledAction {
                    frame: frame.max(1) + 1,
                    action: ScriptAction::Release { button },
                });
            }
        }
        if let Some(list) = value.get("pulses").and_then(Value::as_array) {
            for item in list {
                let frame = item
                    .get("frame")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "pulses[].frame 缺失".to_string())?;
                let button = item
                    .get("button")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "pulses[].button 缺失".to_string())?;
                actions.push(ScheduledAction {
                    frame: frame.max(1),
                    action: ScriptAction::Pulse {
                        button: button.to_string(),
                    },
                });
            }
        }
        if let Some(list) = value.get("actions").and_then(Value::as_array) {
            for item in list {
                let frame = item
                    .get("frame")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "actions[].frame 缺失".to_string())?;
                let kind = item
                    .get("action")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "actions[].action 缺失".to_string())?;
                let action = match kind {
                    "resize" => {
                        let width = item
                            .get("width")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "actions[resize].width 缺失".to_string())?
                            as u32;
                        let height = item
                            .get("height")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "actions[resize].height 缺失".to_string())?
                            as u32;
                        ScriptAction::Resize { width, height }
                    }
                    "minimize" => ScriptAction::Minimize,
                    "restore" => ScriptAction::Restore,
                    "focus_lost" => ScriptAction::FocusLost,
                    "close" => ScriptAction::Close,
                    other => return Err(format!("未知 action `{other}`")),
                };
                actions.push(ScheduledAction {
                    frame: frame.max(1),
                    action,
                });
            }
        }
        actions.sort_by_key(|action| action.frame);
        Ok(Self {
            name,
            frames,
            actions,
            cursor: 0,
        })
    }

    /// 取出所有“该在本次 tick 之前生效”的动作(按脚本顺序)。
    ///
    /// `engine_frame` 是**已完成的**逻辑帧数;`+1` 是下一次 tick 的帧号。
    pub fn due(&mut self, engine_frame: u64) -> Vec<ScheduledAction> {
        let mut out = Vec::new();
        while self.cursor < self.actions.len() {
            if self.actions[self.cursor].frame <= engine_frame.saturating_add(1) {
                out.push(self.actions[self.cursor].clone());
                self.cursor += 1;
            } else {
                break;
            }
        }
        out
    }

    /// 脚本是否已经全部触发。
    pub fn exhausted(&self) -> bool {
        self.cursor >= self.actions.len()
    }

    pub fn remaining(&self) -> usize {
        self.actions.len().saturating_sub(self.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ScriptRunner {
        let value = json::parse(text).unwrap();
        ScriptRunner::from_value(&value, Path::new("inline.json")).unwrap()
    }

    #[test]
    fn schedule_respects_engine_frame_boundary() {
        let mut runner =
            parse(r#"{"name":"t","inputs":[{"frame":300,"button":"MOUSE_L","x":10,"y":20}]}"#);
        assert!(runner.due(297).is_empty(), "298 < 300:太早");
        let due = runner.due(299);
        assert_eq!(due.len(), 1, "engine_frame+1 == 300:按下");
        assert_eq!(
            due[0].action,
            ScriptAction::Press {
                button: "MOUSE_L".to_string(),
                x: 10,
                y: 20
            }
        );
        assert!(runner.due(299).is_empty(), "同一动作不重复");
        let due = runner.due(300);
        assert_eq!(
            due[0].action,
            ScriptAction::Release {
                button: "MOUSE_L".to_string()
            },
            "抬起在下一逻辑帧"
        );
        assert!(runner.exhausted());
    }

    #[test]
    fn pulses_and_actions_are_scheduled_by_frame() {
        let mut runner = parse(
            r#"{
              "name":"t",
              "pulses":[{"frame":10,"button":"MOUSE_D"}],
              "actions":[
                {"frame":20,"action":"resize","width":1024,"height":768},
                {"frame":21,"action":"minimize"},
                {"frame":22,"action":"restore"},
                {"frame":23,"action":"focus_lost"},
                {"frame":24,"action":"close"}
              ]
            }"#,
        );
        assert_eq!(runner.due(8).len(), 0, "10 > 8+1");
        assert_eq!(
            runner.due(9)[0].action,
            ScriptAction::Pulse {
                button: "MOUSE_D".to_string()
            }
        );
        let due = runner.due(19);
        assert_eq!(
            due[0].action,
            ScriptAction::Resize {
                width: 1024,
                height: 768
            }
        );
        let all: Vec<ScriptAction> = runner.due(23).into_iter().map(|a| a.action).collect();
        assert_eq!(
            all,
            vec![
                ScriptAction::Minimize,
                ScriptAction::Restore,
                ScriptAction::FocusLost,
                ScriptAction::Close
            ]
        );
    }

    #[test]
    fn unknown_action_is_rejected_not_ignored() {
        let value =
            json::parse(r#"{"name":"t","actions":[{"frame":1,"action":"explode"}]}"#).unwrap();
        let error = ScriptRunner::from_value(&value, Path::new("x.json")).unwrap_err();
        assert!(error.contains("explode"), "{error}");
    }

    #[test]
    fn frames_field_is_exposed_for_host_loop_limit() {
        let runner = parse(r#"{"name":"t","frames":420}"#);
        assert_eq!(runner.frames, Some(420));
    }
}
