//! 输入状态机:**带符号帧计数器**(不是 bool,也不是事件流)。
//!
//! # 语义(手册 `basic/input.html`,一手)
//!
//! > 「Ａ」キーが押されている間、…@_KEY_A の値が押された瞬間から
//! > `1 → 2 → 3 → …` と１フレーム毎に変化していきます。
//! > 「Ａ」キーが離された場合…`-1 → -2 → -3 → …`
//! > `==1` = 本帧按下;`>0` = 持续按住;`>=60` = 按住一秒(FPS60)。
//!
//! 系统变量页(`basic/svariable.html`)补充:`@_INP(n)` 是数组版;
//! `@_KEY_ANY` 任意键;`@_KEY_*` 覆盖 A–Z/0–9/小键盘/F1–F24/编辑键区/符号键。
//!
//! # 键位 → 系统变量 id
//!
//! **不硬编码**:id 由工程 `YSCom.ycd` 的系统变量段顺序决定
//! (`_KEY_A`=167 … `_KEY_ANY`=275;0.495 实测),本模块只按**名字前缀**
//! 构造查询,id 由调用方从 [`crate::tables::BootTables::ycd`] 解析后传入
//! ([`InputState::bind`])。
//!
//! # 注入(A3 确定性)
//!
//! 时钟与输入全部由调用方注入:[`InputTimeline`] 是「帧号 → 按下集合」的
//! 时间线,`Vm` 每逻辑帧调用 [`InputState::advance`]。内核**绝不读真实输入设备**。

use std::collections::{BTreeMap, BTreeSet};

/// 一个物理键/鼠标键/手柄钮的逻辑名(与 ycd 系统变量名同构,不含前缀 `_`)。
///
/// 例:`KEY_A`、`MOUSE_L`、`PAD_BUTTON`。
pub type ButtonName = String;

/// 系统变量 id 绑定表:名字 → id(全部来自工程 ycd)。
#[derive(Clone, Debug, Default)]
pub struct InputBinding {
    /// 按钮 → 带符号计数器变量 id。
    key_ids: BTreeMap<ButtonName, u16>,
    /// 按钮 → 累计次数变量 id(`_KEYNUM_*` / `_MOUSENUM_*`)。
    count_ids: BTreeMap<ButtonName, u16>,
    /// 数组版 `_INP` 的 id。
    inp_id: Option<u16>,
    /// 累计数值变量 `_KEYLAST` / `_MOUSELAST`。
    keylast_id: Option<u16>,
    mouselast_id: Option<u16>,
    /// 任意键 `_KEY_ANY`。
    any_id: Option<u16>,
    /// `_INP` 数组长度(工程声明;0 = 未声明)。
    inp_len: usize,
    /// `_CODE_<X>`(键盘/鼠标段)系统变量 id → 引擎按钮码。
    /// 键盘/手柄段算术码(`_CODE_PAD_*`)不预绑定,按需计算。
    code_ids: BTreeMap<u16, u16>,
}

impl InputBinding {
    /// 从系统变量 ABI 表构造(有 ycd 时来自 ycd,商业包时来自 ABI 数据表)。
    ///
    /// - `_KEY_<X>` / `_MOUSE_<X>` → 按钮计数器;
    /// - `_KEYNUM_<X>` / `_MOUSENUM_<X>` → 累计次数;
    /// - `_INP` / `_KEYLAST` / `_MOUSELAST` / `_KEY_ANY` → 特殊变量。
    ///
    /// `_CODE_*` 常量(输入码)不在此建模:它们只用于 `TEXT[CLICKBUTTON=…]`
    /// 之类的参数,属文本层配置(见 DIV-RT-006)。
    pub fn from_sysvars(sysvars: &crate::sysvar_abi::SysVarTable) -> Self {
        let mut b = InputBinding::default();
        for (id, decl) in sysvars.decls.iter().enumerate() {
            let id = id as u16;
            let name = decl.name.as_str();
            match name {
                "_INP" => {
                    b.inp_id = Some(id);
                    b.inp_len = decl.dims.first().copied().unwrap_or(0) as usize;
                }
                "_KEYLAST" => b.keylast_id = Some(id),
                "_MOUSELAST" => b.mouselast_id = Some(id),
                "_KEY_ANY" => b.any_id = Some(id),
                _ => {
                    if let Some(rest) = name.strip_prefix("_CODE_") {
                        // `_CODE_<X>`:键盘/鼠标段查 ABI 表;PAD 段是纯算术
                        // (REA case 0x21f/0x223),不占绑定。
                        if let Some(code) = button_code(rest) {
                            b.code_ids.insert(id, code);
                        }
                    } else if let Some(rest) = name.strip_prefix("_KEYNUM_") {
                        b.count_ids.insert(format!("KEY_{rest}"), id);
                    } else if let Some(rest) = name.strip_prefix("_MOUSENUM_") {
                        b.count_ids.insert(format!("MOUSE_{rest}"), id);
                    } else if let Some(rest) = name.strip_prefix("_KEY_") {
                        b.key_ids.insert(format!("KEY_{rest}"), id);
                    } else if let Some(rest) = name.strip_prefix("_MOUSE_") {
                        // `_MOUSE_X/Y` 等坐标不是按钮;只收按钮族。
                        if is_mouse_button(rest) {
                            b.key_ids.insert(format!("MOUSE_{rest}"), id);
                        }
                    }
                }
            }
        }
        b
    }

    pub fn key_id(&self, button: &str) -> Option<u16> {
        self.key_ids.get(button).copied()
    }

    /// 反向查:系统变量 id → 按钮名(用于写入输入系统变量)。
    pub fn button_of_id(&self, id: u16) -> Option<&str> {
        self.key_ids
            .iter()
            .find(|(_, &v)| v == id)
            .map(|(k, _)| k.as_str())
    }

    pub fn count_id(&self, button: &str) -> Option<u16> {
        self.count_ids.get(button).copied()
    }

    pub fn inp_id(&self) -> Option<u16> {
        self.inp_id
    }

    pub fn inp_len(&self) -> usize {
        self.inp_len
    }

    pub fn keylast_id(&self) -> Option<u16> {
        self.keylast_id
    }

    pub fn mouselast_id(&self) -> Option<u16> {
        self.mouselast_id
    }

    pub fn any_id(&self) -> Option<u16> {
        self.any_id
    }

    /// `_CODE_<X>`(键盘/鼠标段)系统变量 id → 引擎按钮码(ABI 缺省值)。
    pub fn code_of_id(&self, id: u16) -> Option<u16> {
        self.code_ids.get(&id).copied()
    }

    /// 全部按钮名(确定顺序),供宿主枚举。
    pub fn buttons(&self) -> impl Iterator<Item = &str> {
        self.key_ids.keys().map(|s| s.as_str())
    }
}

fn is_mouse_button(rest: &str) -> bool {
    // 按钮族 = 单个字母 + 可选数字(1..3)/`NUM_` 前缀。
    let base = rest.trim_start_matches("NUM_");
    let mut chars = base.chars();
    matches!(chars.next(), Some('L') | Some('R') | Some('M') | Some('U') | Some('D'))
        && chars.all(|c| c.is_ascii_digit())
}

/// 引擎按钮码表(键盘/鼠标段)——**数据文件** `data/button-code-abi.tsv`,
/// 编译期内嵌(`include_str!`),首用时解析一次。
///
/// # 证据链(2026-10-19 REA 复核,官方 0.495 引擎 yu-ris.exe,
/// sha256 `4f5eefa3…`,Ghidra `FUN_00427350` = 系统变量读取总分派)
///
/// 1. **单一 VK 计数表**:`_INP(n)`(case 0x90)读 `DAT_00685800[n*8]`
///    (64 位;guard `n<=0 → 0`);`_KEY_*`/`_MOUSE_*`(case 0x93..0x113)
///    经字节表 `DAT_0068996d[id]` 读**同一张**表 ⇒ 按键状态与 `_INP` 同址。
/// 2. **`_CODE_*` = 计数表下标**(case 0x19f..0x21e):值 = 字节表
///    `DAT_00689861[id]`(BSS,运行期填充,静态不可读)。由手册不变量
///    `@_INP(@_CODE_Z)` ≡ `@_KEY_Z`(手册 `basic/svariable.html` ※3)⇒
///    `_CODE_Z` = `_KEY_Z` 的计数表下标 = **Windows 虚拟键码(VK)**;
///    A–Z/0–9/F1–F24/小键盘/编辑键区取 VK 规范值,日文键盘符号键用
///    JP 106 键 OEM 映射(0xBD 减号 / 0xDE 波浪 / 0xDC ¥ / 0xC0 @ /
///    0xBA ;与:同键 / 0xE2 \)。与旧线 `<repo-old>` 的独立逆向结论
///    (`crates/yuris-input/src/keys.rs`)互证一致。
/// 3. **鼠标 1..3 号变体同基码**:`_CODE_MOUSE_L1/L2/L3`(ycd 415..434
///    每基 4 连号)= 基名码(手册只定义基名;见 [`button_code`] 的回退)。
/// 4. 手柄段是**纯算术**(case 0x114/0x118/0x21f/0x223),见
///    [`pad_dir_code`] / [`pad_button_code`],不进数据表。
///
/// 运行期可被脚本改写(手册 ○;键位重映射),见 [`InputState::set_code`]。
static BUTTON_CODE_ABI: std::sync::LazyLock<Vec<(String, u16)>> = std::sync::LazyLock::new(|| {
    parse_button_code_abi(include_str!("../data/button-code-abi.tsv"))
});

fn parse_button_code_abi(tsv: &str) -> Vec<(String, u16)> {
    tsv.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let (name, code) = l.split_once('\t')?;
            Some((name.trim().to_string(), code.trim().parse().ok()?))
        })
        .collect()
}

/// 按钮基名(无 `KEY_`/`MOUSE_` 前缀)→ 引擎按钮码。
///
/// 鼠标 1..3 号变体(`MOUSE_L1` 等)回退到基名:ycd 415..434 每基 4 连号
/// (L,L1,L2,L3),引擎字节表对四者存同值(计数表只有 5 个鼠标下标)。
pub fn button_code(base: &str) -> Option<u16> {
    let table = &*BUTTON_CODE_ABI;
    let hit = |n: &str| table.iter().find(|(t, _)| t == n).map(|(_, c)| *c);
    hit(base).or_else(|| {
        let trimmed = base.trim_end_matches(|c: char| c.is_ascii_digit());
        if trimmed.len() != base.len() {
            hit(trimmed)
        } else {
            None
        }
    })
}

/// 手柄段几何(REA:`FUN_00427350` case 0x114/0x118/0x21f/0x223)。
pub const PAD_BASE: i64 = 384; // 0x180
/// 每只手柄占的下标数(4 方向 + 32 钮,REA `n*36`)。
pub const PAD_STRIDE: i64 = 36;
/// 手柄钮区在手柄块内的起点(方向占 0..3)。
pub const PAD_BUTTON_OFFSET: i64 = 4;

/// `_PAD_*(n)` / `_CODE_PAD_*(n)` 的按钮码(方向序 0=UP 1=DOWN 2=LEFT 3=RIGHT)。
pub fn pad_dir_code(dir: i64, pad: i64) -> i64 {
    PAD_BASE + pad * PAD_STRIDE + dir
}

/// `_PAD_BUTTON(n,m)` / `_CODE_PAD_BUTTON(n,m)` 的按钮码。
pub fn pad_button_code(pad: i64, button: i64) -> i64 {
    PAD_BASE + pad * PAD_STRIDE + PAD_BUTTON_OFFSET + button.rem_euclid(32)
}

/// 按钮码 → 计数器键名(`KEY_*`/`MOUSE_*`,与 `InputTimeline` 同构;
/// 同码多名如 `;`/`:` 取表序第一个)。
pub fn button_of_code(code: i64) -> Option<String> {
    let code = u16::try_from(code).ok()?;
    BUTTON_CODE_ABI.iter().find(|(_, c)| *c == code).map(|(n, _)| {
        if n.starts_with("MOUSE") {
            n.clone()
        } else {
            format!("KEY_{n}")
        }
    })
}
/// 一帧的输入状态(**按下集合**,不是计数器)。
///
/// `pulses` 是只在该帧生效的按下(滚轮一格、单帧点击);它不进
/// [`InputTimeline::effective`] 的“按住保持”,避免滚轮脉冲被后续帧沿用。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputFrame {
    pub pressed: BTreeSet<ButtonName>,
    pub pulses: BTreeSet<ButtonName>,
    /// 光标逻辑坐标(鼠标域;`@_MOUSE_X/Y`)。
    pub cursor: (i64, i64),
}

impl InputFrame {
    pub fn new(pressed: impl IntoIterator<Item = impl Into<ButtonName>>) -> Self {
        Self {
            pressed: pressed.into_iter().map(Into::into).collect(),
            pulses: BTreeSet::new(),
            cursor: (0, 0),
        }
    }

    pub fn with_cursor(mut self, x: i64, y: i64) -> Self {
        self.cursor = (x, y);
        self
    }

    /// 单帧脉冲(本帧按下、下帧自动抬起)。
    pub fn with_pulse(mut self, button: impl Into<ButtonName>) -> Self {
        self.pulses.insert(button.into());
        self
    }

    pub fn is_pressed(&self, button: &str) -> bool {
        self.pressed.contains(button) || self.pulses.contains(button)
    }
}

/// 输入时间线:帧号 → 该帧按下的按钮集合。缺省帧 = 未按任何键
/// (手册:未按下时计数器每帧 `-1`)。
#[derive(Clone, Debug, Default)]
pub struct InputTimeline {
    frames: BTreeMap<u64, InputFrame>,
}

impl InputTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设定某帧的按下集合。同帧重复设定 → 后者覆盖(确定性)。
    pub fn set(&mut self, frame: u64, frame_state: InputFrame) {
        self.frames.insert(frame, frame_state);
    }

    pub fn at(&self, frame: u64) -> Option<&InputFrame> {
        self.frames.get(&frame)
    }

    /// 该帧是否处于「按住」(用于片段内插值:上一次显式设定后保持)。
    pub fn effective(&self, frame: u64) -> InputFrame {
        match self.frames.range(..=frame).next_back() {
            Some((_, f)) => f.clone(),
            None => InputFrame::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// `INPUT[KEY/MOUSE/PAD]` 与 `MOUSE[...]` 的脚本可写状态。
///
/// 证据(0.495 REA):
/// - `INPUT` 处理体 `0x446564`:KEY → `DAT_005b4888`、PAD → `DAT_008015d4`、
///   MOUSE → `DAT_005b488c`(带一次“重启用吞点击”锁存);
/// - `MOUSE` 处理体 `0x44aaa0`:MODE/TIME/MOVE/CLICK/WHEEL/X/Y/CURSOR/HX/HY/ID;
/// - `FUN_0041a4b8`:三个源开关分别整段跳过鼠标/键盘/手柄的计数器更新;
///   MOVE/CLICK/WHEEL 只参与“自动隐藏后是否重新显示光标”的判定;
/// - 手册 `system/input.html` / `system/mouse.html`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSettings {
    /// `INPUT[KEY]`(缺省启用)。
    pub keyboard: bool,
    /// `INPUT[MOUSE]`(缺省启用)。
    pub mouse: bool,
    /// `INPUT[PAD]`(缺省启用)。
    pub pad: bool,
    /// `MOUSE[MODE]`:0 常隐 / 1 自动隐藏 / 2 常显。
    pub mouse_mode: u8,
    /// `MOUSE[TIME]`(毫秒)。
    pub mouse_time_ms: i64,
    /// `MOUSE[MOVE]`。
    pub move_reactive: bool,
    /// `MOUSE[CLICK]`。
    pub click_reactive: bool,
    /// `MOUSE[WHEEL]`。
    pub wheel_reactive: bool,
    /// `MOUSE[CURSOR]`(0..13)。
    pub cursor: i64,
    /// `MOUSE[HX]/[HY]`:自定义光标热点。
    pub cursor_hotspot: (i64, i64),
    /// `MOUSE[ID]`:自定义光标图层 ID。
    pub cursor_id: Vec<u8>,
    /// `MOUSE[X/Y]` 的光标 warp 请求(宿主消费;None = 无请求)。
    pub cursor_warp: Option<(Option<i64>, Option<i64>)>,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            keyboard: true,
            mouse: true,
            pad: true,
            // 引擎在窗口创建时显示系统光标;ERIS 启动即 `MOUSE[MODE=1 ...]`。
            // BSS 零值不是脚本可观测缺省,这里取“可见”作为宿主初态。
            mouse_mode: 2,
            mouse_time_ms: 2500,
            move_reactive: true,
            click_reactive: true,
            wheel_reactive: true,
            cursor: 0,
            cursor_hotspot: (0, 0),
            cursor_id: Vec::new(),
            cursor_warp: None,
        }
    }
}

/// 输入源(按按钮码分类,不按名字)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    Keyboard,
    Mouse,
    Pad,
}

/// 引擎计数器表的一段(REA `FUN_0041a4b8`):
/// 鼠标 1..7、键盘 VK 8..255、手柄 384+;`0` 是 `_KEY_ANY`。
///
/// 分类只看码,不看名字:这样脚本重映射 `_CODE_*`、宿主换键名都不影响。
pub fn source_of_code(code: u16) -> InputSource {
    if code == 0 {
        InputSource::Keyboard
    } else if code <= 7 {
        InputSource::Mouse
    } else if code >= PAD_BASE as u16 {
        InputSource::Pad
    } else {
        InputSource::Keyboard
    }
}

/// 单个按钮的带符号计数器(引擎 64 位 `value` + 独立 `down` 标志)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counter {
    /// 当前值:按下为正、松开为负;从未按过 = 逐帧负增长。
    pub value: i64,
    /// 上一帧的物理按下状态(引擎 `DAT_00685400`)。
    pub down: bool,
}

impl Counter {
    fn step(&mut self, down: bool) {
        self.value = match (self.down, down) {
            (false, false) => self.value.wrapping_sub(1),
            (false, true) => 1,
            (true, false) => -1,
            (true, true) => self.value.wrapping_add(1),
        };
        self.down = down;
    }

    /// 引擎 `FUN_00447014` 的脚本写回钳制:
    /// 未按住时正数/零 → 0,负数保留;按住时正数保留,零/负数 → 0。
    fn write(&mut self, value: i64) {
        self.value = if self.down {
            if value > 0 {
                value
            } else {
                0
            }
        } else if value >= 0 {
            0
        } else {
            value
        };
    }
}

/// 输入运行状态:每逻辑帧推进计数器。
///
/// 语义严格按手册 + REA:
/// - 四态转移 `(prev_down, down)`:`0/0 → value-1`、`0/1 → 1`、
///   `1/0 → -1`、`1/1 → value+1`(`FUN_0041a4b8` + `0x801e84` 函数表);
/// - `INPUT` 关闭某个源时**整段跳过**该源的计数器更新(状态冻结);
/// - 脚本写输入变量按物理状态钳制(`FUN_00447014`),见 [`Counter::write`]。
#[derive(Clone, Debug)]
pub struct InputState {
    counters: BTreeMap<u16, Counter>,
    counts: BTreeMap<u16, i64>,
    settings: InputSettings,
    prev: InputFrame,
    /// 本帧快照(供系统变量读取)。
    current: InputFrame,
    /// `_KEYLAST` / `_MOUSELAST`:最近一次按下的按钮码。
    keylast: i64,
    mouselast: i64,
    /// `_CODE_*` 脚本改写覆盖(手册 ○ 可赋值;键位重映射)。
    code_overrides: BTreeMap<u16, i64>,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    pub fn new() -> Self {
        let mut counters = BTreeMap::new();
        // `_KEY_ANY` 的专用计数器(引擎下标 0)。
        counters.insert(0, Counter::default());
        // 鼠标计数器 1..7(`FUN_0041a4b8` 的定长段)。
        for code in 1..=7u16 {
            counters.insert(code, Counter::default());
        }
        // 键盘:ABI 表里的每个码都建计数器。名字重复(如 `;`/`:`)按码去重。
        for (_, code) in BUTTON_CODE_ABI.iter() {
            counters.entry(*code).or_insert_with(Counter::default);
        }
        Self {
            counters,
            counts: BTreeMap::new(),
            settings: InputSettings::default(),
            prev: InputFrame::default(),
            current: InputFrame::default(),
            keylast: 0,
            mouselast: 0,
            code_overrides: BTreeMap::new(),
        }
    }

    /// 脚本可见的输入设置(宿主用于光标显示/形状)。
    pub fn settings(&self) -> &InputSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut InputSettings {
        &mut self.settings
    }

    /// `INPUT[KEY/PAD/MOUSE]`;`None` = 未给出,保持原值。
    pub fn apply_input(&mut self, key: Option<bool>, pad: Option<bool>, mouse: Option<bool>) {
        if let Some(v) = key {
            self.settings.keyboard = v;
        }
        if let Some(v) = pad {
            self.settings.pad = v;
        }
        if let Some(v) = mouse {
            self.settings.mouse = v;
        }
    }

    /// `MOUSE[...]` 的已出现字段。数值范围按手册 clamp(MODE 0..2,CURSOR 0..13)。
    #[allow(clippy::too_many_arguments)]
    pub fn apply_mouse(
        &mut self,
        mode: Option<i64>,
        time_ms: Option<i64>,
        move_reactive: Option<bool>,
        click_reactive: Option<bool>,
        wheel_reactive: Option<bool>,
        cursor: Option<i64>,
        x: Option<i64>,
        y: Option<i64>,
        hx: Option<i64>,
        hy: Option<i64>,
        id: Option<&[u8]>,
    ) {
        if let Some(v) = mode {
            self.settings.mouse_mode = v.clamp(0, 2) as u8;
        }
        if let Some(v) = time_ms {
            self.settings.mouse_time_ms = v.max(0);
        }
        if let Some(v) = move_reactive {
            self.settings.move_reactive = v;
        }
        if let Some(v) = click_reactive {
            self.settings.click_reactive = v;
        }
        if let Some(v) = wheel_reactive {
            self.settings.wheel_reactive = v;
        }
        if let Some(v) = cursor {
            self.settings.cursor = v.clamp(0, 13);
        }
        if let Some(v) = hx {
            self.settings.cursor_hotspot.0 = v;
        }
        if let Some(v) = hy {
            self.settings.cursor_hotspot.1 = v;
        }
        if let Some(v) = id {
            self.settings.cursor_id = v.to_vec();
        }
        if x.is_some() || y.is_some() {
            self.settings.cursor_warp = Some((x, y));
            if let Some(v) = x {
                self.current.cursor.0 = v;
            }
            if let Some(v) = y {
                self.current.cursor.1 = v;
            }
        }
    }

    /// 取走 `MOUSE[X/Y]` 的 warp 请求(宿主消费)。
    pub fn take_cursor_warp(&mut self) -> Option<(Option<i64>, Option<i64>)> {
        self.settings.cursor_warp.take()
    }

    /// 用时间线的该帧状态推进一帧。
    pub fn advance(&mut self, frame: u64, timeline: &InputTimeline) {
        let mut now = timeline.effective(frame);
        // 脉冲只在该帧有效(不参与“按住保持”)。
        if let Some(exact) = timeline.at(frame) {
            now.pulses = exact.pulses.clone();
        }
        let mut down: BTreeSet<u16> = BTreeSet::new();
        for name in now.pressed.iter().chain(now.pulses.iter()) {
            if let Some(code) = code_of_button_name(name) {
                down.insert(code);
            }
        }
        // 先算键盘“任意键”本帧状态(只看已存在的键盘计数器)。
        let any_down = self.counters.iter().any(|(code, _)| {
            source_of_code(*code) == InputSource::Keyboard
                && *code != 0
                && down.contains(code)
        });

        let keyboard = self.settings.keyboard;
        let mouse = self.settings.mouse;
        let pad = self.settings.pad;
        for (code, counter) in self.counters.iter_mut() {
            let source = source_of_code(*code);
            let allowed = match source {
                InputSource::Keyboard => keyboard,
                InputSource::Mouse => mouse,
                InputSource::Pad => pad,
            };
            if !allowed {
                // 引擎整段跳过;down 保持上一次物理状态。
                continue;
            }
            let pressed = if *code == 0 {
                any_down
            } else {
                down.contains(code)
            };
            let rising = pressed && !counter.down;
            counter.step(pressed);
            if rising {
                *self.counts.entry(*code).or_insert(0) += 1;
                match source {
                    InputSource::Keyboard => self.keylast = i64::from(*code),
                    // 引擎 `_MOUSELAST` 只由 L/R/M 的 WM_*BUTTONDOWN 写(1/2/4);
                    // 滚轮不写。
                    InputSource::Mouse if matches!(*code, 1 | 2 | 4) => {
                        self.mouselast = i64::from(*code)
                    }
                    _ => {}
                }
            }
        }
        self.prev = self.current.clone();
        self.current = now;
    }

    /// 按钮计数器值(未出现 = 0)。
    pub fn counter(&self, button: &str) -> i64 {
        code_of_button_name(button)
            .and_then(|code| self.counters.get(&code).map(|c| c.value))
            .unwrap_or(0)
    }

    /// 按引擎按钮码读计数器(`_INP(code)` / `TEXT[CLICKBUTTON]`)。
    pub fn counter_code(&self, code: i64) -> i64 {
        u16::try_from(code)
            .ok()
            .and_then(|code| self.counters.get(&code).map(|c| c.value))
            .unwrap_or(0)
    }

    /// 累计次数(未出现 = 0)。
    pub fn count(&self, button: &str) -> i64 {
        code_of_button_name(button)
            .and_then(|code| self.counts.get(&code).copied())
            .unwrap_or(0)
    }

    /// 任意键(`_KEY_ANY`):引擎下标 0,任一键盘键按住时按自己的计数器走。
    pub fn any_counter(&self) -> i64 {
        self.counters.get(&0).map(|c| c.value).unwrap_or(0)
    }

    /// `_INP(index)` 数组版:下标 = 按钮码,读同一张计数器表。
    ///
    /// REA(0.495 `FUN_00427350` case 0x90):`index<=0 → 0`;否则读计数表
    /// `[index]`。未知码(无宿主注入的手柄等)按 0 处理。
    pub fn inp(&self, index: i64) -> Result<i64, yu_log::Diag> {
        if index <= 0 {
            return Ok(0);
        }
        Ok(self.counter_code(index))
    }

    /// `_CODE_*` 读取:脚本改写优先,否则引擎 ABI 缺省值。
    pub fn code_value(&self, id: u16, default: u16) -> i64 {
        *self.code_overrides.get(&id).unwrap_or(&(default as i64))
    }

    /// `_CODE_*` 写入(手册 ○;键位重映射)。
    pub fn set_code(&mut self, id: u16, value: i64) {
        self.code_overrides.insert(id, value);
    }

    /// 「推进文本」输入是否出现。
    ///
    /// 引擎 `FUN_004046fc` 读的是 `TEXT[CLICKBUTTON1..4]` / `SKIPBUTTON1..4`
    /// 指定的**按钮码**在计数器表里的值:
    /// - 点击按钮出现按下沿(`==1`)→ 推进;
    /// - 跳过按钮按住(`>0`)→ 推进;
    /// - 未分配(`0x3ff`)忽略。
    pub fn text_advance_requested(&self, click_buttons: &[i64; 4], skip_buttons: &[i64; 4]) -> bool {
        let click = click_buttons
            .iter()
            .any(|code| *code > 0 && *code != 0x3ff && self.counter_code(*code) == 1);
        let skip = skip_buttons
            .iter()
            .any(|code| *code > 0 && *code != 0x3ff && self.counter_code(*code) > 0);
        click || skip
    }

    /// 手柄按钮码的计数器/累计次数(无注入恒 0)。
    pub fn pad_counter(&self, code: i64, is_num: bool) -> i64 {
        let code = u16::try_from(code).unwrap_or(u16::MAX);
        if is_num {
            self.counts.get(&code).copied().unwrap_or(0)
        } else {
            self.counters.get(&code).map(|c| c.value).unwrap_or(0)
        }
    }

    /// 手柄按钮码写入(手册 ○ 可赋值;按物理状态钳制)。
    pub fn set_pad_counter(&mut self, code: i64, value: i64, is_num: bool) {
        let Ok(code) = u16::try_from(code) else {
            return;
        };
        if is_num {
            self.counts.insert(code, value);
        } else {
            self.counters.entry(code).or_default().write(value);
        }
    }

    /// 直接设定按钮计数器(引擎 `FUN_00447014` 钳制)。
    ///
    /// 手册 `basic/svariable.html` 把 `@_KEY_*` / `@_MOUSE_*` 标为 **○ 可赋值**;
    /// ERIS `es_ERIS.yst` 的 `es.S.GO` 用 `@_MOUSE_L=5` / `@_KEY_ENTER=5`
    /// 注释为「シーン変わるときは入力一旦無効に」(切换场景时让输入一次性失效)。
    /// 引擎按当前物理状态钳制,见 [`Counter::write`]。
    pub fn set_counter(&mut self, button: &str, value: i64) {
        if let Some(code) = code_of_button_name(button) {
            self.counters.entry(code).or_default().write(value);
        }
    }

    /// 按按钮码写计数器(`_INP(code)=v` / 脚本重映射后的码)。
    pub fn set_counter_code(&mut self, code: i64, value: i64) {
        let Ok(code) = u16::try_from(code) else {
            return;
        };
        if code == 0 {
            return;
        }
        self.counters.entry(code).or_default().write(value);
    }

    pub fn cursor(&self) -> (i64, i64) {
        self.current.cursor
    }

    /// 宿主/测试直接设定光标(推进前的物理状态)。
    pub fn set_cursor(&mut self, x: i64, y: i64) {
        self.current.cursor = (x, y);
    }

    pub fn keylast(&self) -> i64 {
        self.keylast
    }

    pub fn mouselast(&self) -> i64 {
        self.mouselast
    }

    /// 本帧快照。
    pub fn current(&self) -> &InputFrame {
        &self.current
    }

    /// 上一帧快照。
    pub fn previous(&self) -> &InputFrame {
        &self.prev
    }

    /// 全部已有计数器码(确定顺序;诊断/覆盖普查用)。
    pub fn counter_codes(&self) -> impl Iterator<Item = u16> + '_ {
        self.counters.keys().copied()
    }
}

/// 按钮名 → 引擎按钮码(名字与 `InputTimeline` / 宿主同构)。
fn code_of_button_name(name: &str) -> Option<u16> {
    if let Some(rest) = name.strip_prefix("KEY_") {
        return button_code(rest);
    }
    if let Some(rest) = name.strip_prefix("MOUSE_") {
        return button_code(&format!("MOUSE_{rest}"));
    }
    if let Some(rest) = name.strip_prefix("PAD_") {
        return pad_code_of_name(rest);
    }
    button_code(name)
}

/// `PAD_UP0` / `PAD_BUTTON0_1` 形态(宿主注入约定)。
fn pad_code_of_name(rest: &str) -> Option<u16> {
    let (kind, rest) = rest.split_once(|c: char| c.is_ascii_digit())?;
    let number = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    let pad: i64 = number.parse().ok()?;
    let code = match kind {
        "UP" => pad_dir_code(0, pad),
        "DOWN" => pad_dir_code(1, pad),
        "LEFT" => pad_dir_code(2, pad),
        "RIGHT" => pad_dir_code(3, pad),
        "BUTTON" => {
            let (_, index) = rest.split_once('_')?;
            pad_button_code(pad, index.parse().ok()?)
        }
        _ => return None,
    };
    u16::try_from(code).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_frame_counter_matches_manual() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["KEY_Z"]));
        tl.set(2, InputFrame::new(std::iter::empty::<&str>()));
        let mut st = InputState::new();
        st.advance(0, &tl);
        assert_eq!(st.counter("KEY_Z"), 1, "本帧按下 = 1");
        st.advance(1, &tl);
        assert_eq!(st.counter("KEY_Z"), 2, "持续按住 = 2");
        st.advance(2, &tl);
        assert_eq!(st.counter("KEY_Z"), -1, "松开当帧 = -1");
        st.advance(3, &tl);
        assert_eq!(st.counter("KEY_Z"), -2, "松开后继续递减");
    }

    #[test]
    fn repress_after_release_restarts_at_one() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["KEY_Z"]));
        tl.set(1, InputFrame::new(std::iter::empty::<&str>()));
        tl.set(11, InputFrame::new(["KEY_Z"]));
        let mut st = InputState::new();
        for f in 0..=10 {
            st.advance(f, &tl);
        }
        assert_eq!(st.counter("KEY_Z"), -10, "松开 10 帧后 = -10");
        st.advance(11, &tl);
        assert_eq!(st.counter("KEY_Z"), 1, "重新按下的瞬间从 1 起算");
        st.advance(12, &tl);
        assert_eq!(st.counter("KEY_Z"), 2, "继续按住 = 2");
    }

    #[test]
    fn any_counter_positive_while_held() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["KEY_A", "KEY_B"]));
        let mut st = InputState::new();
        st.advance(0, &tl);
        assert!(st.any_counter() > 0);
    }

    #[test]
    fn text_advance_uses_configured_button_codes() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["KEY_ENTER"]));
        let mut st = InputState::new();
        st.advance(0, &tl);
        let click = [13, 0x3ff, 0x3ff, 0x3ff];
        let skip = [0x3ff; 4];
        assert!(st.text_advance_requested(&click, &skip), "ENTER 默认是点击按钮");
        let click = [1, 0x3ff, 0x3ff, 0x3ff];
        assert!(!st.text_advance_requested(&click, &skip), "改成鼠标后 ENTER 不再推进");
    }

    #[test]
    fn source_disable_freezes_counters() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(std::iter::empty::<&str>()));
        tl.set(1, InputFrame::new(["KEY_Z"]));
        tl.set(2, InputFrame::new(["KEY_Z"]));
        let mut st = InputState::new();
        st.advance(0, &tl);
        let before = st.counter("KEY_Z");
        st.apply_input(Some(false), None, None);
        st.advance(1, &tl);
        assert_eq!(st.counter("KEY_Z"), before, "KEY 禁用时计数器冻结");
        st.apply_input(Some(true), None, None);
        st.advance(2, &tl);
        assert_eq!(st.counter("KEY_Z"), 1, "重新启用后按物理状态产生按下沿");
    }

    #[test]
    fn script_write_is_clamped_by_physical_state() {
        let mut st = InputState::new();
        st.set_counter("MOUSE_L", 5);
        assert_eq!(st.counter("MOUSE_L"), 0, "未按住时正数被清 0");
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["MOUSE_L"]));
        st.advance(0, &tl);
        st.set_counter("MOUSE_L", 5);
        assert_eq!(st.counter("MOUSE_L"), 5, "按住时正数保留");
        st.set_counter("MOUSE_L", 0);
        assert_eq!(st.counter("MOUSE_L"), 0, "按住时写 0 清零");
    }

    #[test]
    fn wheel_pulse_is_one_frame() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(std::iter::empty::<&str>()).with_pulse("MOUSE_U"));
        tl.set(1, InputFrame::new(std::iter::empty::<&str>()));
        let mut st = InputState::new();
        st.advance(0, &tl);
        assert_eq!(st.counter("MOUSE_U"), 1, "脉冲帧 = 1");
        st.advance(1, &tl);
        assert_eq!(st.counter("MOUSE_U"), -1, "下一帧自动抬起");
    }

    #[test]
    fn last_key_and_mouse_codes_follow_engine() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(std::iter::empty::<&str>()));
        tl.set(1, InputFrame::new(["KEY_A"]));
        tl.set(2, InputFrame::new(std::iter::empty::<&str>()));
        tl.set(3, InputFrame::new(["MOUSE_L"]));
        let mut st = InputState::new();
        for frame in 0..=3 {
            st.advance(frame, &tl);
        }
        assert_eq!(st.keylast(), 65, "_KEYLAST = VK_A");
        assert_eq!(st.mouselast(), 1, "_MOUSELAST = 左键码 1");
    }

    #[test]
    fn inp_write_uses_same_clamp_as_named_buttons() {
        let mut st = InputState::new();
        st.set_counter_code(0x5a, 7);
        assert_eq!(st.inp(0x5a).unwrap(), 0, "未按住时正数写回被清 0");
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["KEY_Z"]));
        st.advance(0, &tl);
        st.set_counter_code(0x5a, 7);
        assert_eq!(st.inp(0x5a).unwrap(), 7, "按住时正数保留");
    }

    #[test]
    fn mouse_settings_are_script_visible() {
        let mut st = InputState::new();
        st.apply_mouse(Some(1), Some(1500), Some(false), Some(false), Some(false), Some(11), Some(50), Some(20), None, None, Some(b"ES.MCURSOR"));
        let s = st.settings();
        assert_eq!(s.mouse_mode, 1);
        assert_eq!(s.mouse_time_ms, 1500);
        assert!(!s.move_reactive && !s.click_reactive && !s.wheel_reactive);
        assert_eq!(s.cursor, 11);
        assert_eq!(st.cursor(), (50, 20));
        assert_eq!(s.cursor_id, b"ES.MCURSOR");
        assert_eq!(st.take_cursor_warp(), Some((Some(50), Some(20))));
    }
}
