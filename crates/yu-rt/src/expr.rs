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
    pub payload: Vec<u8>,
}

/// 栈槽:值或变量引用(引擎的平行引用栈模型,§3.9.1)。
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Val(Value),
    Ref(VarRef),
}

/// 左值:变量 + 下标表(空 = 标量)。
#[derive(Clone, Debug, PartialEq)]
pub struct LValue {
    pub var: VarRef,
    pub indices: Vec<i64>,
}

/// 求值期变量访问。由 VM 实现(变量域 + 系统变量 + 输入快照)。
pub trait VarAccess {
    fn read(&mut self, var: VarRef, indices: &[i64]) -> Result<Value, yu_log::Diag>;
}

/// 表达式求值器。
pub struct Evaluator<'a> {
    access: &'a mut dyn VarAccess,
}

impl<'a> Evaluator<'a> {
    pub fn new(access: &'a mut dyn VarAccess) -> Self {
        Self { access }
    }

    /// 执行整个窗口,返回剩余栈。
    pub fn eval_pooled(
        &mut self,
        window: &[u8],
        stack: &mut Vec<Slot>,
        instrs: &mut Vec<RawInstr>,
    ) -> Result<(), yu_log::Diag> {
        stack.clear();
        decode_pooled(window, instrs)?;
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
        self.eval_pooled(window, stack, instrs)?;
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
                    indices: Vec::new(),
                });
            }
        }
        // 末条 `0x29` 是左值标记:剥掉它,其余照常执行。
        let body: &[RawInstr] = match instrs.last() {
            Some(last) if last.op == 0x29 => &instrs[..instrs.len() - 1],
            _ => &instrs[..],
        };
        let mut stack: Vec<Slot> = Vec::new();
        for ins in body {
            if matches!(ins.op, 0x56 | 0x76) {
                // 引用指令原样压栈(0x48 取值语义交给 step)。
                let r = var_of(ins)
                    .ok_or_else(|| diag::fatal(
                        code::rt::EXPR_MALFORMED,
                        Boundary::Script,
                        "变量引用载荷不是 3 字节",
                    ))?;
                stack.push(Slot::Ref(r));
                continue;
            }
            self.step(ins, &mut stack, false)?;
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
        let mut indices = Vec::new();
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
            0x49 if ins.payload.len() == 4 => stack.push(Slot::Val(Value::Int(
                i32::from_le_bytes([ins.payload[0], ins.payload[1], ins.payload[2], ins.payload[3]])
                    as i64,
            ))),
            0x4c if ins.payload.len() == 8 => stack.push(Slot::Val(Value::Int(
                read_i64(&ins.payload)?,
            ))),
            0x46 if ins.payload.len() == 8 => stack.push(Slot::Val(Value::Float(
                f64::from_bits(read_i64(&ins.payload)? as u64),
            ))),
            0x4d => stack.push(Slot::Val(Value::Str(decode_string_literal(
                &ins.payload,
            )?))),
            0x48 | 0x56 | 0x76 => {
                let r = var_of(ins).ok_or_else(|| malformed(ins, "变量引用载荷不是 3 字节"))?;
                if ins.op == 0x48 {
                    let v = self.access.read(r, &[])?;
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
                let mut indices = Vec::new();
                for slot in stack.iter().skip(at + 1) {
                    match slot {
                        Slot::Val(Value::Int(v)) => indices.push(*v),
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
            0x21 | 0x25 | 0x26 | 0x2a | 0x2b | 0x2d | 0x2f | 0x3c | 0x3d | 0x3e | 0x41
            | 0x4f | 0x53 | 0x5a | 0x5e | 0x7c => {
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
                let v = binary(ins.op, &lhs, &rhs)?;
                stack.push(Slot::Val(v));
            }
            0x52 => {
                let v = pop_val(stack, ins, self.access)?;
                stack.push(Slot::Val(v.neg()?));
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

/// 切分表达式记录流。
/// `decode` 的复用外层缓冲版(逐条 payload 仍按 `RawInstr` 所有权拷贝;
/// 语义与 `decode` 一致)。
pub fn decode_pooled(window: &[u8], out: &mut Vec<RawInstr>) -> Result<(), yu_log::Diag> {
    out.clear();
    let mut p = 0usize;
    while p < window.len() {
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
            return Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                format!(
                    "操作数越界:op={op:#04x} 偏移 {p} 声明 {len} 字节,窗口共 {}",
                    window.len()
                ),
            ));
        }
        out.push(RawInstr {
            offset: p,
            op,
            payload: window[p + 3..end].to_vec(),
        });
        p = end;
    }
    Ok(())
}

pub fn decode(window: &[u8]) -> Result<Vec<RawInstr>, yu_log::Diag> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < window.len() {
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
            return Err(diag::fatal(
                code::rt::EXPR_MALFORMED,
                Boundary::Script,
                format!(
                    "操作数越界:op={op:#04x} 偏移 {p} 声明 {len} 字节,窗口共 {}",
                    window.len()
                ),
            ));
        }
        out.push(RawInstr {
            offset: p,
            op,
            payload: window[p + 3..end].to_vec(),
        });
        p = end;
    }
    Ok(out)
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

fn var_of(ins: &RawInstr) -> Option<VarRef> {
    if ins.payload.len() != 3 {
        return None;
    }
    Some(VarRef {
        space: VarSpace::from_prefix(ins.payload[0]),
        id: u16::from_le_bytes([ins.payload[1], ins.payload[2]]),
    })
}

/// 字符串字面量解码(引擎 opcode 0x4d,§3.11)。
///
/// 载荷形态 `[终止符][内容…][终止符]`;`\` 转义:`\\`、`\n`(0x0A)、`\t`(0x09);
/// CP932 前导字节吞掉后继字节(不让终止符落在双字节字符中间)。
pub fn decode_string_literal(payload: &[u8]) -> Result<Vec<u8>, yu_log::Diag> {
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
        if b == term {
            return Ok(out);
        }
        if b == b'\\' {
            i += 1;
            match body.get(i) {
                Some(b'\\') => out.push(b'\\'),
                Some(b'n') => out.push(0x0A),
                Some(b't') => out.push(0x09),
                Some(other) => out.push(*other),
                None => {
                    return Err(diag::fatal(
                        code::rt::EXPR_MALFORMED,
                        Boundary::Script,
                        "字符串字面量以反斜杠结尾",
                    ))
                }
            }
            i += 1;
            continue;
        }
        if is_cp932_lead(b) {
            if let Some(&next) = body.get(i + 1) {
                out.push(b);
                out.push(next);
                i += 2;
                continue;
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

fn is_cp932_lead(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)
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
                Ok(self.array.get(idx[0] as usize).cloned().unwrap_or(Value::Int(0)))
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
}
