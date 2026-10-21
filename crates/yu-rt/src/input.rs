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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputFrame {
    pub pressed: BTreeSet<ButtonName>,
    /// 光标逻辑坐标(鼠标域;`@_MOUSE_X/Y`)。
    pub cursor: (i64, i64),
}

impl InputFrame {
    pub fn new(pressed: impl IntoIterator<Item = impl Into<ButtonName>>) -> Self {
        Self {
            pressed: pressed.into_iter().map(Into::into).collect(),
            cursor: (0, 0),
        }
    }

    pub fn with_cursor(mut self, x: i64, y: i64) -> Self {
        self.cursor = (x, y);
        self
    }

    pub fn is_pressed(&self, button: &str) -> bool {
        self.pressed.contains(button)
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

/// 单个按钮的带符号计数器。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counter {
    /// 当前值:按下为正、松开为负;从未按过 = 0。
    pub value: i64,
}

/// 输入运行状态:每逻辑帧推进计数器。
///
/// 语义严格按手册:`pressed` 的按下沿 → `1`,持续按住逐帧 `+1`;
/// `released` → `-1` 并**逐帧继续递减**;`==1` 恰为「本帧按下」。
///
/// 重新按下时从 `1` 重新起算(手册「押された瞬間から 1 → 2 → 3」),
/// 不是从上次松开后的负数继续加 —— 否则长期未按后的一次短按永远到不了
/// 正值,文本推进/`==1` 判定会失效。
#[derive(Clone, Debug, Default)]
pub struct InputState {
    counters: BTreeMap<ButtonName, Counter>,
    counts: BTreeMap<ButtonName, i64>,
    prev: InputFrame,
    /// 本帧快照(供系统变量读取)。
    current: InputFrame,
    /// `_KEYLAST` / `_MOUSELAST`:最近一次按下/松开的按钮码
    /// (按钮码 = [`BUTTON_CODE_ABI`] 的 VK;REA:引擎存计数表下标)。
    keylast: i64,
    mouselast: i64,
    /// `_CODE_*` 脚本改写覆盖(手册 ○ 可赋值;键位重映射)。
    code_overrides: BTreeMap<u16, i64>,
    /// 手柄计数器(按引擎按钮码;无宿主注入 ⇒ 恒 0 = 从未按过)。
    pad_counters: BTreeMap<i64, Counter>,
    pad_counts: BTreeMap<i64, i64>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 用时间线的该帧状态推进一帧。
    pub fn advance(&mut self, frame: u64, timeline: &InputTimeline) {
        let now = timeline.effective(frame);
        for (button, counter) in self.counters.iter_mut() {
            if now.is_pressed(button) {
                if counter.value <= 0 {
                    // 按下沿:记录「最近按下」的按钮码(_KEYLAST/_MOUSELAST)。
                    let code = button_code(button.strip_prefix("KEY_").unwrap_or(button))
                        .or_else(|| button_code(button.strip_prefix("MOUSE_").unwrap_or(button)))
                        .unwrap_or(0) as i64;
                    if button.starts_with("MOUSE_") {
                        self.mouselast = code;
                    } else {
                        self.keylast = code;
                    }
                }
                counter.value = if counter.value > 0 {
                    counter.value + 1
                } else {
                    1
                };
                self.counts.entry(button.clone()).or_insert(0);
                let n = self.counts.get_mut(button).expect("刚刚插入");
                if *n == 0 {
                    *n = 1;
                } else {
                    *n += 1;
                }
            } else if counter.value > 0 {
                counter.value = -1;
            } else {
                counter.value -= 1;
            }
        }
        // 新出现的按钮:本帧按下 → 1,否则 -1(与「从未按过」一致:
        // 手册的计数器从按下/松开那一刻起算,未出现过的键不参与)。
        for button in now.pressed.iter() {
            if !self.counters.contains_key(button) {
                let code = button_code(button.strip_prefix("KEY_").unwrap_or(button))
                    .or_else(|| button_code(button.strip_prefix("MOUSE_").unwrap_or(button)))
                    .unwrap_or(0) as i64;
                if button.starts_with("MOUSE_") {
                    self.mouselast = code;
                } else {
                    self.keylast = code;
                }
                self.counters.insert(button.clone(), Counter { value: 1 });
                self.counts.insert(button.clone(), 1);
            }
        }
        self.prev = self.current.clone();
        self.current = now;
    }

    /// 按钮计数器值(未出现 = 0)。
    pub fn counter(&self, button: &str) -> i64 {
        self.counters.get(button).map(|c| c.value).unwrap_or(0)
    }

    /// 累计次数(未出现 = 0)。
    pub fn count(&self, button: &str) -> i64 {
        self.counts.get(button).copied().unwrap_or(0)
    }

    /// 任意键:任一按钮当前按下 ⇒ 返回该按钮的计数器值,否则 -1(与按下同形)。
    pub fn any_counter(&self) -> i64 {
        let mut best = 0i64;
        for (button, c) in &self.counters {
            if !button.starts_with("MOUSE_") && self.current.is_pressed(button) {
                best = best.max(c.value);
            }
        }
        if best > 0 {
            best
        } else {
            self.counters
                .values()
                .map(|c| c.value)
                .filter(|v| *v < 0)
                .min()
                .unwrap_or(0)
        }
    }

    /// `_INP(index)` 数组版:下标 = 按钮码,读**同一张**计数表。
    ///
    /// REA(0.495 `FUN_00427350` case 0x90):`index<=0 → 0`;否则读计数表
    /// `[index]`。键盘/鼠标码 = [`button_of_code`];手柄码(384..)无宿主
    /// 注入 → 恒 0(从未按过)。
    pub fn inp(&self, index: i64) -> Result<i64, yu_log::Diag> {
        if index <= 0 {
            return Ok(0);
        }
        match button_of_code(index) {
            Some(name) => Ok(self.counter(&name)),
            // 手柄/未知码:引擎里是独立计数表下标;内核无手柄注入 ⇒ 恒 0。
            None => Ok(0),
        }
    }

    /// `_CODE_*` 读取:脚本改写优先,否则引擎 ABI 缺省值。
    pub fn code_value(&self, id: u16, default: u16) -> i64 {
        *self.code_overrides.get(&id).unwrap_or(&(default as i64))
    }

    /// `_CODE_*` 写入(手册 ○;键位重映射)。
    pub fn set_code(&mut self, id: u16, value: i64) {
        self.code_overrides.insert(id, value);
    }

    /// 「推进文本」输入是否出现(引擎 `FUN_004046fc`:运行期按钮表
    /// `0x5b48ac..0x5b48b8` = MOUSE_L / ENTER / PAD / PAD,`0x5b48bc` = CTRL;
    /// 任一计数器为正即视为点击推进)。
    pub fn text_advance_requested(&self) -> bool {
        // 计数器键 = `MOUSE_L` / `KEY_ENTER` / `KEY_CTRL`(绑定表命名)。
        const ADVANCE_BUTTONS: [&str; 3] = ["MOUSE_L", "KEY_ENTER", "KEY_CTRL"];
        ADVANCE_BUTTONS
            .iter()
            .any(|b| self.counter(b) > 0)
            || self.pad_counters.values().any(|c| c.value > 0)
    }

    /// 手柄按钮码的计数器/累计次数(无注入恒 0)。
    pub fn pad_counter(&self, code: i64, is_num: bool) -> i64 {
        if is_num {
            self.pad_counts.get(&code).copied().unwrap_or(0)
        } else {
            self.pad_counters.get(&code).map(|c| c.value).unwrap_or(0)
        }
    }

    /// 手柄按钮码写入(手册 ○ 可赋值)。
    pub fn set_pad_counter(&mut self, code: i64, value: i64, is_num: bool) {
        if is_num {
            self.pad_counts.insert(code, value);
        } else {
            self.pad_counters.insert(code, Counter { value });
        }
    }

    /// 直接设定按钮计数器。
    ///
    /// 手册 `basic/svariable.html` 把 `@_KEY_*` / `@_MOUSE_*` 标为 **○ 可赋值**;
    /// ERIS `es_ERIS.yst` 的 `es.S.GO` 用 `@_MOUSE_L=5` / `@_KEY_ENTER=5`
    /// 注释为「シーン変わるときは入力一旦無効に」(切换场景时让输入一次性失效)
    /// ——写入即把该按钮的帧计数器置为给定值。见 DIV-RT-021。
    pub fn set_counter(&mut self, button: &str, value: i64) {
        match self.counters.get_mut(button) {
            Some(c) => c.value = value,
            None => {
                self.counters.insert(button.to_string(), Counter { value });
            }
        }
    }

    pub fn cursor(&self) -> (i64, i64) {
        self.current.cursor
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
    fn text_advance_buttons_use_binding_names() {
        let mut tl = InputTimeline::new();
        tl.set(0, InputFrame::new(["KEY_ENTER"]));
        let mut st = InputState::new();
        st.advance(0, &tl);
        assert!(st.text_advance_requested(), "ENTER 必须能推进文本");
        tl.set(1, InputFrame::new(std::iter::empty::<&str>()));
        st.advance(1, &tl);
        assert!(!st.text_advance_requested(), "松开后不再推进");
    }
}
