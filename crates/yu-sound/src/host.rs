//! `SoundHost`：把 `SoundState` 接成 `yu-rt` 的同步声音宿主。
//!
//! 与 [`yu_rt::event::GraphicsHost`] 同契约：事件在**产生时**同步进入状态机，
//! `SOUNDINFO` 在脚本执行期查询同一帧的最新状态；调用方**不得**再把同一批
//! 事件 drain 后喂给该状态。

use std::sync::{Arc, Mutex};

use yu_log::Diag;
use yu_rt::event::{ArgValue, Domain, SoundHost as SoundHostTrait, VmEvent};
use yu_rt::tables::YscmTable;
use yu_rt::value::Value;

use crate::error::{self, code};
use crate::state::{QueryField, SoundCommand, SoundState};

/// `SoundState` 的共享句柄。
#[derive(Clone)]
pub struct SoundHost {
    state: Arc<Mutex<SoundState>>,
    yscm: YscmTable,
}

impl SoundHost {
    pub fn new(state: Arc<Mutex<SoundState>>, yscm: YscmTable) -> Self {
        Self { state, yscm }
    }

    pub fn state(&self) -> Arc<Mutex<SoundState>> {
        self.state.clone()
    }

    fn args_of<'a>(&'a self, ev: &'a VmEvent) -> Vec<(String, &'a Value)> {
        ev.domain_args(&self.yscm)
            .map(|args| {
                args.into_iter()
                    .map(|a| (a.name.into_owned(), a.value))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl SoundHostTrait for SoundHost {
    fn apply_event(&mut self, ev: &VmEvent) {
        match ev {
            VmEvent::FrameStart { frame } => {
                self.state.lock().unwrap().set_frame(*frame);
            }
            VmEvent::DomainCall {
                domain: Domain::Sound,
                name,
                ..
            } => {
                let args = self.args_of(ev);
                match name.as_str() {
                    "SOUND" => {
                        let mut cmd = SoundCommand::default();
                        for (pname, value) in &args {
                            match pname.as_str() {
                                "ID" => match value {
                                    Value::Str(s) => cmd.id = s.clone(),
                                    _ => arg_type(&self.state, "ID", value),
                                },
                                "FILE" => {
                                    // 官方建节点规则按**参数槽存在性**判定
                                    // (`0x684162`,0x44ef57);类型不符只影响
                                    // 装载,不影响"这条命令带 FILE"。
                                    cmd.file_present = true;
                                    match value {
                                        Value::Str(s) => cmd.file = Some(s.clone()),
                                        _ => arg_type(&self.state, "FILE", value),
                                    }
                                }
                                "E" => cmd.e = number(value),
                                "PLAY" => cmd.play = number(value),
                                "PAUSE" => cmd.pause = number(value),
                                "FADE" => cmd.fade = number(value),
                                "START" => cmd.start = number(value),
                                "RESTART" => cmd.restart = number(value),
                                "END" => cmd.end = number(value),
                                "LOOP" => cmd.loop_ = number(value),
                                "VOLUME" => cmd.volume = number(value),
                                "PAN" => cmd.pan = number(value),
                                "PITCH" => cmd.pitch = number(value),
                                // `SPEED`（0.495 yscm 槽 13）官方 SOUND 处理体
                                // （`0x44e1e8`）只读 `0x684160..0x68416c`，
                                // 从不读 `0x68416d` ⇒ 静默忽略（unsupported-
                                // by-official，DIV-SND-001），落到 `_ => {}`。
                                _ => {}
                            }
                        }
                        self.state.lock().unwrap().apply_sound(&cmd);
                    }
                    "SOUNDEND" => {
                        let id = args
                            .iter()
                            .find(|(n, _)| n == "ID")
                            .and_then(|(_, v)| match v {
                                Value::Str(s) => Some(s.clone()),
                                _ => None,
                            });
                        if let Some(id) = id {
                            // 官方 SOUNDEND 不删节点，只入队 type 0xe 释放
                            // 声音对象；带 `/` 结尾的 ID 递归停止子树。
                            self.state.lock().unwrap().apply_soundend(&id);
                        }
                    }
                    // 查询由 `query` 处理；MIDI/CDDA/未知命令由 yu-rt 的
                    // 通用域路径挂号（宿主不重复报）。
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn advance(&mut self, now_ms: u32) {
        self.state.lock().unwrap().advance(now_ms);
    }

    fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, Diag> {
        if command != "SOUNDINFO" {
            // yu-rt 只把 SOUNDINFO 路由到本宿主(handlers.rs 的
            // `HandlerKind::Domain` 分支);此臂是防御性不变量。
            return Err(error::recoverable(
                code::COMMAND_UNIMPLEMENTED,
                format!("SoundHost 不支持查询 {command}"),
            ));
        }
        // 参数名来自工程 yscm（槽号不跨版本稳定）。
        let decl = self
            .yscm
            .index_of("SOUNDINFO")
            .and_then(|i| self.yscm.command(i as u8));
        let name_of = |slot: u8| -> Option<&str> {
            decl.and_then(|c| c.params.get(usize::from(slot)).map(|p| p.name.as_str()))
        };
        let mut id: Option<Vec<u8>> = None;
        let mut fields: Vec<QueryField> = Vec::new();
        for a in args {
            let Some(name) = name_of(a.slot) else {
                continue;
            };
            match name {
                "ID" => {
                    if let Value::Str(s) = &a.value {
                        id = Some(s.clone());
                    }
                }
                "LET" => {}
                other => {
                    if let Some(f) = QueryField::from_name(other) {
                        let on = a.value.as_f64().map(|n| n != 0.0).unwrap_or(false);
                        if on {
                            fields.push(f);
                        }
                    }
                }
            }
        }
        let Some(id) = id else {
            return Err(error::recoverable(code::ARG_MISSING, "SOUNDINFO 缺 ID"));
        };
        self.state.lock().unwrap().query(&id, &fields)
    }
}

fn number(v: &Value) -> Option<i64> {
    v.as_f64().map(|n| n.round() as i64)
}

fn arg_type(state: &Arc<Mutex<SoundState>>, name: &str, v: &Value) {
    state.lock().unwrap().report_arg_type(name, v);
}
