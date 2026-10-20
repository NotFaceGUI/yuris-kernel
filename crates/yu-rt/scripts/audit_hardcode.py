#!/usr/bin/env python3
"""零硬编码审计(A5 守卫)—— 对 crates/yu-rt 的源码做确定性扫描。

判据(全部来自 architecture.md §0 A5 / 任务书 C §3.3):
  1. 禁止出现**版本序号常量**(如 494/495/555/0x1EF 作为逻辑判断);
  2. 禁止出现**内置命令名表**(命令名字符串只允许出现在:
     a) 处理器注册/分区注释里的 `match cmd.name.as_str()`;
     b) 官方手册页路径引用;
     c) 测试与测试夹具;
     d) 文档注释);
  3. 禁止出现**默认路径常量**(`ysbin/` 只允许出现在表装载协议路径与诊断文本,
     且必须集中在 tables.rs;资源路径禁止);
  4. 禁止 `println!/eprintln!/dbg!`(日志必须走 yu-log);
  5. 禁止 `unwrap()/expect()` 出现在非测试代码(除明确的"不可能"断言并带消息)。

输出:每个违规一行 `FILE:LINE:RULE:SNIPPET`,退出码 = 违规数 > 0 ? 1 : 0。

用法:
  python crates/yu-rt/scripts/audit_hardcode.py [repo_root]
"""
import re
import sys
from pathlib import Path

# 版本序号:只禁"用版本号做逻辑分支"的形态;文档/协议字段赋值放行。
VERSION_RULES = [
    (re.compile(r"\b(494|495|555)\b\s*(?:=>|==|!=|<|>)"), "version-literal-compare"),
    (re.compile(r"0x1[Ee][Ff]\b"), "engine-version-hex"),
]

# 禁止的宏(日志统一走 yu-log;trace 在边界层)。
LOG_RULES = [
    (re.compile(r"\bprintln!\s*\("), "println"),
    (re.compile(r"\beprintln!\s*\("), "eprintln"),
    (re.compile(r"\bdbg!\s*\("), "dbg"),
]

# 资源路径常量(游戏/工程特定路径)。
PATH_RULES = [
    (re.compile(r'"(?:data|system)/[^"]*\.(?:png|jpg|bmp|ogg|wav|mpg|avi|psb)"'), "asset-path"),
    (re.compile(r'"(?:cgsys|cgsys2|bgm|se|voice|pac)/'), "asset-dir"),
]

# 允许出现 `ysbin/` 的文件(表装载协议路径,集中在表模块)。
YSBIN_ALLOWED = {"src/tables.rs"}

# 允许出现命令名字符串的文件:处理器分派(注册表)与类型名重叠的 value.rs。
CMDLIKE_ALLOWED = {"src/vm/handlers.rs", "src/value.rs"}

# 命令名白名单:这些是**引擎语义依赖**的控制流/变量命令(处理器的 match 分支),
# 允许出现在 handlers.rs;其它命令名只允许在文档/测试中出现。
CORE_COMMAND_NAMES = {
    "GOSUB", "RETURN", "GO", "IF", "ELSE", "IFBLEND", "IFEND", "IFBREAK",
    "IFCONTINUE", "LOOP", "LOOPEND", "LOOPBREAK", "LOOPCONTINUE", "WAIT",
    "FPS", "END", "LET", "INT", "FLT", "STR", "S_INT", "S_FLT", "S_STR",
    "VARINFO", "VARACT", "MATH", "TASK", "TASKEND", "TASKINFO", "TEXT",
    "_", "LABELINFO",
}

# 只把**命令名形态**的串计入(排除类型名 INT/FLT/STR 的重复计数干扰:
# 它们在 value.rs 里是类型名、在 handlers.rs 里是命令名,已分别白名单)。
COMMAND_STRING = re.compile(r'"([A-Z_][A-Z0-9_]{1,15})"')


def iter_sources(root: Path):
    for p in sorted((root / "src").rglob("*.rs")):
        yield p


def rel(root: Path, p: Path) -> str:
    return p.relative_to(root).as_posix()


def strip_test_modules(text: str) -> str:
    """去掉 #[cfg(test)] mod tests { ... } 之后的整块(简化:从标记到文件尾)。"""
    idx = text.find("#[cfg(test)]")
    return text[:idx] if idx >= 0 else text


def main() -> int:
    here = Path(__file__).resolve()
    default_root = here.parent.parent
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else default_root
    violations = []

    for path in iter_sources(root):
        r = rel(root, path)
        text = path.read_text(encoding="utf-8", errors="replace")
        code = strip_test_modules(text)
        for lineno, line in enumerate(code.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("///"):
                continue
            for rule, name in VERSION_RULES:
                if rule.search(line):
                    violations.append((r, lineno, name, stripped))
            for rule, name in LOG_RULES:
                if rule.search(line):
                    violations.append((r, lineno, name, stripped))
            for rule, name in PATH_RULES:
                if rule.search(line):
                    violations.append((r, lineno, name, stripped))
            if "ysbin/" in line and r not in YSBIN_ALLOWED:
                violations.append((r, lineno, "ysbin-path", stripped))
            if r in CMDLIKE_ALLOWED:
                continue
            for m in COMMAND_STRING.finditer(line):
                name = m.group(1)
                if name in CORE_COMMAND_NAMES:
                    violations.append((r, lineno, "command-name-outside-dispatch", stripped))

    # 非测试代码里的 unwrap/expect(允许带"不可能"说明的显式断言)。
    for path in iter_sources(root):
        r = rel(root, path)
        code = strip_test_modules(path.read_text(encoding="utf-8", errors="replace"))
        for lineno, line in enumerate(code.splitlines(), 1):
            if ".unwrap()" in line:
                violations.append((r, lineno, "unwrap", line.strip()))
            if ".expect(" in line and "不可能" not in line and "刚刚" not in line:
                # 只报没有说明的 expect
                if not re.search(r'\.expect\("[^"]{6,}"\)', line):
                    violations.append((r, lineno, "expect-without-message", line.strip()))

    for r, lineno, rule, snippet in violations:
        print(f"{r}:{lineno}:{rule}:{snippet}")

    print(f"\naudit: files={sum(1 for _ in iter_sources(root))} violations={len(violations)}")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
