//! `yu-sound` · 声音域宿主（SOUND / SOUNDINFO / SOUNDEND）。
//!
//! 净室实现：只依据官方手册、ERIS 语料与 0.495 `yu-ris.exe` 的 REA 事实；
//! 不复制旧线代码。证据等级与逐条出处见 `docs/sound-ledger.md`。
//!
//! # 证据等级
//!
//! - **Confirmed**：手册原文 + 语料/REA 双重佐证（层 ID、扩展名探测序、
//!   参数表、淡变线性、LOOPCOUNT 边界、TIME/TOTALTIME、CODEC/FILETYPE、
//!   DATALEFT/DATARIGHT 帧采样——手册无页，按 REA 实现）。
//! - **Likely**：REA 可读但缺少动态对拍（默认音量 256、负 START/RESTART
//!   延迟期间的 TIME/PLAY）。
//! - **Unverified**：重采样插值、限幅、混音细节（代码处显式标注）。
//! - **fixed(no-manual)**：ACM WAVE（win32 ACM，官方 `0x422e04` 非 PCM
//!   分支 1:1；手册「不支持」句过时）。
//! - **aligned-stub**：MIDI/MIDIEND/MIDIINFO（官方 `0x44aa40/60/80`）、
//!   CDDA（`0x429fb0`）。
//! - **unsupported-by-official**：`SOUND[SPEED=]`（官方处理体不读槽 13）。
//!
//! # 架构(与官方 0.495 同构)
//!
//! - **装载只读头部**:OGG 走 `ov_open` + `ov_time_total` 的等价物
//!   (`meta::probe` + 内部 `stream` 模块),O(1),`TOTALTIME` 立即精确;官方
//!   `0x422ab0` 之后只建 0.5s DirectSound 环(`0x422c65`),不整段解码。
//! - **播放期按需解码**:tick 侧预取 0.5s(`LEAD_MS`),音频回调
//!   只拷贝;窗口不足时回调按需解码并计数。WAVE PCM/ACM 沿官方
//!   mmio 直读/一次 ACM 转换,整段样本在手(旧路径逐样本一致)。
//!
//! # 边界
//!
//! - 不碰文件系统、不认识 ypf；装载经 [`loader::SoundLoader`] 注入。
//! - 状态只由注入时钟 [`state::SoundState::advance`] 推进。
//! - 设备输出不在本 crate（headless 混音见 [`mixer`]）。

// `acm` 是全仓第一个第一方 unsafe 点（win32 ACM FFI）；其余代码保持
// 无 unsafe。理由与边界见 `acm` 模块文档。
#![deny(unsafe_code)]
// `Diag` 承载边界/位置/因果链；与 yu-log/yu-gfx 的显式放行同理由。
#![allow(clippy::result_large_err)]

pub mod acm;
pub mod decode;
pub mod error;
pub mod host;
pub mod loader;
pub mod meta;
pub mod mixer;
pub mod state;
mod stream;

pub use host::SoundHost;
pub use loader::{MemoryLoader, SoundLoader};
pub use meta::AudioMeta;
pub use state::{DecodeStats, FileType, Layer, PlayState, QueryField, SoundCommand, SoundState};
