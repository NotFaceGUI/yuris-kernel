//! 表达式字节码求值。
//!
//! # 记录格式(引擎取证)
//!
//! `<side-repo>/docs/format-vm-opcodes.md` §3.9(0.495 `yu-ris.exe`
//! sha256 `4f5eefa3…`,`FUN_00426760` 逐指令读出):
//!
//! ```text
//! [op:u8][payload_len:u16 LE][payload:payload_len]
//! ```
//!
//! 走查器按 `i += 3 + u16(src+i+1)` 前进,按首字节查 256 项分派表。
//! 本文件的 [`decode`] 复刻该记录切分,**不网络文本解析**(旧结论"表达式是
//! 文本"已被引擎汇编推翻,见 §3.8→§3.9 的勘误)。
//!
//! # 已实现操作码(逐条出处)
//!
//! 表内 `码` 为十进制(与同文档 §3.9.2 一致,十六进制见括号)。三层出处:
//! ①手册:表达式语法/比较/字符串拼接/显式转换;
//! ②语料:ERIS 137 个 YSTB 的 80,102 个窗口全量扫描频度(括号内次数);
//! ③引擎:§3.9.2 处理体地址与指令序列(只列关键判据)。
//!
//! | 码 | 语义 | 手册 | 语料频度 | 引擎判据 |
//! |---|---|---|---|---|
//! | 33 (0x21) | `!=` | `control/if.html` 例 | 7,092 | `setne`+`sub/sbb` |
//! | 37 (0x25) | `%` | — | 542 | `call 0x46a590`,除数为 0 → 0 |
//! | 38 (0x26) | `&&` | `control/if.html` | 5,413 | 两处 `je` 写 0 |
//! | 41 (0x29) | 数组取元素 | `basic/variable.html` 数组例 | 138,709 | 引用点上方值=下标 |
//! | 42 (0x2a) | `*` | `basic/variable.html` 例 2 | 17,719 | `fmul`+`call 0x46a550` |
//! | 43 (0x2b) | `+`(含串拼接) | `variable.html` 例 6/7 | 48,686 | `add/adc`+`fadd`+串拼上限 0x1000 |
//! | 44 (0x2c) | 组分隔 | — | 24,321 | 空操作 |
//! | 45 (0x2d) | `-` | 例 4 | 14,722 | `sub/sbb`+`fsub` |
//! | 47 (0x2f) | `/` | 例 4 | 7,513 | `fdivrp`+`call 0x469bd0` |
//! | 60 (0x3c) | `<` | `control/if.html` | 4,532 | `jge→0` |
//! | 61 (0x3d) | `==` | `control/if.html` 例 | 29,231 | `sete`+`strcmp` |
//! | 62 (0x3e) | `>` | 例 | 6,179 | `jl→0` |
//! | 65 (0x41) | `&` | — | 622 | 仅 `and` |
//! | 66 (0x42) | 压 i8 | — | 204,797 | `movsx` |
//! | 70 (0x46) | 压 f64 | — | 7,944 | `fld qword` |
//! | 72 (0x48) | 取变量值 | `variable.html` | 321,267 | 数组→0 号元素 |
//! | 73 (0x49) | 压 i32 | — | 3,297 | `fild dword` |
//! | 76 (0x4c) | 压 i64 | — | 8,861 | `fild qword` |
//! | 77 (0x4d) | 压字符串 | `variable.html` 例 | 72,131 | 界定符+转义 |
//! | 79 (0x4f) | `\|` | — | 15 | 仅 `or` |
//! | 82 (0x52) | 一元 `-` | — | 6,499 | `sub/sbb`+`fchs` |
//! | 83 (0x53) | `<=` | `control/if.html` | 1,189 | `jl→1` |
//! | 86 (0x56) | 取变量引用 | `variable/let.html` | 139,664 | 赋值目标/数组基点 |
//! | 87 (0x57) | 压 i16 | — | 16,552 | `movsx word` |
//! | 90 (0x5a) | `>=` | `control/if.html` | 3,586 | `jge→1` |
//! | 94 (0x5e) | `^` | — | 119 | 仅 `xor` |
//! | 105 (0x69) | 字符串→数值 | `variable.html` 例 4 | 14 | `0x`/十进制扫描 |
//! | 115 (0x73) | 数值→字符串 | `variable.html` 例 1/3 | 9,173 | 十进制/`%f` |
//! | 118 (0x76) | 取变量引用(延迟) | `variable/let.html` 数组左值 | 3,220 | 编译器 fixup(细节 Likely) |
//! | 124 (0x7c) | `\|\|` | `control/if.html` | 4,037 | `(a≠0) or (b≠0)` |

use yu_log::Boundary;

use crate::code;
use crate::diag;
use crate::value::{Value, VarRef, VarSpace};

/// 一条已切分的表达式记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawInstr {
    pub offset: usize,
    pub op: u8,
    /// 指令载荷(窗口里 `[op][len:u16][payload]` 的 payload 段)。
    ///
    /// 实测 `SmallVec<[u8; 16]>` 内联版本反而更慢(IF 233→256ns):求值循环
    /// 里 `SmallVec` 的访问分支比一次指针追堆更贵,故保持 `Vec`。
    pub payload: Vec<u8>,
}

/// 栈槽:值或变量引用(引擎的平行引用栈模型,§3.9.1)。
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Val(Value),
    Ref(VarRef),
}

/// GOSUB/RETURN 参数槽的目标类型(由工程 ysc 参数名决定)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamTy {
    Int,
    Float,
    Str,
    /// 未识别前缀:原样传递(与 `coerce_param` 的 else 分支一致)。
    Raw,
}

impl ParamTy {
    /// 与 `handlers::coerce_param` 同口径的转换。
    pub fn coerce(self, v: Value) -> Value {
        match self {
            // silent-ok:与旧 `coerce_param` 同口径(非数值按 0)
            ParamTy::Int => Value::Int(
                v.as_int()
                    .unwrap_or_else(|| v.as_f64().unwrap_or(0.0) as i64), // silent-ok:非数值按 0
            ),
            // silent-ok:同口径
            ParamTy::Float => Value::Float(v.as_f64().unwrap_or(0.0)),
            ParamTy::Str => match v {
                Value::Str(_) => v,
                other => Value::Str(other.to_event_repr().into_bytes()),
            },
            ParamTy::Raw => v,
        }
    }
}

/// 左值:变量 + 下标表(空 = 标量)。
#[derive(Clone, Debug, PartialEq)]
pub struct LValue {
    pub var: VarRef,
    /// 数组下标。绝大多数是 0–2 维;用 SmallVec 避免每次 LET 的堆分配。
    pub indices: smallvec::SmallVec<[i64; 4]>,
}

/// 预编译的数组下标表达式(启动后窗口字节不变,`LET` 热路径直接执行)。
///
/// 只覆盖编译器实际生成的最常见形态(`var` / `var±const` / 常量);
/// 其它形态返回 `None`,运行时回退到通用栈机,语义不变。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IndexExpr {
    Const(i64),
    Var(VarRef),
    /// `var <op> const`,op ∈ + - * & | ^(与 `binary()` 同族)。
    VarOpConst { var: VarRef, op: u8, value: i64 },
}

/// 预编译的简单值表达式(IF 条件 / LET 右值等热路径)。
///
/// 只覆盖 `常量`、`变量` 与一次整数二元运算的形态;数组取值、字符串、
/// 浮点常量等仍走通用栈机。求值顺序与 `binary()` 完全一致。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimpleExpr {
    Const(i64),
    Var(VarRef),
    VarOpConst { var: VarRef, op: u8, value: i64 },
    ConstOpVar { value: i64, op: u8, var: VarRef },
    VarOpVar { lhs: VarRef, op: u8, rhs: VarRef },
}

/// 尝试把已解码窗口识别为 [`SimpleExpr`]。
#[must_use]
pub fn simple_expr(instrs: &[RawInstr]) -> Option<SimpleExpr> {
    match instrs {
        [one] => match int_const(one) {
            Some(value) => Some(SimpleExpr::Const(value)),
            None if one.op == 0x48 => Some(SimpleExpr::Var(var_of(one)?)),
            None => None,
        },
        [first, second, op] if is_simple_binop(op.op) => {
            let lhs = match int_const(first) {
                Some(value) => SimpleExpr::Const(value),
                None if first.op == 0x48 => SimpleExpr::Var(var_of(first)?),
                None => return None,
            };
            let rhs = match int_const(second) {
                Some(value) => SimpleExpr::Const(value),
                None if second.op == 0x48 => SimpleExpr::Var(var_of(second)?),
                None => return None,
            };
            fold_simple(op.op, lhs, rhs)
        }
        _ => None,
    }
}

fn fold_simple(op: u8, lhs: SimpleExpr, rhs: SimpleExpr) -> Option<SimpleExpr> {
    match (lhs, rhs) {
        (SimpleExpr::Const(a), SimpleExpr::Const(b)) => Some(SimpleExpr::Const(fold_int_pair(
            op, a, b,
        )?)),
        (SimpleExpr::Var(var), SimpleExpr::Const(value)) => {
            Some(SimpleExpr::VarOpConst { var, op, value })
        }
        (SimpleExpr::Const(value), SimpleExpr::Var(var)) => {
            if is_commutative(op) {
                Some(SimpleExpr::VarOpConst { var, op, value })
            } else {
                Some(SimpleExpr::ConstOpVar { value, op, var })
            }
        }
        (SimpleExpr::Var(lhs), SimpleExpr::Var(rhs)) => {
            Some(SimpleExpr::VarOpVar { lhs, op, rhs })
        }
        _ => None,
    }
}

fn is_simple_binop(op: u8) -> bool {
    matches!(
        op,
        0x21 | 0x25 | 0x26 | 0x2a | 0x2b | 0x2d | 0x2f | 0x3c | 0x3d | 0x3e | 0x41 | 0x4f
            | 0x53 | 0x5a | 0x5e | 0x7c
    )
}

fn is_commutative(op: u8) -> bool {
    matches!(op, 0x2b | 0x2a | 0x26 | 0x41 | 0x4f | 0x5e | 0x21 | 0x3d | 0x7c)
}

fn fold_int_pair(op: u8, a: i64, b: i64) -> Option<i64> {
    Some(match op {
        0x2b => a.wrapping_add(b),
        0x2d => a.wrapping_sub(b),
        0x2a => a.wrapping_mul(b),
        0x26 => i64::from((a != 0) && (b != 0)),
        0x41 => a & b,
        0x4f => a | b,
        0x5e => a ^ b,
        0x21 => i64::from(a != b),
        0x3d => i64::from(a == b),
        0x3c => i64::from(a < b),
        0x3e => i64::from(a > b),
        0x53 => i64::from(a <= b),
        0x5a => i64::from(a >= b),
        0x7c => i64::from((a != 0) || (b != 0)),
        // 除法/取余有整数除零特例,不在常量折叠里复刻。
        _ => return None,
    })
}

/// 整数立即数(0x42/0x57/0x49/0x4c)。
fn int_const(ins: &RawInstr) -> Option<i64> {
    match ins.op {
        0x42 if ins.payload.len() == 1 => Some(i64::from(ins.payload[0] as i8)),
        0x57 if ins.payload.len() == 2 => Some(i64::from(i16::from_le_bytes([
            ins.payload[0],
            ins.payload[1],
        ]))),
        0x49 if ins.payload.len() == 4 => Some(i64::from(i32::from_le_bytes([
            ins.payload[0],
            ins.payload[1],
            ins.payload[2],
            ins.payload[3],
        ]))),
        0x4c if ins.payload.len() == 8 => read_i64(&ins.payload).ok(),
        _ => None,
    }
}

/// 左值窗口的预编译计划(变量 + 各维下标表达式)。
#[derive(Clone, Debug, PartialEq)]
pub struct LValuePlan {
    pub var: VarRef,
    pub indices: Vec<IndexExpr>,
}

/// 尝试把已解码窗口识别为「变量引用 + 简单下标」。
///
/// 判定与 [`Evaluator::eval_lvalue_instrs_with`] 的通用路径严格同形:
/// 首条必须是引用指令(`0x56/0x76`),末条 `0x29` 是左值标记;
/// 中间只允许常量、取值引用、`0x2c` 空操作与可折叠的整数二元运算。
#[must_use]
pub fn lvalue_plan(instrs: &[RawInstr]) -> Option<LValuePlan> {
    let (first, rest) = instrs.split_first()?;
    if !matches!(first.op, 0x56 | 0x76) {
        return None;
    }
    let var = var_of(first)?;
    // 末条 `0x29` 只是「把左值交给 LET」的标记;内层 `0x29`(数组取值)
    // 不在本快速路径覆盖范围内。
    let body = match rest.last() {
        Some(last) if last.op == 0x29 => &rest[..rest.len() - 1],
        _ => rest,
    };
    let mut stack: Vec<IndexExpr> = Vec::new();
    for ins in body {
        match ins.op {
            0x2c => {}
            0x42 if ins.payload.len() == 1 => {
                stack.push(IndexExpr::Const(i64::from(ins.payload[0] as i8)));
            }
            0x57 if ins.payload.len() == 2 => {
                stack.push(IndexExpr::Const(i64::from(i16::from_le_bytes([
                    ins.payload[0],
                    ins.payload[1],
                ]))));
            }
            0x49 if ins.payload.len() == 4 => {
                stack.push(IndexExpr::Const(i64::from(i32::from_le_bytes([
                    ins.payload[0],
                    ins.payload[1],
                    ins.payload[2],
                    ins.payload[3],
                ]))));
            }
            0x4c if ins.payload.len() == 8 => {
                stack.push(IndexExpr::Const(read_i64(&ins.payload).ok()?));
            }
            0x48 => stack.push(IndexExpr::Var(var_of(ins)?)),
            // 0x56/0x76 是“引用”指令:通用左值路径只把**第一条**当左值基址,
            // 后续 Ref 落到下标表会报「下标表混入变量引用」。这里直接回退,
            // 让通用路径给出同一条诊断。
            0x21 | 0x25 | 0x26 | 0x2a | 0x2b | 0x2d | 0x2f | 0x3c | 0x3d | 0x3e | 0x41 | 0x4f
            | 0x53 | 0x5a | 0x5e | 0x7c => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(fold_index_binop(ins.op, lhs, rhs)?);
            }
            // 0x29(内层数组取值)/字符串/浮点常量/一元运算等:走通用路径。
            _ => return None,
        }
    }
    Some(LValuePlan {
        var,
        indices: stack,
    })
}

fn fold_index_binop(op: u8, lhs: IndexExpr, rhs: IndexExpr) -> Option<IndexExpr> {
    match (lhs, rhs) {
        (IndexExpr::Const(a), IndexExpr::Const(b)) => {
            let value = match op {
                0x2b => a.wrapping_add(b),
                0x2d => a.wrapping_sub(b),
                0x2a => a.wrapping_mul(b),
                0x41 => a & b,
                0x4f => a | b,
                0x5e => a ^ b,
                _ => return None,
            };
            Some(IndexExpr::Const(value))
        }
        (IndexExpr::Var(var), IndexExpr::Const(value)) => {
            matches!(op, 0x2b | 0x2d | 0x2a | 0x41 | 0x4f | 0x5e).then_some(IndexExpr::VarOpConst {
                var,
                op,
                value,
            })
        }
        // 交换律运算:`const <op> var` 与 `var <op> const` 同值。
        (IndexExpr::Const(value), IndexExpr::Var(var)) if matches!(op, 0x2b | 0x2a | 0x41 | 0x4f | 0x5e) => {
            Some(IndexExpr::VarOpConst { var, op, value })
        }
        _ => None,
    }
}

/// 求值期变量访问。由 VM 实现(变量域 + 系统变量 + 输入快照)。
pub trait VarAccess {
    fn read(&mut self, var: VarRef, indices: &[i64]) -> Result<Value, yu_log::Diag>;

    /// 标量整数快速读取(命中返回 `Some`;非标量/非 INT 返回 `None`)。
    ///
    /// 默认实现回退到 [`Self::read`];VM 覆盖它以避免表达式热路径上的
    /// `Value` 构造/克隆。返回 `None` 时调用方必须再走 `read`,类型与错误
    /// 语义完全不变。
    fn read_scalar_int(&mut self, var: VarRef) -> Result<Option<i64>, yu_log::Diag> {
        Ok(self.read(var, &[])?.as_int())
    }
}

/// 表达式求值器。
pub struct Evaluator<'a> {
    access: &'a mut dyn VarAccess,
    codec: yu_format::Codec,
    /// 官方「整数路径」(`0x7b99c4` 为奇数)语义。
    ///
    /// 证据(0.495 `yu-ris.exe`):
    /// - ysc 参数类型 3(`ParamKind::Other(3)`,如 `LOOP` 的 `SET`)在
    ///   `0x4620b8` 的求值表里映射到 `0x426870`;该 wrapper 置
    ///   `0x7b99c4=1`(整数模式)后调表达式机 `0x426760`;
    /// - 整数模式下压 f64 常量走 `0x4287e8`:
    ///   `or 0xC00; fldcw; fistp qword` —— 显式向零截断后只压 i64 槽;
    /// - `/`、`%` 等的整数分支只读 i64 槽(`0x4294ec`、`0x4295a0`),
    ///   与浮点分支互斥。
    ///
    /// 默认 `false`(按运行期值类型分派);仅官方整数参数的求值入口
    /// (`Vm::eval_window_int`)打开。
    int_flavor: bool,
}

impl<'a> Evaluator<'a> {
    pub fn new(access: &'a mut dyn VarAccess) -> Self {
        Self {
            access,
            codec: yu_format::Codec::Cp932,
            int_flavor: false,
        }
    }

    /// 按执行层全局 codec 扫描字符串字面量。
    pub fn with_codec(access: &'a mut dyn VarAccess, codec: yu_format::Codec) -> Self {
        Self {
            access,
            codec,
            int_flavor: false,
        }
    }

    /// 切到官方整数路径(证据见 [`Self::int_flavor`])。
    pub fn with_int_flavor(mut self) -> Self {
        self.int_flavor = true;
        self
    }

    /// 执行整个窗口,返回剩余栈。
    pub fn eval_pooled(
        &mut self,
        window: &[u8],
        stack: &mut Vec<Slot>,
        instrs: &mut Vec<RawInstr>,
    ) -> Result<(), yu_log::Diag> {
        decode_pooled(window, instrs)?;
        self.eval_instrs(instrs, stack)
    }

    /// 已解码窗口的执行入口(VM 解码缓存命中路径)。
    pub fn eval_instrs(
        &mut self,
        instrs: &[RawInstr],
        stack: &mut Vec<Slot>,
    ) -> Result<(), yu_log::Diag> {
        stack.clear();
        for (n, ins) in instrs.iter().enumerate() {
            self.step(ins, stack, n + 1 == instrs.len())?;
        }
        Ok(())
    }

    pub fn eval(&mut self, window: &[u8]) -> Result<Vec<Slot>, yu_log::Diag> {
        let instrs = decode(window)?;
        let mut stack: Vec<Slot> = Vec::new();
        for (i, ins) in instrs.iter().enumerate() {
            self.step(ins, &mut stack, i + 1 == instrs.len())?;
        }
        Ok(stack)
    }

    /// 求值并取末值(LET 右值 / 条件 / 参数)。
    /// `eval_value` 的复用栈版(语义一致;热路径零分配)。
    pub fn eval_value_pooled(
        &mut self,
        window: &[u8],
        stack: &mut Vec<Slot>,
        instrs: &mut Vec<RawInstr>,
    ) -> Result<Value, yu_log::Diag> {
        decode_pooled(window, instrs)?;
        self.eval_value_instrs(instrs, stack)
    }

    /// 已解码窗口的求值入口(VM 解码缓存命中路径)。
    pub fn eval_value_instrs(
        &mut self,
        instrs: &[RawInstr],
        stack: &mut Vec<Slot>,
    ) -> Result<Value, yu_log::Diag> {
        self.eval_instrs(instrs, stack)?;
        match stack.pop() {
            Some(Slot::Val(v)) => Ok(v),
            Some(Slot::Ref(r)) => Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                format!("求值结果是被引用的变量槽({}),不是值", r.display()),
            )),
            None => Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                "空表达式(无结果)",
            )),
        }
    }

    pub fn eval_value(&mut self, window: &[u8]) -> Result<Value, yu_log::Diag> {
        let mut stack = self.eval(window)?;
        match stack.pop() {
            Some(Slot::Val(v)) => Ok(v),
            Some(Slot::Ref(r)) => Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                format!("表达式以变量引用 {} 结尾,此处需要值", r.display()),
            )),
            None => Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                "表达式窗口为空",
            )),
        }
    }

    /// 解析左值(`LET` 第 1 窗口 / `LET` 型参数槽)。
    ///
    /// 语料形态(全量扫描,8,136 个 LET):
    /// - 标量:`48/56/76 [前缀][id]`;
    /// - 数组:`56 [前缀][id] <下标表达式…> 29 01 00 00`。
    ///
    /// **关键细节**(按真实字节修正,project-eris s2 pc16):
    /// 窗口里可能有**多条** `0x29`:只有**最后一条**是「延迟装载」标记
    /// (把左值交给 LET),内层的 `0x29` 是普通数组取值,必须真的执行。
    /// 例:`ref($arr) ref(@cnt) 0 aload 1 sub aload` =
    /// `$arr[@cnt(0)-1]` —— 若把两条 aload 都跳过,左值会错解析成 `@cnt[-1]`。
    pub fn eval_lvalue(&mut self, window: &[u8]) -> Result<LValue, yu_log::Diag> {
        let instrs = decode(window)?;
        self.eval_lvalue_instrs(&instrs)
    }

    /// 已解码窗口的左值解析(VM 解码缓存命中路径)。
    pub fn eval_lvalue_instrs(&mut self, instrs: &[RawInstr]) -> Result<LValue, yu_log::Diag> {
        let mut stack = Vec::new();
        self.eval_lvalue_instrs_with(instrs, &mut stack)
    }

    /// 已解码窗口的左值解析(复用调用方栈;VM 热路径零分配)。
    pub fn eval_lvalue_instrs_with(
        &mut self,
        instrs: &[RawInstr],
        stack: &mut Vec<Slot>,
    ) -> Result<LValue, yu_log::Diag> {
        stack.clear();
        if instrs.is_empty() {
            return Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                "左值窗口为空",
            ));
        }
        // 单条变量指令 = 标量左值。
        if instrs.len() == 1 {
            if let Some(r) = var_of(&instrs[0]) {
                return Ok(LValue {
                    var: r,
                    indices: smallvec::SmallVec::new(),
                });
            }
        }
        // 末条 `0x29` 是左值标记:剥掉它,其余照常执行。
        let body: &[RawInstr] = match instrs.last() {
            Some(last) if last.op == 0x29 => &instrs[..instrs.len() - 1],
            _ => instrs,
        };
        for ins in body {
            if matches!(ins.op, 0x56 | 0x76) {
                // 引用指令原样压栈(0x48 取值语义交给 step)。
                let r = var_of(ins).ok_or_else(|| {
                    diag::fatal(
                        code::rt::EXPR_MALFORMED,
                        Boundary::Script,
                        "变量引用载荷不是 3 字节",
                    )
                })?;
                stack.push(Slot::Ref(r));
                continue;
            }
            self.step(ins, stack, false)?;
        }
        let at = stack
            .iter()
            .position(|s| matches!(s, Slot::Ref(_)))
            .ok_or_else(|| {
                diag::fatal(
                    code::rt::EXPR_MALFORMED,
                    Boundary::Script,
                    "左值窗口不含变量引用",
                )
            })?;
        let mut indices = smallvec::SmallVec::<[i64; 4]>::new();
        for slot in stack.iter().skip(at + 1) {
            match slot {
                Slot::Val(Value::Int(v)) => indices.push(*v),
                Slot::Val(other) => {
                    return Err(diag::fatal(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        format!("下标必须是整数,得到 {}", other.type_name()),
                    ))
                }
                Slot::Ref(r) => {
                    return Err(diag::fatal(
                        code::rt::EXPR_MALFORMED,
                        Boundary::Script,
                        format!("下标表混入变量引用 {}", r.display()),
                    ))
                }
            }
        }
        let var = match &stack[at] {
            Slot::Ref(r) => *r,
            Slot::Val(_) => unreachable!("at 由 Ref 位置确定"),
        };
        Ok(LValue { var, indices })
    }

    /// 预编译简单表达式的执行(IF/LET 热路径;语义与通用栈机一致)。
    pub fn eval_simple(&mut self, expr: &SimpleExpr) -> Result<Value, yu_log::Diag> {
        match *expr {
            SimpleExpr::Const(value) => Ok(Value::Int(value)),
            SimpleExpr::Var(var) => match self.access.read_scalar_int(var)? {
                Some(value) => Ok(Value::Int(value)),
                None => self.access.read(var, &[]),
            },
            SimpleExpr::VarOpConst { var, op, value } => {
                let lhs = match self.access.read_scalar_int(var)? {
                    Some(value) => Value::Int(value),
                    None => self.access.read(var, &[])?,
                };
                binary(op, &lhs, &Value::Int(value))
            }
            SimpleExpr::ConstOpVar { value, op, var } => {
                let rhs = match self.access.read_scalar_int(var)? {
                    Some(value) => Value::Int(value),
                    None => self.access.read(var, &[])?,
                };
                binary(op, &Value::Int(value), &rhs)
            }
            SimpleExpr::VarOpVar { lhs, op, rhs } => {
                let a = match self.access.read_scalar_int(lhs)? {
                    Some(value) => Value::Int(value),
                    None => self.access.read(lhs, &[])?,
                };
                let b = match self.access.read_scalar_int(rhs)? {
                    Some(value) => Value::Int(value),
                    None => self.access.read(rhs, &[])?,
                };
                binary(op, &a, &b)
            }
        }
    }

    /// 预编译左值计划的执行(与通用路径同语义,省掉槽栈与逐指令分派)。
    ///
    /// 先求值全部下标再统一校验整数,与 `eval_lvalue_instrs_with` 的
    /// 「表达式错误优先、下标类型错误在后」顺序一致。
    pub fn eval_lvalue_plan(&mut self, plan: &LValuePlan) -> Result<LValue, yu_log::Diag> {
        let mut values: smallvec::SmallVec<[Value; 4]> = smallvec::SmallVec::new();
        for expr in &plan.indices {
            values.push(match *expr {
                IndexExpr::Const(v) => Value::Int(v),
                IndexExpr::Var(var) => match self.access.read_scalar_int(var)? {
                    Some(value) => Value::Int(value),
                    None => self.access.read(var, &[])?,
                },
                IndexExpr::VarOpConst { var, op, value } => {
                    let lhs = match self.access.read_scalar_int(var)? {
                        Some(value) => Value::Int(value),
                        None => self.access.read(var, &[])?,
                    };
                    binary(op, &lhs, &Value::Int(value))?
                }
            });
        }
        let mut indices = smallvec::SmallVec::<[i64; 4]>::new();
        for value in values {
            match value {
                Value::Int(v) => indices.push(v),
                other => {
                    return Err(diag::fatal(
                        code::rt::VAR_TYPE,
                        Boundary::Script,
                        format!("下标必须是整数,得到 {}", other.type_name()),
                    ))
                }
            }
        }
        Ok(LValue {
            var: plan.var,
            indices,
        })
    }

    fn step(
        &mut self,
        ins: &RawInstr,
        stack: &mut Vec<Slot>,
        is_last: bool,
    ) -> Result<(), yu_log::Diag> {
        match ins.op {
            0x42 if ins.payload.len() == 1 => {
                stack.push(Slot::Val(Value::Int(ins.payload[0] as i8 as i64)))
            }
            0x57 if ins.payload.len() == 2 => stack.push(Slot::Val(Value::Int(
                i16::from_le_bytes([ins.payload[0], ins.payload[1]]) as i64,
            ))),
            0x49 if ins.payload.len() == 4 => {
                stack.push(Slot::Val(Value::Int(i32::from_le_bytes([
                    ins.payload[0],
                    ins.payload[1],
                    ins.payload[2],
                    ins.payload[3],
                ]) as i64)))
            }
            0x4c if ins.payload.len() == 8 => {
                stack.push(Slot::Val(Value::Int(read_i64(&ins.payload)?)))
            }
            0x46 if ins.payload.len() == 8 => {
                let v = f64::from_bits(read_i64(&ins.payload)? as u64);
                // 官方整数模式:0x4287e8 用 `or 0xC00 + fistp qword` 把 f64
                // 向零截断后压入 i64 槽(证据见 `int_flavor` 文档)。
                stack.push(Slot::Val(if self.int_flavor {
                    Value::Int(v as i64)
                } else {
                    Value::Float(v)
                }));
            }
            0x4d => stack.push(Slot::Val(Value::Str(decode_string_literal_with(
                &ins.payload,
                self.codec,
            )?))),
            0x48 | 0x56 | 0x76 => {
                let r = var_of(ins).ok_or_else(|| malformed(ins, "变量引用载荷不是 3 字节"))?;
                if ins.op == 0x48 {
                    let v = self.access.read(r, &[])?;
                    // 整数模式只认 i64 槽:FLT 变量取值向零截断(§3.9.3 的
                    // 整数路径由 `0x7b99c4` 锁定,不因值类型回退浮点)。
                    let v = if self.int_flavor {
                        match v {
                            Value::Float(f) => Value::Int(f as i64),
                            other => other,
                        }
                    } else {
                        v
                    };
                    stack.push(Slot::Val(v));
                } else {
                    stack.push(Slot::Ref(r));
                }
            }
            0x29 => {
                let at = stack
                    .iter()
                    .rposition(|s| matches!(s, Slot::Ref(_)))
                    .ok_or_else(|| {
                        diag::fatal(
                            code::rt::EXPR_MALFORMED,
                            Boundary::Script,
                            "数组取值没有变量引用(引擎 0x1a5ea 同族)",
                        )
                    })?;
                let var = match &stack[at] {
                    Slot::Ref(r) => *r,
                    Slot::Val(_) => unreachable!(),
                };
                let mut indices = smallvec::SmallVec::<[i64; 4]>::new();
                for slot in stack.iter().skip(at + 1) {
                    match slot {
                        Slot::Val(Value::Int(v)) => indices.push(*v),
                        // 整数模式下 FLT 下标向零截断(与整数路径同口径)。
                        Slot::Val(Value::Float(v)) if self.int_flavor => {
                            indices.push(*v as i64)
                        }
                        Slot::Val(other) => {
                            return Err(diag::fatal(
                                code::rt::VAR_TYPE,
                                Boundary::Script,
                                format!("数组下标必须是整数,得到 {}", other.type_name()),
                            ))
                        }
                        Slot::Ref(r) => {
                            return Err(diag::fatal(
                                code::rt::EXPR_MALFORMED,
                                Boundary::Script,
                                format!("数组下标混入引用 {}", r.display()),
                            ))
                        }
                    }
                }
                let v = self.access.read(var, &indices)?;
                stack.truncate(at);
                stack.push(Slot::Val(v));
            }
            0x2c => {}
            0x21 | 0x25 | 0x26 | 0x2a | 0x2b | 0x2d | 0x2f | 0x3c | 0x3d | 0x3e | 0x41 | 0x4f
            | 0x53 | 0x5a | 0x5e | 0x7c => {
                let rhs = match stack.pop() {
                    Some(Slot::Val(v)) => v,
                    Some(Slot::Ref(r)) => {
                        // 引用出现在运算数位:引擎在整数路径里会取值(§3.9.3)。
                        self.access.read(r, &[])?
                    }
                    None => {
                        // 编译器「实参缺席」形态:窗口 = [PushInt(0), <二元运算>]。
                        // 语料 20 例(19 例操作数齐全的掩码,1 例缺席);
                        // 见 doc 顶部 DIV-RT-004。
                        if is_last && stack.is_empty() {
                            return Ok(());
                        }
                        return Err(underflow(ins));
                    }
                };
                let lhs = match stack.pop() {
                    Some(Slot::Val(v)) => v,
                    Some(Slot::Ref(r)) => self.access.read(r, &[])?,
                    None => {
                        if is_last {
                            // 缺席形态:保留 0 作为该参数的值。
                            stack.push(Slot::Val(rhs));
                            return Ok(());
                        }
                        return Err(underflow(ins));
                    }
                };
                let v = if self.int_flavor {
                    binary_int(ins.op, &lhs, &rhs)?
                } else {
                    binary(ins.op, &lhs, &rhs)?
                };
                stack.push(Slot::Val(v));
            }
            0x52 => {
                let v = pop_val(stack, ins, self.access)?;
                // 官方一元 `-`(0x428858):整数路径 `sub/sbb` 自减,
                // 浮点路径 `fchs`;整数模式下 f64 先转 i64。
                let v = if self.int_flavor {
                    let n = int_of(&v).ok_or_else(|| {
                        diag::fatal(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!("一元 - 需要数值,得到 {}", v.type_name()),
                        )
                    })?;
                    Value::Int(n.wrapping_neg())
                } else {
                    v.neg()?
                };
                stack.push(Slot::Val(v));
            }
            0x69 => {
                let v = pop_val(stack, ins, self.access)?;
                stack.push(Slot::Val(v.cast_to_int()?));
            }
            0x73 => {
                let v = pop_val(stack, ins, self.access)?;
                stack.push(Slot::Val(v.cast_to_str()?));
            }
            other => {
                return Err(diag::unimplemented(
                    code::rt::EXPR_UNSUPPORTED,
                    Boundary::Script,
                    format!(
                        "表达式操作码 {other:#04x}(窗口偏移 {})未取证/未实现",
                        ins.offset
                    ),
                ))
            }
        }
        Ok(())
    }
}

fn underflow(ins: &RawInstr) -> yu_log::Diag {
    diag::fatal(
        code::rt::EXPR_MALFORMED,
        Boundary::Script,
        format!("操作数不足:op={:#04x} 偏移 {}", ins.op, ins.offset),
    )
}

fn pop_val(
    stack: &mut Vec<Slot>,
    ins: &RawInstr,
    access: &mut dyn VarAccess,
) -> Result<Value, yu_log::Diag> {
    match stack.pop() {
        Some(Slot::Val(v)) => Ok(v),
        Some(Slot::Ref(r)) => access.read(r, &[]),
        None => Err(underflow(ins)),
    }
}

/// 整数路径的操作数取值:INT 原样,FLT 向零截断(官方 `fistp` 截断)。
fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) => Some(*f as i64),
        Value::Str(_) => None,
    }
}

/// 官方整数路径的二元运算(`0x7b99c4` 奇数)。
///
/// 仅数值操作数走 i64;字符串仍交给 [`binary`](`+` 拼接/比较的已验证语义),
/// 避免把字符串表达式误送进整数算术。除零与 `%` 的 0 结果沿用
/// [`Value::div`]/[`Value::rem`] 的官方口径(0x4294ec/0x4295a0)。
fn binary_int(op: u8, lhs: &Value, rhs: &Value) -> Result<Value, yu_log::Diag> {
    let (Some(a), Some(b)) = (int_of(lhs), int_of(rhs)) else {
        return binary(op, lhs, rhs);
    };
    Ok(match op {
        0x2a => Value::Int(a.wrapping_mul(b)),
        0x2b => Value::Int(a.wrapping_add(b)),
        0x2d => Value::Int(a.wrapping_sub(b)),
        0x2f => Value::Int(if b == 0 { 0 } else { a.wrapping_div(b) }),
        0x25 => Value::Int(if b == 0 { 0 } else { a.wrapping_rem(b) }),
        0x41 => Value::Int(a & b),
        0x4f => Value::Int(a | b),
        0x5e => Value::Int(a ^ b),
        0x21 => Value::Int((a != b) as i64),
        0x3d => Value::Int((a == b) as i64),
        0x3c => Value::Int((a < b) as i64),
        0x3e => Value::Int((a > b) as i64),
        0x53 => Value::Int((a <= b) as i64),
        0x5a => Value::Int((a >= b) as i64),
        0x26 => Value::Int(((a != 0) && (b != 0)) as i64),
        0x7c => Value::Int(((a != 0) || (b != 0)) as i64),
        _ => unreachable!("binary_int() 调用点已限定 opcode"),
    })
}

fn binary(op: u8, lhs: &Value, rhs: &Value) -> Result<Value, yu_log::Diag> {
    match op {
        0x21 => Ok(Value::Int(lhs.ne(rhs)? as i64)),
        0x25 => lhs.rem(rhs),
        0x26 => lhs.logand(rhs),
        0x2a => lhs.mul(rhs),
        0x2b => lhs.add(rhs),
        0x2d => lhs.sub(rhs),
        0x2f => lhs.div(rhs),
        0x3c => Ok(Value::Int(lhs.lt(rhs)? as i64)),
        0x3d => Ok(Value::Int(lhs.eq(rhs)? as i64)),
        0x3e => Ok(Value::Int(lhs.gt(rhs)? as i64)),
        0x41 => lhs.bitand(rhs),
        0x4f => lhs.bitor(rhs),
        0x53 => Ok(Value::Int(lhs.le(rhs)? as i64)),
        0x5a => Ok(Value::Int(lhs.ge(rhs)? as i64)),
        0x5e => lhs.bitxor(rhs),
        0x7c => lhs.logor(rhs),
        _ => unreachable!("binary() 调用点已限定 opcode"),
    }
}

/// 切一条 `[op][len:u16][payload]` 记录,返回 `(下一条偏移, 指令)`。
///
/// 0x4d 字符串字面量的 `len` 只用于走查前进,内容由终止符界定
/// (旁线 §3.11)。部分 GBK 汉化 ybn 重写文本后保留了旧 CP932 串的
/// `len`,新串更短但**终止符仍在窗口内**;官方按指针扫到终止符就停,
/// 故这里同样放宽:载荷裁到窗口末尾,终止符必须在窗口内。其余操作码
/// 仍按越界报错 —— 缺的是窗口外数据,不能猜。
fn split_record(window: &[u8], p: usize) -> Result<(usize, RawInstr), yu_log::Diag> {
    if p + 3 > window.len() {
        return Err(diag::fatal(
            code::rt::EXPR_MALFORMED,
            Boundary::Script,
            format!("记录头截断:窗口偏移 {p},剩 {} 字节", window.len() - p),
        ));
    }
    let op = window[p];
    let len = u16::from_le_bytes([window[p + 1], window[p + 2]]) as usize;
    let end = p + 3 + len;
    if end > window.len() {
        let body = &window[p + 3..];
        let terminated = body
            .first()
            .is_some_and(|&term| body[1..].contains(&term));
        if op != 0x4d || !terminated {
            return Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                format!(
                    "操作数越界:op={op:#04x} 偏移 {p} 声明 {len} 字节,窗口共 {}",
                    window.len()
                ),
            ));
        }
        return Ok((
            end,
            RawInstr {
                offset: p,
                op,
                payload: body.to_vec(),
            },
        ));
    }
    Ok((
        end,
        RawInstr {
            offset: p,
            op,
            payload: window[p + 3..end].to_vec(),
        },
    ))
}

/// 切分表达式记录流。
/// `decode` 的复用外层缓冲版(逐条 payload 仍按 `RawInstr` 所有权拷贝;
/// 语义与 `decode` 一致)。
pub fn decode_pooled(window: &[u8], out: &mut Vec<RawInstr>) -> Result<(), yu_log::Diag> {
    out.clear();
    let mut p = 0usize;
    while p < window.len() {
        let (next, ins) = split_record(window, p)?;
        out.push(ins);
        p = next;
    }
    Ok(())
}

pub fn decode(window: &[u8]) -> Result<Vec<RawInstr>, yu_log::Diag> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < window.len() {
        let (next, ins) = split_record(window, p)?;
        out.push(ins);
        p = next;
    }
    Ok(out)
}

/// 窗口里是否有 `0x4d` 字符串记录**没有在自己的声明长度内闭合**。
///
/// 555 汉化/重打包数据会出现两种坏头:
/// - 记录自声明长度越过窗口(窗口 length 过期);
/// - 记录/窗口长度都覆盖不到结束引号(字符串记录被截短)。
///
/// 两者都可能让严格解码失败或让字面量在求值时“未闭合”。这里只做
/// 静态判定,不猜内容;命中后由 [`decode_self_delimited`] 按界定符重解。
pub fn window_literal_unclosed(window: &[u8], codec: yu_format::Codec) -> bool {
    let mut pos = 0usize;
    while pos + 3 <= window.len() {
        let op = window[pos];
        let declared = u16::from_le_bytes([window[pos + 1], window[pos + 2]]) as usize;
        let Some(end) = pos.checked_add(3 + declared) else {
            return true;
        };
        if op == 0x4d {
            // 字符串记录至少要有界定符;声明长度不覆盖界定符本身也按坏头处理。
            let Some(&term) = window.get(pos + 3) else {
                return true;
            };
            if end > window.len() || declared < 2 {
                return true;
            }
            if scan_literal_term(window, &[], pos + 4, term, end, codec).is_none() {
                return true;
            }
        } else if end > window.len() {
            return true;
        }
        pos = end;
    }
    false
}

/// 555 自定界的窗口解码:非字符串记录按声明长度走;`0x4d` 记录以
/// **界定符**为边界,声明长度不闭合时继续读到 `window + tail` 里的
/// 结束引号(上限由调用方用“下一个窗口/池尾”给)。
///
/// 返回 `(记录, 窗口实际占用长度)`;占用长度可能大于 `window.len()`
/// (字符串补读)。无法安全闭合、或非字符串记录越过窗口时返回 `None`,
/// 调用方保留严格路径的错误。
pub fn decode_self_delimited(
    window: &[u8],
    tail: &[u8],
    codec: yu_format::Codec,
) -> Option<(Vec<RawInstr>, usize)> {
    let limit = window.len() + tail.len();
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut consumed = window.len();
    while pos < window.len() {
        if pos + 3 > window.len() {
            return None;
        }
        let op = window[pos];
        let declared = u16::from_le_bytes([window[pos + 1], window[pos + 2]]) as usize;
        if op == 0x4d {
            let term = *window.get(pos + 3)?;
            let Some(declared_end) = pos.checked_add(3 + declared) else {
                return None;
            };
            // 声明长度内已闭合:与严格切片逐字节一致(记录里的尾随字节
            // 保持原样,字面量解码器本来就会在第一个界定符处停止)。
            if declared_end <= window.len()
                && scan_literal_term(window, tail, pos + 4, term, declared_end, codec).is_some()
            {
                out.push(RawInstr {
                    offset: pos,
                    op,
                    payload: window.get(pos + 3..declared_end)?.to_vec(),
                });
                pos = declared_end;
                continue;
            }
            // 声明长度内不闭合:继续向 tail 读到真正的界定符。
            let close = scan_literal_term(window, tail, pos + 4, term, limit, codec)?;
            let end = close + 1;
            let mut payload = Vec::with_capacity(end - pos - 3);
            for index in pos + 3..end {
                payload.push(if index < window.len() {
                    window[index]
                } else {
                    *tail.get(index - window.len())?
                });
            }
            out.push(RawInstr {
                offset: pos,
                op,
                payload,
            });
            pos = end;
            consumed = end.max(consumed);
            continue;
        }
        let Some(end) = pos.checked_add(3 + declared) else {
            return None;
        };
        if end > window.len() {
            return None;
        }
        out.push(RawInstr {
            offset: pos,
            op,
            payload: window[pos + 3..end].to_vec(),
        });
        pos = end;
    }
    Some((out, consumed))
}

/// 在 `window + tail` 的视图里从 `start` 扫到界定符 `term`。
///
/// 与字面量解码器同一套规则(此处只关心界定符落点,不解内容):
/// 1. 界定符闭合优先;
/// 2. `\` + 界定符:收尾位(`limit - 1`)是官方字面量的真收尾,按单字节
///    让界定符命中;段内位置是翻译重写器转义的引号,成对跳过;
/// 3. lead 字节仅在下一字节是当前编码的合法 trail 时成对跳过
///    (`Codec::forms_two_byte`;三种编码的合法 trail 都不含 `0x22`,
///    不会把界定符吞进「双字节」)。
fn scan_literal_term(
    window: &[u8],
    tail: &[u8],
    start: usize,
    term: u8,
    limit: usize,
    codec: yu_format::Codec,
) -> Option<usize> {
    let mut index = start;
    while index < limit {
        let byte = if index < window.len() {
            window[index]
        } else {
            *tail.get(index - window.len())?
        };
        if byte == term {
            return Some(index);
        }
        if byte == b'\\' {
            // 与字面量解码器同一条位置规则:`\` + 界定符在收尾位(limit-1)
            // 是官方字面量的真收尾(`<v555-kuro>` 字体脚本),按单字节让
            // 界定符闭合;在段内则是翻译重写器转义的引号,成对跳过。
            let next = if index + 1 < window.len() {
                Some(window[index + 1])
            } else {
                tail.get(index + 1 - window.len()).copied()
            };
            if next == Some(term) && index + 1 + 1 != limit {
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }
        if codec.is_lead(byte) {
            let next = if index + 1 < window.len() {
                Some(window[index + 1])
            } else {
                tail.get(index + 1 - window.len()).copied()
            };
            if next.is_some_and(|trail| codec.is_trail(trail)) {
                index += 2;
                continue;
            }
        }
        index += 1;
    }
    None
}

fn malformed(ins: &RawInstr, what: &str) -> yu_log::Diag {
    diag::fatal(
        code::rt::EXPR_MALFORMED,
        Boundary::Script,
        format!("{what}(op={:#04x} 偏移 {})", ins.op, ins.offset),
    )
}

fn read_i64(payload: &[u8]) -> Result<i64, yu_log::Diag> {
    let b: [u8; 8] = payload
        .get(..8)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| {
            diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                format!("8 字节立即数载荷长度不符:{}", payload.len()),
            )
        })?;
    Ok(i64::from_le_bytes(b))
}

/// 单条变量引用窗口(`0x48` 取值 / `0x56`/`0x76` 引用)。
///
/// 只认这三种 opcode:3 字节字面量不得被误读成变量载荷。
pub(crate) fn single_var(instrs: &[RawInstr]) -> Option<VarRef> {
    let [ins] = instrs else { return None };
    if !matches!(ins.op, 0x48 | 0x56 | 0x76) {
        return None;
    }
    var_of(ins)
}

/// 单条**取值**变量窗口(`0x48`)。与 [`single_var`] 分开:引用窗口
/// (`0x56`/`0x76`)在参数记录时按旧行为求值失败、不产生 arg。
pub(crate) fn single_value_var(instrs: &[RawInstr]) -> Option<VarRef> {
    let [ins] = instrs else { return None };
    if ins.op != 0x48 {
        return None;
    }
    var_of(ins)
}

fn var_of(ins: &RawInstr) -> Option<VarRef> {
    if ins.payload.len() != 3 {
        return None;
    }
    Some(VarRef {
        space: VarSpace::from_prefix(ins.payload[0]),
        id: u16::from_le_bytes([ins.payload[1], ins.payload[2]]),
    })
}

/// 该条指令是否**必定**能被 [`Evaluator::step`] 执行成功(不判定运行时
/// 语义错误,如未声明变量/下标越界)。
///
/// `false` = 执行到这条指令必然返回 `Err`(未实现 opcode,或已知 opcode 的
/// 载荷长度不符);用于 `fill_cmd` 给窗口打「求值必失败」静态标记。
/// **保守方向**:拿不准时返回 `true`(最多错过优化,不会改变可观察行为)。
pub(crate) fn instr_supported(ins: &RawInstr) -> bool {
    match ins.op {
        0x42 => ins.payload.len() == 1,
        0x57 => ins.payload.len() == 2,
        0x49 => ins.payload.len() == 4,
        0x4c | 0x46 => ins.payload.len() == 8,
        0x4d => decode_string_literal(&ins.payload).is_ok(),
        0x48 | 0x56 | 0x76 => ins.payload.len() == 3,
        _ => matches!(
            ins.op,
            0x29 | 0x2c
                | 0x21
                | 0x25
                | 0x26
                | 0x2a
                | 0x2b
                | 0x2d
                | 0x2f
                | 0x3c
                | 0x3d
                | 0x3e
                | 0x41
                | 0x4f
                | 0x53
                | 0x5a
                | 0x5e
                | 0x7c
                | 0x52
                | 0x69
                | 0x73
        ),
    }
}

/// 单条字面量窗口的常量值(0x42/0x57/0x49/0x4c/0x46/0x4d)。
///
/// VM 在 `fill_cmd` 时用它做常量折叠;只识别**单条**字面量指令,
/// 其它窗口返回 `None` 走正常求值。语义与 `eval_instrs` 的这些分支一致。
pub(crate) fn const_value_instrs(instrs: &[RawInstr]) -> Option<Value> {
    let [ins] = instrs else { return None };
    let bytes = &ins.payload;
    match ins.op {
        0x42 if bytes.len() == 1 => Some(Value::Int(i64::from(bytes[0] as i8))),
        0x57 if bytes.len() == 2 => Some(Value::Int(i64::from(i16::from_le_bytes([
            bytes[0], bytes[1],
        ])))),
        0x49 if bytes.len() == 4 => Some(Value::Int(i64::from(i32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])))),
        0x4c if bytes.len() == 8 => Some(Value::Int(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))),
        0x46 if bytes.len() == 8 => Some(Value::Float(f64::from_bits(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])))),
        0x4d => decode_string_literal(bytes).ok().map(Value::Str),
        _ => None,
    }
}

/// 字符串字面量解码(引擎 opcode 0x4d,§3.11)。
///
/// 载荷形态 `[终止符][内容…][终止符]`;`\` 转义:`\\`、`\n`(0x0A)、`\t`(0x09);
/// CP932 前导字节吞掉后继字节(不让终止符落在双字节字符中间)。
pub fn decode_string_literal(payload: &[u8]) -> Result<Vec<u8>, yu_log::Diag> {
    decode_string_literal_with(payload, yu_format::Codec::Cp932)
}

/// 按指定 codec 解码字符串字面量。
pub fn decode_string_literal_with(
    payload: &[u8],
    codec: yu_format::Codec,
) -> Result<Vec<u8>, yu_log::Diag> {
    let (Some(&term), true) = (payload.first(), payload.len() >= 2) else {
        return Err(diag::fatal(
            code::rt::EXPR_MALFORMED,
            Boundary::Script,
            "字符串字面量缺少界定符",
        ));
    };
    let body = &payload[1..];
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let b = body[i];
        // 界定符检查先于转义。`\` + 界定符有两种产源,按位置区分:
        // - 官方字面量的收尾引号(界定符 = payload 末字节;`<v555-kuro>`
        //   `es_fontcg.yst@358` 的比较字面量 `22 5c 22`,内容 = `\`)→
        //   `\` 保持字面,界定符闭合;
        // - 翻译重写器(`yu-format::literal::escape`)转义的段内引号
        //   `\"`(后面必有真正的收尾引号)→ 解成 `"`。
        // `\n`/`\t` 等常规转义不受影响(`ysvr` 字符串初值 `a\nb` → a LF b)。
        if b == term {
            return Ok(out);
        }
        if b == b'\\' {
            match body.get(i + 1) {
                None => {
                    return Err(diag::fatal(
                        code::rt::EXPR_MALFORMED,
                        Boundary::Script,
                        "字符串字面量以反斜杠结尾",
                    ))
                }
                Some(x) if *x == term => {
                    if i + 1 == body.len() - 1 {
                        // 收尾:界定符优先,`\` 字面;下一轮命中界定符闭合。
                        out.push(b);
                        i += 1;
                    } else {
                        // 段内:重写器转义引号,解回 `"`,连同 `\` 一起消费。
                        out.push(*x);
                        i += 2;
                    }
                }
                Some(b'\\') => {
                    out.push(b'\\');
                    i += 2;
                }
                Some(b'n') => {
                    out.push(0x0A);
                    i += 2;
                }
                Some(b't') => {
                    out.push(0x09);
                    i += 2;
                }
                Some(other) => {
                    out.push(*other);
                    i += 2;
                }
            }
            continue;
        }
        if codec.is_lead(b) {
            // 成对消费要求尾字节合法:trail 越界/是界定符(0x22 在三种编码
            // 里都不是合法 trail)时按单字节,保持界定符可闭合。
            if let Some(&next) = body.get(i + 1) {
                if codec.is_trail(next) {
                    out.push(b);
                    out.push(next);
                    i += 2;
                    continue;
                }
            }
        }
        out.push(b);
        i += 1;
    }
    Err(diag::fatal(
        code::rt::EXPR_MALFORMED,
        Boundary::Script,
        "字符串字面量未闭合",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed {
        scalar: Value,
        array: Vec<Value>,
    }

    impl VarAccess for Fixed {
        fn read(&mut self, var: VarRef, idx: &[i64]) -> Result<Value, yu_log::Diag> {
            if idx.is_empty() {
                let _ = var;
                Ok(self.scalar.clone())
            } else {
                Ok(self
                    .array
                    .get(idx[0] as usize)
                    .cloned()
                    .unwrap_or(Value::Int(0)))
            }
        }
    }

    fn ev(window: &[u8]) -> Result<Value, yu_log::Diag> {
        let mut acc = Fixed {
            scalar: Value::Int(5),
            array: vec![Value::Int(10), Value::Int(20)],
        };
        Evaluator::new(&mut acc).eval_value(window)
    }

    #[test]
    fn arithmetic_matches_manual_examples() {
        // @A=5; @A*2+1
        let mut w = vec![0x48, 3, 0, 0x40, 0x10, 0x00];
        w.extend_from_slice(&[0x42, 1, 0, 2, 0x2a, 0, 0, 0x42, 1, 0, 1, 0x2b, 0, 0]);
        assert_eq!(ev(&w).unwrap(), Value::Int(11));
    }

    /// 官方整数路径:FLT 常量向零截断后按 i64 运算。
    ///
    /// `8*2/16.6667` 官方 = `16/16` = 1;浮点系 = 1.0? 否,16/16.6667
    /// ≈ 0.96。样本 `es_select.yst:544` 的计数正是这一形态。
    #[test]
    fn int_flavor_truncates_float_literals_and_uses_i64_division() {
        let mut w = vec![0x42, 1, 0, 8, 0x42, 1, 0, 2, 0x2a, 0, 0];
        w.extend_from_slice(&[0x46, 8, 0]);
        w.extend_from_slice(&16.666666666666668f64.to_bits().to_le_bytes());
        w.push(0x2f);
        w.extend_from_slice(&[0, 0]);

        let mut acc = Fixed {
            scalar: Value::Int(8),
            array: Vec::new(),
        };
        let int = Evaluator::new(&mut acc)
            .with_int_flavor()
            .eval_value(&w)
            .expect("int flavor");
        assert_eq!(int, Value::Int(1), "16 / trunc(16.6667) = 1");

        let mut acc = Fixed {
            scalar: Value::Int(8),
            array: Vec::new(),
        };
        let float = Evaluator::new(&mut acc).eval_value(&w).expect("auto");
        assert!(
            matches!(float, Value::Float(v) if (v - 16.0 / 16.666666666666668).abs() < 1e-12),
            "对照:浮点系是 0.96,不是 1;得到 {float:?}"
        );
    }

    #[test]
    fn string_concat_and_compare() {
        // "ABC" + ".BMP"
        let mut w = vec![0x4d, 5, 0, b'"', b'A', b'B', b'C', b'"'];
        w.extend_from_slice(&[0x4d, 6, 0, b'"', b'.', b'B', b'M', b'P', b'"', 0x2b, 0, 0]);
        assert_eq!(ev(&w).unwrap(), Value::Str(b"ABC.BMP".to_vec()));
    }

    #[test]
    fn array_load_uses_reference_and_index() {
        // 56 @id ; 42 01 ; 29
        let w = [0x56, 3, 0, 0x40, 0x10, 0x00, 0x42, 1, 0, 1, 0x29, 1, 0, 0];
        assert_eq!(ev(&w).unwrap(), Value::Int(20));
    }

    #[test]
    fn literal_widths() {
        assert_eq!(ev(&[0x42, 1, 0, 0xff]).unwrap(), Value::Int(-1));
        assert_eq!(ev(&[0x57, 2, 0, 0xff, 0xff]).unwrap(), Value::Int(-1));
        assert_eq!(
            ev(&[0x49, 4, 0, 0x00, 0x00, 0x00, 0x80]).unwrap(),
            Value::Int(-2147483648)
        );
    }

    #[test]
    fn malformed_record_is_error() {
        assert!(ev(&[0x42, 5, 0, 1]).is_err());
    }

    #[test]
    fn lvalue_plan_matches_hot_array_assignment() {
        // 真实热站点(es_Button 的 `@P.I(@mLC-1,@_LC)=0`)窗口字节:
        // 引用基变量 → 读 @mLC → 1 → sub → nop → 读 @_LC → 数组标记。
        let raw = [
            0x56, 3, 0, 0x40, 0x0f, 0x05, 0x48, 3, 0, 0x40, 0x85, 0x05, 0x42, 1, 0, 1, 0x2d,
            0, 0, 0x2c, 0, 0, 0x48, 3, 0, 0x40, 0x30, 0, 0x29, 1, 0, 0,
        ];
        let instrs = decode(&raw).expect("decode");
        let plan = lvalue_plan(&instrs).expect("plan");
        assert_eq!(
            plan.indices,
            vec![
                IndexExpr::VarOpConst {
                    var: VarRef {
                        space: VarSpace::from_prefix(0x40),
                        id: 0x0585,
                    },
                    op: 0x2d,
                    value: 1,
                },
                IndexExpr::Var(VarRef {
                    space: VarSpace::from_prefix(0x40),
                    id: 0x0030,
                }),
            ]
        );

        // 执行结果与通用 `0x29` 左值路径一致。
        let fixed = Fixed {
            scalar: Value::Int(5),
            array: Vec::new(),
        };
        let mut access = fixed;
        let mut eval = Evaluator::new(&mut access);
        let lv = eval.eval_lvalue_plan(&plan).expect("eval plan");
        let mut generic = Vec::new();
        let lv_generic = eval
            .eval_lvalue_instrs_with(&instrs, &mut generic)
            .expect("eval generic");
        assert_eq!(lv, lv_generic);
        assert_eq!(lv.indices.as_slice(), &[4, 5]);
    }

    #[test]
    fn lvalue_plan_falls_back_on_reference_index() {
        // 下标位置出现 0x56/0x76(引用)时通用路径会报「下标表混入变量引用」;
        // 快速路径必须回退,不能把它当值读。
        let raw = [
            0x56, 3, 0, 0x40, 0x10, 0x00, 0x56, 3, 0, 0x40, 0x11, 0x00, 0x29, 1, 0, 0,
        ];
        let instrs = decode(&raw).expect("decode");
        assert!(lvalue_plan(&instrs).is_none());
    }

    #[test]
    fn string_literal_tolerates_stale_operand_len() {
        // 汉化 ybn 常见形态:0x4d 的 u16 仍是旧 CP932 串长度,新 GBK 串
        // 更短,但终止符在窗口内。走查按声明长度前进(越过窗口即结束),
        // 内容按终止符取。
        let raw = [0x4d, 0x0c, 0x00, 0x22, b'a', 0x22];
        let instrs = decode(&raw).expect("stale len accepted");
        assert_eq!(instrs.len(), 1);
        assert_eq!(instrs[0].op, 0x4d);
        assert_eq!(instrs[0].payload, b"\"a\"");
        assert_eq!(
            decode_string_literal(&instrs[0].payload).unwrap(),
            b"a".to_vec()
        );
    }

    #[test]
    fn string_literal_still_requires_terminator_in_window() {
        let raw = [0x4d, 0x0c, 0x00, 0x22, b'a'];
        assert!(decode(&raw).is_err());
    }

    /// Big5 双字节 trail 可落在 0x40..=0x7E(ASCII 区);字面量扫描必须按
    /// 当前 codec 的 lead 表跳 2 字节,不能把 trail 0x40 当普通字符。
    /// 证据:样本 ybn 的「【護士】」= A1 69 C5 40 A4 68 A1 6A,只有 Big5
    /// 能正确解出(CP932/GBK 均失败或乱码)。
    #[test]
    fn string_literal_scans_lead_bytes_with_active_codec() {
        let raw = [
            0x22, 0xa1, 0x69, 0xc5, 0x40, 0xa4, 0x68, 0xa1, 0x6a, 0x22,
        ];
        let out =
            decode_string_literal_with(&raw, yu_format::Codec::Big5).expect("big5 literal");
        assert_eq!(out, raw[1..9].to_vec());
    }

    /// v555 黑兽2改实测(2026-11-24):exe 静态 lead 表 = Big5,系统脚本
    /// (CP932)字面量里的 SJIS 半角假名 `0xA1..=0xDF` 在 Big5 下是 lead,
    /// 紧跟闭合引号时旧实现盲跳 2 字节把界定符吞掉 → 「字面量未闭合」
    /// Fatal(`es_fontcg.yst:es.IDSubTask2@358 IF`)。三种编码的合法
    /// trail 都不含 0x22,lead 后跟界定符必须按单字节处理。
    #[test]
    fn lead_followed_by_term_is_single_byte_not_a_pair() {
        let raw = [0x22, 0xde, 0x22];
        let out = decode_string_literal_with(&raw, yu_format::Codec::Big5)
            .expect("界定符必须闭合");
        assert_eq!(out, vec![0xde]);

        // 成对要求不破坏合法双字节:Big5 `0xC5 0x40`(trail 0x40 合法)。
        let raw = [0x22, 0xc5, 0x40, 0x22];
        let out =
            decode_string_literal_with(&raw, yu_format::Codec::Big5).expect("valid pair");
        assert_eq!(out, vec![0xc5, 0x40]);
    }

    #[test]
    fn scan_literal_term_stops_at_quote_after_invalid_trail() {
        let window = [0x22u8, 0xde, 0x22];
        let found = scan_literal_term(&window, &[], 1, 0x22, window.len(), yu_format::Codec::Big5);
        assert_eq!(found, Some(2));
        // 合法 trail 仍成对跳过:界定符在 trail 之后才命中。
        let window = [0x22u8, 0xc5, 0x40, 0x22];
        let found = scan_literal_term(&window, &[], 1, 0x22, window.len(), yu_format::Codec::Big5);
        assert_eq!(found, Some(3));
        // 段内转义引号(重写器产物)不提前闭合:真收尾在最后。
        let window = [0x22u8, 0x5c, 0x22, 0x5c, 0x22, 0x22];
        let found = scan_literal_term(&window, &[], 1, 0x22, window.len(), yu_format::Codec::Big5);
        assert_eq!(found, Some(5));
    }

    /// `<v555-kuro>` `es_fontcg.yst@358` 实测字节(2026-11-24):比较字面量
    /// `22 5c 22` 的内容是单个 `\`(路径分隔符判定表)。收尾位的 `\` +
    /// 界定符:界定符优先,`\` 字面;常规转义 `\n` 不受影响(`ysvr` 初值
    /// `a\nb` → a LF b)。段内 `\"` 是翻译重写器的转义引号,解回 `"`。
    #[test]
    fn terminator_wins_over_escape_backslash() {
        let raw = [0x22u8, 0x5c, 0x22];
        let out =
            decode_string_literal_with(&raw, yu_format::Codec::Big5).expect("backslash literal");
        assert_eq!(out, vec![0x5c]);
        let out =
            decode_string_literal_with(&raw, yu_format::Codec::Cp932).expect("cp932 too");
        assert_eq!(out, vec![0x5c]);

        // 段内转义引号(重写器产物):`" \" "` → 内容 = 一个 `"`;
        // `" \" \" "` → 内容 = 两个 `"`。
        let raw = [0x22u8, 0x5c, 0x22, 0x22];
        let out =
            decode_string_literal_with(&raw, yu_format::Codec::Big5).expect("escaped quote");
        assert_eq!(out, vec![0x22]);
        let raw = [0x22u8, 0x5c, 0x22, 0x5c, 0x22, 0x22];
        let out =
            decode_string_literal_with(&raw, yu_format::Codec::Big5).expect("two escaped quotes");
        assert_eq!(out, vec![0x22, 0x22]);

        let raw = b"\"a\\nb\"";
        let out =
            decode_string_literal_with(raw, yu_format::Codec::Cp932).expect("newline escape");
        assert_eq!(out, vec![b'a', 0x0a, b'b']);
    }

    #[test]
    fn simple_expr_matches_generic_for_var_compare() {
        // @A < 5
        let raw = [0x48, 3, 0, 0x40, 0x10, 0x00, 0x42, 1, 0, 5, 0x3c, 0, 0];
        let instrs = decode(&raw).expect("decode");
        let plan = simple_expr(&instrs).expect("plan");
        let mut fixed = Fixed {
            scalar: Value::Int(3),
            array: Vec::new(),
        };
        let mut eval = Evaluator::new(&mut fixed);
        let simplified = eval.eval_simple(&plan).expect("simple");
        let generic = eval
            .eval_value_instrs(&instrs, &mut Vec::new())
            .expect("generic");
        assert_eq!(simplified, generic);
        assert_eq!(simplified, Value::Int(1));
    }
}

#[cfg(test)]
mod self_delimited_tests {
    use super::*;

    /// <v555-sample> `s273 pc13`:`4d 0c 00` 声明 12 字节负载,窗口只有
    /// 11 字节;补读 4 字节(尾部路线)到结束引号后整条 15 字节。
    #[test]
    fn record_length_short_of_window_is_completed_from_tail() {
        let window = b"\x4d\x0c\x00\x22\xd0\xa1\xca\xb1\xba\xf2\xb5";
        let tail = b"\xc9\xd4\xad\x22";
        assert!(window_literal_unclosed(window, yu_format::Codec::Gbk));
        let (records, consumed) = decode_self_delimited(window, tail, yu_format::Codec::Gbk)
            .expect("self-delimited decode");
        assert_eq!(records.len(), 1);
        assert_eq!(consumed, 15);
        assert_eq!(
            records[0].payload,
            b"\x22\xd0\xa1\xca\xb1\xba\xf2\xb5\xc9\xd4\xad\x22"
        );
    }

    /// `s273 pc216`:`4d 04 00` 声明 4 字节负载,窗口 7 字节,但 GBK
    /// 字面量在窗口末尾仍是悬空前导;补读 2 字节后闭合。
    #[test]
    fn literal_dangling_lead_is_completed_from_tail() {
        let window = b"\x4d\x04\x00\x22\x99\xe1\x91";
        let tail = b"\x97\x22";
        assert!(window_literal_unclosed(window, yu_format::Codec::Gbk));
        let (records, consumed) = decode_self_delimited(window, tail, yu_format::Codec::Gbk)
            .expect("self-delimited decode");
        assert_eq!(records.len(), 1);
        assert_eq!(consumed, 9);
        assert_eq!(records[0].payload, b"\x22\x99\xe1\x91\x97\x22");
    }

    /// 结构正常的窗口:自定界结果与严格切片逐字节一致(不改变旧路径)。
    #[test]
    fn well_formed_window_matches_strict_decode() {
        let window = b"\x42\x01\x00\x01\x4d\x04\x00\x22\xd1\xe0\x22";
        assert!(!window_literal_unclosed(window, yu_format::Codec::Gbk));
        let (records, consumed) = decode_self_delimited(window, b"", yu_format::Codec::Gbk)
            .expect("self-delimited decode");
        assert_eq!(consumed, window.len());
        let strict = decode(window).expect("strict decode");
        assert_eq!(records.len(), strict.len());
        for (a, b) in records.iter().zip(&strict) {
            assert_eq!((a.op, &a.payload), (b.op, &b.payload));
        }
    }

    /// 尾部也闭合不了:返回 `None`,调用方保留严格错误(不猜内容)。
    #[test]
    fn unclosed_without_tail_is_none() {
        let window = b"\x4d\x04\x00\x22\x99\xe1\x91";
        assert!(decode_self_delimited(window, b"\x97", yu_format::Codec::Gbk).is_none());
    }
}
