//! v555 独立剧本 DSL 的词法器(纯函数,便于单测)。
//!
//! 555 商业作的剧情正文是 `scenario/*.txt` 文本 DSL(`#标签`、
//! `\命令(args)`、`//` 注释),由包内 `es_scenario_*.yst` 解释。
//! `PRODUCT[COM=22]`(`0x45a9dc`,4470 B)是官方词法器:把一行切成
//! token,文本写入字符串数组、类型码写入整数数组、token 数写入 `LET`。
//!
//! # 官方输出契约(b3c02 `FUN_0045a874` RE,2026-11-22)
//!
//! - 输出数组由 `PRODUCT` 的 `PINT2`/`PINT3` 指定(实测 2918=STR[1024]、
//!   2919=INT[1024]),**不是**由 `LET` 指定;`PINT` 是行缓冲/位置指针。
//! - 每行至少一个 token:行尾 `\r`(类型码 **0**,文本 `"\r"`,0x45ad63)。
//!   `//` 注释行只有这一个 token(`PRODUCT[COM=22]` 返回 1)。
//! - 正常行的 token 文本/码从 0 开始连续写入;token 数 = `LET`。
//! - 符号专用码:`\`=0x33、`(`=0x34、`)`=0x35、`,`=0x36、`@`=0x37、
//!   `$`=0x38、`#`=0x39。**`#=` 仍是一个 0x39 token,文本只有 `"#"`,
//!   前进 2 字节**(0x45b260..0x45b2e1);后面的词才是标签名。
//! - 运算符(`0x879ca0` 表):`++`=0x0b、`+=`=0x0c、`-=`=0x0d、
//!   `*=`=0x0e、`/=`=0x0f。
//! - 字符串 `"..."` = **0x1f**(0x45b883 分支),遇 `"` 或 `\r` 结束。
//! - 单个未归类标点 = **0x10**(0x45ab9f),文本 = 该字节。
//! - `;` **跳过**(0x45ab2c,不产生 token)。
//! - **标识符/数字 = 0x20(32)**。证据不是 COM=22 的 asm 状态码
//!   (`0x5e/0x5f/0x60` 是词法器内部**状态**,不是输出码),而是 555
//!   剧本词法器 `es_scenario_lexer.yst`(s59)的产出:`\CSEL` →
//!   `[51, 32, 16]`、`\DrawBG(logo_a,…)` → `[51, 32, 52, 32, 54]`,
//!   以及消费方 `es_scenario_parser.yst`(s61)按 `@3015 == 32` 判标识符、
//!   `== 57` 判 `#`。早期把状态码当输出码会让标签表一条也建不起来。
//! - 数字同样归入 0x20(与 s59 的「词」状态一致)。
//!
//! 未对齐挂账:`/* ... */` 块注释(官方状态 0x63,跨行)当前逐行词法器
//! 未实现;语料暂未发现使用,见 `docs/divergences/`。
//!
//! 注释 `//` 之后整行丢弃,只保留行尾 `\r` token,与官方 `0x45a9dc` 的
//! `//` 分支一致(不输出 `//` 文本)。

/// 一个 DSL token。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// token 类型码(见模块文档)。
    pub code: i64,
    /// token 原始字节(CP932/GBK 原样;不含外围引号)。
    pub text: Vec<u8>,
}

/// 行尾 token 类型码。
///
/// 官方 `FUN_0045a9dc` 在 `\r` / `//` / 行尾写出的 INT 元素是
/// `(low=0, high=0)`(证据:b3c02 `0x45b62f`/`0x45ad27` 写
/// `[ESP+8]=0,[ESP+c]=0` 后 `CALL 0x45baf8`),文本固定为 `"\r"`。
/// 因此类型码是 **0**,不是文本字节 0x0d。
pub const EOL_CODE: i64 = 0;

/// 跨行词法状态(`/* ... */` 块注释,官方状态 0x63)。
///
/// `tokenize` 是逐行调用,块注释必须由调用方(COM=22)在行间保存。
/// 官方 `FUN_0045a874` 的块注释分支:0x45b5eb 进入状态 99(前进 2 字节),
/// 0x45adb5 见到 `*/` 后退回状态 0;行内 `\r` 仍写 EOL token(0x45acc0)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LexState {
    /// 块注释未闭合。
    pub block_comment: bool,
}

/// 无状态包装(块注释从新行开始;仅单测/非逐行调用用)。
#[cfg(test)]
#[must_use]
pub fn tokenize(line: &[u8], codec: yu_format::Codec) -> Vec<Token> {
    let mut state = LexState::default();
    tokenize_stateful(line, codec, &mut state)
}

/// 把一行 DSL 切成 token;`state` 跨行保存块注释状态。
///
/// `line` 可带行尾 `\r`/`\n`,会被跳过;行尾/注释 token 由官方规则生成
/// (见 [`EOL_CODE`])。空行 = **0 个 token**(官方 `FUN_0045a874` 直接走
/// 0x45b9a8 只写计数 0)。
///
/// `codec` 是当前工程的执行文本编码:官方 `DAT_0059b0c0` 的 lead-byte
/// 表随语言/补丁变化(CP932 / GBK / Big5),不能用固定 Shift-JIS 表。
#[must_use]
pub fn tokenize_stateful(
    line: &[u8],
    codec: yu_format::Codec,
    state: &mut LexState,
) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = line.len();
    while i < n {
        // 块注释(官方状态 0x63):不产生普通 token;行内 CR 仍写 EOL
        // (0x45acc0),`*/` 后回到正常词法(0x45adb5)。
        if state.block_comment {
            while i < n {
                if line[i] == b'\r' {
                    out.push(Token {
                        code: EOL_CODE,
                        text: vec![0x0d],
                    });
                    i += 1;
                    continue;
                }
                if line[i] == b'*' && line.get(i + 1) == Some(&b'/') {
                    state.block_comment = false;
                    i += 2;
                    break;
                }
                i += scan_unit_len(line, i, codec);
            }
            if state.block_comment {
                break;
            }
            continue;
        }
        let b = line[i];
        match b {
            b' ' | b'\t' | b'\n' => i += 1,
            // `;` 官方直接跳过,不产生 token(0x45ab2c:INC EBX 后继续)。
            b';' => i += 1,
            b'\r' => {
                // 官方 CR token:类型码 0、文本固定 "\r";空行不带 CR 时
                // 不会追加任何 token。
                out.push(Token {
                    code: EOL_CODE,
                    text: vec![0x0d],
                });
                i += 1;
            }
            b'/' if line.get(i + 1) == Some(&b'/') => {
                // 注释:官方只保留行尾 CR token,不输出 `//` 文本。
                out.push(Token {
                    code: EOL_CODE,
                    text: vec![0x0d],
                });
                break;
            }
            b'#' | b'\\' | b'(' | b')' | b',' | b'@' | b'$' => {
                // 官方词法器给这七个符号分配了专用码 0x33..0x39
                // (`0x45a9dc`: `\`=0x33、`(`=0x34、`)`=0x35、`,`=0x36、
                // `@`=0x37、`$`=0x38、`#`=0x39);文本仍是符号本身。
                let code = match b {
                    b'\\' => 0x33,
                    b'(' => 0x34,
                    b')' => 0x35,
                    b',' => 0x36,
                    b'@' => 0x37,
                    b'$' => 0x38,
                    _ => 0x39, // '#'
                };
                // `#=` 是**一个** token(码仍归 `#`=0x39),后面的词才是标签名。
                //
                // 官方 `FUN_0045a874` 0x45b260..0x45b2e1(sample b3c02):
                // `CMP ECX,0x3d` → 不是 `=` 走普通 `#` 分支;是 `=` 时写
                // 类型码 0x39、文本**只有 `"#"`**、`ADD EBX,2`(跳过 `=`)。
                //
                // 语料证据(样本2 `saku02_01_14h_num007.txt` L54/L62):
                //   `#=saku02_01_14h_num007_rp0`                  ← 定义
                //   `\GO.IF.RPMODE0(saku02_01_14h_num007_rp0)`    ← 引用(无 `=`)
                // 官方 s58 标签注册 pc105 取 `#` 后**一个** token 作为标签名:
                // 若把 `#=` 切成 `#` + `=`,注册名变成 `=`,合并数组里每个
                // scenario 文件的 `#=<file>_rp0` 都撞成同一个 `=` → 游戏弹
                // 「ラベル : = が重複しています。」并中断启动链。
                if b == b'#' && line.get(i + 1) == Some(&b'=') {
                    out.push(Token {
                        code,
                        text: vec![b'#'],
                    });
                    i += 2;
                    continue;
                }
                out.push(Token {
                    code,
                    text: vec![b],
                });
                i += 1;
            }
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < n && line[j] != b'"' && line[j] != b'\r' {
                    j += scan_unit_len(line, j, codec);
                }
                out.push(Token {
                    // 官方字符串 token 类型码 = 0x1f(0x45b883 状态 0x16 分支)。
                    code: 0x1f,
                    text: line[start..j.min(n)].to_vec(),
                });
                i = (j + 1).min(n);
            }
            // `/* ... */` 块注释开始:官方状态 99(0x45b5eb),前进 2 字节。
            // 行内未闭合时由循环顶部的状态分支续扫(跨行由 LexState 保存)。
            b'/' if line.get(i + 1) == Some(&b'*') => {
                state.block_comment = true;
                i += 2;
            }
            // `-=` 是运算符表 0x0d;单独的 `-` 是**词首**(官方 0x45aa34
            // 与数字同状态 0x5b),不在这里处理,落到 `_` 的 scan_word。
            b'-' if line.get(i + 1) == Some(&b'=') => {
                out.push(Token {
                    code: 0x0d,
                    text: b"-=".to_vec(),
                });
                i += 2;
            }
            // 其余运算符表(`0x879ca0` = `++` `+=` `*=` `/=`):0x0b..0x0f,
            // 每个前进 2 字节;单个 `+`/`*`/`/` 走官方未归类分支 0x10。
            b'+' | b'*' | b'/' => {
                let next = line.get(i + 1).copied();
                let operator = match (b, next) {
                    (b'+', Some(b'+')) => Some((0x0b, b"++".as_slice())),
                    (b'+', Some(b'=')) => Some((0x0c, b"+=".as_slice())),
                    (b'*', Some(b'=')) => Some((0x0e, b"*=".as_slice())),
                    (b'/', Some(b'=')) => Some((0x0f, b"/=".as_slice())),
                    _ => None,
                };
                if let Some((code, text)) = operator {
                    out.push(Token {
                        code,
                        text: text.to_vec(),
                    });
                    i += 2;
                } else {
                    // 单个运算符走官方未归类分支:码 0x10、文本 = 字节。
                    out.push(Token {
                        code: 0x10,
                        text: vec![b],
                    });
                    i += 1;
                }
            }
            _ => {
                // 词/数字(0x20):官方状态字符集 = 字母/数字/`_`/`.`/`/`/lead,
                // 首字符额外允许 `-`(0x45aa34 → 状态 0x5b);词内 `-` 结束词。
                let (j, text) = scan_word(line, i, codec);
                if j > i {
                    out.push(Token { code: 0x20, text });
                    i = j;
                } else {
                    // 未归类字节:官方 else 分支码 **0x10**、文本 = 字节
                    // (0x45ab9f `MOV dword ptr [EDX+ECX*8],0x10`)。
                    out.push(Token {
                        code: 0x10,
                        text: vec![b],
                    });
                    i += 1;
                }
            }
        }
    }
    out
}

/// FUN_0045a874 的扫描按 DAT_0059b0c0 前导表跳两字节,不验证
/// 尾字节/Unicode 映射(状态 0x16/0x5b..0x60/0x63)。严格解码会把
/// 不透明尾字节误认成引号、分隔符或注释结束;UTF-8 单独按完整字符走。
fn scan_unit_len(line: &[u8], at: usize, codec: yu_format::Codec) -> usize {
    if codec == yu_format::Codec::Utf8 {
        codec.unit_len_at(line, at).max(1)
    } else {
        1 + usize::from(codec.is_lead(line[at]))
    }
}

/// 词/数字扫描(官方状态 0x5b–0x60)。
///
/// 字符集(0x45b6be 之后的状态机):`0-9`/`A-Z`/`a-z`/`_`/`.`/`/`/lead;
/// 首字符额外允许 `-`(0x45aa34:`CMP EDX,0x2d` 进状态 0x5b)。词内 `-`
/// 不在集合里 ⇒ `a-b` 官方切成 `a` + `-b`,不是一个词。
fn scan_word(line: &[u8], start: usize, codec: yu_format::Codec) -> (usize, Vec<u8>) {
    let n = line.len();
    let is_lead = |b: u8| codec.is_lead(b);
    let mut j = start;
    if line.get(j) == Some(&b'-') {
        j += 1;
    }
    while j < n {
        let c = line[j];
        if !(c.is_ascii_alphanumeric()
            || c == b'_'
            || c == b'.'
            || c == b'/'
            || is_lead(c))
        {
            break;
        }
        j += scan_unit_len(line, j, codec);
    }
    (j, line[start..j.min(n)].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yu_format::Codec;

    fn cp932(line: &[u8]) -> Vec<Token> {
        tokenize(line, Codec::Cp932)
    }

    fn texts(tokens: &[Token]) -> Vec<&[u8]> {
        tokens.iter().map(|t| t.text.as_slice()).collect()
    }

    #[test]
    fn legacy_word_and_quote_scans_keep_opaque_lead_pairs() {
        // FUN_0045a874 的前导表步进不能被严格码表验证替代。
        for codec in [Codec::Cp932, Codec::Gbk, Codec::Big5] {
            let word = tokenize(b"#a\x82,b\r", codec);
            assert_eq!(texts(&word), [b"#".as_slice(), b"a\x82,b", b"\r"], "{codec:?}");
            let quote = tokenize(b"\"a\x82\"b\"\r", codec);
            assert_eq!(texts(&quote), [b"a\x82\"b".as_slice(), b"\r"], "{codec:?}");
            assert_eq!(quote[0].code, 0x1f);
        }
    }

    #[test]
    fn legacy_block_comment_does_not_close_on_a_trail_star() {
        let mut state = LexState::default();
        let first = tokenize_stateful(b"/*\x82*/ hidden\r", Codec::Cp932, &mut state);
        assert_eq!(texts(&first), [b"\r".as_slice()]);
        assert!(state.block_comment, "星号是前导后的不透明尾字节");
        let second = tokenize_stateful(b"*/ visible\r", Codec::Cp932, &mut state);
        assert_eq!(texts(&second), [b"visible".as_slice(), b"\r"]);
        assert!(!state.block_comment);
    }

    #[test]
    fn utf8_words_quotes_and_comments_keep_character_boundaries() {
        let tokens = tokenize("#é終😀\r\"é終😀\"/*é終😀*/ok\r".as_bytes(), Codec::Utf8);
        assert_eq!(texts(&tokens), [
            b"#".as_slice(), "é終😀".as_bytes(), b"\r",
            "é終😀".as_bytes(), b"ok", b"\r",
        ]);
        assert_eq!(tokens.iter().map(|token| token.code).collect::<Vec<_>>(), [0x39, 0x20, 0, 0x1f, 0x20, 0]);
    }

    #[test]
    fn label_line_splits_hash_and_identifier() {
        let t = cp932(b"#SCENARIO_START_RELEASE\r");
        assert_eq!(
            texts(&t),
            vec![b"#".as_slice(), b"SCENARIO_START_RELEASE", b"\r".as_slice()]
        );
        assert_eq!(t[0].code, 0x39);
        assert_eq!(t[1].code, 0x20);
        assert_eq!(t[2].code, EOL_CODE);
        assert_eq!(EOL_CODE, 0, "官方 CR token 类型码 = 0");
    }

    /// `#=` 是标签别名前缀:合成**一个** token,后面的词才是标签名。
    ///
    /// 官方 0x45b260..0x45b2e1:类型码 0x39、文本 `"#"`、前进 2 字节。
    /// 语料:`#=saku02_01_14h_num007_rp0`(定义)与
    /// `\GO.IF.RPMODE0(saku02_01_14h_num007_rp0)`(引用,无 `=`)。
    /// 切成 `#`+`=` 会让官方 s58 把 `=` 注册成标签名,多个 scenario 文件
    /// 的 `#=<file>_rp0` 互相重复(「ラベルが重複しています」)。
    #[test]
    fn label_alias_hash_equals_is_one_token() {
        let t = cp932(b"#=saku02_01_14h_num007_rp0\r");
        assert_eq!(
            texts(&t),
            vec![
                b"#".as_slice(),
                b"saku02_01_14h_num007_rp0",
                b"\r".as_slice()
            ]
        );
        assert_eq!(t[0].code, 0x39, "`#=` 仍归 `#` 的专用码");
        assert_eq!(t[1].code, 0x20, "标签名是词(token 0x20)");
    }

    #[test]
    fn macro_line_splits_punctuation_and_args() {
        let t = cp932(b"\\BGM(bgm05, 1000)\r");
        assert_eq!(
            texts(&t),
            vec![
                b"\\".as_slice(),
                b"BGM",
                b"(",
                b"bgm05",
                b",",
                b"1000",
                b")",
                b"\r"
            ]
        );
        assert_eq!(t[0].code, 0x33);
        assert_eq!(t[2].code, 0x34);
        assert_eq!(t[4].code, 0x36);
        assert_eq!(t[6].code, 0x35);
        assert_eq!(t[5].code, 0x20);
        assert_eq!(t[7].code, EOL_CODE);
    }

    #[test]
    fn comment_line_is_only_eol_token() {
        let t = cp932(b"// this is a comment\r");
        assert_eq!(texts(&t), vec![b"\r".as_slice()]);
        assert_eq!(t[0].code, EOL_CODE);
    }

    #[test]
    fn empty_line_has_no_tokens_blank_line_has_cr_token() {
        assert!(cp932(b"").is_empty(), "空行 = 0 token(官方只写计数 0)");
        assert!(cp932(b" \t").is_empty(), "纯空白无 CR = 0 token");
        let t = cp932(b" \t\r\n");
        assert_eq!(texts(&t), vec![b"\r".as_slice()]);
        assert_eq!(t[0].code, EOL_CODE);
    }

    #[test]
    fn string_literal_keeps_cp932_bytes_without_quotes() {
        let t = cp932(b"\\LC(\"\x82\xa0\")");
        assert_eq!(t[3].text, b"\x82\xa0".to_vec());
        assert_eq!(t[3].code, 0x1f, "官方字符串 token 类型码 0x1f");
        assert_eq!(t[4].code, 0x35); // 词法器码表:`)` = 0x35
        assert_eq!(t.len(), 5, "无行尾 CR → 不追加 EOL token");
    }

    /// 运算符表在 `0x879ca0`:`++` `+=` `-=` `*=` `/=` → 0x0b..0x0f。
    #[test]
    fn operator_pairs_use_dedicated_codes() {
        let t = cp932(b"++ += -= *= /=");
        let codes: Vec<i64> = t.iter().map(|x| x.code).collect();
        assert_eq!(codes, vec![0x0b, 0x0c, 0x0d, 0x0e, 0x0f]);
        let texts: Vec<&[u8]> = t.iter().map(|x| x.text.as_slice()).collect();
        assert_eq!(texts, vec![b"++".as_slice(), b"+=", b"-=", b"*=", b"/="]);
    }

    /// 未归类单字节:官方 else 分支码 0x10、文本 = 字节(0x45ab9f)。
    #[test]
    fn unclassified_punctuation_is_code_0x10() {
        let t = cp932(b"=");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].code, 0x10);
        assert_eq!(t[0].text, b"=".to_vec());
    }

    /// `;` 官方直接跳过(0x45ab2c),不产生 token。
    #[test]
    fn semicolon_is_skipped() {
        let t = cp932(b"a ; b");
        assert_eq!(texts(&t), vec![b"a".as_slice(), b"b"]);
    }

    /// `/* ... */` 同行:注释内容不产 token,`*/` 后继续正常词法。
    #[test]
    fn same_line_block_comment_is_skipped() {
        let t = cp932(b"\\LC(/*x*/a)");
        assert_eq!(
            texts(&t),
            vec![b"\\".as_slice(), b"LC", b"(", b"a", b")"]
        );
    }

    /// `/*` 跨行:状态由调用方保存;块注释内 CR 仍写 EOL token(0x45acc0)。
    #[test]
    fn block_comment_state_survives_lines() {
        let mut state = LexState::default();
        let first = tokenize_stateful(b"a /* start\r", Codec::Cp932, &mut state);
        assert_eq!(texts(&first), vec![b"a".as_slice(), b"\r".as_slice()]);
        assert!(state.block_comment, "本行未闭合,状态跨行保持");

        let second = tokenize_stateful(b"still comment */ b\r", Codec::Cp932, &mut state);
        assert_eq!(texts(&second), vec![b"b".as_slice(), b"\r".as_slice()]);
        assert!(!state.block_comment, "`*/` 后回到正常词法");
    }

    /// 单独 `-` 是词首(官方 0x45aa34 → 状态 0x5b),不是未归类标点;
    /// 词内 `-` 不在官方字符集里 → `a-b` 切成 `a` + `-b`。
    #[test]
    fn leading_minus_is_a_word() {
        for line in [b"-".as_slice(), b"-5", b"-abc"] {
            let t = cp932(line);
            assert_eq!(t.len(), 1, "{line:?}");
            assert_eq!(t[0].code, 0x20, "{line:?}");
            assert_eq!(t[0].text, line.to_vec());
        }
        let t = cp932(b"a-b");
        assert_eq!(texts(&t), vec![b"a".as_slice(), b"-b"]);
        assert_eq!(t[1].code, 0x20);
    }

    /// `/` 在词内是普通字符(官方状态字符集含 0x2f)。
    #[test]
    fn slash_inside_word_stays_in_word() {
        let t = cp932(b"scenario/foo");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].code, 0x20);
        assert_eq!(t[0].text, b"scenario/foo".to_vec());
    }

    /// 词法器不在 1023 处静默截断;越界由 COM=22 写手报诊断。
    #[test]
    fn long_line_is_not_silently_truncated() {
        let mut line = Vec::new();
        for _ in 0..2000 {
            line.extend_from_slice(b"a ");
        }
        let t = cp932(&line);
        assert_eq!(t.len(), 2000);
    }

    /// lead-byte 分类必须跟随当前工程 codec,不能写死 CP932。
    #[test]
    fn lead_bytes_follow_active_codec() {
        let gbk = tokenize(b"\xB0\xA1\r", Codec::Gbk); // GBK「啊」
        assert_eq!(gbk[0].text, b"\xB0\xA1");
        assert_eq!(gbk[0].code, 0x20);
        let big5 = tokenize(b"\xA4\x40\r", Codec::Big5); // Big5「一」
        assert_eq!(big5[0].text, b"\xA4\x40");
        assert_eq!(big5[0].code, 0x20);
        let cp932 = tokenize(b"\x82\xa0\r", Codec::Cp932); // CP932「あ」
        assert_eq!(cp932[0].text, b"\x82\xa0");
        assert_eq!(cp932[0].code, 0x20);
    }

    /// v555 标签/命令名可以数字开头且带字母:必须整体成一个词。
    /// 回归:旧实现把 `#03_01_22h` 切成 `03_01_22` + `h`，造成标签重复弹窗。
    #[test]
    fn digit_leading_label_is_one_word() {
        let t = cp932(b"#03_01_22h\r");
        assert_eq!(texts(&t), vec![b"#".as_slice(), b"03_01_22h", b"\r"]);
        assert_eq!(t[1].code, 0x20);
        assert_eq!(t[2].code, EOL_CODE);

        let t = cp932(b"\\DrawCG(03_kur001_f,bg)");
        assert_eq!(t[3].text, b"03_kur001_f".to_vec());
        assert_eq!(t[3].code, 0x20);
    }
}
