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
//!   (AUTORETURN/AUTOPAGE/AUTOCLICK 默认开)、`+0x20/+0x24=0x14`、
//!   `+0x28=0xffffff`、`+0x2c=0xffffffff`、`+0x30=1`(AA)、`+0x31=0`
//!   (GRADATION)、`+0x32=0`(BOLD)、`+0x33=0`(DRAWMODE)、`+0x34=1`(BNO)、
//!   `+0x36=1`;
//! - `TEXT` 处理体 `0x453448`:参数槽映射与写入次序(见下);
//! - `TEXTINFO` 处理体 `0x4538cc`:按**参数是否出现**查询,
//!   次序 OUTPUTMODE,CX,CY,CLICK,AUTORETURN,AUTOPAGE,AUTOCLICK;
//! - `WORD` 处理体 `0x458d44`:`+0x40[i]=SX`(半角也不折半)、
//!   `+0x44[i]=SY`、`+0x48[i]=WAIT/(全角1:半角2)`、
//!   `+0x4c/50/54/58/5c[i]` = 当时的 COLOR/COLOR2/AA/GRADATION/BOLD;
//! - `_` 处理体 `0x4567c0` 同表;非打印 ASCII 写空格;
//! - `RETURNCODE` 处理体 `0x44c390`:自动模式按 AUTOCLICK/AUTORETURN/
//!   AUTOPAGE 追加 'C'/'R'/'P',最后恒追加 'E'(0x45);
//!   手动模式置 `+0x36` 后追加 'E';
//! - `FUN_00404914`(`0x404914`)逐标志布局:见 [`TextState::advance_reveal`];
//! - `FUN_00404c60`(`0x404c60`)折行/禁则:见 [`TextState`] 的 `wrap_or_stop`;
//! - `FUN_004046fc`(`0x4046fc`)每帧 tick:手动模式只清待显示缓冲后恢复;
//!   自动模式在标志游标追上总数时置 `+0x36` 再清 + 恢复;
//! - `FUN_004638e4`(`0x4638e4`)只清 `+0x60/64/68/6c` 与十块堆数组,
//!   **不触碰** `+0x14/+0x18` 光标、`+0x35` 点击、`+0x00` 层对象与图层表面。
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
//! 见 `docs/coverage-matrix.md`。严格模型见
//! `docs/tasks/text-oracle-2026-11-16.md`。

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

/// 行末禁则(`FUN_00404c60` 的 `「` `『` `（`;必须在换行前处理)。
pub const KINSOKU_LINE_END: [[u8; 2]; 3] = [[0x81, 0x75], [0x81, 0x77], [0x81, 0x69]];

/// 行头禁则(悬挂进右边界)。顺序照官方比较链 `0x824c → 0x81f4`。
pub const KINSOKU_LINE_START: [[u8; 2]; 23] = [
    [0x82, 0x9f],
    [0x82, 0xa1],
    [0x82, 0xa3],
    [0x82, 0xa5],
    [0x82, 0xa7],
    [0x82, 0xe1],
    [0x82, 0xe3],
    [0x82, 0xe5],
    [0x82, 0xc1],
    [0x83, 0x40],
    [0x83, 0x42],
    [0x83, 0x44],
    [0x83, 0x46],
    [0x83, 0x48],
    [0x83, 0x83],
    [0x83, 0x85],
    [0x83, 0x87],
    [0x83, 0x62],
    [0x81, 0x42],
    [0x81, 0x41],
    [0x81, 0x76],
    [0x81, 0x78],
    [0x81, 0x6a],
];

/// `！？` 成对禁则的第一字(`0x808254`)。
pub const KINSOKU_PAIR_HEAD: [u8; 2] = [0x81, 0x49];
/// `！？` 成对禁则的第二字(`0x808250`)。
pub const KINSOKU_PAIR_TAIL: [u8; 2] = [0x81, 0x48];

/// 一个标志位的逐字符快照(引擎 `+0x40/44/48/4c/50/54/58/5c` 的同下标项)。
///
/// 控制标志不写这些表时,对应项是零(官方堆数组初值);`R` 的行高
/// 因此取该下标 `height`,与 `RETURNCODE`/`TEXT` 写入 `SX/SY` 的路径一致。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCell {
    /// 单元请求宽(引擎 `+0x40`;恒为当时 `SX`)。
    pub width: i16,
    /// 单元高 / 行高(引擎 `+0x44`;恒为当时 `SY`)。
    pub height: i16,
    /// 每字符延迟 ms(引擎 `+0x48`)。
    pub delay: i16,
    /// 前景色(引擎 `+0x4c`;追加时快照 `COLOR`)。
    pub fg: u32,
    /// 背景/渐变终点色(引擎 `+0x50`;追加时快照 `COLOR2`)。
    pub bg: u32,
    /// `ANTIALIASING` 原始字节(引擎 `+0x54`)。
    pub antialias: u8,
    /// `GRADATION` 原始字节(引擎 `+0x58`)。
    pub gradation: u8,
    /// `BOLD` 原始字节(引擎 `+0x5c`)。
    pub bold: u8,
}

/// 文本状态里的 FONT 子集(引擎 `+0x20..+0x34`)。
///
/// 初值来自 `FUN_0045fcb4`:SX=SY=0x14、COLOR=0xffffff、COLOR2=0xffffffff、
/// ANTIALIASING=1、GRADATION=0、BOLD=0、DRAWMODE=0、BNO=1。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFontState {
    pub sx: i32,
    pub sy: i32,
    pub color: u32,
    pub color2: u32,
    pub antialias: u8,
    pub gradation: u8,
    pub bold: u8,
    pub draw_mode: u8,
    pub bno: u8,
}

impl Default for TextFontState {
    fn default() -> Self {
        Self {
            sx: 0x14,
            sy: 0x14,
            color: 0x00FF_FFFF,
            color2: 0x00FF_FFFF,
            antialias: 1,
            gradation: 0,
            bold: 0,
            draw_mode: 0,
            bno: 1,
        }
    }
}

/// 已绘制字形的逐字符样式(与状态级 `DRAWMODE/BNO` 分开)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextGlyphStyle {
    pub fg: u32,
    pub bg: u32,
    pub antialias: bool,
    pub gradation: u8,
    pub bold: bool,
}

/// 一次“已提交”字形绘制(引擎在 `FUN_00404914` 里逐字写进层表面)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextGlyphDraw {
    /// 显示 codec(全局;官方 CHS 同构)。
    pub codec: yu_format::Codec,
    /// 单字字节(1 或 2;与 `full` 一致)。
    pub bytes: smallvec::SmallVec<[u8; 2]>,
    /// 是否全角。
    pub full: bool,
    /// 绘制位置(层局部像素;读取当时的 `+0x14/+0x18`)。
    pub x: i64,
    pub y: i64,
    /// 单元请求宽/高(引擎 `+0x40/+0x44`)。
    pub width: i16,
    pub height: i16,
    /// 逐字符样式快照。
    pub style: TextGlyphStyle,
    /// 绘制时刻的状态级 `DRAWMODE`(引擎 `+0x33`,不快照)。
    pub draw_mode: u8,
    /// 绘制时刻的状态级 `BNO`(引擎 `+0x34`,不快照)。
    pub bno: u8,
}

/// 一页内已提交的布局步骤。
///
/// `PageClear` 对应 `'P'/'p'`:官方 `zero_fill` 层表面并把 `+0x14/+0x18`
/// 归零;gfx 据此清空“已提交字形”。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextStep {
    Glyph(TextGlyphDraw),
    PageClear,
}

/// 文本层状态(引擎 `[0x801800]` 的已取证字段)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextState {
    /// 显示层文本 codec(全局;官方 CHS 同构,不逐文件/逐段区分)。
    pub codec: yu_format::Codec,
    /// 当前文本层 ID(`TEXT[ID]`;CP932 原始字节,空 = 默认层)。
    ///
    /// 引擎 `[state+0x00]` 存的是按 ID 解析出的图层对象;解析
    /// (`FUN_00414559`)用 MurmurHash2 + **逐字节比较**,不校验 UTF-8
    /// (REA `0x414559`/`0x41462f..0x4146da`;手册 `text/text.html` ID)。
    /// 内核保留原始字节,层匹配与 trace 都不丢信息。
    pub layer_id: Vec<u8>,
    /// `$_TEXT` 内容(CP932 原始字节;引擎 `[state+0x38]`)。
    ///
    /// **待显示缓冲**:行完成 / `TEXT[CLEAR]` / 手动任务释放时清空;
    /// 已画像素不在这里(见 [`TextStep`])。
    pub bytes: Vec<u8>,
    /// `$_TEXT_FLAG` 控制字节流(引擎 `[state+0x3c]`;见 [`flag`])。
    pub flags: Vec<u8>,
    /// 逐标志快照表(与 `flags` 同下标;引擎 `+0x40..+0x5c` 十块数组)。
    pub cells: Vec<TextCell>,
    /// 文本状态里的当前 FONT 子集(引擎 `+0x20..+0x34`)。
    pub font: TextFontState,
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
    /// 清待显示缓冲请求(引擎 `+0x36`;行完成/手动 `RETURNCODE`/`TEXT[CLEAR]`)。
    pub clear_pending: bool,
    /// `TEXT[CLEAR]` 代数(内核桥接字段;仅供诊断/测试)。
    pub clear_seq: u64,
    /// `'P'` 换页代数(内核桥接字段;gfx 据此重置已提交字形)。
    pub page_clear_seq: u64,
    /// `_TEXT_FLAG` 消耗游标(引擎 `+0x6c`;含控制标志)。
    pub reveal_flags: usize,
    /// 已布局文本字节游标(引擎 `+0x68`;与 `reveal_flags` **独立**)。
    pub reveal_bytes: usize,
    /// 重置计时基准(引擎 `+0x37`;CLEAR/显示段开始时置 1)。
    pub timer_reset: bool,
    /// 快进模式(引擎 `0x801c70`:推进按钮按住且不在点击等待)。
    pub skip_mode: bool,
    /// 文本光标 X/Y(引擎 `+0x14/+0x18`;`TEXTINFO[CX/CY]`)。
    pub cursor_x: i64,
    pub cursor_y: i64,
    /// `CLICKBUTTON1..4` 按钮码(引擎全局 `0x5b48ac..`;0 → `0x3ff`)。
    pub click_buttons: [i64; 4],
    /// `SKIPBUTTON1..4` 按钮码(引擎全局 `0x5b48bc..`;0 → `0x3ff`)。
    pub skip_buttons: [i64; 4],
    /// 目标层表面宽高(引擎 `*(L+0x34)` 的 `+0x78/+0x7c`)。
    ///
    /// `None` = 没有已解析的目标层 ⇒ 官方 `FUN_00404914` 入口直接返回,
    /// 不推进任何标志(调用方必须显式注入;引擎在 tick 前从 gfx 查询)。
    window: Option<(i64, i64)>,
    /// 最近一次推进基准(引擎 `0x801c74`,u32 回绕;毫秒)。
    last_char_ms: u32,
    /// 当前页的已提交步骤日志(`PageClear` 会清空并重新开始)。
    steps: Vec<TextStep>,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            codec: yu_format::Codec::Cp932,
            layer_id: Vec::new(),
            bytes: Vec::new(),
            flags: Vec::new(),
            cells: Vec::new(),
            font: TextFontState::default(),
            // FUN_0045fcb4:OUTPUTMODE=1,AUTO*=1。
            output_mode: 1,
            wait_ms: 0,
            autoreturn: 1,
            autopage: 1,
            autoclick: 1,
            click: false,
            clear_pending: false,
            clear_seq: 0,
            page_clear_seq: 0,
            reveal_flags: 0,
            reveal_bytes: 0,
            timer_reset: true,
            skip_mode: false,
            cursor_x: 0,
            cursor_y: 0,
            // 手册 `text/text.html`:CLICKBUTTON 默认 `_CODE_MOUSE_L`(1)、
            // CLICKBUTTON2 默认 `_CODE_ENTER`(13);SKIPBUTTON 默认
            // `_CODE_CTRL`(17)。其余未分配 = 0x3ff(DIV-RT-035 的按钮表)。
            click_buttons: [1, 13, 0x3ff, 0x3ff],
            skip_buttons: [17, 0x3ff, 0x3ff, 0x3ff],
            window: None,
            last_char_ms: 0,
            steps: Vec::new(),
        }
    }
}

impl TextState {
    #[must_use]
    pub fn with_codec(codec: yu_format::Codec) -> Self {
        Self {
            codec,
            ..Self::default()
        }
    }

    /// 目标层表面尺寸(引擎 `*(L+0x34)` 的 `+0x78/+0x7c`)。
    #[must_use]
    pub fn window(&self) -> Option<(i64, i64)> {
        self.window
    }

    /// 注入/清除目标层表面尺寸。`None` = 没有目标层(官方不自动绘制)。
    pub fn set_window(&mut self, size: Option<(i64, i64)>) {
        self.window = size;
    }

    /// 当前页的步骤日志长度(引擎增量拉取用)。
    #[must_use]
    pub fn steps_len(&self) -> usize {
        self.steps.len()
    }

    /// 当前页的步骤日志(诊断/测试用;渲染走 [`Self::text_frame`])。
    #[must_use]
    pub fn steps(&self) -> &[TextStep] {
        &self.steps
    }

    /// 取当前页 `since..` 的步骤(引擎每 tick 增量应用)。
    #[must_use]
    pub fn text_frame(&self, since: usize) -> TextFrame {
        let since = since.min(self.steps.len());
        TextFrame {
            layer_id: self.layer_id.clone(),
            output_mode: self.output_mode,
            page_clear_seq: self.page_clear_seq,
            cursor_x: self.cursor_x,
            cursor_y: self.cursor_y,
            click: self.click,
            bytes: self.bytes.clone(),
            flags: self.flags.clone(),
            font: self.font,
            first_step: since,
            total_steps: self.steps.len(),
            steps: self.steps[since..].to_vec(),
        }
    }

    /// 追加一段文本,并按字符写入 `'1'`/`'2'` 与逐字符快照(REA `0x458d44`)。
    ///
    /// 延迟按引擎 `+0x48` 表快照:`WAIT / (全角 ? 1 : 2)`
    /// (`WORD 0x458e78` 的 `idiv edi`;`_ 0x45697f/0x456a8e` 同口径)。
    pub fn append_text(&mut self, bytes: &[u8]) {
        self.append_text_with_codec(self.codec, bytes);
    }

    /// 按指定显示 codec 追加(全局 codec 由 VM 在启动时确定)。
    pub fn append_text_with_codec(&mut self, codec: yu_format::Codec, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.bytes.extend_from_slice(bytes);
        let wait = self.wait_ms as i32;
        let mut i = 0;
        while i < bytes.len() {
            // 官方按前导表判定;孤立前导字节也按全角标志写(不猜第二字节)。
            let lead = codec.is_lead(bytes[i]);
            let delay = if lead { wait } else { wait / 2 };
            self.flags.push(if lead { flag::FULL } else { flag::HALF });
            self.cells.push(TextCell {
                width: self.font.sx.clamp(0, 0x100) as i16,
                height: self.font.sy.clamp(0, 0x100) as i16,
                delay: delay as i16,
                fg: self.font.color,
                bg: self.font.color2,
                antialias: self.font.antialias,
                gradation: self.font.gradation,
                bold: self.font.bold,
            });
            i += usize::from(lead) + 1;
        }
    }

    /// 追加一个控制标志(`TEXT`/`RETURNCODE`),不写宽/高表(官方零项)。
    pub fn push_flag(&mut self, byte: u8) {
        self.flags.push(byte);
        self.cells.push(TextCell::default());
    }

    /// 追加一个带当前 `SX/SY` 的控制标志(官方 `TEXT` 的 `C/R/r/P/p`
    /// 与 `RETURNCODE` 的 `C/R` 在追加前写宽/高表)。
    pub fn push_flag_sized(&mut self, byte: u8) {
        self.flags.push(byte);
        self.cells.push(TextCell {
            width: self.font.sx.clamp(0, 0x100) as i16,
            height: self.font.sy.clamp(0, 0x100) as i16,
            ..TextCell::default()
        });
    }

    /// 切换目标层时重置“已提交表面”世代。
    ///
    /// 官方 `TEXT[ID]` 只改 `state+0x00`:旧层像素留在旧层,新层从空表面
    /// 继续画。内核的 step 日志是全局的,必须在新层上重新起一页,避免把
    /// 旧层字形重放进新层。
    pub fn reset_surface_epoch(&mut self) {
        self.page_clear_seq = self.page_clear_seq.wrapping_add(1);
        self.steps.clear();
    }

    /// `TEXT[CLEAR=1]` / 引擎 `0x453894` → `FUN_004638e4`。
    ///
    /// 只清待显示缓冲与四个游标,置 `+0x37`;光标、点击状态、目标层、
    /// **已提交字形**都不动。`clear_seq` 是内核桥接代数。
    pub fn clear(&mut self) {
        self.reset_pending();
        self.clear_pending = false;
        self.clear_seq = self.clear_seq.wrapping_add(1);
    }

    /// 行完成/手动释放时消费 `+0x36`(引擎 `FUN_004046fc` `0x404828`)。
    pub fn consume_clear_request(&mut self) {
        if self.clear_pending {
            self.reset_pending();
            self.clear_pending = false;
        }
    }

    fn reset_pending(&mut self) {
        self.bytes.clear();
        self.flags.clear();
        self.cells.clear();
        self.reveal_flags = 0;
        self.reveal_bytes = 0;
        self.timer_reset = true;
    }

    /// 当前显示段是否已消费到缓冲末尾。
    #[must_use]
    pub fn line_complete(&self) -> bool {
        self.reveal_flags >= self.flags.len()
    }

    /// 按官方 `FUN_004046fc`/`FUN_00404914`/`FUN_00404c60` 推进。
    ///
    /// 时间基 = 宿主 `tick_at(now_ms)`(毫秒;官方 `timeGetTime()` 同单位,
    /// REA `0x404c21`)。`advance` = 本帧点击或跳过按钮处于“推进”状态
    /// (由 VM 的输入状态给出)。
    ///
    /// 返回 `Some(byte)` 表示遇到未知 `_TEXT_FLAG` 字节(官方错误
    /// `0xdbbaa`),调用方负责挂号;布局在该字节处停止。
    ///
    /// `click` = 点击按钮出现按下沿;`skip` = 跳过按钮按住。官方
    /// `FUN_004046fc` **pre-layout 只检查跳过按钮**,post-layout 才检查
    /// 点击+跳过;两次不能合并,否则“逐字显示途中点击”会提前一帧快进。
    pub fn advance_reveal(&mut self, now_ms: u32, click: bool, skip: bool) -> Option<u8> {
        // 官方 `FUN_004046fc`:手动模式不调用 `FUN_00404914`。
        if self.output_mode != 1 {
            return None;
        }
        // 官方 `FUN_00404914` 入口:`state+0x00 == 0` 直接返回。
        self.window?;
        self.apply_advance(skip);
        let bad = self.layout(now_ms);
        self.apply_advance(click || skip);
        if bad.is_none() && self.line_complete() && self.reveal_flags > 0 {
            // `0x404824`:置 `+0x36`,随后 `FUN_004638e4` 由释放路径消费。
            self.clear_pending = true;
        }
        bad
    }

    /// 点击/跳过按钮的推进语义(`FUN_004046fc` 的 `0x404748/0x4047f6`)。
    fn apply_advance(&mut self, advance: bool) {
        if !advance {
            return;
        }
        if self.click {
            // 消费 'C':重置计时、清点击、标志游标 +1。
            self.timer_reset = true;
            self.click = false;
            self.reveal_flags = self.reveal_flags.saturating_add(1);
        } else if !self.line_complete() {
            // 不在点击等待 ⇒ 本帧快进(0x801c70=1)。
            self.skip_mode = true;
        }
    }

    fn layout(&mut self, now_ms: u32) -> Option<u8> {
        loop {
            if self.reveal_flags >= self.flags.len() {
                self.skip_mode = false;
                return None;
            }
            if self.timer_reset {
                self.timer_reset = false;
                self.last_char_ms = now_ms;
            }
            let idx = self.reveal_flags;
            match self.flags[idx] {
                flag::HALF | flag::FULL => {
                    if !self.timing_gate(now_ms) {
                        return None;
                    }
                    if self.wrap_or_stop() {
                        // 换行/悬挂/耗尽:本字符不再由调用方绘制。
                        self.reveal_flags += 1;
                        continue;
                    }
                    if !self.draw_current() {
                        // 数据不足:不猜、不越界。
                        return None;
                    }
                    self.reveal_flags += 1;
                }
                // 官方 `0x4049c2`:+0x35=1、0x801c70=0,停。
                flag::CLICK => {
                    self.click = true;
                    self.skip_mode = false;
                    return None;
                }
                // `'E'`:仅推进游标。
                flag::END => self.reveal_flags += 1,
                // `'P'/'p'`:清层表面并复位光标(`0x40498c`)。
                flag::PAGE | flag::CLICK_PAGE => {
                    self.cursor_x = 0;
                    self.cursor_y = 0;
                    self.page_clear_seq = self.page_clear_seq.wrapping_add(1);
                    self.steps.clear();
                    self.steps.push(TextStep::PageClear);
                    self.reveal_flags += 1;
                }
                // `'R'/'r'`:x=0、y += 该标志行高(`0x404976`)。
                flag::RETURN | flag::CLICK_RETURN => {
                    self.cursor_x = 0;
                    self.cursor_y += i64::from(self.cell(idx).height);
                    self.reveal_flags += 1;
                }
                other => return Some(other),
            }
        }
    }

    /// `FUN_00404c0c`:快进不等;缓冲首字符立即显示;之后按 `+0x48` 累加。
    fn timing_gate(&mut self, now_ms: u32) -> bool {
        if self.skip_mode || self.reveal_bytes == 0 {
            return true;
        }
        let delay = i32::from(self.cell(self.reveal_flags).delay) as u32;
        if now_ms.wrapping_sub(self.last_char_ms) < delay {
            return false;
        }
        self.last_char_ms = self.last_char_ms.wrapping_add(delay);
        true
    }

    fn cell(&self, idx: usize) -> TextCell {
        self.cells.get(idx).copied().unwrap_or_default() // silent-ok: 控制标志零快照(官方堆数组零项)
    }

    /// 当前字节游标处的双字节(不足补 0;禁则比较用)。
    fn bytes_at(&self, off: usize) -> [u8; 2] {
        [
            self.bytes.get(off).copied().unwrap_or(0), // silent-ok: 禁则比较不足补 0
            self.bytes.get(off.wrapping_add(1)).copied().unwrap_or(0), // silent-ok: 禁则比较不足补 0
        ]
    }

    /// 当前字符的显示宽度步进。
    ///
    /// 官方 `+0x14 += FUN_004056b8()`(GDI 实测 advance);固定幅 CJK 字体下
    /// 全角 = `SX`、半角 = `SX/2`。比例字体的逐像素差归 `DIV-FONT-002`,
    /// 这里按单元宽建模(`Likely`;见 `text-oracle-2026-11-16.md` §6)。
    fn advance(cell: &TextCell, full: bool) -> i64 {
        let sx = i64::from(cell.width.max(0));
        if full {
            sx
        } else {
            sx / 2
        }
    }

    fn glyph_style(cell: &TextCell) -> TextGlyphStyle {
        TextGlyphStyle {
            fg: cell.fg,
            bg: cell.bg,
            antialias: cell.antialias != 0,
            gradation: cell.gradation,
            bold: cell.bold != 0,
        }
    }

    /// 从 `reveal_bytes` 取 `len` 字节,产出一个已提交字形。
    ///
    /// `style_idx` 只用于取样式表;官方 `！？` 特例的第二字也用第一字的
    /// 样式(调用方传同一 `idx`)。
    fn draw_span(&mut self, len: usize, style_idx: usize) -> bool {
        let start = self.reveal_bytes;
        let Some(end) = start.checked_add(len) else {
            return false;
        };
        if end > self.bytes.len() {
            return false;
        }
        let full = len == 2;
        let cell = self.cell(style_idx);
        let mut bytes = smallvec::SmallVec::new();
        bytes.extend_from_slice(&self.bytes[start..end]);
        self.steps.push(TextStep::Glyph(TextGlyphDraw {
            codec: self.codec,
            bytes,
            full,
            x: self.cursor_x,
            y: self.cursor_y,
            width: cell.width,
            height: cell.height,
            style: Self::glyph_style(&cell),
            draw_mode: self.font.draw_mode,
            bno: self.font.bno,
        }));
        self.cursor_x += Self::advance(&cell, full);
        self.reveal_bytes = end;
        true
    }

    fn draw_current(&mut self) -> bool {
        let idx = self.reveal_flags;
        let len = if self.flags[idx] == flag::FULL { 2 } else { 1 };
        self.draw_span(len, idx)
    }

    /// `FUN_00404c60`:返回 `true` = 本字符不再由调用方绘制。
    ///
    /// 普通换行返回 `false`(调用方在新行绘制);纵向耗尽、`！？` 成对、
    /// 行头禁则悬挂返回 `true`(后两者已在内部画出字符)。
    fn wrap_or_stop(&mut self) -> bool {
        let Some((win_w, win_h)) = self.window else {
            return true;
        };
        let idx = self.reveal_flags;
        if self.reveal_bytes > self.bytes.len() {
            self.reveal_bytes = self.bytes.len();
        }
        let cur = self.cell(idx);
        let next_h = i64::from(self.cell(idx.saturating_add(1)).height);
        // 纵向:本行下一格放不下 ⇒ 文本耗尽(`0x404c85..0x404c90`)。
        if win_h <= self.cursor_y + 2 + next_h {
            self.reveal_bytes = self.bytes.len();
            return true;
        }
        let w = i64::from(cur.width.max(0));
        if self.cursor_x + 2 + w >= win_w {
            return self.wrap_line(idx);
        }
        if self.cursor_x + 2 + 2 * w < win_w {
            // 还剩至少两格;靠近右边界时处理 `！？` 与行末禁则。
            if win_w <= self.cursor_x + 2 + 4 * w && self.flags[idx] == flag::FULL {
                if self.bytes_at(self.reveal_bytes) == KINSOKU_PAIR_HEAD
                    && self.flags.get(idx + 2) == Some(&flag::FULL)
                    && self.bytes_at(self.reveal_bytes + 2) == KINSOKU_PAIR_TAIL
                {
                    // 官方:两个字都用当前标志样式画出,`+0x68 += 4`,
                    // 调用方只 `+0x6c += 1`(字节/标志游标故意不同步)。
                    let _ = self.draw_span(2, idx);
                    let _ = self.draw_span(2, idx);
                    return true;
                }
                if KINSOKU_LINE_END.contains(&self.bytes_at(self.reveal_bytes)) {
                    return self.wrap_line(idx);
                }
            }
            return false;
        }
        // 只剩一格:行头禁则悬挂;否则换行。
        if self.flags[idx] == flag::FULL
            && KINSOKU_LINE_START.contains(&self.bytes_at(self.reveal_bytes))
        {
            let _ = self.draw_span(2, idx);
            return true;
        }
        self.wrap_line(idx)
    }

    /// 换行并做纵向检查;返回 `false` = 调用方在新行绘制。
    fn wrap_line(&mut self, idx: usize) -> bool {
        let Some((_, win_h)) = self.window else {
            return true;
        };
        self.cursor_x = 0;
        self.cursor_y += i64::from(self.cell(idx).height);
        let next_h = i64::from(self.cell(idx.saturating_add(1)).height);
        if win_h <= self.cursor_y + 2 + next_h {
            self.reveal_bytes = self.bytes.len();
            return true;
        }
        false
    }

    /// 设置输出模式(引擎只存低字节)。
    pub fn set_output_mode(&mut self, v: i64) {
        self.output_mode = v as u8;
    }

    /// `TEXT[CLICKBUTTONn]`/`SKIPBUTTONn`:0 = 解除分配 → `0x3ff`。
    pub fn set_button(slot: &mut i64, v: i64) {
        *slot = if v == 0 { 0x3ff } else { v };
    }

    /// 读取文本状态里的当前 `FONT` 子集值(相对参数 `+=`/`-=` 用)。
    #[must_use]
    pub fn font_param(&self, name: &str) -> Option<i64> {
        Some(match name {
            "SX" => i64::from(self.font.sx),
            "SY" => i64::from(self.font.sy),
            "COLOR" => i64::from(self.font.color),
            "COLOR2" => i64::from(self.font.color2),
            "ANTIALIASING" => i64::from(self.font.antialias),
            "GRADATION" => i64::from(self.font.gradation),
            "BOLD" => i64::from(self.font.bold),
            "DRAWMODE" => i64::from(self.font.draw_mode),
            "BNO" => i64::from(self.font.bno),
            _ => return None,
        })
    }

    /// `FONT` 参数写入文本状态子集(参数名来自工程 ysc 表)。
    ///
    /// 槽序/映射(0.495 `FONT` 处理体 `0x444380..0x444b20`):
    /// `0 BNO、1 NO、2 NAME、3 ANTIALIASING、4 COLOR、5 COLOR2、
    /// 6 GRADATION、7 ROTATION、8 DRAWMODE、9 SX、10 SY、11 BOLD`。
    /// 文本状态只保存 `+0x20..+0x34` 的字段;`NO/NAME/ROTATION` 归字体对象
    /// (由 gfx/后端消费),这里不保存。
    pub fn apply_font_param(&mut self, name: &str, value: i64) {
        match name {
            "SX" => self.font.sx = value.clamp(0, 0x100) as i32,
            "SY" => self.font.sy = value.clamp(0, 0x100) as i32,
            "COLOR" => self.font.color = (value as u32) & 0x00FF_FFFF,
            "COLOR2" => self.font.color2 = (value as u32) & 0x00FF_FFFF,
            "ANTIALIASING" => self.font.antialias = value as u8,
            "GRADATION" => self.font.gradation = value.clamp(0, 8) as u8,
            "BOLD" => self.font.bold = value as u8,
            "DRAWMODE" => self.font.draw_mode = value.clamp(0, 1) as u8,
            "BNO" => self.font.bno = value as u8,
            _ => {}
        }
    }
}

/// 一次 tick 的文本增量(引擎 → gfx)。
///
/// `steps` 只含 `first_step..total_steps`;`page_clear_seq`/`layer_id`
/// 变化时调用方必须从 0 重放(见 `GfxState::apply_text_frame`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFrame {
    pub layer_id: Vec<u8>,
    pub output_mode: u8,
    pub page_clear_seq: u64,
    pub cursor_x: i64,
    pub cursor_y: i64,
    pub click: bool,
    /// 当前待显示缓冲(诊断镜像;引擎渲染只用 `steps`)。
    pub bytes: Vec<u8>,
    pub flags: Vec<u8>,
    /// 当前 `FONT` 子集(诊断/字体身份提示)。
    pub font: TextFontState,
    pub first_step: usize,
    pub total_steps: usize,
    pub steps: Vec<TextStep>,
}

/// 文本层状态的冻结快照(诊断/探针;渲染用 [`TextFrame`])。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSnapshot {
    /// 当前文本层 ID(CP932 原始字节;权威层键,不做 UTF-8 转换)。
    pub layer_id: Vec<u8>,
    pub bytes: Vec<u8>,
    pub flags: Vec<u8>,
    pub cells: Vec<TextCell>,
    pub font: TextFontState,
    /// 显示 codec(全局;官方 CHS 同构)。
    pub codec: yu_format::Codec,
    pub output_mode: u8,
    pub wait_ms: i64,
    pub autoreturn: i64,
    pub autopage: i64,
    pub autoclick: i64,
    pub click: bool,
    pub clear_pending: bool,
    pub clear_seq: u64,
    pub page_clear_seq: u64,
    /// 当前显示段标志游标(引擎 `+0x6c`)。
    pub reveal_flags: usize,
    /// 当前显示段已布局字节数(引擎 `+0x68`)。
    pub reveal_bytes: usize,
    /// 快进模式(引擎 `0x801c70`;审计/诊断)。
    pub skip_mode: bool,
    pub cursor_x: i64,
    pub cursor_y: i64,
    pub click_buttons: [i64; 4],
    pub skip_buttons: [i64; 4],
    pub window: Option<(i64, i64)>,
}

impl From<&TextState> for TextSnapshot {
    fn from(s: &TextState) -> Self {
        Self {
            layer_id: s.layer_id.clone(),
            bytes: s.bytes.clone(),
            flags: s.flags.clone(),
            cells: s.cells.clone(),
            font: s.font,
            codec: s.codec,
            output_mode: s.output_mode,
            wait_ms: s.wait_ms,
            autoreturn: s.autoreturn,
            autopage: s.autopage,
            autoclick: s.autoclick,
            click: s.click,
            clear_pending: s.clear_pending,
            clear_seq: s.clear_seq,
            page_clear_seq: s.page_clear_seq,
            reveal_flags: s.reveal_flags,
            reveal_bytes: s.reveal_bytes,
            skip_mode: s.skip_mode,
            cursor_x: s.cursor_x,
            cursor_y: s.cursor_y,
            click_buttons: s.click_buttons,
            skip_buttons: s.skip_buttons,
            window: s.window,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_window(w: i64, h: i64) -> TextState {
        let mut t = TextState::default();
        t.set_window(Some((w, h)));
        t
    }

    #[test]
    fn append_text_flags_and_cells_match_cp932() {
        let mut t = TextState::default();
        t.font.sx = 24;
        t.font.sy = 30;
        t.font.color = 0x11_2233;
        t.font.color2 = 0x44_5566;
        t.font.antialias = 1;
        t.font.gradation = 3;
        t.font.bold = 1;
        t.wait_ms = 15;
        // "AこB":A=半角,こ=82 B1 全角,B=半角。
        t.append_text(b"A\x82\xB1B");
        assert_eq!(t.bytes, b"A\x82\xB1B");
        assert_eq!(t.flags, vec![flag::HALF, flag::FULL, flag::HALF]);
        assert_eq!(t.cells.len(), 3);
        assert_eq!(t.cells[0].width, 24);
        assert_eq!(t.cells[0].height, 30);
        assert_eq!(t.cells[0].delay, 7, "半角 WAIT/2");
        assert_eq!(t.cells[1].delay, 15, "全角 WAIT");
        assert_eq!(t.cells[2].fg, 0x11_2233);
        assert_eq!(t.cells[2].bg, 0x44_5566);
        assert_eq!(
            (t.cells[2].antialias, t.cells[2].gradation, t.cells[2].bold),
            (1, 3, 1)
        );
    }

    #[test]
    fn control_flags_and_clear_keep_cursor_and_surface() {
        let mut t = TextState::default();
        t.push_flag_sized(flag::RETURN);
        t.push_flag(flag::END);
        assert_eq!(t.flags, vec![b'R', b'E']);
        assert_eq!(t.cells[0].height, 20, "RETURN 带 SX/SY");
        assert_eq!(t.cells[1].height, 0, "E 不写宽/高");
        t.cursor_x = 7;
        t.cursor_y = 9;
        t.click = true;
        t.steps.push(TextStep::PageClear);
        t.clear();
        assert!(t.flags.is_empty() && t.bytes.is_empty());
        assert_eq!(t.cursor_x, 7, "引擎 CLEAR 不重置光标 +0x14");
        assert_eq!(t.cursor_y, 9, "引擎 CLEAR 不重置光标 +0x18");
        assert!(t.click, "引擎 CLEAR 不重置点击状态 +0x35");
        assert_eq!(t.steps.len(), 1, "CLEAR 不清已提交字形");
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
        let t = TextState::default();
        assert_eq!(t.click_buttons, [1, 13, 0x3ff, 0x3ff]);
        assert_eq!(t.skip_buttons, [17, 0x3ff, 0x3ff, 0x3ff]);
    }

    #[test]
    fn reveal_uses_full_half_delays_and_click_stops() {
        let mut t = with_window(800, 600);
        t.wait_ms = 15;
        t.append_text(b"A\x82\xa0"); // A 半角,あ 全角
        t.push_flag(flag::CLICK);
        t.push_flag_sized(flag::RETURN);
        t.push_flag(flag::END);
        assert_eq!(t.cells[0].delay, 7);
        assert_eq!(t.cells[1].delay, 15);

        t.advance_reveal(0, false, false);
        assert_eq!((t.reveal_flags, t.reveal_bytes), (1, 1));
        t.advance_reveal(14, false, false);
        assert_eq!(t.reveal_bytes, 1, "全角延迟 15ms 未到");
        t.advance_reveal(15, false, false);
        assert_eq!((t.reveal_flags, t.reveal_bytes), (2, 3));
        assert!(t.click, "遇到 'C' 置点击等待并停");
        assert_eq!(t.steps.len(), 2);
        match &t.steps[0] {
            TextStep::Glyph(g) => assert_eq!((g.x, g.y, g.full), (0, 0, false)),
            other => panic!("first step = {other:?}"),
        }
        match &t.steps[1] {
            TextStep::Glyph(g) => assert_eq!((g.x, g.y, g.full), (10, 0, true)),
            other => panic!("second step = {other:?}"),
        }
        assert_eq!(t.cursor_x, 30);

        // 点击推进:post-layout 只消费 'C'(官方 pre 不检查点击按钮),
        // R/E 在下一 tick 处理;行完成置 +0x36。
        t.advance_reveal(15, true, false);
        assert!(!t.click);
        assert!(!t.line_complete(), "点击只消费 'C',R/E 留到下一帧");
        t.advance_reveal(15, false, false);
        assert_eq!(t.cursor_x, 0);
        assert_eq!(t.cursor_y, 20, "R 用该标志行高(SY=20)");
        assert!(t.line_complete());
        assert!(t.clear_pending);
    }

    #[test]
    fn p_clears_cursor_and_pushes_page_clear() {
        let mut t = with_window(800, 600);
        t.append_text(b"AB");
        t.push_flag_sized(flag::PAGE);
        t.advance_reveal(0, false, false);
        // `P` 清空当前页日志并只留一条 PageClear(gfx 据此清已提交字形)。
        assert_eq!(t.steps, vec![TextStep::PageClear]);
        assert_eq!((t.cursor_x, t.cursor_y), (0, 0));
        assert_eq!(t.page_clear_seq, 1);
    }

    #[test]
    fn wrap_moves_to_next_line_before_drawing() {
        let mut t = with_window(70, 600);
        t.font.sx = 20;
        t.font.sy = 20;
        // 3 个全角「あ」(CP932 0x82A0)。
        t.append_text(b"\x82\xa0\x82\xa0\x82\xa0");
        t.advance_reveal(0, false, false);
        let pos: Vec<(i64, i64)> = t
            .steps
            .iter()
            .filter_map(|s| match s {
                TextStep::Glyph(g) => Some((g.x, g.y)),
                _ => None,
            })
            .collect();
        // W=70:x=0、20 各画一字;第三字 40+2+20=62<70 且 40+2+40=82>=70
        // ⇒ 只剩一格分支,非禁则 ⇒ 换行到 y=20、x=0。
        assert_eq!(pos, vec![(0, 0), (20, 0), (0, 20)]);
    }

    #[test]
    fn line_end_prohibition_wraps_before_open_bracket() {
        let mut t = with_window(44, 600);
        t.font.sx = 20;
        // 「(0x8175)在只剩一格时换行;'A' 先占 0。
        t.append_text(b"A\x81\x75");
        t.advance_reveal(0, false, false);
        let glyphs: Vec<(i64, i64)> = t
            .steps
            .iter()
            .filter_map(|s| match s {
                TextStep::Glyph(g) => Some((g.x, g.y)),
                _ => None,
            })
            .collect();
        // A: 0;「: x=10+2+20=32<44 且 10+2+40=52>=44 ⇒ 只剩一格分支,
        // 「 是行末禁则 ⇒ 换行到 y=20、x=0。
        assert_eq!(glyphs, vec![(0, 0), (0, 20)]);
    }

    #[test]
    fn hanging_punctuation_stays_on_line() {
        let mut t = with_window(45, 600);
        t.font.sx = 20;
        t.append_text(b"A\x81\x42"); // 。 是行头禁则
        t.advance_reveal(0, false, false);
        let glyphs: Vec<(i64, i64)> = t
            .steps
            .iter()
            .filter_map(|s| match s {
                TextStep::Glyph(g) => Some((g.x, g.y)),
                _ => None,
            })
            .collect();
        // A 在 0;。在 x=10:10+2+20=32<45 且 10+2+40=52>=45 ⇒ 只剩一格,
        // 行头禁则悬挂(不进下一行)。
        assert_eq!(glyphs, vec![(0, 0), (10, 0)]);
    }

    #[test]
    fn pair_kinsoku_draws_two_glyphs_with_one_flag() {
        let mut t = with_window(45, 600);
        t.font.sx = 11;
        t.wait_ms = 15;
        // 剩余不足四格:！？ + 第三个全角字,触发成对禁则。
        // ！=0x8149、？=0x8148、あ=0x82A0。
        t.append_text(b"\x81\x49\x81\x48\x82\xa0");
        t.advance_reveal(0, false, false);
        let glyphs: Vec<(i64, i64)> = t
            .steps
            .iter()
            .filter_map(|s| match s {
                TextStep::Glyph(g) => Some((g.x, g.y)),
                _ => None,
            })
            .collect();
        // W=45、SX=11:0+2+44=46>=45 且 0+2+22<45 ⇒ 成对禁则直接画
        // ！(0)与 ？(11);之后计时门挡住第三字(WAIT=15)。
        assert_eq!(glyphs, vec![(0, 0), (11, 0)]);
        // 字节游标 4、标志游标 1(官方不对称,由调用方 +1 后继续)。
        assert_eq!((t.reveal_bytes, t.reveal_flags), (4, 1));
    }

    #[test]
    fn switching_surface_epoch_clears_journal_but_not_buffer() {
        let mut t = with_window(800, 600);
        t.append_text(b"A");
        t.advance_reveal(0, false, false);
        assert_eq!(t.steps.len(), 1);
        let seq = t.page_clear_seq;
        t.reset_surface_epoch();
        assert_eq!(t.steps.len(), 0, "新层从空表面开始");
        assert_eq!(t.page_clear_seq, seq.wrapping_add(1));
        assert_eq!(t.bytes, b"A", "待显示缓冲不清(官方只换 state+0x00)");
    }

    #[test]
    fn no_target_layer_does_not_advance() {
        let mut t = TextState::default();
        t.append_text(b"A");
        t.push_flag(flag::END);
        t.advance_reveal(1000, false, false);
        assert_eq!(t.reveal_flags, 0, "state+0x00==0 官方直接返回");
        assert!(t.steps.is_empty());
    }

    #[test]
    fn manual_mode_does_not_advance_reveal() {
        let mut t = with_window(800, 600);
        t.set_output_mode(0);
        t.append_text(b"A");
        t.advance_reveal(1000, false, false);
        assert_eq!(t.reveal_flags, 0, "手动模式无引擎自动推进");
    }

    #[test]
    fn unknown_flag_stops_and_reports() {
        let mut t = with_window(800, 600);
        t.push_flag(b'X');
        assert_eq!(t.advance_reveal(0, false, false), Some(b'X'));
        assert_eq!(t.reveal_flags, 0);
    }
}
