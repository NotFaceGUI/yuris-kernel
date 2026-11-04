//! 文本层状态(`TEXT`/`WORD`/`_`/`RETURNCODE` 的系统侧语义)。
//!
//! # 三层出处(0.495)
//!
//! ① 手册:
//! - `text/text.html`:`TEXT[ID/OUTPUTMODE/WAIT/RETURN/PAGE/CLICK/AUTORETURN/
//!   AUTOPAGE/AUTOCLICK/CLICKBUTTON1..4/SKIPBUTTON1..4/CLEAR]`;
//!   例 1「テキストの表示速度を１文字あたり15msに設定する」=`TEXT[WAIT=15]`;
//! - `text/textinfo.html`:查询同一组属性(OUTPUTMODE/CX/CY/CLICK/AUTO*)。
//!
//! ② 语料:ERIS `es_first.yst` 首指令 `TEXT[OUTPUTMODE=0]`;
//!   `text/es_text.yst` 读 `$_TEXT`/`$_TEXT_FLAG`;600 帧普查 LET 读
//!   `_TEXT_FLAG` 120 次(每个文本循环帧)。
//!
//! ③ REA(0.495 `yu-ris.exe`,sha256 `4f5eefa3…`):
//! - 文本状态全局 `[0x801800]`,初值 `FUN_0045fcb4`:
//!   `+0x1c=1`(OUTPUTMODE 默认自动)、`+0x08/+0x0c/+0x10=1`
//!   (AUTORETURN/AUTOPAGE/AUTOCLICK 默认开)、`+0x36=1`;
//! - `TEXT` 处理体 `0x453448`:参数槽映射与写入次序(见下);
//! - `TEXTINFO` 处理体 `0x4538cc`:按**参数是否出现**查询,
//!   次序 OUTPUTMODE,CX,CY,CLICK,AUTORETURN,AUTOPAGE,AUTOCLICK;
//! - `WORD` 处理体 `0x458d44`:文本字节进 `[state+0x38]`,每字符一个标志
//!   进 `[state+0x3c]`,`0x5b40c0[lead]` 决定 '2'(全角)/'1'(半角);
//! - `RETURNCODE` 处理体 `0x44c390`:自动模式按 AUTOCLICK/AUTORETURN/
//!   AUTOPAGE 追加 'C'/'R'/'P',最后恒追加 'E'(0x45)并置 `task+9`;
//!   手动模式置 `+0x36` 后追加 'E';
//! - 字符串系统变量取值 `0x446b93`:id 0x7a(`_TEXT`)→ `[state+0x38]`,
//!   id 0x7b(`_TEXT_FLAG`)→ `[state+0x3c]`。
//!
//! # 编码边界(2026-11-04)
//!
//! `layer_id` / `bytes` / `flags` 都是**脚本原样字节**,本模块不做
//! CP932↔Unicode 转换,也不假设 ID 是 UTF-8。`is_lead` 读的是引擎
//! `0x5b40c0` 前导字节表(0.495 日文构建的 CP932 区间),只用于
//! `_TEXT_FLAG` 的 `'2'/'1'`(该字符占 1 还是 2 字节),不把字节
//! 解释成字符。需要字形/字体的地方在 `yu-font`/渲染层,不在这里。
//!
//! # 标志字节
//!
//! `_TEXT_FLAG` 是一个与控制元素一一对应的字节串(引擎 `[state+0x3c]`):
//!
//! | 字节 | 含义 | 写入点 |
//! |---|---|---|
//! | `'1'` (0x31) | 半角字符(文本占 1 字节) | WORD / `_` |
//! | `'2'` (0x32) | 全角字符(文本占 2 字节) | WORD / `_` |
//! | `'R'` (0x52) | 改行(`TEXT[RETURN=1]`,自动模式 AUTO*) | `0x4536b5` / `0x44c411` |
//! | `'P'` (0x50) | 改页(`TEXT[PAGE=1]`,自动模式 AUTOPAGE) | `0x453714` / `0x44c42c` |
//! | `'r'` (0x72) | 点击等待+改行(`TEXT[CLICKRETURN=1]`) | `0x453765` |
//! | `'p'` (0x70) | 点击等待+改页(`TEXT[CLICKPAGE=1]`) | `0x4537c4` |
//! | `'C'` (0x43) | 点击等待(`TEXT[CLICK=1]`,自动模式 AUTOCLICK) | `0x453876` / `0x44c3d0` |
//! | `'E'` (0x45) | 行尾(`RETURNCODE` 恒写) | `0x44c447` |
//!
//! 这份表只覆盖**已取证**的写入点;其它字节不在这里发明。
//!
//! 台账:`COV-M-01`(读 `_TEXT_FLAG`)/ `COV-M-03`(逐字符宽度标志);
//! 见 `docs/coverage-matrix.md`。

/// `_TEXT_FLAG` 控制字节(已取证部分)。
pub mod flag {
    /// 半角字符(文本 1 字节)。
    pub const HALF: u8 = b'1';
    /// 全角字符(文本 2 字节;CP932 首字节)。
    pub const FULL: u8 = b'2';
    /// 改行(`TEXT[RETURN=1]`)。
    pub const RETURN: u8 = b'R';
    /// 改页(`TEXT[PAGE=1]`)。
    pub const PAGE: u8 = b'P';
    /// 点击等待+改行(`TEXT[CLICKRETURN=1]`)。
    pub const CLICK_RETURN: u8 = b'r';
    /// 点击等待+改页(`TEXT[CLICKPAGE=1]`)。
    pub const CLICK_PAGE: u8 = b'p';
    /// 点击等待(`TEXT[CLICK=1]`)。
    pub const CLICK: u8 = b'C';
    /// 行尾(`RETURNCODE` 恒写)。
    pub const END: u8 = b'E';
}

/// CP932 首字节(= 双字节字符)。与 `handlers::str_char_count` 同口径。
pub(crate) fn is_lead(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)
}

/// 文本层状态(引擎 `[0x801800]` 的已取证字段)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextState {
    /// 当前文本层 ID(`TEXT[ID]`;CP932 原始字节,空 = 默认层)。
    ///
    /// 引擎 `[state+0x00]` 存的是按 ID 解析出的图层对象;解析
    /// (`FUN_00414559`)用 MurmurHash2 + **逐字节比较**,不校验 UTF-8
    /// (REA `0x414559`/`0x41462f..0x4146da`;手册 `text/text.html` ID)。
    /// 内核保留原始字节,层匹配与 trace 都不丢信息。
    pub layer_id: Vec<u8>,
    /// `$_TEXT` 内容(CP932 原始字节;引擎 `[state+0x38]`)。
    pub bytes: Vec<u8>,
    /// `$_TEXT_FLAG` 控制字节流(引擎 `[state+0x3c]`;见 [`flag`])。
    pub flags: Vec<u8>,
    /// `TEXT[OUTPUTMODE]`(引擎 `+0x1c`,字节;0=手动,非 0=自动)。
    pub output_mode: u8,
    /// `TEXT[WAIT]`(毫秒;引擎 `+0x04`)。
    pub wait_ms: i64,
    /// `TEXT[AUTORETURN]`(引擎 `+0x08`;默认 1)。
    pub autoreturn: i64,
    /// `TEXT[AUTOPAGE]`(引擎 `+0x0c`;默认 1)。
    pub autopage: i64,
    /// `TEXT[AUTOCLICK]`(引擎 `+0x10`;默认 1)。
    pub autoclick: i64,
    /// 当前是否点击等待(引擎 `+0x35`;`TEXTINFO[CLICK]` 数据源)。
    pub click: bool,
    /// 手动模式请求清屏(引擎 `+0x36`;`RETURNCODE` 手动路径置 1)。
    pub clear_pending: bool,
    /// `TEXT[CLEAR]` 代数(内核桥接字段)。
    ///
    /// 引擎 `CLEAR` 没有事件,`FUN_004638e4` 只在 `+0x36` 在位时清十个
    /// 堆数组与 `+0x60/+0x64/+0x68/+0x6c`(REA `0x4638e4`)。宿主需要
    /// 知道“这一帧发生过 CLEAR”;每次 [`TextState::clear`] 自增,宿主据此
    /// **显式**清显示层,而不是把空 `bytes` 当成 CLEAR
    /// (见 `docs/tasks/task-text-auto-pipeline.md` C1)。
    pub clear_seq: u64,
    /// `_TEXT_FLAG` 消耗游标(引擎 `+0x6c`;**当前显示段内**,含控制标志)。
    ///
    /// 官方 `FUN_00404914` 每处理一个标志 `+0x6c += 1`;遇 `'C'` 停,
    /// 遇 `'R'/'P'` 换行/清页,遇 `'E'` 仅跳过(REA `0x404914..`)。
    pub reveal_flags: usize,
    /// 已布局文本字节数(引擎 `+0x68`;当前显示段内)。
    pub reveal_bytes: usize,
    /// 当前显示段已显字符数(`'1'/'2'` 个数)。
    pub line_reveal_chars: usize,
    /// 已完成显示段累计字符数(内核累积显示缓冲的可见前缀)。
    pub committed_chars: usize,
    /// 当前显示段在 `flags`/`bytes` 中的起点。
    line_start_flags: usize,
    line_start_bytes: usize,
    /// 每标志延迟毫秒(引擎 `+0x48` 表;`WORD`/`_` 追加时按当时 WAIT 快照)。
    delays: Vec<i16>,
    /// 上次推进基准(引擎 `0x801c74`,u32 回绕;毫秒)。
    last_char_ms: u32,
    /// 重置计时基准(引擎 `+0x37`;CLEAR/显示段开始时置 1)。
    pub timer_reset: bool,
    /// 快进模式(引擎 `0x801c70`:推进按钮按下且不在点击等待)。
    pub skip_mode: bool,
    /// 文本光标 X/Y(引擎 `+0x14/+0x18`;`TEXTINFO[CX/CY]`)。
    pub cursor_x: i64,
    pub cursor_y: i64,
    /// `CLICKBUTTON1..4` 按钮码(引擎全局 `0x5b48ac..`;0 → `0x3ff`)。
    pub click_buttons: [i64; 4],
    /// `SKIPBUTTON1..4` 按钮码(引擎全局 `0x5b48bc..`;0 → `0x3ff`)。
    pub skip_buttons: [i64; 4],
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            layer_id: Vec::new(),
            bytes: Vec::new(),
            flags: Vec::new(),
            // FUN_0045fcb4:OUTPUTMODE=1,AUTO*=1。
            output_mode: 1,
            wait_ms: 0,
            autoreturn: 1,
            autopage: 1,
            autoclick: 1,
            click: false,
            clear_pending: false,
            clear_seq: 0,
            reveal_flags: 0,
            reveal_bytes: 0,
            line_reveal_chars: 0,
            committed_chars: 0,
            line_start_flags: 0,
            line_start_bytes: 0,
            delays: Vec::new(),
            last_char_ms: 0,
            timer_reset: true,
            skip_mode: false,
            cursor_x: 0,
            cursor_y: 0,
            // 手册 `text/text.html`:CLICKBUTTON 默认 `_CODE_MOUSE_L`(1)、
            // CLICKBUTTON2 默认 `_CODE_ENTER`(13);SKIPBUTTON 默认
            // `_CODE_CTRL`(17)。其余未分配 = 0x3ff(DIV-RT-035 的按钮表)。
            click_buttons: [1, 13, 0x3ff, 0x3ff],
            skip_buttons: [17, 0x3ff, 0x3ff, 0x3ff],
        }
    }
}

impl TextState {
    /// 追加一段文本,并按字符写入 `'1'`/`'2'` 标志(REA `0x458d44`)。
    ///
    /// 同时按引擎 `+0x48` 表快照每字符延迟:`WAIT / (全角 ? 1 : 2)`
    /// (`WORD 0x458e78` 的 `idiv edi`;`_ 0x45697f/0x456a8e` 同口径)。
    /// 半角 = `WAIT/2` 是实机行为;手册“1 文字あたり 15ms”只描述全角。
    pub fn append_text(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        let wait = self.wait_ms as i32;
        let mut i = 0;
        while i < bytes.len() {
            if is_lead(bytes[i]) && i + 1 < bytes.len() {
                self.flags.push(flag::FULL);
                self.delays.push(wait as i16);
                i += 2;
            } else {
                self.flags.push(flag::HALF);
                self.delays.push((wait / 2) as i16);
                i += 1;
            }
        }
    }

    /// 追加一个控制标志(`TEXT`/`RETURNCODE`)。
    ///
    /// 控制标志不参与 `FUN_00404c0c` 的 WAIT 判定,延迟表占位 0
    /// (引擎在控制标志位置不写 `+0x48`,本表只保持与 `flags` 对齐)。
    pub fn push_flag(&mut self, byte: u8) {
        self.flags.push(byte);
        self.delays.push(0);
    }

    /// `TEXT[CLEAR=1]` / 引擎 `0x4638e4`:清空文本与标志。
    ///
    /// 引擎只清 `+0x36/+0x60/+0x64/+0x68/+0x6c` 并置 `+0x37`;光标
    /// (`+0x14/+0x18`)与点击状态(`+0x35`)保持不变。`clear_seq` 是内核
    /// 桥接字段:每次 CLEAR 自增,供宿主显式清显示层。
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.flags.clear();
        self.delays.clear();
        self.clear_pending = false;
        self.clear_seq = self.clear_seq.wrapping_add(1);
        // 引擎 `FUN_004638e4` 清 `+0x60/+0x64/+0x68/+0x6c` 并置 `+0x37`;
        // `+0x35`(click)与 `+0x14/+0x18` 光标**不清**(REA 0x4638e4)。
        self.reveal_flags = 0;
        self.reveal_bytes = 0;
        self.line_reveal_chars = 0;
        self.committed_chars = 0;
        self.line_start_flags = 0;
        self.line_start_bytes = 0;
        self.timer_reset = true;
        self.skip_mode = false;
    }

    /// 当前显示段可见字符数(已完成段 + 本段已显)。
    #[must_use]
    pub fn reveal_chars(&self) -> usize {
        self.committed_chars.saturating_add(self.line_reveal_chars)
    }

    /// 当前显示段是否已消费到缓冲末尾(不区分 `'C'` 等待)。
    #[must_use]
    pub fn line_complete(&self) -> bool {
        self.line_start_flags + self.reveal_flags >= self.flags.len()
    }

    /// 按官方 `FUN_004046fc`/`FUN_00404914` 推进当前显示段。
    ///
    /// 时间基 = 宿主 `tick_at(now_ms)`(毫秒;官方 `timeGetTime()` 同单位,
    /// REA `0x404c21`)。规则:
    /// - 只有自动模式(`+0x1c == 1`)才推进;手动模式由脚本自己驱动;
    /// - `0x801c70`(skip)在位时不等 WAIT,一次性消费到 `'C'`/段尾;
    /// - 一行的第一个字符立即显示(`+0x68 == 0` 门),之后按
    ///   `+0x48[+0x6c]` 毫秒等待,`last += delay`(u32 回绕);
    /// - `'C'` 置点击等待并停;`'E'`/`'R'`/`'P'` 只推进游标
    ///   (换行/清页由 gfx 的 flag 排版消费)。
    pub fn advance_reveal(&mut self, now_ms: u32) {
        if self.output_mode != 1 {
            return;
        }
        loop {
            if self.line_complete() {
                return;
            }
            if self.timer_reset {
                self.timer_reset = false;
                self.last_char_ms = now_ms;
            }
            let cursor = self.line_start_flags + self.reveal_flags;
            let f = self.flags[cursor];
            if f == flag::CLICK {
                self.click = true;
                self.skip_mode = false;
                return;
            }
            if f == flag::HALF || f == flag::FULL {
                let len = if f == flag::FULL { 2 } else { 1 };
                // 数据不足:不猜、不越界;留给诊断/后续追加。
                if self.line_start_bytes + self.reveal_bytes + len > self.bytes.len() {
                    return;
                }
                if !self.skip_mode && self.reveal_bytes > 0 {
                    // 缺省:延迟表与 flags 同步追加;缺失按 0 继续,不 panic。
                    let delay = self.delays.get(cursor).copied().unwrap_or(0) as i32 as u32;
                    if now_ms.wrapping_sub(self.last_char_ms) < delay {
                        return;
                    }
                    self.last_char_ms = self.last_char_ms.wrapping_add(delay);
                }
                self.reveal_bytes += len;
                self.reveal_flags += 1;
                self.line_reveal_chars += 1;
            } else {
                // 'E' / 'R' / 'P' / 未取证控制字节:控制语义归 gfx 排版,
                // 这里只按引擎 `FUN_00404914` 推进一个标志。
                self.reveal_flags += 1;
            }
        }
    }

    /// 把当前显示段快进到末尾并提交(点击/跳过恢复、`RETURNCODE` 行尾)。
    ///
    /// 官方点击/跳过路径(`0x404748/0x4047f6`):`+0x37=1`、`+0x35=0`、
    /// `+0x6c += 1`,随后 `FUN_00404914` 继续处理剩余 `R/P/E`。内核累积
    /// 显示缓冲不在行尾丢弃已画内容,只把本段字符计入 `committed_chars`,
    /// 下一段从缓冲末尾开始(对应官方“像素留在表面”的可观测结果)。
    pub fn finish_line(&mut self) {
        self.skip_mode = true;
        while !self.line_complete() {
            let cursor = self.line_start_flags + self.reveal_flags;
            if self.flags[cursor] == flag::HALF || self.flags[cursor] == flag::FULL {
                let len = if self.flags[cursor] == flag::FULL {
                    2
                } else {
                    1
                };
                if self.line_start_bytes + self.reveal_bytes + len > self.bytes.len() {
                    break;
                }
                self.reveal_bytes += len;
                self.line_reveal_chars += 1;
            }
            self.reveal_flags += 1;
        }
        self.committed_chars = self.committed_chars.saturating_add(self.line_reveal_chars);
        self.line_start_flags = self.flags.len();
        self.line_start_bytes = self.bytes.len();
        self.reveal_flags = 0;
        self.reveal_bytes = 0;
        self.line_reveal_chars = 0;
        self.click = false;
        self.skip_mode = false;
        self.timer_reset = true;
    }

    /// `$_TEXT`/`$_TEXT_FLAG` 被脚本整体替换后,让新内容全部可见。
    ///
    /// 引擎写入路径(`0x446b98`)只拷字节、不重建 `+0x40/0x44/0x48` 表;
    /// 本函数是内核显示缓冲的自保:不猜新内容的逐字节奏,直接全显
    /// (与 `DIV-RT-032` 的写入取证范围一致)。
    pub fn reset_reveal_all(&mut self) {
        self.committed_chars = 0;
        self.line_start_flags = 0;
        self.line_start_bytes = 0;
        self.reveal_flags = 0;
        self.reveal_bytes = 0;
        self.line_reveal_chars = 0;
        self.click = false;
        self.skip_mode = false;
        self.timer_reset = true;
        self.finish_line();
    }

    /// 设置输出模式(引擎只存低字节)。
    pub fn set_output_mode(&mut self, v: i64) {
        self.output_mode = v as u8;
    }

    /// `TEXT[CLICKBUTTONn]`/`SKIPBUTTONn`:0 = 解除分配 → `0x3ff`。
    pub fn set_button(slot: &mut i64, v: i64) {
        *slot = if v == 0 { 0x3ff } else { v };
    }
}

/// 文本层状态的冻结快照(宿主/gfx 桥读取;避免借用 `Vm`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSnapshot {
    /// 当前文本层 ID(CP932 原始字节;权威层键,不做 UTF-8 转换)。
    pub layer_id: Vec<u8>,
    pub bytes: Vec<u8>,
    pub flags: Vec<u8>,
    pub output_mode: u8,
    pub wait_ms: i64,
    pub autoreturn: i64,
    pub autopage: i64,
    pub autoclick: i64,
    pub click: bool,
    pub clear_pending: bool,
    pub clear_seq: u64,
    /// 当前显示段标志游标(引擎 `+0x6c`)。
    pub reveal_flags: usize,
    /// 当前显示段已布局字节数(引擎 `+0x68`)。
    pub reveal_bytes: usize,
    /// 可见字符总数(已完成段 + 本段;gfx `set_text_reveal` 的唯一输入)。
    pub reveal_chars: usize,
    /// 快进模式(引擎 `0x801c70`;审计/诊断)。
    pub skip_mode: bool,
    pub cursor_x: i64,
    pub cursor_y: i64,
    pub click_buttons: [i64; 4],
    pub skip_buttons: [i64; 4],
}

impl From<&TextState> for TextSnapshot {
    fn from(s: &TextState) -> Self {
        Self {
            layer_id: s.layer_id.clone(),
            bytes: s.bytes.clone(),
            flags: s.flags.clone(),
            output_mode: s.output_mode,
            wait_ms: s.wait_ms,
            autoreturn: s.autoreturn,
            autopage: s.autopage,
            autoclick: s.autoclick,
            click: s.click,
            clear_pending: s.clear_pending,
            clear_seq: s.clear_seq,
            reveal_flags: s.reveal_flags,
            reveal_bytes: s.reveal_bytes,
            reveal_chars: s.reveal_chars(),
            skip_mode: s.skip_mode,
            cursor_x: s.cursor_x,
            cursor_y: s.cursor_y,
            click_buttons: s.click_buttons,
            skip_buttons: s.skip_buttons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_text_flags_match_cp932_width() {
        let mut t = TextState::default();
        // "AこB":A=半角,こ=82 B1 全角,B=半角。
        t.append_text(b"A\x82\xB1B");
        assert_eq!(t.bytes, b"A\x82\xB1B");
        assert_eq!(t.flags, vec![flag::HALF, flag::FULL, flag::HALF]);
    }

    #[test]
    fn control_flags_and_clear() {
        let mut t = TextState::default();
        t.push_flag(flag::RETURN);
        t.push_flag(flag::END);
        assert_eq!(t.flags, vec![b'R', b'E']);
        t.cursor_x = 7;
        t.click = true;
        t.clear();
        assert!(t.flags.is_empty() && t.bytes.is_empty());
        assert_eq!(t.cursor_x, 7, "引擎 CLEAR 不重置光标 +0x14");
        assert!(t.click, "引擎 CLEAR 不重置点击状态 +0x35");
    }

    #[test]
    fn output_mode_is_low_byte() {
        let mut t = TextState::default();
        t.set_output_mode(1);
        assert_eq!(t.output_mode, 1);
        t.set_output_mode(0);
        assert_eq!(t.output_mode, 0);
        t.set_output_mode(-1);
        assert_eq!(t.output_mode, 0xff);
    }

    #[test]
    fn default_buttons_match_manual_assignments() {
        // 手册 text/text.html:MOUSE_L=1 / ENTER=13 / CTRL=17;未分配 0x3ff。
        let t = TextState::default();
        assert_eq!(t.click_buttons, [1, 13, 0x3ff, 0x3ff]);
        assert_eq!(t.skip_buttons, [17, 0x3ff, 0x3ff, 0x3ff]);
    }

    #[test]
    fn reveal_uses_full_half_delays_and_resets_on_clear() {
        // REA WORD 0x458e78:全角 = WAIT,半角 = WAIT/2;一行的第一个
        // 字符立即显示(+0x68 == 0 门,0x404c1b..0x404c1f)。
        let mut t = TextState::default();
        t.wait_ms = 15;
        t.append_text(b"A\x82\xa0"); // A 半角,あ 全角
        t.push_flag(flag::CLICK);
        t.push_flag(flag::END);
        assert_eq!(t.delays, vec![7, 15, 0, 0]);

        t.advance_reveal(0);
        assert_eq!(
            (t.reveal_flags, t.reveal_bytes, t.reveal_chars()),
            (1, 1, 1)
        );
        t.advance_reveal(14);
        assert_eq!(t.reveal_chars(), 1, "全角延迟 15ms 未到");
        t.advance_reveal(15);
        assert_eq!(
            (t.reveal_flags, t.reveal_bytes, t.reveal_chars()),
            (2, 3, 2)
        );
        assert!(t.click, "遇到 'C' 置点击等待并停");

        t.finish_line();
        assert_eq!(t.reveal_chars(), 2, "提交后已显字符数保留");
        assert!(!t.click);
        assert!(t.timer_reset);

        t.append_text(b"B");
        t.advance_reveal(15); // 新段首字符立即显示
        assert_eq!(t.reveal_chars(), 3);

        t.clear();
        assert_eq!(t.reveal_chars(), 0);
        assert_eq!(t.reveal_flags, 0);
        assert!(t.timer_reset);
    }

    #[test]
    fn manual_mode_does_not_advance_reveal() {
        let mut t = TextState::default();
        t.set_output_mode(0);
        t.append_text(b"A");
        t.advance_reveal(1000);
        assert_eq!(t.reveal_chars(), 0, "手动模式无引擎自动推进");
    }
}
