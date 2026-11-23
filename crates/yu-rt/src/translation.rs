//! 运行时翻译表(纯数据)。
//!
//! 表由宿主从 `translate.json` 装载、按目标 codec 编码后交给 VM;本模块
//! 只负责坐标匹配与命中统计,不做 JSON/编码。三个入口:
//!
//! - `WORD`:`(script_id, pc)` → 整段译文;
//! - 外置剧本 `TEXT[FILE]`:归一化 `file + line` → 行内若干单元(宏/行内文本);
//! - `GOSUB` 字符串参数:`(script_id, pc, arg)` → 角色名/选项等。
//!
//! 译文必须与构建 codec 一致(CP932 工程用 CP932 + 隧道,GBK/Big5 工程
//! 直接编码),这样脚本层 `STRLEN`/`_strmid`/`TEXT[FILE]` 的字节步进不变。

use rustc_hash::{FxHashMap, FxHashSet};

use crate::value::Value;
use yu_format::scenario::UnitKind;

/// 外置剧本键的归一化路径(小写 ASCII、`\` → `/`、去掉前导 `./`/`/`)。
///
/// 导出侧用工程相对路径(正斜杠,如 `scenario/foo.txt`);运行期
/// `TEXT[FILE]` 传入的是游戏内路径(反斜杠,`normalize_text_file_path`
/// 已做 ASCII 小写化)。两边都过这个函数后才能对齐。
#[must_use]
pub fn normalize_scenario_path(path: &str) -> String {
    let mut out = path.replace('\\', "/").to_ascii_lowercase();
    while let Some(rest) = out.strip_prefix("./") {
        out = rest.to_string();
    }
    while out.starts_with('/') {
        out.remove(0);
    }
    out
}

/// 运行期剧本行坐标(`file` 已归一)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScenarioKey {
    pub file: String,
    /// 1 起的物理行号(与 `translate.json` 的 `line` 一致)。
    pub line: u32,
}

impl ScenarioKey {
    #[must_use]
    pub fn new(file: &str, line: u32) -> Self {
        Self {
            file: normalize_scenario_path(file),
            line,
        }
    }
}

/// 一行里的一条可替换单元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioUnit {
    /// 行内出现序号(0 起;与导出 `seq` 一致)。
    pub seq: u32,
    pub kind: UnitKind,
    /// 宏名(大写);行内文本为空。
    pub name: Vec<u8>,
    /// 已按目标 codec 编码的译文。
    pub bytes: Vec<u8>,
}

/// `GOSUB` 字符串参数的替换项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallArg {
    /// 参数求值顺序(0 起;标签窗口不算)。
    pub arg: u32,
    pub bytes: Vec<u8>,
}

/// 脚本文本常量(`0x4D` 记录)的替换项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiteralSegment {
    /// 该窗口 payload 内引号段的出现顺序(0 起)。
    pub segment: u32,
    pub bytes: Vec<u8>,
}

/// 命中/失败统计(宿主启动日志与 `nativeStatus` 用)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TranslationStats {
    pub word_hits: u64,
    pub word_misses: u64,
    pub scenario_line_hits: u64,
    pub scenario_line_misses: u64,
    pub scenario_units: u64,
    pub scenario_rewrite_errors: u64,
    pub call_hits: u64,
    pub call_misses: u64,
    pub literal_hits: u64,
    pub literal_misses: u64,
}

/// 运行时翻译表。
#[derive(Clone, Debug, Default)]
pub struct TranslationTable {
    words: FxHashMap<(u32, u32), Vec<u8>>,
    scenarios: FxHashMap<ScenarioKey, Vec<ScenarioUnit>>,
    /// 有译文的文件集:没有条目的文件不逐行计 miss(70k 行剧本的噪声)。
    scenario_files: FxHashSet<String>,
    calls: FxHashMap<(u32, u32), Vec<CallArg>>,
    /// `(script_id, pc, window)` → 该窗口的文本常量段。
    literals: FxHashMap<(u32, u32, u16), Vec<LiteralSegment>>,
    /// 有文本常量的 `(script_id, pc)` 站点(绝大多数命令没有,先查这里)。
    literal_sites: FxHashSet<(u32, u32)>,
    stats: TranslationStats,
}

impl TranslationTable {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
            && self.scenarios.is_empty()
            && self.calls.is_empty()
            && self.literals.is_empty()
    }

    #[must_use]
    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    #[must_use]
    pub fn scenario_count(&self) -> usize {
        self.scenarios.len()
    }

    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    #[must_use]
    pub fn literal_count(&self) -> usize {
        self.literals.len()
    }

    pub fn insert_word(&mut self, script_id: u32, pc: u32, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.words.insert((script_id, pc), bytes);
        }
    }

    pub fn insert_scenario(&mut self, file: &str, line: u32, units: Vec<ScenarioUnit>) {
        if !units.is_empty() {
            let key = ScenarioKey::new(file, line);
            self.scenario_files.insert(key.file.clone());
            self.scenarios.insert(key, units);
        }
    }

    pub fn insert_call(&mut self, script_id: u32, pc: u32, arg: u32, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let entries = self.calls.entry((script_id, pc)).or_default();
        if let Some(existing) = entries.iter_mut().find(|entry| entry.arg == arg) {
            existing.bytes = bytes;
        } else {
            entries.push(CallArg { arg, bytes });
            entries.sort_by_key(|entry| entry.arg);
        }
    }

    pub fn insert_literal(
        &mut self,
        script_id: u32,
        pc: u32,
        window: u16,
        segment: u32,
        bytes: Vec<u8>,
    ) {
        if bytes.is_empty() {
            return;
        }
        self.literal_sites.insert((script_id, pc));
        let entries = self.literals.entry((script_id, pc, window)).or_default();
        if let Some(existing) = entries.iter_mut().find(|entry| entry.segment == segment) {
            existing.bytes = bytes;
        } else {
            entries.push(LiteralSegment { segment, bytes });
            entries.sort_by_key(|entry| entry.segment);
        }
    }

    /// 某条命令所有窗口的文本常量(供 `fill_cmd` 在借用脚本前取走)。
    #[must_use]
    pub fn literals_for(&self, script_id: u32, pc: u32) -> Vec<(u16, Vec<LiteralSegment>)> {
        if !self.literal_sites.contains(&(script_id, pc)) {
            return Vec::new();
        }
        self.literals
            .iter()
            .filter(|((sid, at, _), _)| *sid == script_id && *at == pc)
            .map(|((_, _, window), entries)| (*window, entries.clone()))
            .collect()
    }

    /// 窗口文本常量命中查询;是否命中由调用方在重写后记账。
    pub fn lookup_literals(
        &mut self,
        script_id: u32,
        pc: u32,
        window: u16,
    ) -> Option<Vec<LiteralSegment>> {
        self.literals.get(&(script_id, pc, window)).cloned()
    }

    pub fn note_literal_rewrite(&mut self, applied: usize, missing: usize) {
        self.stats.literal_hits += applied as u64;
        self.stats.literal_misses += missing as u64;
    }

    /// `WORD` 命中查询(命中/未命中都计数)。
    pub fn lookup_word(&mut self, script_id: u32, pc: u32) -> Option<Vec<u8>> {
        match self.words.get(&(script_id, pc)) {
            Some(bytes) => {
                self.stats.word_hits += 1;
                Some(bytes.clone())
            }
            None => {
                self.stats.word_misses += 1;
                None
            }
        }
    }

    /// 剧本行命中查询(返回克隆,调用方随后还要可变借用 `self`)。
    pub fn lookup_scenario(&mut self, file: &str, line: u32) -> Option<Vec<ScenarioUnit>> {
        let key = ScenarioKey::new(file, line);
        if !self.scenario_files.contains(&key.file) {
            return None;
        }
        match self.scenarios.get(&key) {
            Some(units) => {
                self.stats.scenario_line_hits += 1;
                self.stats.scenario_units += units.len() as u64;
                Some(units.clone())
            }
            None => {
                self.stats.scenario_line_misses += 1;
                None
            }
        }
    }

    /// `GOSUB` 调用点命中查询。
    pub fn lookup_call(&mut self, script_id: u32, pc: u32) -> Option<Vec<CallArg>> {
        self.calls.get(&(script_id, pc)).cloned()
    }

    pub fn note_rewrite_error(&mut self) {
        self.stats.scenario_rewrite_errors += 1;
    }

    #[must_use]
    pub fn stats(&self) -> TranslationStats {
        self.stats
    }

    /// 对 `GOSUB` 参数做一次替换:只处理字符串值;该调用点有表但参数
    /// 序号未命中时计一次 miss。
    pub fn translate_call_arg(&mut self, script_id: u32, pc: u32, arg: u32, value: Value) -> Value {
        let Some(entries) = self.calls.get(&(script_id, pc)) else {
            return value;
        };
        let Some(entry) = entries.iter().find(|entry| entry.arg == arg) else {
            self.stats.call_misses += 1;
            return value;
        };
        match value {
            Value::Str(_) => {
                self.stats.call_hits += 1;
                Value::Str(entry.bytes.clone())
            }
            other => {
                self.stats.call_misses += 1;
                other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_scenario_paths() {
        assert_eq!(
            normalize_scenario_path("scenario\\Foo.TXT"),
            "scenario/foo.txt"
        );
        assert_eq!(
            normalize_scenario_path("./scenario/foo.txt"),
            "scenario/foo.txt"
        );
        assert_eq!(
            normalize_scenario_path("/scenario/foo.txt"),
            "scenario/foo.txt"
        );
    }

    #[test]
    fn word_lookup_counts_hits_and_misses() {
        let mut table = TranslationTable::default();
        table.insert_word(3, 9, b"ok".to_vec());
        assert_eq!(table.lookup_word(3, 9), Some(b"ok".to_vec()));
        assert_eq!(table.lookup_word(3, 10), None);
        let stats = table.stats();
        assert_eq!(stats.word_hits, 1);
        assert_eq!(stats.word_misses, 1);
    }

    #[test]
    fn scenario_lookup_is_path_and_line_exact() {
        let mut table = TranslationTable::default();
        table.insert_scenario(
            "scenario\\A.txt",
            2,
            vec![ScenarioUnit {
                seq: 0,
                kind: UnitKind::Text,
                name: Vec::new(),
                bytes: b"hi".to_vec(),
            }],
        );
        assert!(table.lookup_scenario("scenario/a.txt", 2).is_some());
        assert!(table.lookup_scenario("scenario/a.txt", 1).is_none());
        assert!(table.lookup_scenario("scenario/b.txt", 2).is_none());
    }

    #[test]
    fn call_translation_replaces_only_strings_and_known_args() {
        let mut table = TranslationTable::default();
        table.insert_call(5, 12, 0, b"name".to_vec());
        assert_eq!(
            table.translate_call_arg(5, 12, 0, Value::Str(b"old".to_vec())),
            Value::Str(b"name".to_vec())
        );
        assert_eq!(
            table.translate_call_arg(5, 12, 0, Value::Int(1)),
            Value::Int(1)
        );
        assert_eq!(
            table.translate_call_arg(5, 12, 7, Value::Str(b"x".to_vec())),
            Value::Str(b"x".to_vec())
        );
        let stats = table.stats();
        assert_eq!(stats.call_hits, 1);
        assert_eq!(stats.call_misses, 2);
    }
}
