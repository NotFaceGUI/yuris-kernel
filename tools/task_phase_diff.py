#!/usr/bin/env python3
"""官方 vs 内核任务相位 diff(P5 / DIV-L-002)。

输入:
  - 官方 `task_phase.jsonl`:`probe_dispatch.py --mode task-phase` 输出,
    每行一个 tick 的任务链表快照(`tick_counter` + 扁平化 tasks)。
  - 内核 `kernel_phase.jsonl`:`emit_kernel_phase` 输出,首行 header
    (`click_frame`),其后每行一个 tick(`frame` + tasks)。

对齐:引擎 tick 与内核逻辑帧不是同一时钟。默认按**点击相对**对齐:
  官方 relative = tick_counter - click_tick(click_tick 取首次 trigger.tick)
  内核 relative = frame - click_frame(click_frame 取内核 header)
`--align index` 改为按采样序号对齐(两边都必须从进程启动开始采)。
显式 `--kernel-offset N` 可整体平移内核 relative;偏移会写进报告,不隐藏。

比较:每 tick 的**启用任务集合**:
  - 严格判据 = `(script, pc)` 多重集合;
  - 附加判据 = `(script)` 多重集合(相位在哪个脚本);
  - 报告首个分歧 tick 的期望/实际任务表,以及焦点脚本的 presence 区间。
退出码:0 = 对齐窗口内零分歧;1 = 有分歧;2 = 数据/对齐不足。
"""

import argparse
import json
import sys

FOCUS_DEFAULT = "60,58,66,20"


def load_official(path):
    rows = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    rows.sort(key=lambda row: row.get("i", 0))
    return rows


def load_kernel(path):
    header = None
    rows = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            if row.get("kind") == "kernel_task_phase":
                header = row
            else:
                rows.append(row)
    rows.sort(key=lambda row: row.get("tick", 0))
    return header, rows


def flatten_official(tasks):
    out = []
    for task in tasks or []:
        out.append(task)
        out.extend(flatten_official(task.get("children")))
    return out


def official_effective(row):
    tasks = [
        task
        for task in flatten_official(row.get("tasks"))
        if task.get("enabled") is not False and task.get("script") is not None
    ]
    return tasks


def kernel_effective(row):
    tasks = [
        task
        for task in (row.get("tasks") or [])
        if task.get("enabled") is not False and not task.get("finished")
    ]
    return tasks


def task_key(task, with_pc):
    script = task.get("script")
    if with_pc:
        return (script, task.get("pc"))
    return (script,)


def multiset(tasks, with_pc):
    return sorted(task_key(task, with_pc) for task in tasks)


def key_text(keys):
    parts = []
    for key in keys:
        if len(key) == 2:
            script, pc = key
            parts.append(f"{script}:{pc}" if pc is not None else f"{script}:?")
        else:
            parts.append(str(key[0]))
    return "[" + ", ".join(parts) + "]"


def presence(rows, focus):
    """脚本 → relative tick 的连续存在区间。"""
    out = {}
    for script in focus:
        intervals = []
        start = None
        last = None
        for relative, tasks in rows:
            present = any(task.get("script") == script for task in tasks)
            if present:
                if start is None:
                    start = relative
                last = relative
            elif start is not None:
                intervals.append((start, last))
                start = None
        if start is not None:
            intervals.append((start, last))
        out[script] = intervals
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--official", required=True)
    parser.add_argument("--kernel", required=True)
    parser.add_argument("--out", default=None)
    parser.add_argument("--align", choices=["click", "index", "anchor"], default="click")
    parser.add_argument("--anchor-script", type=int, default=66,
                        help="anchor 对齐:以该脚本首次出现为相对 0(默认 66 场景任务)")
    parser.add_argument("--official-click-tick", type=int, default=None)
    parser.add_argument("--kernel-click-frame", type=int, default=None)
    parser.add_argument("--kernel-offset", type=int, default=0,
                        help="显式整体平移内核 relative(默认 0,写进报告)")
    parser.add_argument("--focus", default=FOCUS_DEFAULT)
    parser.add_argument("--ignore-scripts", default="",
                        help="显式忽略的 script id(逗号分隔,如 0,8);写进报告")
    parser.add_argument("--from-relative", type=int, default=None,
                        help="只比较 >= 该相对 tick 的样本(anchor 对齐后常用 0)")
    parser.add_argument("--max-report", type=int, default=10)
    args = parser.parse_args()

    official = load_official(args.official)
    kernel_header, kernel = load_kernel(args.kernel)
    if not official or not kernel:
        print("no samples: official=%d kernel=%d" % (len(official), len(kernel)))
        return 2

    official_click = args.official_click_tick
    if official_click is None:
        for row in official:
            trigger = row.get("trigger") or {}
            if trigger.get("tick") is not None:
                official_click = trigger["tick"]
                break
    kernel_click = args.kernel_click_frame
    if kernel_click is None and kernel_header is not None:
        kernel_click = kernel_header.get("click_frame")

    if args.align == "anchor":
        def first_presence(rows, script):
            for index, row in rows:
                if any(task.get("script") == script for task in row):
                    return index
            return None

        official_indexed = {
            row["tick_counter"]: official_effective(row)
            for row in official
            if row.get("tick_counter") is not None
        }
        kernel_indexed = {
            row["frame"]: kernel_effective(row)
            for row in kernel
            if row.get("frame") is not None
        }
        official_anchor = first_presence(sorted(official_indexed.items()), args.anchor_script)
        kernel_anchor = first_presence(sorted(kernel_indexed.items()), args.anchor_script)
        if official_anchor is None or kernel_anchor is None:
            print("anchor script %d not observed (official=%s kernel=%s)"
                  % (args.anchor_script, official_anchor, kernel_anchor))
            return 2
        official_rows = {
            index - official_anchor: tasks for index, tasks in official_indexed.items()
        }
        kernel_rows = {
            index - kernel_anchor + args.kernel_offset: tasks
            for index, tasks in kernel_indexed.items()
        }
        anchor_note = (
            "anchor script %d: official_tick=%d kernel_frame=%d"
            % (args.anchor_script, official_anchor, kernel_anchor)
        )
    elif args.align == "click":
        if official_click is None or kernel_click is None:
            print("click alignment needs --official-click-tick/--kernel-click-frame "
                  "or trigger/header evidence")
            return 2
        official_rows = {
            row["tick_counter"] - official_click: official_effective(row)
            for row in official
            if row.get("tick_counter") is not None
        }
        kernel_rows = {
            row["frame"] - kernel_click + args.kernel_offset: kernel_effective(row)
            for row in kernel
            if row.get("frame") is not None
        }
    else:
        official_rows = {
            index: official_effective(row) for index, row in enumerate(official)
        }
        kernel_rows = {
            index + args.kernel_offset: kernel_effective(row)
            for index, row in enumerate(kernel)
        }

    ignore = {int(part) for part in args.ignore_scripts.split(",") if part.strip()}
    if ignore:
        official_rows = {
            key: [task for task in tasks if task.get("script") not in ignore]
            for key, tasks in official_rows.items()
        }
        kernel_rows = {
            key: [task for task in tasks if task.get("script") not in ignore]
            for key, tasks in kernel_rows.items()
        }
    common = sorted(set(official_rows) & set(kernel_rows))
    if args.from_relative is not None:
        common = [key for key in common if key >= args.from_relative]
    focus = [int(part) for part in args.focus.split(",") if part.strip()]
    lines = []
    if args.align == "anchor":
        lines.append(anchor_note)
    lines.append(
        "align=%s official_click_tick=%s kernel_click_frame=%s kernel_offset=%d"
        % (args.align, official_click, kernel_click, args.kernel_offset)
    )
    if args.from_relative is not None:
        lines.append("relative window starts at %d" % args.from_relative)
    if ignore:
        lines.append("ignored scripts (explicit): %s" % sorted(ignore))
    lines.append(
        "official samples=%d range=%s..%s; kernel samples=%d range=%s..%s; "
        "aligned=%d"
        % (
            len(official_rows),
            min(official_rows) if official_rows else None,
            max(official_rows) if official_rows else None,
            len(kernel_rows),
            min(kernel_rows) if kernel_rows else None,
            max(kernel_rows) if kernel_rows else None,
            len(common),
        )
    )

    divergences = 0
    first = None
    for relative in common:
        expected = multiset(official_rows[relative], with_pc=True)
        actual = multiset(kernel_rows[relative], with_pc=True)
        if expected != actual:
            divergences += 1
            if first is None:
                first = (relative, expected, actual)
    if first is not None:
        relative, expected, actual = first
        lines.append(
            "first divergence: relative_tick=%d script+pc expected=%s actual=%s"
            % (relative, key_text(expected), key_text(actual))
        )
    # 脚本级(不含 pc)单独统计:pc 差一步与"完全不同的任务"分开看。
    script_divergences = 0
    first_script = None
    for relative in common:
        expected = multiset(official_rows[relative], with_pc=False)
        actual = multiset(kernel_rows[relative], with_pc=False)
        if expected != actual:
            script_divergences += 1
            if first_script is None:
                first_script = (relative, expected, actual)
    if first_script is not None:
        relative, expected, actual = first_script
        lines.append(
            "first script-only divergence: relative_tick=%d expected=%s actual=%s"
            % (relative, key_text(expected), key_text(actual))
        )
    lines.append(
        "divergences: script+pc=%d/%d ticks, script-only=%d/%d ticks"
        % (divergences, len(common), script_divergences, len(common))
    )

    official_presence = presence(
        [(relative, official_rows[relative]) for relative in common], focus
    )
    kernel_presence = presence(
        [(relative, kernel_rows[relative]) for relative in common], focus
    )
    for script in focus:
        lines.append(
            "script %d presence: official=%s kernel=%s"
            % (script, official_presence[script], kernel_presence[script])
        )

    # 首个 script 级分歧的前后上下文(每边最多 --max-report 条)。
    if first_script is not None and len(common) > 1:
        relative = first_script[0]
        position = common.index(relative)
        start = max(0, position - args.max_report // 2)
        for item in common[start:start + args.max_report]:
            lines.append(
                "context relative_tick=%d official=%s kernel=%s"
                % (
                    item,
                    key_text(multiset(official_rows[item], with_pc=True)),
                    key_text(multiset(kernel_rows[item], with_pc=True)),
                )
            )

    report = "\n".join(lines) + "\n"
    print(report, end="")
    if args.out:
        with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(report)
    if len(common) < 2:
        return 2
    return 1 if divergences else 0


if __name__ == "__main__":
    sys.exit(main())
