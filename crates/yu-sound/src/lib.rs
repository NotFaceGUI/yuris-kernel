//! `yu-sound` · 声音域宿主（SOUND / SOUNDINFO / SOUNDEND）。
//!
//! 净室实现：只依据官方手册、ERIS 语料与 0.495 `yu-ris.exe` 的 REA 事实；
//! 不复制旧线代码。证据等级与逐条出处见 `docs/sound-ledger.md`。
//!
//! # 证据等级
//!
//! - **Confirmed**：手册原文 + 语料/REA 双重佐证（层 ID、扩展名探测序、
//!   参数表、淡变线性、LOOPCOUNT 边界、TIME/TOTALTIME、CODEC/FILETYPE）。
//! - **Likely**：REA 可读但缺少动态对拍（默认音量 256、负 START 的延迟）。
//! - **Unverified**：重采样插值、限幅、混音细节（代码处显式标注）。
//! - **Unknown/Unimplemented**：ACM WAVE、MIDI/MIDIEND/MIDIINFO、CDDA、
//!   DATALEFT/DATARIGHT、SOUND[SPEED=]。
//!
//! # 边界
//!
//! - 不碰文件系统、不认识 ypf；装载经 [`loader::SoundLoader`] 注入。
//! - 状态只由注入时钟 [`state::SoundState::advance`] 推进。
//! - 设备输出不在本 crate（headless 混音见 [`mixer`]）。

#![forbid(unsafe_code)]
// `Diag` 承载边界/位置/因果链；与 yu-log/yu-gfx 的显式放行同理由。
#![allow(clippy::result_large_err)]

pub mod decode;
pub mod error;
pub mod host;
pub mod loader;
pub mod mixer;
pub mod state;

pub use host::SoundHost;
pub use loader::{MemoryLoader, SoundLoader};
pub use state::{FileType, Layer, PlayState, QueryField, SoundCommand, SoundState};
