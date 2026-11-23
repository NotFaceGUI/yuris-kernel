//! 独立运行面的 `TEXT[FILE, LET, CLEAR]` 处理体。
//!
//! 与共享旧版的 `TEXT`(文本属性设置)同名不同义;本文件只在独立运行面
//! 安装后由 `other::handlers::dispatch` 按 `SemanticProfile::text`
//! (`TextMode::FileArray`)分派,共享 handler 不感知本语义。

use crate::code;
use crate::diag;
use crate::event::VmEvent;
use crate::expr::LValue;
use crate::task::StepOutcome;
use crate::value::{ElemType, Value};
use crate::vm::{Cmd, ExecState, Vm, VmError};
use yu_log::Boundary;

impl Vm {
    /// `TEXT[FILE, LET, CLEAR]`(v555 形态;处理体 <v555-4> `0x452a0c`)。
    ///
    /// 与 0.495 的 TEXT 是**同名不同义**:555 的 TEXT 负责把整份场景文本
    /// 文件读进一个字符串数组,交给独立剧本 DSL 解释器(`s56–s76`)逐行
    /// 解析。语义(REA + 语料实证,详见 `docs/formats/v555-commercial.md`
    /// §7.3/§13):
    ///
    /// 1. `FILE`:字符串 → 按当前 codec 前导表跳双字节的 ASCII 小写化 →
    ///    同样跳双字节的 `/`→`\` → 按官方文件源顺序打开(**不补扩展名**,
    ///    FILE 必须是完整资源名);打不开时官方返回 -1(本内核 `Yield`,
    ///    pc 前进,由调用方 `sc_cn_`/`sc_tw_` 回退)。官方
    ///    `FUN_00464dac` 只在 `DAT_0059b0c0[b] != 1` 时把 `A..Z` 加 0x20,
    ///    前导字节直接前进 2 —— 逐字节 `make_ascii_lowercase` 会改写
    ///    Big5/GBK 尾字节(尾字节落在 0x41..=0x5A 时),必须同表跳步。
    /// 2. 读入字节 → 追加 CRLF → 按 CRLF/CR/LF 切行 → 每行末尾加 `0x0D`
    ///    → 写入 `LET` 字符串数组元素 `0..N-1`;循环结束后再写两个空串
    ///    (末行后的空行 + 终止空元素)。
    /// 3. `LET` 必须是 `ty=Str` 的**一维数组**(语料 `$B34..$B39` =
    ///    `ty=3, dims=[70000]`);不是字符串数组时官方报 `0x18b14`,
    ///    本内核报可恢复诊断后继续(不终止脚本)。
    /// 4. `CLEAR=1`:清系统文本缓冲(与 0.495 `TEXT[CLEAR]` 同义),
    ///    **不清 LET 数组**。只有 CLEAR 的调用合法(样本 s205 pc78)。
    pub(crate) fn other_op_text_file(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
    ) -> Result<StepOutcome, VmError> {
        let names: &[String] = &cmd.param_names;
        let mut file: Option<Vec<u8>> = None;
        let mut target: Option<LValue> = None;
        let mut clear = false;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = names
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            match name {
                "FILE" => match self.eval_window(st, w)? {
                    Value::Str(bytes) => file = Some(bytes),
                    other => {
                        self.report(diag::recoverable(
                            code::rt::VAR_TYPE,
                            Boundary::Script,
                            format!("TEXT[FILE] 需要字符串,得到 {}", other.type_name()),
                        ));
                        return Ok(StepOutcome::Continue);
                    }
                },
                "LET" => target = Some(self.eval_lvalue_window(st, w)?),
                "CLEAR" => {
                    if let Ok(v) = self.eval_window(st, w) {
                        clear = v.as_int() == Some(1);
                    }
                }
                _ => {
                    // 555 表里没有其它参数;若未来追加,求值以保持官方
                    // 「先装载全部参数」的副作用。
                    let _ = self.eval_window(st, w);
                }
            }
        }
        let Some(bytes) = file else {
            // 只有 CLEAR 的合法形态;没有 FILE 就没有 LET 要做的事。
            // 官方在 FILE 分支之外仍会执行 CLEAR(`0x452a0c:134`)。
            if clear {
                self.clear_text();
            }
            self.emit(VmEvent::SystemCall {
                frame: self.frame(),
                name: "TEXT".to_string(),
                args: Vec::new(),
            });
            return Ok(StepOutcome::Continue);
        };
        // 缺 LET 的形态未逐字节取证(语料 TEXT 30 组全部带 LET)。保持旧
        // 行为(先清后报、不打开文件),避免把成功路径的 CLEAR 时机修正
        // 扩散到这个 Unknown 形态。
        let Some(lv) = target else {
            if clear {
                self.clear_text();
            }
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "TEXT[FILE] 缺 LET 目标(555 语义:FILE 必须配 LET)",
            ));
            return Ok(StepOutcome::Continue);
        };
        // 官方打开前只做两件事:ASCII 小写化、`/`→`\`;不补扩展名。
        // 本函数只由 `TextMode::FileArray`(精确 555)分派进入,多语言
        // 前导表语义属于这一代;旧版 `TEXT` 走属性设置路径,不会到这里。
        let mut path = bytes;
        normalize_text_file_path(&mut path, self.script_text_codec(st.script_id));
        let Some(data) = self.open_asset(&path) else {
            // 官方 `0x452a0c:61-64`:打开失败直接 return -1,**不执行 CLEAR**。
            self.report(diag::recoverable(
                code::rt::TEXT_FILE,
                Boundary::Script,
                format!(
                    "TEXT[FILE] 打不开 {:?}(官方处理体返回 -1,由调用方回退)",
                    String::from_utf8_lossy(&path)
                ),
            ));
            return Ok(StepOutcome::Yield);
        };
        // 官方 `0x452a0c:134-139`:文件成功装载/切行后才判 CLEAR 并调用
        // `FUN_00469f9c`。
        if clear {
            self.clear_text();
        }
        let var = self.canonical_var(lv.var);
        let Some((ty, dims)) = self.var_shape(var) else {
            self.report(diag::recoverable(
                code::rt::VAR_UNDECLARED,
                Boundary::Script,
                format!("TEXT[LET] 目标未声明 {}", var.display()),
            ));
            return Ok(StepOutcome::Continue);
        };
        if ty != ElemType::Str || dims.len() != 1 {
            self.report(diag::recoverable(
                code::rt::VAR_SHAPE,
                Boundary::Script,
                format!(
                    "TEXT[LET] 必须是字符串一维数组,目标 {} = {}{:?}(官方错误号 0x18b14)",
                    var.display(),
                    ty.as_str(),
                    dims
                ),
            ));
            return Ok(StepOutcome::Continue);
        }
        if !lv.indices.is_empty() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "TEXT[LET] 目标带下标(555 语义要求整组字符串数组)",
            ));
            return Ok(StepOutcome::Continue);
        }
        let limit = dims[0];
        let lines = text_file_lines(&data);
        if let Some(state) = self.other.as_mut() {
            state.last_text_lines = lines.len();
        }
        self.report(diag::verbose(
            code::rt::TEXT_FILE_LINES,
            Boundary::Script,
            format!(
                "TEXT[FILE] {:?}: {} 行 → {} (上限 {limit})",
                String::from_utf8_lossy(&path),
                lines.len(),
                var.display()
            ),
        ));
        let lines = self.rewrite_scenario_lines(st, &path, lines);
        for (index, line) in lines.into_iter().enumerate() {
            if index >= limit {
                // 官方:元素索引上限取自变量长度,越界即停止(不报错)。
                break;
            }
            if let Err(d) = self
                .store_mut()
                .write(var, &[index as i64], &Value::Str(line))
            {
                self.report(d);
                break;
            }
        }
        self.emit(VmEvent::SystemCall {
            frame: self.frame(),
            name: "TEXT".to_string(),
            args: Vec::new(),
        });
        Ok(StepOutcome::Continue)
    }

    /// 把 `TEXT[FILE]` 切好的行按运行时翻译表重写。
    ///
    /// 必须在写 `LET` 数组之前完成:下游剧本 lexer/parser 会对数组元素
    /// 按 `script_codec` 的字节步进切词,译文必须已经是目标 codec 的
    /// 合法字节(CP932 工程可含隧道码,仍是合法双字节步进)。
    ///
    /// 行号与导出侧一致(`index + 1`);文件键运行期是归一化反斜杠路径,
    /// 表查询内部再统一成小写正斜杠。没有条目的行原样保留,但仍推进
    /// 块注释状态。
    fn rewrite_scenario_lines(
        &mut self,
        st: &ExecState,
        path: &[u8],
        lines: Vec<Vec<u8>>,
    ) -> Vec<Vec<u8>> {
        if self.translation.is_none() {
            return lines;
        }
        let codec = self.script_text_codec(st.script_id);
        let (chars, _) = codec.decode_lossy(path);
        let file: String = chars.into_iter().collect();
        let mut state = yu_format::scenario::LineState::default();
        let mut out = Vec::with_capacity(lines.len());
        for (index, line) in lines.into_iter().enumerate() {
            let line_no = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let units = self
                .translation
                .as_mut()
                .and_then(|table| table.lookup_scenario(&file, line_no));
            let Some(units) = units else {
                let _ = yu_format::scenario::scan_units(codec, &line, &mut state);
                out.push(line);
                continue;
            };
            let replacements: Vec<yu_format::scenario::Replacement<'_>> = units
                .iter()
                .map(|unit| yu_format::scenario::Replacement {
                    seq: unit.seq,
                    kind: unit.kind,
                    name: &unit.name,
                    bytes: &unit.bytes,
                })
                .collect();
            match yu_format::scenario::rewrite_line(codec, &line, &mut state, &replacements) {
                Ok(rewritten) => out.push(rewritten),
                Err(error) => {
                    if let Some(table) = self.translation.as_mut() {
                        table.note_rewrite_error();
                    }
                    self.report(diag::recoverable(
                        code::rt::TEXT_FILE,
                        Boundary::Script,
                        format!("翻译行重写失败 {file} line {line_no}: {error}(保留原行)"),
                    ));
                    out.push(line);
                }
            }
        }
        out
    }
}

/// 555 `TEXT[FILE]` 的路径规范化(纯函数,便于单测;只由
/// `SemanticProfile::text == TextMode::FileArray` 的调用点使用,
/// 旧版属性设置路径不走这里)。
///
/// 官方两段处理共用同一张前导表(`DAT_0059b0c0`):
/// - `FUN_00464dac`:前导字节直接前进 2,其余 `A..Z` 加 0x20;
/// - 处理体 `0x452a0c`:`/` 换成 `\`,同样不碰双字节字符的尾字节。
///
/// 合并成一趟等价变换;`codec` 取运行期脚本 codec(555 样本为 Big5)。
/// 逐字节 `make_ascii_lowercase` 会把尾字节 `0x41..=0x5A` 改写,
/// 逐字节斜杠替换会改写尾字节 `0x2F`,两者都不是官方行为。
fn normalize_text_file_path(path: &mut [u8], codec: yu_format::Codec) {
    let mut i = 0usize;
    while i < path.len() {
        let b = path[i];
        if codec.is_lead(b) {
            // 双字节字符整体跳过:尾字节原样保留。
            i += 2;
            continue;
        }
        if b.is_ascii_uppercase() {
            path[i] = b + 0x20;
        } else if b == b'/' {
            path[i] = b'\\';
        }
        i += 1;
    }
}

/// v555 `TEXT[FILE]` 的切行规则(纯函数,便于单测)。
///
/// 官方处理体:`读入字节 + 追加 CRLF` → 按 CRLF / CR / LF 切行 → 每行末尾
/// 写 `0x0D` 后写入 `LET`;循环结束后再对空缓冲写两次(末行之后的空行 +
/// 终止空元素)。因此返回值 = `每条终止符一段(含 0x0D)` + 两个空串。
fn text_file_lines(data: &[u8]) -> Vec<Vec<u8>> {
    let mut buf = Vec::with_capacity(data.len() + 2);
    buf.extend_from_slice(data);
    buf.extend_from_slice(b"\r\n");
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        match buf[i] {
            b'\r' => {
                let mut line = buf[start..i].to_vec();
                line.push(0x0D);
                lines.push(line);
                i += 1;
                if i < buf.len() && buf[i] == b'\n' {
                    i += 1;
                }
                start = i;
            }
            b'\n' => {
                let mut line = buf[start..i].to_vec();
                line.push(0x0D);
                lines.push(line);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    // 最后一个终止符之后的尾段(追加 CRLF 后通常为空)由处理体的
    // 两次空缓冲调用覆盖;不把尾段再加一次 0x0D,避免多出一行。
    lines.push(Vec::new());
    lines.push(Vec::new());
    lines
}

#[cfg(test)]
mod text_file_tests {
    use super::{normalize_text_file_path, text_file_lines};
    use yu_format::Codec;

    /// 555 多语言路径:前导字节跳过 2 字节,尾字节即使是 `A` 也原样保留。
    /// Big5 `A1 41` 是合法双字节字符(`A1` 前导,`41` 尾),旧版逐字节
    /// 小写化会把它改成 `A1 61`。
    #[test]
    fn big5_trail_is_not_lowercased() {
        let mut path = vec![0xA1, 0x41, b'/', b'A'];
        normalize_text_file_path(&mut path, Codec::Big5);
        assert_eq!(
            path,
            vec![0xA1, 0x41, b'\\', b'a'],
            "尾字节 0x41 不得改写;ASCII 斜杠与 A 正常处理"
        );
    }

    /// 尾字节 0x2F 不是任何候选 codec 的合法尾字节,但官方只按前导表
    /// 跳步、不看尾字节合法性;这里锁定同一机制,避免以后有人用
    /// 「合法尾字节」优化把它改回逐字节斜杠替换。
    #[test]
    fn lead_stepping_is_independent_of_trail_validity() {
        let mut path = vec![0xA1, 0x2F, b'/'];
        normalize_text_file_path(&mut path, Codec::Big5);
        assert_eq!(path, vec![0xA1, 0x2F, b'\\']);
    }

    /// CP932 尾字节 `0x5C`(日文路径里常见的「ソ」第二字节)同样不参与
    /// 任何 ASCII 变换。
    #[test]
    fn cp932_trail_bytes_are_opaque() {
        let mut path = vec![0x83, 0x5C, b'/', b'X'];
        normalize_text_file_path(&mut path, Codec::Cp932);
        assert_eq!(path, vec![0x83, 0x5C, b'\\', b'x']);
    }

    /// 纯 ASCII 路径与旧版逐字节结果一致。
    #[test]
    fn ascii_path_matches_legacy_normalization() {
        let mut path = b"SCENARIO/Start.TXT".to_vec();
        normalize_text_file_path(&mut path, Codec::Big5);
        assert_eq!(path, b"scenario\\start.txt");
    }

    /// CRLF 文件:每行 + `0x0D`,末尾两个空元素(样本语料全部是 CRLF)。
    #[test]
    fn crlf_lines_get_cr_and_two_terminators() {
        let lines = text_file_lines(b"#a\r\n#b\r\n");
        assert_eq!(
            lines,
            vec![
                b"#a\r".to_vec(),
                b"#b\r".to_vec(),
                b"\r".to_vec(),
                Vec::new(),
                Vec::new(),
            ],
            "追加 CRLF 会多出一个空行(带 CR),然后是两个终止空元素"
        );
    }

    /// 纯 LF / 纯 CR / 混合终止符都按官方三选一拆分。
    #[test]
    fn lf_cr_and_mixed_terminators() {
        assert_eq!(
            text_file_lines(b"a\nb"),
            vec![b"a\r".to_vec(), b"b\r".to_vec(), Vec::new(), Vec::new()]
        );
        assert_eq!(
            text_file_lines(b"a\rb"),
            vec![b"a\r".to_vec(), b"b\r".to_vec(), Vec::new(), Vec::new()]
        );
        assert_eq!(
            text_file_lines(b"a\r\nb\nc"),
            vec![
                b"a\r".to_vec(),
                b"b\r".to_vec(),
                b"c\r".to_vec(),
                Vec::new(),
                Vec::new(),
            ]
        );
    }

    /// 空文件:只有追加的 CRLF → 一个带 CR 的空行 + 两个终止空元素。
    #[test]
    fn empty_file_keeps_terminator_rows() {
        assert_eq!(
            text_file_lines(b""),
            vec![b"\r".to_vec(), Vec::new(), Vec::new()]
        );
    }
}
