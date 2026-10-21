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
    pub fn append_text(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        let mut i = 0;
        while i < bytes.len() {
            if is_lead(bytes[i]) && i + 1 < bytes.len() {
                self.flags.push(flag::FULL);
                i += 2;
            } else {
                self.flags.push(flag::HALF);
                i += 1;
            }
        }
    }

    /// 追加一个控制标志(`TEXT`/`RETURNCODE`)。
    pub fn push_flag(&mut self, byte: u8) {
        self.flags.push(byte);
    }

    /// `TEXT[CLEAR=1]` / 引擎 `0x4638e4`:清空文本与标志。
    ///
    /// 引擎只清 `+0x36/+0x60/+0x64/+0x68/+0x6c` 并置 `+0x37`;光标
    /// (`+0x14/+0x18`)与点击状态(`+0x35`)保持不变。
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.flags.clear();
        self.clear_pending = false;
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
    pub bytes: Vec<u8>,
    pub flags: Vec<u8>,
    pub output_mode: u8,
    pub wait_ms: i64,
    pub autoreturn: i64,
    pub autopage: i64,
    pub autoclick: i64,
    pub click: bool,
    pub clear_pending: bool,
    pub cursor_x: i64,
    pub cursor_y: i64,
    pub click_buttons: [i64; 4],
    pub skip_buttons: [i64; 4],
}

impl From<&TextState> for TextSnapshot {
    fn from(s: &TextState) -> Self {
        Self {
            bytes: s.bytes.clone(),
            flags: s.flags.clone(),
            output_mode: s.output_mode,
            wait_ms: s.wait_ms,
            autoreturn: s.autoreturn,
            autopage: s.autopage,
            autoclick: s.autoclick,
            click: s.click,
            clear_pending: s.clear_pending,
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
}
