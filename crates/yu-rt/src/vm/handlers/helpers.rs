//! handlers 的共享自由函数与跨命令族 impl 方法。
//!
//! 由 `handlers/mod.rs` 末尾拆分而来;只搬运,函数体逐字不变。

use super::*;

/// `YU_SP_TRACE` 取证开关(进程内只读一次)。
///
/// GOSUB 在 v555 按钮/逐字宏里每帧可达数万次;旧实现在每次 GOSUB 内调
/// `std::env::var_os`(Windows 上是 `GetEnvironmentVariableW`),把一条
/// 本应 ~100ns 的控制流命令放大到 ~0.4–0.6µs。开关语义不变。
pub(crate) fn sp_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("YU_SP_TRACE").is_some())
}

impl Vm {

    /// 按 ysc 参数名求值全部窗口(空窗口跳过)。
    ///
    /// 与 `eval_arg_windows` 的区别:保留参数名(借用,不 clone),供
    /// INPUT/MOUSE 之类“按名消费、跨版本槽号会漂移”的命令使用(项目范式:
    /// 不写死槽号)。参数名表按调用点缓存(`cmd.param_names`)。
    pub(super) fn named_command_values<'a>(
        &mut self,
        st: &mut ExecState,
        cmd: &'a Cmd,
    ) -> Result<smallvec::SmallVec<[(u8, &'a str, Value); 8]>, VmError> {
        let names: &[String] = &cmd.param_names;
        let mut out = smallvec::SmallVec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or(""); // silent-ok: 槽号越界 = 无名
            let value = self.eval_window(st, w)?;
            out.push((w.rec.kind, name, value));
        }
        Ok(out)
    }

    pub(super) fn emit_domain_from_values(
        &mut self,
        name: &str,
        domain: Domain,
        vals: smallvec::SmallVec<[(u8, &str, Value); 8]>,
    ) {
        // 值直接 move 进事件参数(#82:不再 `value.clone()`)。
        let args = vals
            .into_iter()
            .map(|(slot, _, value)| ArgValue::new(slot, value))
            .collect();
        self.emit_domain(domain, name.to_string(), args);
    }
}


// ---------------------------------------------------------------- 纯函数

/// `TASKINFO` 所需的任务执行位标量投影(在借用域内取出,避免克隆 `ExecState`)。
#[derive(Default)]
pub(super) struct TaskinfoFacts {
    pub(super) sid: u32,
    pub(super) pc: u32,
    pub(super) origin: (u32, u32),
    pub(super) text_line: i64,
    pub(super) next_voice: i64,
    pub(super) if_nest: i64,
    pub(super) gosub_nest: i64,
    pub(super) loop_nest: i64,
}

/// `TINT`/`TFLT`/`TSTR` 族的**槽位索引**:后缀数字 - 1。
///
/// `TINT` ⇒ 0、`TINT2` ⇒ 1 …… `TINT10` ⇒ 9。
/// 与 `sysvar::param_index`(`@_TINT(n)` 读 `n-1`)及 `TaskExecState`
/// 的 `task_ints[16]` 同口径。
///
/// **后缀 N 不是索引 N**:第一版按 `N` 取值,导致 `G_INT2`/`TINT2` 全部
/// 错位一格(`TINT10` ⇒ 索引 10 越界被静默丢弃)。
pub(super) fn family_index(name: &str, prefix: &str) -> Option<usize> {
    let rest = name.strip_prefix(prefix)?;
    let n = match rest {
        "" => 1,
        digits => digits.parse::<usize>().ok()?,
    };
    let idx = n.checked_sub(1)?;
    (idx < 16).then_some(idx)
}

/// 构建 `VARACT` 站点计划:参数名 → 槽号解析一次(`fill_cmd` 调用)。
///
/// 语义与旧的就地扫描逐项等价:旗标按**参数名**判定(槽号跨版本会位移),
/// `G_*` 族按前缀 + 后缀编号取掩码位,`DIMSIZE` 的复合赋值码取该槽窗口的
/// `slot` 高字节(静态)。
pub(crate) fn build_varact_plan(names: &[String], windows: &[CmdWindow]) -> VaractPlan {
    let slot_of = |want: &str| names.iter().position(|n| n == want).map(|i| i as u8);
    let mut plan = VaractPlan {
        type_slot: slot_of("TYPE"),
        init_slot: slot_of("INIT"),
        dimsize_slot: slot_of("DIMSIZE"),
        dimsize_code: 0,
        push_slot: slot_of("PUSH"),
        pop_slot: slot_of("POP"),
        copy_slot: slot_of("COPY"),
        cut_slot: slot_of("CUT"),
        pos_slot: slot_of("POS"),
        length_slot: slot_of("LENGTH"),
        upper_slot: slot_of("UPPER"),
        upper2_slot: slot_of("UPPER2"),
        lower_slot: slot_of("LOWER"),
        lower2_slot: slot_of("LOWER2"),
        hantozen_slot: slot_of("HANTOZEN"),
        zentohan_slot: slot_of("ZENTOHAN"),
        g_slots: Vec::new(),
    };
    if let Some(slot) = plan.dimsize_slot {
        plan.dimsize_code = windows
            .iter()
            .find(|w| w.rec.kind == slot)
            .map(|w| (w.rec.slot >> 8) as u8)
            .unwrap_or(0);
    }
    for (index, name) in names.iter().enumerate() {
        let Ok(slot) = u8::try_from(index) else {
            break;
        };
        let family = if name.starts_with("G_INT") {
            Some(0u8)
        } else if name.starts_with("G_FLT") {
            Some(1u8)
        } else if name.starts_with("G_STR") {
            Some(2u8)
        } else {
            None
        };
        let Some(family) = family else { continue };
        let mask = ["G_INT", "G_FLT", "G_STR"]
            .get(usize::from(family))
            .and_then(|prefix| family_index(name, prefix))
            .filter(|idx| *idx < 4)
            .map(|idx| (family, idx as u8));
        plan.g_slots.push((slot, mask));
    }
    plan
}

pub(super) fn flags_get(flags: &[(u8, i64)], slot: u8) -> Option<i64> {
    flags.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v)
}

/// VARINFO SEARCH 的针值:`INT=`/`FLT=`/`STR=`(手册例 5/6/7)。
///
/// **槽号必须按工程 ysc 的参数名解析,不能写死数字**(2026-10-25 实测):
/// `VARINFO` 的参数表随版本新增条目而整体位移 ——
///
/// | 工程 | INT | FLT | STR | NO |
/// |---|---|---|---|---|
/// | SDK 0.495 ERIS 语料 | 17 | 18 | 19 | 20 |
/// | 商业包 v494(本作) | 15 | 16 | 17 | 18 |
///
/// 旧实现写死 17/18/19 与 `NO=20`。在 v494 上 `SEARCH` 取不到针值,
/// 一律返回 -1:`es.LNO.GET("BGF")` 明明存在(`$3153[4]`)却判「未找到」,
/// 后续 sprite 注册链整条走空。参数名跨版本稳定,槽号不稳定。
pub(super) fn array_needle(
    varinfo_names: &[String],
    vm: &mut Vm,
    cmd: &Cmd,
    st: &ExecState,
    set: VarRef,
) -> Result<Option<Value>, VmError> {
    let ty = vm.store.shape(set).map(|s| s.0);
    let want = match ty {
        Some(ElemType::Str) => "STR",
        Some(ElemType::Float) => "FLT",
        Some(ElemType::Int) => "INT",
        None => return Ok(None),
    };
    let Some(slot) = varinfo_names
        .iter()
        .position(|n| n == want)
        .map(|i| i as u8)
    else {
        return Ok(None);
    };
    for w in &cmd.windows {
        if w.rec.length == 0 || w.rec.kind != slot {
            continue;
        }
        return Ok(Some(vm.eval_window(st, w)?));
    }
    let _ = cmd;
    Ok(None)
}

/// `VARINFO[NO=]`:SEARCH 的起始下标;按参数名解析(理由同 [`array_needle`])。
pub(super) fn varinfo_no_slot(varinfo_names: &[String]) -> Option<u8> {
    varinfo_slot(varinfo_names, "NO")
}

/// 参数名 → 0 基槽位(`VARINFO` 的 `NO`/`NO2` 等辅助参数)。
pub(super) fn varinfo_slot(varinfo_names: &[String], name: &str) -> Option<u8> {
    varinfo_names
        .iter()
        .position(|n| n == name)
        .map(|i| i as u8)
}

/// 555 `VARINFO[STRFIRST]` 的字符串比较:ASCII 大小写不敏感、逐字节。
///
/// 官方 `FUN_00471840` 默认分支把 `A..Z` 降为小写后逐字节比较;CP932
/// 尾字节落在 `0x41..0x5A` 时同样会被降格,这里保持一致(不做 lead-byte
/// 跳过 —— 官方也没有跳)。
pub(super) fn str_less_ci(a: &[u8], b: &[u8]) -> bool {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
        .is_lt()
}

pub(crate) fn windows_error(cmd: &Cmd, detail: &str) -> yu_log::Diag {
    diag::fatal(
        code::rt::CMD_WINDOWS,
        Boundary::Script,
        format!("{}: {detail}(实给 {} 个窗口)", cmd.name, cmd.windows.len()),
    )
}

/// 参数名 → **0 基槽位**,带 1..=16 守卫(违者 Fatal,不 panic)。
///
/// 生产侧(`GOSUB[PINTn=]` / `RETURN[RINTn=]`)与消费侧
/// (`@_PINT(n)` / `@_RINT(n)`,`sysvar::param_index` 读 `n-1`)必须指向
/// 同一格:名字里的后缀 N ⇒ 索引 `N-1`。2026-10-25 审查发现 `RETURN`
/// 少了 GOSUB 已有的边界守卫,`RINT0` 会 `usize` 下溢、`RINT17+` 会越界,
/// 故抽成本函数统一口径。
/// `GOSUB`/`RETURN` 参数编号守卫(1..=16)。零分配快路径:编号合法就直接
/// 返回 0 基下标;不合法才回到旧路径取参数名生成**原文案**(含 ysc 表异常)。
pub(crate) fn checked_param_index(
    n: usize,
    opcode: u8,
    slot: u8,
    vm: &crate::vm::Vm,
    command: &str,
) -> Result<usize, VmError> {
    if (1..=16).contains(&n) {
        return Ok(n - 1);
    }
    let pname = vm.param_name(opcode, slot).unwrap_or_default(); // silent-ok:诊断文案用,缺声明即空名
    param_slot_index(&pname, slot, command)
}

pub(crate) fn param_slot_index(name: &str, slot: u8, command: &str) -> Result<usize, VmError> {
    let n = param_index_of(name).unwrap_or(1);
    if !(1..=16).contains(&n) {
        return Err(diag::fatal(
            code::rt::CMD_WINDOWS,
            Boundary::Script,
            format!(
                "{command} 参数 `{name}`(槽 {slot})编号 {n} 超出 1..16(工程 ysc 表异常;DIV-RT-034)"
            ),
        )
        .into());
    }
    Ok(n - 1)
}

/// 参数名尾部编号:`PINT2` → 2,`PINT` → 1。
pub(crate) fn param_index_of(name: &str) -> Option<usize> {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if digits.is_empty() {
        Some(1)
    } else {
        digits.parse().ok()
    }
}

/// 命令窗口复合赋值算子 → (交给宿主的增量值, 是否增量)。
///
/// 算子 = 窗口 `slot` 的高字节:**1 = `+=`,2 = `-=`**(0 = 普通赋值)。
/// 证据:`es_Sprite.yst:8173` 的 `X+=` 编译为 `X~1`;`es_SaveLoad.yst:623`
/// 的 `A-=` 编译为 `A~2`(0.495 官方产物,`dump_pc_cmds 19` 实测)。
/// `-=` 在这里取负,宿主统一按「加到当前值」处理。
pub(super) fn compose_window_arg(comp: u16, value: Value) -> (Value, bool) {
    let value = if comp == 2 {
        match value {
            Value::Int(n) => Value::Int(n.wrapping_neg()),
            Value::Float(f) => Value::Float(-f),
            other => other,
        }
    } else {
        value
    };
    (value, comp != 0)
}

/// 键的最后一个路径组件是否已带图像扩展名(与渲染器 `has_extension` 同判)。
///
/// 以 `.` 开头视为无扩展名;`/` 与 `\` 都算分隔符 —— 但分隔符只在**单字节
/// 字符**位置上生效:CP932 尾字节区间(`0x40..=0x7E`)含 `\`(`0x5C`),
/// `ソ`=`83 5C`、`表`=`95 5C` 若被当成路径分隔符,`cg/ソ.png` 就被判成
/// “无扩展名”,原 key 不参与候选,存在的资产会报缺失。
#[cfg(test)]
pub(super) fn has_image_extension(key: &[u8]) -> bool {
    has_image_extension_with(key, yu_format::Codec::Cp932)
}

pub(super) fn has_image_extension_with(key: &[u8], codec: yu_format::Codec) -> bool {
    let mut start = 0;
    let mut i = 0;
    while i < key.len() {
        let step = codec.char_len_at(key, i);
        if step == 1 && (key[i] == b'/' || key[i] == b'\\') {
            start = i + 1;
        }
        i += step;
    }
    let name = &key[start..];
    match name.iter().rposition(|&b| b == b'.') {
        Some(0) | None => false,
        Some(_) => true,
    }
}

/// 按参数名前缀做类型归一化(旧路径;热路径改用 `ParamTy::coerce`)。
#[allow(dead_code)]
pub(super) fn coerce_param(name: &str, v: Value) -> Value {
    if name.starts_with("PFLT") || name.starts_with("RFLT") {
        Value::Float(v.as_f64().unwrap_or(0.0))
    } else if name.starts_with("PSTR") || name.starts_with("RSTR") {
        match v {
            Value::Str(_) => v,
            other => Value::Str(other.to_event_repr().into_bytes()),
        }
    } else if name.starts_with("PINT") || name.starts_with("RINT") {
        Value::Int(
            v.as_int()
                .unwrap_or_else(|| v.as_f64().unwrap_or(0.0) as i64),
        )
    } else {
        v
    }
}

/// 复合赋值码(语料实测 0..8;`LET` 窗口 `slot` 高字节)。
pub(super) fn compound(code: u8, cur: &Value, rhs: &Value) -> Result<Value, yu_log::Diag> {
    Ok(match code {
        0 => rhs.clone(),
        1 => cur.add(rhs)?,
        2 => cur.sub(rhs)?,
        3 => cur.mul(rhs)?,
        4 => cur.div(rhs)?,
        5 => cur.rem(rhs)?,
        6 => cur.bitand(rhs)?,
        7 => cur.bitor(rhs)?,
        8 => cur.bitxor(rhs)?,
        other => {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("LET 复合赋值码 {other} 超出语料观测范围 0..8"),
            ))
        }
    })
}

/// 声明窗口的目标变量(单条变量指令或数组引用)。
///
/// 复用 `fill_cmd` 预解码的 `instrs`(#45);只有未预解码时才回退 `decode`。
pub(super) fn target_var(w: &CmdWindow) -> Result<VarRef, yu_log::Diag> {
    let decoded;
    let instrs: &[crate::expr::RawInstr] = match &w.instrs {
        Some(instrs) => instrs,
        None => {
            decoded = crate::expr::decode(&w.payload)?;
            &decoded
        }
    };
    let first = instrs
        .iter()
        .find(|i| matches!(i.op, 0x48 | 0x56 | 0x76))
        .ok_or_else(|| {
            diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                "声明窗口不含变量引用",
            )
        })?;
    if first.payload.len() != 3 {
        return Err(diag::fatal(
            code::rt::EXPR_MALFORMED,
            Boundary::Script,
            "变量引用载荷不是 3 字节",
        ));
    }
    Ok(VarRef::new(
        VarSpace::from_prefix(first.payload[0]),
        u16::from_le_bytes([first.payload[1], first.payload[2]]),
    ))
}

/// CP932 字符串的字符数(前导字节吞掉后继)。
pub fn str_char_count_with(codec: yu_format::Codec, bytes: &[u8]) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        i += codec.char_len_at(bytes, i);
        n += 1;
    }
    n
}

/// 按字符位置取子串(1 起算;手册 `VARACT` 例 5)。
///
/// 字符边界统一走 [`crate::cp932_text`](CP932 lead byte 跳步),不在本文件
/// 另写一套 —— 见该模块文档里的片头 LOGO 事故。
pub(super) fn copy_chars(bytes: &[u8], pos: usize, len: usize, codec: yu_format::Codec) -> Vec<u8> {
    let n = str_char_count_with(codec, bytes);
    let start = pos.saturating_sub(1).min(n);
    slice_chars_with(codec, bytes, start, start + len)
}

/// 挖掉 `[pos, pos+len)` 字符,保留头尾(手册 `VARACT` 例 6)。
pub(super) fn cut_chars(bytes: &[u8], pos: usize, len: usize, codec: yu_format::Codec) -> Vec<u8> {
    let n = str_char_count_with(codec, bytes);
    let start = pos.saturating_sub(1).min(n);
    let end = (start + len).min(n);
    let mut out = slice_chars_with(codec, bytes, 0, start);
    out.extend_from_slice(&slice_chars_with(codec, bytes, end, n));
    out
}

pub(super) fn slice_chars_with(codec: yu_format::Codec, bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    if start >= end {
        return Vec::new();
    }
    let mut spans = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let step = codec.char_len_at(bytes, i);
        spans.push((i, i + step));
        i += step;
    }
    let start = start.min(spans.len());
    let end = end.min(spans.len());
    if start >= end {
        return Vec::new();
    }
    bytes[spans[start].0..spans[end - 1].1].to_vec()
}

/// 半角 ASCII 大小写(`VARACT[UPPER/LOWER]`;手册 `variable/varact.html`
/// 只列半角,引擎 §3.26)。
///
/// **必须按 CP932 lead byte 跳步**:字符串是 CP932 原始字节,双字节字符的
/// 尾字节常落在 ASCII 大写区(`0x41..0x5A`),逐字节转换会把它当半角字母
/// 改写,整字变成别的汉字。实测(2026-10-24,`<game-b>` 片头):
/// `サークル` = `83 54 81 5b 83 4e 83 8b`,逐字节小写后 =
/// `83 74 81 5b 83 6e 83 8b` = **`フーハル`** —— 厂商 LOGO
/// `cg/ev/サークル.jpg`(包内实际存在,293KB)因此查不到,片头整段黑屏。
/// 官方 `VARACT` 处理体按双字节字符推进,不会出现该改写。
pub(super) fn ascii_case(bytes: &[u8], upper: bool, codec: yu_format::Codec) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let step = codec.char_len_at(bytes, i);
        if step == 2 {
            // 双字节字符原样保留(半角大小写只碰 ASCII 段)。
            out.extend_from_slice(&bytes[i..i + step]);
            i += step;
            continue;
        }
        out.push(if upper {
            bytes[i].to_ascii_uppercase()
        } else {
            bytes[i].to_ascii_lowercase()
        });
        i += 1;
    }
    out
}

/// 半角 + 全角拉丁字母大小写(手册 UPPER2/LOWER2)。
///
/// CP932 全角:Ａ-Ｚ = `0x82 0x60..0x79`,ａ-ｚ = `0x82 0x81..0x9A`。
///
/// 兜底分支同样按 [`crate::cp932_text::char_len_at`] 跳步:非 `0x82` 的双字节
/// 字符(汉字/假名)原样保留,绝不逐字节 ASCII 化 —— 与 `ascii_case` 同一
/// 类事故(见其文档)。
pub(super) fn fullwidth_case(bytes: &[u8], upper: bool, codec: yu_format::Codec) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if codec == yu_format::Codec::Cp932 && i + 1 < bytes.len() && bytes[i] == 0x82 {
            let t = bytes[i + 1];
            let mapped = if upper && (0x81..=0x9A).contains(&t) {
                Some(0x82u8)
            } else if !upper && (0x60..=0x79).contains(&t) {
                Some(0x81u8)
            } else {
                None
            };
            if let Some(marker) = mapped {
                let tt = if marker == 0x82 {
                    0x60 + (t - 0x81)
                } else {
                    0x81 + (t - 0x60)
                };
                out.push(0x82);
                out.push(tt);
                i += 2;
                continue;
            }
        }
        let step = codec.char_len_at(bytes, i);
        if step == 2 {
            out.extend_from_slice(&bytes[i..i + step]);
            i += step;
            continue;
        }
        let b = bytes[i];
        out.push(if upper {
            b.to_ascii_uppercase()
        } else {
            b.to_ascii_lowercase()
        });
        i += 1;
    }
    out
}

/// 值 → 字符串字节(已是 `Str` 时直接搬,不再 `cast_to_str` 克隆一次;#81)。
pub(super) fn into_str_bytes(v: Value) -> Result<Vec<u8>, yu_log::Diag> {
    match v {
        Value::Str(bytes) => Ok(bytes),
        other => match other.cast_to_str()? {
            Value::Str(bytes) => Ok(bytes),
            _ => Ok(Vec::new()),
        },
    }
}

/// `_KEY_<X>` / `_MOUSE_<X>` 的按钮族后缀(排除坐标槽与 `LAST`;
/// 与旧写路径的内联过滤同口径)。非按钮族返回 `None`。
pub(super) fn button_family_rest(name: &str) -> Option<&str> {
    let rest = name
        .strip_prefix("_KEY_")
        .or_else(|| name.strip_prefix("_MOUSE_"))?;
    let excluded = matches!(rest, "LAST" | "X" | "Y")
        // X/Y 后缀数字 = 窗口槽坐标族(REA `FUN_00427350` case 0x85..0x8e)。
        || rest
            .strip_suffix(['0', '1', '2', '3'])
            .map(|base| matches!(base, "X" | "Y"))
            .unwrap_or(false)
        || rest
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(true);
    (!excluded).then_some(rest)
}

/// CP932 → 显示文本(lossy;仅事件/诊断)。
pub(super) fn cp932_lossy(bytes: &[u8]) -> String {
    // 不引入码表:非 ASCII 字节按 `\xNN` 显示,保证确定性与可逆诊断。
    // 预分配 + `write!` 直接写入(旧实现每个非 ASCII 字节一次 `format!`)。
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        if (0x20..0x7F).contains(&b) {
            s.push(b as char);
        } else {
            // 写入 String 不会失败(内存分配 panic 除外);忽略 Result。
            let _ = write!(s, "\\x{b:02x}");
        }
    }
    s
}

/// 命令名 → 域(仅用于事件归属;命令名来自工程表)。
/// 把行主序扁平下标拆成各维下标(与 `value::linear_offset_for` 同口径)。
pub(super) fn flat_indices(dims: &[usize], flat: usize) -> Vec<i64> {
    if dims.is_empty() {
        return Vec::new();
    }
    let mut idx = vec![0i64; dims.len()];
    let mut rest = flat;
    for k in (0..dims.len()).rev() {
        let dim = dims[k].max(1);
        idx[k] = (rest % dim) as i64;
        rest /= dim;
    }
    idx
}

/// `CSV` 的文件解析(REA v474 `0x43577c` 的 `0x43583d..0x435bb5`)。
///
/// 语义要点:
/// - 文件字节后**追加 CRLF**(官方 `len+2` 分配并写 `0x0d 0x0a`);
/// - **只有 `,` 受引号保护**:`0x43591b` 的 CR/LF 分支在引号判断之前,
///   换行一律终止当前行(引号内的 CR/LF 也不保护);`"`/`'` 只切换
///   “保护”状态,**引号本身保留**在单元格里;
/// - 单引号状态跨行**不复位**(官方 `[esp+0x148]` 在 `0x435acd/0x435b7a`
///   的行终止路径没有清零;双引号 `esi` 会清零)——这是官方行为,照转;
/// - 行首空格/Tab 跳过,行尾空白保留;
/// - 每个分隔符都产生一个单元格(连续逗号产生空串);行终止只在“本行出现过
///   逗号”或“整行为空”时产生末尾单元格 —— 这是官方 `0x4359f8`/`0x435b1b`
///   两个标志位的直接转写;
/// - 单元格按出现顺序写入一维字符串数组,不保留行列结构。
pub(super) fn parse_csv_cells(bytes: &[u8], codec: yu_format::Codec) -> Vec<Vec<u8>> {
    parse_csv_cells_lined(bytes, codec)
        .into_iter()
        .map(|(_, cell)| cell)
        .collect()
}

/// 同 [`parse_csv_cells`],额外返回单元格所属的 **1 基行号**。
///
/// 0.466 `CSV[LINE]`(REA `0x431970`)按行号过滤:行号从 1 起,每遇
/// CR/LF(含 CRLF 一对)+1;单元格归属**终止它的那个行终止符之前的行**。
/// 文件尾追加的 CRLF 会再产生一个空行(其空单元格归入下一行)。
pub(super) fn parse_csv_cells_lined(bytes: &[u8], codec: yu_format::Codec) -> Vec<(u32, Vec<u8>)> {
    let mut buf = Vec::with_capacity(bytes.len() + 2);
    buf.extend_from_slice(bytes);
    buf.extend_from_slice(b"\r\n");
    let mut cells = Vec::new();
    let mut pos = 0usize;
    let mut field_start = 0usize;
    let mut in_double = false;
    let mut in_single = false;
    let mut seen_nonspace = false;
    let mut empty = true;
    let mut saw_comma = false;
    let mut line = 1u32;
    while pos < buf.len() {
        let ch = buf[pos];
        if ch == b',' && !in_double && !in_single {
            cells.push((line, buf[field_start..pos].to_vec()));
            pos += 1;
            field_start = pos;
            seen_nonspace = false;
            empty = false;
            saw_comma = true;
            continue;
        }
        if ch == b'\r' || ch == b'\n' {
            let advance = if ch == b'\r' && buf.get(pos + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            if saw_comma {
                cells.push((line, buf[field_start..pos].to_vec()));
            } else if empty {
                cells.push((line, Vec::new()));
            }
            pos += advance;
            field_start = pos;
            in_double = false;
            // 单引号状态**不**在行终止复位(官方 `[esp+0x148]` 同样没有
            // 清零);这是官方解析器的既有行为,不要“修”成按行复位。
            seen_nonspace = false;
            empty = true;
            saw_comma = false;
            line += 1;
            continue;
        }
        if ch == b'"' {
            in_double = !in_double;
            pos += 1;
            continue;
        }
        if ch == b'\'' {
            in_single = !in_single;
            pos += 1;
            continue;
        }
        if !in_double && !in_single && !seen_nonspace && (ch == b' ' || ch == b'\t') {
            pos += 1;
            field_start = pos;
            continue;
        }
        pos += codec.char_len_at(&buf, pos);
        seen_nonspace = true;
        empty = false;
    }
    cells
}

/// 0.466 `CSVINFO[SIZE]` 的单元格计数(REA `0x432050`)。
///
/// 与 [`count_csv_cells`] 的 v474/0.495 口径不同:
/// - **不追加 CRLF**;
/// - 每个引号外逗号计 1;
/// - 每行终止时若本行有内容(`edi==0`)计 1,空行计 0;
/// - **文件末行无终止符时计入最后一格**(EOF `0x43225a` 的 `edi` 分支)。
///
/// `line > 0` 时只统计该 1 基行;`None`/`<=0` 统计全文件。
pub(super) fn count_csv_cells_0466(bytes: &[u8], codec: yu_format::Codec, line: Option<i64>) -> usize {
    let target = line.filter(|n| *n > 0);
    let mut count = 0usize;
    let mut pos = 0usize;
    let mut in_double = false;
    let mut in_single = false;
    let mut seen = false;
    let mut cur = 1i64;
    let hit = |ln: i64| target.is_none_or(|t| t == ln);
    while pos < bytes.len() {
        let ch = bytes[pos];
        if ch == b',' && !in_double && !in_single {
            if hit(cur) {
                count += 1;
            }
            pos += 1;
            seen = true;
            continue;
        }
        if ch == b'\r' || ch == b'\n' {
            let advance = if ch == b'\r' && bytes.get(pos + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            if seen && hit(cur) {
                count += 1;
            }
            pos += advance;
            in_double = false;
            // 与 0.466 解析器同源:单引号状态跨行保留。
            seen = false;
            cur += 1;
            continue;
        }
        if ch == b'"' {
            in_double = !in_double;
            pos += 1;
            continue;
        }
        if ch == b'\'' {
            in_single = !in_single;
            pos += 1;
            continue;
        }
        if !in_double && !in_single && !seen && (ch == b' ' || ch == b'\t') {
            pos += 1;
            continue;
        }
        pos += codec.char_len_at(bytes, pos);
        seen = true;
    }
    if seen && hit(cur) {
        count += 1;
    }
    count
}

/// 0.466 `CSVINFO[LINE]`(无 SIZE)的行数:CR/LF 序列数 + 1。
///
/// 官方解析器在行号 `ebp` 从 1 起、每个行终止 +1,结束时直接写 `ebp`;
/// 引号内的换行同样计数(换行判断在引号判断之前)。
pub(super) fn csv_line_count_0466(bytes: &[u8]) -> usize {
    let mut lines = 1usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\r' => {
                pos += if bytes.get(pos + 1) == Some(&b'\n') { 2 } else { 1 };
                lines += 1;
            }
            b'\n' => {
                pos += 1;
                lines += 1;
            }
            _ => pos += 1,
        }
    }
    lines
}

/// `CSVINFO[SIZE=1]` 的计数(REA v474 `0x435cfc` 的 `0x435df7..0x435f42`)。
///
/// 与 [`parse_csv_cells`] 共用分隔规则,但**不追加 CRLF**,且行终止只在
/// “本行已有非空白字符/逗号”时计数(`esi` 标志);空行计 0。结果等于
/// “按行展开后的单元格数”(文件以 CRLF 结尾时)。
pub(super) fn count_csv_cells(bytes: &[u8], codec: yu_format::Codec) -> usize {
    let mut count = 0usize;
    let mut pos = 0usize;
    let mut in_double = false;
    let mut in_single = false;
    let mut seen = false;
    while pos < bytes.len() {
        let ch = bytes[pos];
        if ch == b',' && !in_double && !in_single {
            count += 1;
            pos += 1;
            seen = true;
            continue;
        }
        if ch == b'\r' || ch == b'\n' {
            let advance = if ch == b'\r' && bytes.get(pos + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            if seen {
                count += 1;
            }
            pos += advance;
            in_double = false;
            // 与 `parse_csv_cells` 同一官方怪癖:单引号状态跨行保留。
            seen = false;
            continue;
        }
        if ch == b'"' {
            in_double = !in_double;
            pos += 1;
            continue;
        }
        if ch == b'\'' {
            in_single = !in_single;
            pos += 1;
            continue;
        }
        if !in_double && !in_single && !seen && (ch == b' ' || ch == b'\t') {
            pos += 1;
            continue;
        }
        pos += codec.char_len_at(bytes, pos);
        seen = true;
    }
    count
}

pub(super) fn domain_of(name: &str) -> Domain {
    if name.starts_with("CG") {
        Domain::Graphics
    } else if name.starts_with("SOUND")
        || name == "CDDA"
        || name == "MIDI"
        || name == "MIDIEND"
        || name == "MIDIINFO"
    {
        // CDDA 与 MIDI 族同域(B2:官方 0x429fb0 空桩也在声音域;coverage §8)。
        Domain::Sound
    } else if name == "SAVE" || name == "LOAD" || name.starts_with("SAVEINFO") || name == "SAVEEND"
    {
        Domain::Save
    } else if name.starts_with("FILE") {
        Domain::File
    } else if name.starts_with("MENU") {
        Domain::Menu
    } else if name.starts_with("WINDOW") {
        Domain::Window
    } else if name.starts_with("MOVIE") {
        Domain::Movie
    } else if name.starts_with("FLASH") {
        Domain::Flash
    } else if name.starts_with("EMOTE") {
        Domain::Emote
    } else if name.starts_with("SYSTEM") || name == "INPUT" || name == "MOUSE" {
        Domain::System
    } else if name.starts_with("FONT") {
        // 手册 `font/font.html` / `fontinfo.html`(字体渲染域,归宿主 gfx)。
        Domain::Graphics
    } else {
        Domain::Other
    }
}

/// `MATH3D` 的投影核心(纯函数,便于单测)。
///
/// 直接转写官方 `FUN_00448900`(`0x448900`)的 TIP==0/!=0 两个分支:
/// 官方用 x87 double 计算,输出 F31/F32 在写回 INT 目标时取整、写回 FLT
/// 目标时保留原值;此处保留 double,由调用方按目标类型处理。
pub(super) fn math3d_project(
    lno: i64,
    lbg: i64,
    parent: i64,
    wsx: f64,
    wsy: f64,
    tip: i64,
    f: &dyn Fn(i64) -> f64,
) -> (f64, f64, f64, f64) {
    let cam_x = f(3) + f(28);
    let cam_y = f(4) + f(29);
    let cam_z = f(5) + f(30);
    let def_z = f(27);
    let mx = f(20) / 100.0;
    let my = f(21) / 100.0;
    if tip != 0 {
        // 官方 else 分支:TIP 非 0 时不做透视,只保留图层内偏移/缩放。
        let x = f(13) - f(18) * f(16) / 100.0;
        let y = f(14) - f(19) * f(17) / 100.0;
        return (x, y, f(16), f(17));
    }
    // 背景与父层的透视比例(官方:fVar15/fVar16 = `pow(2.0, (…)/100)`)。
    // `FUN_00469779` 的 9 字节外壳后紧跟 pow 主体(0x469782 `fyl2x`),
    // 两个 4 字节实参 0/0x40000000 = double 2.0,第三个实参是指数。
    let z_bg = ((cam_z + def_z - f(8)) / 100.0).exp2();
    let mut z_layer = ((cam_z + def_z - f(15)) / 100.0).exp2();
    let mut x = ((f(6) - f(9)) - cam_x * mx) * z_bg + f(11) * mx + wsx * 0.5;
    let mut y = f(12) * my + wsy * 0.5 + ((f(7) - f(10)) - cam_y * my) * z_bg;
    if lno != lbg {
        let bx = f(9) * z_bg + x;
        let by = y + f(10) * z_bg;
        if parent == 0 {
            x = bx + (f(13) * mx - f(18)) * z_layer;
            y = by + (f(14) * my - f(19)) * z_layer;
        } else {
            // 父层存在时比例改用父层 Z(官方在同一局部变量上覆写 fVar16)。
            z_layer = ((cam_z + def_z - f(24)) / 100.0).exp2();
            x = bx + (((f(13) + f(22)) * mx - f(18)) - f(25)) * z_layer;
            y = by + (((f(14) + f(23)) * my - f(19)) - f(26)) * z_layer;
        }
    }
    x += f(18) * (100.0 - f(16)) * z_layer / 100.0;
    y += f(19) * (100.0 - f(17)) * z_layer / 100.0;
    (x, y, f(16) * z_layer, f(17) * z_layer)
}


#[cfg(test)]
mod window_ops_tests {
    use super::compose_window_arg;
    use crate::value::Value;

    /// 窗口 `slot` 高字节:**1 = `+=`,2 = `-=`**(0.495 官方产物实测:
    /// `es_Sprite.yst:8173 X+=` → `X~1`;`es_SaveLoad.yst:623 A-=` → `A~2`)。
    ///
    /// 回归:`A-=8` 曾被当成加法,把全屏截图幕布 CGS999 的 alpha 从 256
    /// 一路加到 512(不 clamp),读档完成后画面永远停在读档界面。
    #[test]
    fn plus_and_minus_operators_are_distinct() {
        assert_eq!(
            compose_window_arg(1, Value::Int(8)),
            (Value::Int(8), true),
            "+= 是正增量"
        );
        assert_eq!(
            compose_window_arg(2, Value::Int(8)),
            (Value::Int(-8), true),
            "-= 必须取负"
        );
        assert_eq!(
            compose_window_arg(2, Value::Float(2.5)),
            (Value::Float(-2.5), true),
            "-= 浮点同样取负"
        );
        assert_eq!(
            compose_window_arg(0, Value::Int(256)),
            (Value::Int(256), false),
            "普通赋值不加增量标志"
        );
    }
}


#[cfg(test)]
mod image_extension_tests {
    use super::has_image_extension;

    /// `ソ`=`83 5C`：尾字节是 `\`，不能被当成路径分隔符，否则 `cg/ソ.png`
    /// 的“最后组件”只剩 `.png`，会被判成无扩展名。
    #[test]
    fn cp932_trail_backslash_is_not_a_path_separator() {
        assert!(has_image_extension(b"cg/\x83\x5c.png"));
        assert!(has_image_extension(b"cg\\\x83\x5c.png"));
        assert!(!has_image_extension(b"cg/\x83\x5c"));
    }

    #[test]
    fn leading_dot_is_not_an_extension() {
        assert!(!has_image_extension(b"cg/.hidden"));
        assert!(has_image_extension(b"cg/a.png"));
        assert!(!has_image_extension(b"cg/a"));
    }

    /// 回归护栏：所有 CP932 lead 与尾字节 `0x5C`(`\`) 的组合都不得被当成
    /// 路径分隔符；带 `.png` 后缀时必须判为“有扩展名”。
    #[test]
    fn trail_backslash_never_splits_last_component() {
        for lead in (0x81u8..=0x9f).chain(0xe0..=0xfc) {
            let mut key = b"cg/".to_vec();
            key.push(lead);
            key.push(0x5c);
            assert!(!has_image_extension(&key), "lead={lead:#04x}");
            key.extend_from_slice(b".png");
            assert!(has_image_extension(&key), "lead={lead:#04x}");
        }
    }
}


#[cfg(test)]
mod csv_parse_tests {
    use super::{
        count_csv_cells, count_csv_cells_0466, csv_line_count_0466, parse_csv_cells,
        parse_csv_cells_lined,
    };
    use yu_format::Codec;

    /// 引号只保护 `,`;引号本身保留在单元格里。文件尾追加 CRLF 会产生
    /// 末尾空单元格(官方 `len+2` 的直接结果)。
    #[test]
    fn comma_inside_quotes_is_not_a_separator() {
        let cells = parse_csv_cells(b"a,\"b,c\"\r\n", Codec::Cp932);
        assert_eq!(
            cells,
            vec![b"a".to_vec(), b"\"b,c\"".to_vec(), Vec::new()]
        );
    }

    /// `0x43591b` 的 CR/LF 分支在引号判断之前:换行在引号内也终止行。
    #[test]
    fn newline_terminates_even_inside_quotes() {
        let cells = parse_csv_cells(b"a,\"b\nc\"\r\n", Codec::Cp932);
        assert_eq!(
            cells,
            vec![b"a".to_vec(), b"\"b".to_vec(), Vec::new()],
            "引号内的换行仍断行,第二行无逗号被丢弃"
        );
    }

    /// 官方行终止只复位双引号状态(`esi`),单引号状态(`[esp+0x148]`)
    /// 跨行保留 —— 未闭合的 `'` 会让后续行逗号继续被保护。
    #[test]
    fn single_quote_state_persists_across_lines() {
        let cells = parse_csv_cells(b"a,'b\r\nc,d\r\n", Codec::Cp932);
        assert_eq!(
            cells,
            vec![b"a".to_vec(), b"'b".to_vec(), Vec::new()],
            "单引号未复位 ⇒ 第二行逗号不分隔"
        );
    }

    /// CSVINFO 不追加 CRLF:末行无终止符时最后一格不计入。
    #[test]
    fn csvinfo_does_not_count_unterminated_last_cell() {
        assert_eq!(count_csv_cells(b"a,b\r\nc,d", Codec::Cp932), 3);
        assert_eq!(count_csv_cells(b"a,\"b,c\"\r\n", Codec::Cp932), 2);
    }

    /// 0.466 `CSV[LINE]`:行号 1 基,单元格归属终止它的行;文件尾追加
    /// CRLF 产生的空单元格归入下一行。
    #[test]
    fn lined_cells_carry_one_based_line_numbers() {
        let cells = parse_csv_cells_lined(b"a,b\r\nc,d\r\n", Codec::Cp932);
        assert_eq!(
            cells,
            vec![
                (1, b"a".to_vec()),
                (1, b"b".to_vec()),
                (2, b"c".to_vec()),
                (2, b"d".to_vec()),
                (3, Vec::new()),
            ]
        );
    }

    /// 0.466 `CSVINFO[SIZE]`:逗号 + 有内容的行终止;EOF 计入最后一行
    /// 未终止的单元格(与 v474/0.495 口径不同)。LINE 过滤按 1 基行号。
    #[test]
    fn csvinfo_0466_counts_eof_cell_and_filters_line() {
        assert_eq!(count_csv_cells_0466(b"a,b\r\nc,d", Codec::Cp932, None), 4);
        assert_eq!(
            count_csv_cells_0466(b"a,b\r\nc,d", Codec::Cp932, Some(1)),
            2
        );
        assert_eq!(
            count_csv_cells_0466(b"a,b\r\nc,d", Codec::Cp932, Some(2)),
            2
        );
        assert_eq!(
            count_csv_cells_0466(b"a,b\r\nc,d", Codec::Cp932, Some(3)),
            0
        );
        // 无逗号但有内容的行计 1;空行计 0。
        assert_eq!(count_csv_cells_0466(b"a\r\n\r\n", Codec::Cp932, None), 1);
    }

    /// 0.466 `CSVINFO[LINE]`(无 SIZE)= CR/LF 序列数 + 1;引号内换行也计。
    #[test]
    fn csvinfo_0466_line_count_counts_all_newlines() {
        assert_eq!(csv_line_count_0466(b""), 1);
        assert_eq!(csv_line_count_0466(b"a,b"), 1);
        assert_eq!(csv_line_count_0466(b"a,b\r\n"), 2);
        assert_eq!(csv_line_count_0466(b"a,b\r\nc,d"), 2);
        assert_eq!(csv_line_count_0466(b"a,\"b\nc\""), 2);
    }
}
