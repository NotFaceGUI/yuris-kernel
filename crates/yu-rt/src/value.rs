//! 值语义与变量域。
//!
//! # 类型模型(手册)
//!
//! `basic/variable.html`:YU-RIS 变量三类型 **INT(i64)/FLT(f64)/STR**;
//! `@` 数值变量、`$` 字符串变量;数组最多 8 维;声明后可赋值。
//! `LET`(`variable/let.html`)是唯一赋值命令,`@A=…` 是它的语法糖。
//!
//! # 取值/赋值通路(引擎取证)
//!
//! `<side-repo>/docs/format-vm-opcodes.md` §3.10(0.495 `yu-ris.exe` sha256
//! `4f5eefa3…`)给出槽位结构(64 字节:kind/动态类型标签/维数/各维长度/
//! 元素总数/三条独立类型数组)与两条硬结论:
//!
//! 1. **写时不转换**:赋值直接改写该槽位的动态类型标签,并按新类型重填整个
//!    数组(标量值填满所有元素)——本文件的 [`VariableStore::write`] 复刻该行为。
//! 2. **读时转换**:INT/FLT 读取时按目标口味互转;STR 读取走名字表。
//!    本内核把「读时转换」实现为:数值类型之间提升为 f64;字符串参与数值运算
//!    时报 [`crate::code::rt::VAR_TYPE`](未取证细节,见 DIV-RT-002)。
//!
//! # 表达式字符串→数值(引擎 opcode 0x69)
//!
//! §3.11:先看 `0x` 前缀走十六进制,否则扫十进制/浮点;扫描后仍有剩余字符
//! 报错。这里实现为 [`Value::cast_to_int`],非法输入返回 `Unimplemented`。

use yu_log::Boundary;

use crate::code;
use crate::diag;

/// 变量空间(源码前缀字节)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VarSpace {
    /// `@`(0x40)数值变量。
    At,
    /// `$`(0x24)字符串变量。
    Dollar,
    /// `` ` ``(0x60)= ERIS 旧式**数值**变量空间(`@G(n)` / `@L(n)`)。
    ///
    /// 证据(官方 0.495 ERIS 语料 4 处,逐条与源码对应):源码
    /// `\DBG.VAR("…", @G(0001))` 编译成 `56 03 00 60 89 04 42 01 00 01 29`
    /// (前缀 0x60、id 1161);YSVR 声明 1161 为 INT 数组 [8192]。
    /// `@L(0101)` 同前缀、id 1163(YSVR: INT [8192])。
    /// 因此该前缀 = 「数值」空间,id 决定具体变量(全局 G / 局部 L 由 YSVR
    /// 的恢复组号区分)。见 DIV-RT-017。
    LegacyNum,
    /// `#`(0x23)= ERIS 旧式**字符串**变量空间(`$G(n)` / `$L(n)`)。
    ///
    /// 同一处源码的 `$G(0001)` → `56 03 00 23 8a 04 …`(id 1162,
    /// YSVR 声明为 STR [8192]);`$L(0001)` → id 1164(STR [8192])。
    LegacyStr,
    /// 其它前缀,原样保留。
    Other(u8),
}

impl VarSpace {
    /// 由前缀字节解析(编译产物里就是源码字符的字节)。
    pub const fn from_prefix(b: u8) -> Self {
        match b {
            0x40 => VarSpace::At,
            0x24 => VarSpace::Dollar,
            0x23 => VarSpace::LegacyStr,
            0x60 => VarSpace::LegacyNum,
            other => VarSpace::Other(other),
        }
    }

    /// 元素类型 → 该类型变量所在的空间(`@` 数值 / `$` 字符串)。
    pub const fn for_elem(ty: ElemType) -> Self {
        match ty {
            ElemType::Int | ElemType::Float => VarSpace::At,
            ElemType::Str => VarSpace::Dollar,
        }
    }

    /// 该空间是否为 ERIS 旧式空间(读取时按 id 的声明类型归一化)。
    pub const fn is_legacy(self) -> bool {
        matches!(self, VarSpace::LegacyNum | VarSpace::LegacyStr)
    }

    /// 前缀字节。
    pub const fn prefix(self) -> u8 {
        match self {
            VarSpace::At => 0x40,
            VarSpace::Dollar => 0x24,
            VarSpace::LegacyStr => 0x23,
            VarSpace::LegacyNum => 0x60,
            VarSpace::Other(b) => b,
        }
    }
}

/// 变量引用:`[前缀][id:u16]`(编译期已把名字消解成 id)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarRef {
    pub space: VarSpace,
    pub id: u16,
}

impl VarRef {
    pub const fn new(space: VarSpace, id: u16) -> Self {
        Self { space, id }
    }

    /// `@1234` / `$55` 形式,供事件流与诊断。
    pub fn display(&self) -> String {
        format!("{}{}", self.space.prefix() as char, self.id)
    }
}

/// 变量元素类型(与 YSVR 的 `value_type` 1/2/3 一致)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElemType {
    Int,
    Float,
    Str,
}

impl ElemType {
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(ElemType::Int),
            2 => Some(ElemType::Float),
            3 => Some(ElemType::Str),
            _ => None,
        }
    }

    pub const fn default_value(self) -> Value {
        match self {
            ElemType::Int => Value::Int(0),
            ElemType::Float => Value::Float(0.0),
            ElemType::Str => Value::Str(Vec::new()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ElemType::Int => "INT",
            ElemType::Float => "FLT",
            ElemType::Str => "STR",
        }
    }
}

/// 运行时值。字符串保存 **CP932 原始字节**(编码转换属于显示/格式边界)。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "INT",
            Value::Float(_) => "FLT",
            Value::Str(_) => "STR",
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            Value::Str(_) => None,
        }
    }

    /// 真值判定:数值非零为真。字符串真值语义未取证(手册未给)。
    pub fn truthy(&self) -> Result<bool, yu_log::Diag> {
        match self {
            Value::Int(v) => Ok(*v != 0),
            Value::Float(v) => Ok(*v != 0.0),
            Value::Str(_) => Err(diag::unimplemented(
                code::rt::UNDETERMINED,
                Boundary::Script,
                "字符串真值语义未取证(手册仅给数值/字符串比较;DIV-RT-002)",
            )),
        }
    }

    pub fn to_event_repr(&self) -> String {
        match self {
            Value::Int(v) => v.to_string(),
            Value::Float(v) => format_float(*v),
            Value::Str(bytes) => format!("{:?}", String::from_utf8_lossy(bytes)),
        }
    }

    /// `+`:`STR+STR` 拼接(手册 `basic/variable.html` 例 6/7:字符串用 `+` 连接);
    /// `INT+INT` i64 环绕(64 位宽度由手册给定,溢出行为未取证 → DIV-RT-002);
    /// 数值混合提升 f64。
    pub fn add(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Str(a), Value::Str(b)) => {
                let mut v = Vec::with_capacity(a.len() + b.len());
                v.extend_from_slice(a);
                v.extend_from_slice(b);
                Ok(Value::Str(v))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            _ => match (self.as_f64(), rhs.as_f64()) {
                (Some(a), Some(b)) => Ok(Value::Float(a + b)),
                _ => Err(type_mix("+", self, rhs)),
            },
        }
    }

    pub fn sub(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            _ => match (self.as_f64(), rhs.as_f64()) {
                (Some(a), Some(b)) => Ok(Value::Float(a - b)),
                _ => Err(type_mix("-", self, rhs)),
            },
        }
    }

    pub fn mul(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            _ => match (self.as_f64(), rhs.as_f64()) {
                (Some(a), Some(b)) => Ok(Value::Float(a * b)),
                _ => Err(type_mix("*", self, rhs)),
            },
        }
    }

    /// `/`。官方 opcode 47(`0x004294ec`)的除零分支**不是**硬件异常:
    ///
    /// - 整数:`or eax,edx; je 0x42954e` → `xor eax,eax` 把结果写 0,
    ///   根本不调用 `__alldiv`(`expr_handlers.txt` opcode 47);
    /// - 浮点:`fcom [0x58ecf8]; je 0x429589` 直接跳到出口,结果保持
    ///   被除数不变(既不产生 ±Inf 也不产生 NaN)。
    ///
    /// 旧实现按 x87/CRT 硬件异常报 Fatal 是误判(DIV-RT-003 勘误,
    /// 2026-11-01 依 REA 反汇编收口)。
    pub fn div(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(_), Value::Int(0)) => Ok(Value::Int(0)),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_div(*b))),
            _ => match (self.as_f64(), rhs.as_f64()) {
                (Some(a), Some(0.0)) => Ok(Value::Float(a)),
                (Some(a), Some(b)) => Ok(Value::Float(a / b)),
                _ => Err(type_mix("/", self, rhs)),
            },
        }
    }

    /// `%`。opcode 37(`0x004295a0`)与 `/` 同构:整数除数为 0 时直接写 0
    /// (`je 0x429602`);浮点模式直接跳到出口(保持被除数)。
    pub fn rem(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(_), Value::Int(0)) => Ok(Value::Int(0)),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_rem(*b))),
            _ => match (self.as_f64(), rhs.as_f64()) {
                (Some(a), Some(0.0)) => Ok(Value::Float(a)),
                (Some(a), Some(b)) => Ok(Value::Float(a % b)),
                _ => Err(type_mix("%", self, rhs)),
            },
        }
    }

    /// `==`。数值比较与字符串逐字节比较(手册 `control/if.html` 例 2)。
    /// 字符串与数值互比未取证 → 报 `VAR_TYPE`。
    pub fn eq(&self, rhs: &Value) -> Result<bool, yu_log::Diag> {
        match (self, rhs) {
            (Value::Str(a), Value::Str(b)) => Ok(a == b),
            (Value::Int(a), Value::Int(b)) => Ok(a == b),
            (Value::Float(a), Value::Float(b)) => Ok(a == b),
            _ => match (self.as_f64(), rhs.as_f64()) {
                (Some(a), Some(b)) => Ok(a == b),
                _ => Err(type_mix("==", self, rhs)),
            },
        }
    }

    pub fn ne(&self, rhs: &Value) -> Result<bool, yu_log::Diag> {
        Ok(!self.eq(rhs)?)
    }

    /// 排序比较。仅数值;字符串排序未取证 → `VAR_TYPE`。
    fn cmp(&self, rhs: &Value) -> Result<std::cmp::Ordering, yu_log::Diag> {
        let (a, b) = match (self.as_f64(), rhs.as_f64()) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(type_mix("compare", self, rhs)),
        };
        Ok(match a.partial_cmp(&b) {
            Some(o) => o,
            None => std::cmp::Ordering::Greater,
        })
    }

    pub fn lt(&self, rhs: &Value) -> Result<bool, yu_log::Diag> {
        Ok(self.cmp(rhs)? == std::cmp::Ordering::Less)
    }
    pub fn le(&self, rhs: &Value) -> Result<bool, yu_log::Diag> {
        Ok(self.cmp(rhs)? != std::cmp::Ordering::Greater)
    }
    pub fn gt(&self, rhs: &Value) -> Result<bool, yu_log::Diag> {
        Ok(self.cmp(rhs)? == std::cmp::Ordering::Greater)
    }
    pub fn ge(&self, rhs: &Value) -> Result<bool, yu_log::Diag> {
        Ok(self.cmp(rhs)? != std::cmp::Ordering::Less)
    }

    /// 位运算(仅 INT;旁线 §3.9.2:65/79/94 只有整数分支,无浮点分支)。
    pub fn bitand(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
            _ => Err(type_mix("&", self, rhs)),
        }
    }
    pub fn bitor(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            _ => Err(type_mix("|", self, rhs)),
        }
    }
    pub fn bitxor(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        match (self, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
            _ => Err(type_mix("^", self, rhs)),
        }
    }

    /// `&&` / `||`(手册 `control/if.html`:两侧表达式先行求值,无短路)。
    pub fn logand(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        Ok(Value::Int((self.truthy()? && rhs.truthy()?) as i64))
    }
    pub fn logor(&self, rhs: &Value) -> Result<Value, yu_log::Diag> {
        Ok(Value::Int((self.truthy()? || rhs.truthy()?) as i64))
    }

    pub fn neg(&self) -> Result<Value, yu_log::Diag> {
        match self {
            Value::Int(v) => Ok(Value::Int(v.wrapping_neg())),
            Value::Float(v) => Ok(Value::Float(-v)),
            Value::Str(_) => Err(type_mix("unary -", self, self)),
        }
    }

    /// `$()`(opcode 0x73):INT → 十进制;FLT → `%f`(六位小数)。
    ///
    /// 证据:`<side-repo>/docs/format-vm-opcodes.md` §3.22 —— 引擎字符串化
    /// 浮点用格式串 `%f`(字面量 `0x8081f0`),故 `1.5` → `"1.500000"`。
    /// 手册 `basic/variable.html` 例 1/3 只用整数。
    pub fn cast_to_str(&self) -> Result<Value, yu_log::Diag> {
        Ok(match self {
            Value::Str(_) => self.clone(),
            Value::Int(v) => Value::Str(v.to_string().into_bytes()),
            Value::Float(v) => Value::Str(format!("{v:.6}").into_bytes()),
        })
    }

    /// `@()`(opcode 0x69):字符串 → 数值。
    ///
    /// §3.11:`0x` 前缀按十六进制;否则十进制/浮点。含非法字符 → 引擎报错;
    /// 手册称 β 版行为「不確定」(DIV-RT-002)。这里返回 `Unimplemented`。
    pub fn cast_to_int(&self) -> Result<Value, yu_log::Diag> {
        match self {
            Value::Int(_) => Ok(self.clone()),
            Value::Float(v) => Ok(Value::Int(*v as i64)),
            Value::Str(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                let t = text.trim();
                let parsed =
                    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                        i64::from_str_radix(hex, 16).ok()
                    } else if t.contains('.') {
                        t.parse::<f64>().ok().map(|v| v as i64)
                    } else {
                        t.parse::<i64>().ok()
                    };
                match parsed {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(diag::unimplemented(
                        code::rt::UNDETERMINED,
                        Boundary::Script,
                        format!("字符串转数值失败:{t:?}(手册:β 版不確定;DIV-RT-002)"),
                    )),
                }
            }
        }
    }
}

/// 浮点事件表示(稳定):整数值省略小数,其余用最短可回读表示。
fn format_float(v: f64) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else if v.is_infinite() {
        if v > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        }
    } else if v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v:?}")
    }
}

fn type_mix(op: &str, a: &Value, b: &Value) -> yu_log::Diag {
    diag::unimplemented(
        code::rt::VAR_TYPE,
        Boundary::Script,
        format!(
            "{op} 两侧类型不支持:{} {} {}(引擎读时转换细节未取证;DIV-RT-002)",
            a.type_name(),
            op,
            b.type_name()
        ),
    )
}

/// 一个已声明变量槽:元素类型 + 各维长度 + 行主序数据。
#[derive(Clone, Debug, PartialEq)]
pub struct VarSlot {
    pub ty: ElemType,
    pub dims: Vec<usize>,
    pub data: Vec<Value>,
}

impl VarSlot {
    pub fn new(ty: ElemType, dims: &[usize], init: &Value) -> Self {
        let n = element_count(dims);
        // 引擎写路径:标量值填满整个数组(§3.10 结论一)。
        let mut data = Vec::with_capacity(n);
        for _ in 0..n {
            data.push(coerce_to(ty, init));
        }
        Self {
            ty,
            dims: dims.to_vec(),
            data,
        }
    }

    pub fn is_scalar(&self) -> bool {
        self.dims.is_empty()
    }
}

fn coerce_to(ty: ElemType, v: &Value) -> Value {
    match (ty, v) {
        (ElemType::Int, Value::Int(i)) => Value::Int(*i),
        (ElemType::Int, Value::Float(f)) => Value::Int(*f as i64),
        (ElemType::Int, Value::Str(s)) => {
            let t = String::from_utf8_lossy(s);
            Value::Int(t.trim().parse::<i64>().unwrap_or(0))
        }
        (ElemType::Float, Value::Int(i)) => Value::Float(*i as f64),
        (ElemType::Float, Value::Float(f)) => Value::Float(*f),
        (ElemType::Float, Value::Str(s)) => {
            let t = String::from_utf8_lossy(s);
            Value::Float(t.trim().parse::<f64>().unwrap_or(0.0))
        }
        (ElemType::Str, Value::Str(s)) => Value::Str(s.clone()),
        (ElemType::Str, Value::Int(i)) => Value::Str(i.to_string().into_bytes()),
        (ElemType::Str, Value::Float(f)) => Value::Str(format!("{f:.6}").into_bytes()),
    }
}

fn element_count(dims: &[usize]) -> usize {
    if dims.is_empty() {
        1
    } else {
        // checked 乘积 + 防御上限(任务书 E R7:畸形维长不再触发
        // 乘法回绕/巨型分配 panic)。上限不是引擎语义,是内核自保:
        // 2^26 元素(int = 512 MiB)远超任何真实声明。
        const MAX_ELEMS: usize = 1 << 26;
        dims.iter()
            .try_fold(1usize, |a, &d| a.checked_mul(d))
            .filter(|n| *n <= MAX_ELEMS)
            .unwrap_or(MAX_ELEMS)
    }
}

/// 行主序线性化(最后一维最快)。
///
/// 证据:`docs/format-vm-opcodes.md` §3.10 末尾
/// `flat = Σ_k index[k] · Π_{j>k} dims[j]`。
pub fn linear_offset(dims: &[usize], indices: &[i64]) -> Result<usize, yu_log::Diag> {
    linear_offset_for(None, dims, indices)
}

/// 同 [`linear_offset`],但诊断里带上变量标识。
pub fn linear_offset_for(
    var: Option<VarRef>,
    dims: &[usize],
    indices: &[i64],
) -> Result<usize, yu_log::Diag> {
    if indices.len() != dims.len() {
        return Err(diag::fatal(
            code::rt::VAR_SHAPE,
            Boundary::Script,
            format!(
                "数组维数不匹配:目标 {} 维,给了 {} 个下标",
                dims.len(),
                indices.len()
            ),
        ));
    }
    let mut flat: usize = 0;
    for (k, (&dim, &idx)) in dims.iter().zip(indices.iter()).enumerate() {
        if idx < 0 {
            // 负下标的引擎行为未取证(引擎判据只写 `index > dims-1` 越界)。
            // 商业语料实测出现 `-1`(常见于「空数组读末元素」)。
            // 按军规:挂号 + 该次访问作废,不猜内存越界读(DIV-RT-002)。
            let who = var.map(|v| format!("{} ", v.display())).unwrap_or_default(); // 缺省:仅错误文案前缀
            return Err(diag::unimplemented(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!(
                    "{who}下标[{k}] 为负:{idx}(维长 {dims:?});负下标语义未取证,本次访问忽略(DIV-RT-002)"
                ),
            ));
        }
        let idx = idx as usize;
        if idx >= dim {
            // 引擎判据只写「`index > dims-1` → 报错 0x1a202」,而报错后
            // 是否中止脚本未取证;商业语料实测存在越界访问(常见于
            // 「先声明 1 长度数组、再按运行期长度用」)。按军规:
            // 挂号 + 该次访问作废,不越界读写内存(DIV-RT-002)。
            let who = var.map(|v| format!("{} ", v.display())).unwrap_or_default(); // 缺省:仅错误文案前缀
            return Err(diag::unimplemented(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!(
                    "{who}下标[{k}]={idx} 越界(第 {k} 维长度 {dim},全维 {dims:?});                     引擎报 0x1a202,后续行为未取证,本次访问忽略(DIV-RT-002)"
                ),
            ));
        }
        flat = flat * dim + idx;
    }
    Ok(flat)
}

/// 变量域:声明、读、写。键 = `(space,id)`。
///
/// # 扁平 slab(热路径去哈希)
///
/// `read`/`write` 是逐命令热路径:ERIS 场景构建帧实测单帧 52.7 万次 `LET`。
/// 旧实现每次都算 `(VarSpace, u16)` 元组的哈希再查 `FxHashMap`。
///
/// 变量 id 由 `ysv.ybn` 声明,实测分布很窄(ERIS 语料 548..4595,跨度 4048、
/// 实际条目 1781)——够直接数组索引。`id` 本身就是 `u16`,所以
/// `dense[space][id]` 可以覆盖**全部** id:
///
/// - `dense[space]`:`Vec<Option<Box<VarSlot>>>`,下标 = id,零哈希零探测;
/// - `overflow`:仅为防御保留(当前 `id: u16 ⇒ DENSE_CAP = 65536` 恒覆盖),
///   保持「任意 id 都正确」这一约束不被数组上限破坏。
///
/// 与单一哈希表**语义完全等价**:`declare`/`remove`/`iter` 都覆盖两级。
/// 依据:旁线 `docs/architecture.md` §6.1「变量用扁平 slab(按域预分配),
/// 避免哈希表查找」。数组按需增长(首次声明到该 id 才 `resize`),
/// ERIS 实测到 4596 项 × 8 字节 = 37 KB。
#[derive(Clone, Debug, Default)]
pub struct VariableStore {
    /// 直接索引区(按 space 分表,下标 = id)。
    dense: [Vec<Option<Box<VarSlot>>>; 2],
    /// `id >= DENSE_CAP` 的兜底(当前不可达,见类型文档)。
    overflow: rustc_hash::FxHashMap<(VarSpace, u16), VarSlot>,
    /// 每次可观察状态变化 +1。`VARACT[INIT]` 的“同掩码且期间无写入”
    /// 快速路径用它判断是否需要真的重建全局槽。
    generation: u64,
}

/// `dense` 的索引上限 = `u16` 全域(`VarRef::id` 是 `u16`,故全覆盖)。
const DENSE_CAP: usize = u16::MAX as usize + 1;

/// `VarSpace` → `dense` 下标(数值域 = 0,字符串域 = 1)。
///
/// `LegacyNum`/`LegacyStr` 只是**编码前缀**不同,正常路径已由
/// `canonical_var` 归一;这里再折叠一次,保证任何调用顺序都不会把同一个
/// 变量分裂成两个槽。
#[inline]
fn dense_index(space: VarSpace) -> usize {
    match space {
        VarSpace::Dollar | VarSpace::LegacyStr => 1,
        _ => 0,
    }
}

impl VariableStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn cell(&self, r: VarRef) -> Option<&VarSlot> {
        let index = usize::from(r.id);
        if index >= DENSE_CAP {
            return self.overflow.get(&(r.space, r.id));
        }
        self.dense[dense_index(r.space)]
            .get(index)
            .and_then(Option::as_deref)
    }

    #[inline]
    fn cell_mut(&mut self, r: VarRef) -> Option<&mut VarSlot> {
        let index = usize::from(r.id);
        if index >= DENSE_CAP {
            return self.overflow.get_mut(&(r.space, r.id));
        }
        self.dense[dense_index(r.space)]
            .get_mut(index)
            .and_then(Option::as_deref_mut)
    }

    pub fn is_declared(&self, r: VarRef) -> bool {
        self.cell(r).is_some()
    }

    /// 标量整数快速读取(表达式热路径):非标量/非 INT 返回 `None`,
    /// 调用方回退到通用 [`Self::read`](保持类型与错误语义)。
    #[inline]
    pub fn scalar_int(&self, r: VarRef) -> Option<i64> {
        let slot = self.cell(r)?;
        if !slot.is_scalar() {
            return None;
        }
        match slot.data.first() {
            Some(Value::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn slot(&self, r: VarRef) -> Option<&VarSlot> {
        self.cell(r)
    }

    pub fn declare(&mut self, r: VarRef, ty: ElemType, dims: &[usize], init: &Value) {
        let slot = VarSlot::new(ty, dims, init);
        let index = usize::from(r.id);
        if index >= DENSE_CAP {
            self.overflow.insert((r.space, r.id), slot);
        } else {
            let table = &mut self.dense[dense_index(r.space)];
            if table.len() <= index {
                table.resize_with(index + 1, || None);
            }
            table[index] = Some(Box::new(slot));
        }
        self.bump();
    }

    /// 声明并**尽量复用已有存储**:类型/维数不变时只重置元素值。
    ///
    /// 与 [`Self::declare`] 的可观察状态相同(每个元素 = `coerce(ty, init)`、
    /// 同 type/dims);区别是不重新分配 Vec。`VARACT[INIT]` 在启动脚本里
    /// 被循环调用数万次,原实现每次重建所有全局槽是首帧 2s 的主因。
    pub fn reset_declared(&mut self, r: VarRef, ty: ElemType, dims: &[usize], init: &Value) {
        if let Some(slot) = self.cell_mut(r) {
            if slot.ty == ty && slot.dims.as_slice() == dims {
                for value in &mut slot.data {
                    *value = coerce_to(ty, init);
                }
                self.bump();
                return;
            }
        }
        self.declare(r, ty, dims, init);
    }

    /// 可观察状态代号(写入/声明/删除/改长度都会改变它)。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// 改变一维数组长度,**保留已有元素**(新增元素取类型零值)。
    ///
    /// 手册 `variable/varact.html` 例 9/10:`DIMSIZE` 只改「数组大小」;
    /// ERIS 的 `VAR.ADD` 宏依赖它做追加(`DIMSIZE+=1` → 写末元素),
    /// 若重填整个数组会把先前追加的元素抹掉(实测:13 次追加只剩最后 1 个)。
    pub fn resize_1d(&mut self, r: VarRef, new_len: usize) -> Result<(), yu_log::Diag> {
        // 防御上限:脚本给的 DIMSIZE 值超出 2^26 ⇒ 拒绝(畸形脚本,任务书 E
        // R7;真实声明最大 .TX.MAX=4096)。不是引擎语义,是内核自保。
        const MAX_ELEMS: usize = 1 << 26;
        if new_len > MAX_ELEMS {
            return Err(diag::fatal(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!("DIMSIZE 新长度 {new_len} 超出防御上限 {MAX_ELEMS}(畸形脚本)"),
            ));
        }
        let Some(slot) = self.cell_mut(r) else {
            return Err(diag::fatal(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!("DIMSIZE 目标未声明 {}", r.display()),
            ));
        };
        if slot.dims.len() != 1 {
            return Err(diag::fatal(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!("DIMSIZE 只支持一维数组,目标 {} 维", slot.dims.len()),
            ));
        }
        let old = slot.dims[0];
        if new_len == old {
            return Ok(());
        }
        let ty = slot.ty;
        slot.data.resize(new_len, ty.default_value());
        slot.dims[0] = new_len;
        self.bump();
        Ok(())
    }

    /// 一维数组的 PUSH/POP(在 `pos` 处插入/删除一个元素,长度不变,
    /// 空出的位置填类型零值)。
    ///
    /// 与旧实现(整数组读成 `Vec<Value>`、clone、逐元素写回)可观察状态
    /// 相同;这里用切片 rotate 原地搬运,不分配、不 clone 元素。
    /// 证据:手册 `variable/varact.html` 例 9/10 的 `PUSH/POP` 语义。
    pub fn shift_1d(&mut self, r: VarRef, pos: usize, push: bool) -> Result<(), yu_log::Diag> {
        let slot = self.cell_mut(r).ok_or_else(|| {
            diag::unimplemented(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!("PUSH/POP 目标未声明 {}", r.display()),
            )
        })?;
        if slot.dims.len() != 1 {
            return Err(diag::fatal(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                "PUSH/POP 只支持一维数组",
            ));
        }
        let n = slot.dims[0];
        if n > 0 {
            let p = pos.min(n - 1);
            if push {
                slot.data[p..n].rotate_right(1);
                slot.data[p] = slot.ty.default_value();
            } else {
                slot.data[p..n].rotate_left(1);
                slot.data[n - 1] = slot.ty.default_value();
            }
        }
        self.bump();
        Ok(())
    }

    /// 声明若不存在(用于 YSVR 初值镜像与声明命令)。
    pub fn declare_if_absent(&mut self, r: VarRef, ty: ElemType, dims: &[usize], init: &Value) {
        if self.cell(r).is_some() {
            return;
        }
        self.declare(r, ty, dims, init);
    }

    pub fn remove(&mut self, r: VarRef) -> Option<VarSlot> {
        let index = usize::from(r.id);
        let removed = if index >= DENSE_CAP {
            self.overflow.remove(&(r.space, r.id))
        } else {
            let table = &mut self.dense[dense_index(r.space)];
            table.get_mut(index).and_then(Option::take).map(|b| *b)
        };
        if removed.is_some() {
            self.bump();
        }
        removed
    }

    /// 读取(标量或数组元素)。未声明 → `rt.var.undeclared`。
    ///
    pub fn read(&self, r: VarRef, indices: &[i64]) -> Result<Value, yu_log::Diag> {
        let slot = self.cell(r).ok_or_else(|| {
            // 引擎错误文案「変数を初期化しないで使用しようとしました。」
            // (ysc.ybn 尾部消息表)。脚本错误 ⇒ Fatal(官方继续策略未取证;
            // DIV-RT-005)。
            diag::fatal(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!(
                    "读取未声明变量 {}(引擎错误「変数を初期化しないで使用しようとしました。」;                     声明来自工程 ysv/声明命令)",
                    r.display()
                ),
            )
        })?;
        if slot.is_scalar() && indices.is_empty() {
            return Ok(slot.data[0].clone());
        }
        let flat = if indices.is_empty() {
            // 引擎 opcode 0x48:数组变量不带下标时读 0 号元素(§3.10)。
            let zeros: smallvec::SmallVec<[i64; 4]> = smallvec::smallvec![0i64; slot.dims.len()];
            linear_offset_for(Some(r), &slot.dims, &zeros)?
        } else {
            linear_offset_for(Some(r), &slot.dims, indices)?
        };
        Ok(slot.data[flat].clone())
    }

    /// 写入:`indices` 空 = 标量;数组元素写入,类型按**槽位当前类型**转换。
    /// 写时无跨类型转换(§3.10 结论一)——只有同类型的旧值被替换。
    /// 写入。返回值：所有调用方都丢弃(旧签名返回存储值，白白多一次
    /// `Value` 克隆 —— 字符串变量就是一次堆复制)。
    pub fn write(&mut self, r: VarRef, indices: &[i64], value: &Value) -> Result<(), yu_log::Diag> {
        let slot = self.cell_mut(r).ok_or_else(|| {
            diag::fatal(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!(
                    "写入未声明变量 {}(引擎同族错误;脚本先声明或用工程 ysv 初值)",
                    r.display()
                ),
            )
        })?;
        let ty = slot.ty;
        if slot.is_scalar() && indices.is_empty() {
            slot.data[0] = coerce_to(ty, value);
            self.bump();
            return Ok(());
        }
        let stored = coerce_to(ty, value);
        let flat = if indices.is_empty() {
            let zeros: smallvec::SmallVec<[i64; 4]> = smallvec::smallvec![0i64; slot.dims.len()];
            linear_offset_for(Some(r), &slot.dims, &zeros)?
        } else {
            linear_offset_for(Some(r), &slot.dims, indices)?
        };
        slot.data[flat] = stored;
        self.bump();
        Ok(())
    }

    /// 槽位类型/维数(诊断与 `VARINFO` 用)。
    pub fn shape(&self, r: VarRef) -> Option<(ElemType, &[usize])> {
        self.cell(r).map(|s| (s.ty, s.dims.as_slice()))
    }

    /// 整组恢复(`LOAD[LET]` 写回;B1):目标槽已按同型声明时替换全部元素。
    ///
    /// 手册 `file/load.html`:「保存した配列変数を読み出す時は、保存した時と
    /// 全く同じ配列サイズの配列変数を用意します。サイズが違う場合は、
    /// エラーとなります」——类型/维数/元素数不一致由调用方(VM)先查声明
    /// 并报错;本方法只做一致路径(引擎 `0x447b3d` 同型整写)。
    pub fn restore_elements(&mut self, r: VarRef, data: Vec<Value>) -> Result<(), yu_log::Diag> {
        let slot = self.cell_mut(r).ok_or_else(|| {
            diag::fatal(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!("恢复目标未声明 {}(手册:读档前脚本须先声明)", r.display()),
            )
        })?;
        if slot.data.len() != data.len() {
            return Err(diag::fatal(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!(
                    "恢复元素数不符:{} 槽 {} 个,块 {} 个",
                    r.display(),
                    slot.data.len(),
                    data.len()
                ),
            ));
        }
        slot.data = data;
        self.bump();
        Ok(())
    }

    /// 一维数组线性查找(`VARINFO SEARCH`):从 `start` 起找第一个等于
    /// `needle` 的元素下标。
    ///
    /// 直接扫槽内数据;旧实现每个元素走一次 `VmAccess::read`(槽查找 +
    /// `Value` clone,字符串还会复制),169 元素的按钮表搜索要 ~10µs。
    pub fn search_1d(
        &self,
        r: VarRef,
        start: usize,
        needle: &Value,
        ty: ElemType,
    ) -> Option<usize> {
        let slot = self.cell(r)?;
        for (index, value) in slot.data.iter().enumerate().skip(start) {
            let hit = match ty {
                ElemType::Str => {
                    matches!((needle, value), (Value::Str(a), Value::Str(b)) if a == b)
                }
                _ => needle.as_f64() == value.as_f64(),
            };
            if hit {
                return Some(index);
            }
        }
        None
    }

    /// 键的**确定顺序**遍历(存档/事件用)。
    ///
    /// 顺序契约不变:按 `(space, id)` 升序。两级索引的物理顺序不同
    /// (`dense[0]` 全部数值域、`dense[1]` 全部字符串域),这里显式合并两层
    /// 再排序,保证与旧哈希表实现同样的输出序列。
    pub fn iter(&self) -> impl Iterator<Item = (VarRef, &VarSlot)> {
        let mut rows: Vec<(VarSpace, u16, &VarSlot)> = Vec::with_capacity(self.len());
        for (table_index, table) in self.dense.iter().enumerate() {
            // `dense` 的下标 0 = 数值域(`At`)、1 = 字符串域(`Dollar`)。
            // 存的 `VarRef` 必须回填**真实 space**:`LegacyNum`/`LegacyStr`
            // 编码的前缀不同,但归一化后进 store 的恒为 `At`/`Dollar`
            // (`canonical_var`),与旧实现的键集合一致。
            let space = if table_index == 1 {
                VarSpace::Dollar
            } else {
                VarSpace::At
            };
            for (id, cell) in table.iter().enumerate() {
                if let Some(slot) = cell.as_deref() {
                    rows.push((space, id as u16, slot));
                }
            }
        }
        for (&(space, id), slot) in &self.overflow {
            rows.push((space, id, slot));
        }
        rows.sort_unstable_by_key(|(space, id, _)| (*space, *id));
        rows.into_iter()
            .map(|(space, id, slot)| (VarRef { space, id }, slot))
    }

    pub fn len(&self) -> usize {
        let dense: usize = self
            .dense
            .iter()
            .map(|table| table.iter().filter(|cell| cell.is_some()).count())
            .sum();
        dense + self.overflow.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_major_last_dim_fastest() {
        let dims = [2, 3];
        assert_eq!(linear_offset(&dims, &[0, 0]).unwrap(), 0);
        assert_eq!(linear_offset(&dims, &[0, 2]).unwrap(), 2);
        assert_eq!(linear_offset(&dims, &[1, 0]).unwrap(), 3);
        assert_eq!(linear_offset(&dims, &[1, 2]).unwrap(), 5);
    }

    #[test]
    fn array_write_fills_scalar_and_reads_zero_element() {
        let mut store = VariableStore::new();
        let r = VarRef::new(VarSpace::At, 1000);
        store.declare(r, ElemType::Int, &[10], &Value::Int(0));
        store.write(r, &[3], &Value::Int(7)).unwrap();
        // 不带下标读 = 0 号元素(引擎 0x48)
        assert_eq!(store.read(r, &[]).unwrap(), Value::Int(0));
        assert_eq!(store.read(r, &[3]).unwrap(), Value::Int(7));
    }

    #[test]
    fn out_of_range_is_fatal() {
        let mut store = VariableStore::new();
        let r = VarRef::new(VarSpace::At, 1001);
        store.declare(r, ElemType::Int, &[2, 2], &Value::Int(0));
        let d = store.write(r, &[2, 0], &Value::Int(1)).unwrap_err();
        assert_eq!(d.code.as_ref(), code::rt::VAR_SHAPE);
    }

    #[test]
    fn float_div_zero_keeps_dividend() {
        // 官方 opcode 47 浮点分支:`fcom 0; je 出口` ⇒ 除数为 0 时结果槽
        // 原样保留,即被除数(不是 IEEE 的 ±Inf/NaN;DIV-RT-003 勘误)。
        assert_eq!(
            Value::Float(5.0).div(&Value::Float(0.0)).unwrap(),
            Value::Float(5.0)
        );
        assert_eq!(
            Value::Float(-5.0).div(&Value::Float(0.0)).unwrap(),
            Value::Float(-5.0)
        );
        assert_eq!(
            Value::Float(0.0).div(&Value::Float(0.0)).unwrap(),
            Value::Float(0.0)
        );
    }

    #[test]
    fn string_concat_and_int_div_zero() {
        let a = Value::Str(b"ABC".to_vec());
        let b = Value::Str(b".BMP".to_vec());
        assert_eq!(a.add(&b).unwrap(), Value::Str(b"ABC.BMP".to_vec()));
        // 官方整数分支除数 0 → 结果写 0(`0x42954e`),不是 Fatal。
        assert_eq!(Value::Int(1).div(&Value::Int(0)).unwrap(), Value::Int(0));
        assert_eq!(Value::Int(7).rem(&Value::Int(0)).unwrap(), Value::Int(0));
    }

    #[test]
    fn float_to_string_is_six_decimals() {
        assert_eq!(
            Value::Float(1.5).cast_to_str().unwrap(),
            Value::Str(b"1.500000".to_vec())
        );
        assert_eq!(
            Value::Int(123).cast_to_str().unwrap(),
            Value::Str(b"123".to_vec())
        );
    }
}
