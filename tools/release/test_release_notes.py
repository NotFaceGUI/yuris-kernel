#!/usr/bin/env python3
"""tools/release/release_notes.py 的本地回归测试。

运行：
    python tools/release/test_release_notes.py -v

测试用临时 git 仓库覆盖 tag 区间、首个版本、分组、破坏性变更、
非 conventional 提交、合并提交、版本校验与 Release 创建/更新分支。
workflow 静态检查需要 PyYAML；未安装时显式跳过那一项。
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"

sys.path.insert(0, str(HERE))
import release_notes as rn  # noqa: E402


class GitRepoTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="yu-release-test-")
        self.repo = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "release-test@example.invalid")
        self.git("config", "user.name", "Release Test")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")
        self.branch = self.git("symbolic-ref", "--short", "HEAD").strip()
        self._counter = 0

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def git(self, *args: str) -> str:
        proc = subprocess.run(
            ["git", "-C", str(self.repo), *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if proc.returncode != 0:
            raise AssertionError(
                f"git {' '.join(args)} 失败: {proc.stderr.strip()}"
            )
        return proc.stdout

    def commit(
        self, subject: str, body: str | None = None, filename: str = "history.txt"
    ) -> str:
        self._counter += 1
        (self.repo / filename).write_text(
            f"{self._counter}\n", encoding="utf-8"
        )
        self.git("add", filename)
        args = ["commit", "-q", "-m", subject]
        if body is not None:
            args += ["-m", body]
        self.git(*args)
        return self.git("rev-parse", "HEAD").strip()

    def tag(self, name: str, annotated: bool = False) -> None:
        if annotated:
            self.git("tag", "-a", name, "-m", name)
        else:
            self.git("tag", name)

    def notes(self, tag: str, **kwargs) -> str:
        text, _, _ = rn.build_notes(self.repo, tag, **kwargs)
        return text


class NotesTests(GitRepoTest):
    def test_groups_sections_and_breaking_changes(self) -> None:
        self.commit("feat(rt): 首个功能")
        self.tag("v0.1.0")
        self.commit("fix(gfx): 修复渲染")
        self.commit("feat(ui)!: 重做布局", "BREAKING CHANGE: 布局 API 不再兼容")
        self.commit("docs: 补充说明")
        self.commit("随手提交")
        self.tag("v0.2.0")

        text = self.notes("v0.2.0")

        self.assertIn("**完整变更**: v0.1.0...v0.2.0", text)
        self.assertIn("### 破坏性变更", text)
        self.assertIn("`ui`: 重做布局", text)
        self.assertIn("BREAKING CHANGE: 布局 API 不再兼容", text)
        self.assertIn("### 修复 (fix)", text)
        self.assertIn("`gfx`: 修复渲染", text)
        self.assertIn("### 文档 (docs)", text)
        self.assertIn("### 其他变更", text)
        self.assertIn("随手提交", text)
        self.assertNotIn("首个功能", text)
        self.assertLess(
            text.index("### 破坏性变更"), text.index("### 修复 (fix)"),
            "破坏性变更必须排在普通分组之前",
        )

    def test_first_release_has_no_compare_link(self) -> None:
        self.commit("feat: 初始功能")
        self.tag("v0.1.0")

        text = self.notes("v0.1.0")

        self.assertIn("首个版本", text)
        self.assertNotIn("完整变更", text)
        self.assertIn("### 新功能 (feat)", text)
        self.assertIn("初始功能", text)

    def test_previous_tag_is_nearest_not_highest_version(self) -> None:
        self.commit("feat: 起点")
        self.tag("v0.10.0")
        self.commit("feat: 中间")
        self.tag("v0.9.0")
        self.commit("fix: 修复")
        self.tag("v0.11.0")

        text = self.notes("v0.11.0")

        self.assertIn("**完整变更**: v0.9.0...v0.11.0", text)
        self.assertNotIn("v0.10.0...v0.11.0", text)

    def test_annotated_tags_are_used_as_baseline(self) -> None:
        self.commit("feat: 基线")
        self.tag("v0.1.0", annotated=True)
        self.commit("fix: 修复")
        self.tag("v0.2.0", annotated=True)

        text = self.notes("v0.2.0")

        self.assertIn("**完整变更**: v0.1.0...v0.2.0", text)

    def test_merge_commits_are_skipped(self) -> None:
        self.commit("feat: 基线")
        self.tag("v0.1.0")
        self.git("checkout", "-q", "-b", "side")
        self.commit("fix: 支线修复", filename="side.txt")
        self.git("checkout", "-q", self.branch)
        self.commit("feat: 主线功能", filename="main.txt")
        self.git("merge", "-q", "--no-ff", "side", "-m", "Merge branch 'side'")
        self.tag("v0.2.0")

        text = self.notes("v0.2.0")

        self.assertIn("支线修复", text)
        self.assertIn("主线功能", text)
        self.assertNotIn("Merge branch", text)

    def test_special_characters_are_escaped(self) -> None:
        self.commit("feat: 基线")
        self.tag("v0.1.0")
        self.commit("fix(core): 处理 Vec<T> & A&B")
        self.tag("v0.2.0")

        text = self.notes("v0.2.0")

        self.assertIn("Vec&lt;T&gt; &amp; A&amp;B", text)

    def test_empty_range_reports_no_changes(self) -> None:
        self.commit("feat: 唯一提交")
        self.tag("v0.1.0")

        text = self.notes("v0.1.0", from_ref="v0.1.0")

        self.assertIn("本版本没有代码变更。", text)


class RepoUrlTests(GitRepoTest):
    def test_commit_and_compare_links(self) -> None:
        self.commit("feat: 基线")
        self.tag("v0.1.0")
        self.commit("fix: 修复")
        self.tag("v0.2.0")

        text = self.notes("v0.2.0", repo_url="https://github.com/example/repo")

        self.assertIn(
            "https://github.com/example/repo/compare/v0.1.0...v0.2.0", text
        )
        self.assertIn("https://github.com/example/repo/commit/", text)


class VersionTests(GitRepoTest):
    def write_manifest(self, version: str | None) -> None:
        body = "[workspace]\nmembers = []\n"
        if version is not None:
            body += f'\n[workspace.package]\nversion = "{version}"\n'
        (self.repo / "Cargo.toml").write_text(body, encoding="utf-8")

    def test_matching_version(self) -> None:
        self.write_manifest("0.1.0")
        rn.check_version(self.repo, "v0.1.0")

    def test_mismatch_raises(self) -> None:
        self.write_manifest("0.1.0")
        with self.assertRaises(rn.ReleaseError) as ctx:
            rn.check_version(self.repo, "v0.2.0")
        self.assertIn("不一致", str(ctx.exception))

    def test_missing_workspace_version_raises(self) -> None:
        self.write_manifest(None)
        with self.assertRaises(rn.ReleaseError):
            rn.check_version(self.repo, "v0.1.0")


FAKE_GH = r"""
import json, os, sys

mode = os.environ.get("FAKE_GH_MODE", "absent")
args = sys.argv[1:]
with open(os.environ["FAKE_GH_LOG"], "a", encoding="utf-8") as fh:
    fh.write(json.dumps(args, ensure_ascii=False) + "\n")

if args[:2] == ["release", "view"]:
    if mode == "present":
        print("title: fake")
        sys.exit(0)
    if mode == "error":
        sys.stderr.write("HTTP 401: Bad credentials\n")
        sys.exit(1)
    sys.stderr.write("release not found\n")
    sys.exit(1)
sys.exit(0)
"""


class PublishTests(GitRepoTest):
    def setUp(self) -> None:
        super().setUp()
        self.fake_gh = self.repo / "fake_gh.py"
        self.fake_gh.write_text(FAKE_GH, encoding="utf-8")
        self.gh_log = self.repo / "gh.log"
        self.notes_file = self.repo / "notes.md"
        self.notes_file.write_text("## 更新内容\n\n- 测试\n", encoding="utf-8")
        self.gh_cmd = [sys.executable, str(self.fake_gh)]
        self._old_env = dict(os.environ)
        os.environ["FAKE_GH_LOG"] = str(self.gh_log)

    def tearDown(self) -> None:
        os.environ.clear()
        os.environ.update(self._old_env)
        super().tearDown()

    def calls(self) -> list[list[str]]:
        return [
            json.loads(line)
            for line in self.gh_log.read_text(encoding="utf-8").splitlines()
        ]

    def test_creates_when_release_missing(self) -> None:
        os.environ["FAKE_GH_MODE"] = "absent"
        rn.publish("v0.1.0", self.notes_file, gh_cmd=self.gh_cmd)

        self.assertEqual(self.calls()[-1][:3], ["release", "create", "v0.1.0"])
        self.assertIn("--verify-tag", self.calls()[-1])
        self.assertNotIn("--prerelease", self.calls()[-1])

    def test_edits_when_release_exists(self) -> None:
        os.environ["FAKE_GH_MODE"] = "present"
        rn.publish("v0.1.0", self.notes_file, gh_cmd=self.gh_cmd)

        self.assertEqual(self.calls()[-1][:3], ["release", "edit", "v0.1.0"])
        self.assertIn("--draft=false", self.calls()[-1])
        self.assertIn("--prerelease=false", self.calls()[-1])

    def test_prerelease_tag_is_created_as_prerelease(self) -> None:
        os.environ["FAKE_GH_MODE"] = "absent"
        rn.publish("v0.2.0-rc.1", self.notes_file, gh_cmd=self.gh_cmd)

        self.assertIn("--prerelease", self.calls()[-1])
        self.assertNotIn("--prerelease=false", self.calls()[-1])

    def test_unexpected_view_error_raises(self) -> None:
        os.environ["FAKE_GH_MODE"] = "error"
        with self.assertRaises(rn.ReleaseError):
            rn.publish("v0.1.0", self.notes_file, gh_cmd=self.gh_cmd)

    def test_dry_run_does_not_call_gh(self) -> None:
        rn.publish("v0.1.0", self.notes_file, dry_run=True, gh_cmd=self.gh_cmd)
        self.assertFalse(self.gh_log.exists())


class CliTests(GitRepoTest):
    def test_output_file_is_utf8_lf(self) -> None:
        self.commit("feat: 基线")
        self.tag("v0.1.0")
        self.commit("fix: 修复中文")
        self.tag("v0.2.0")
        out = self.repo / "notes.md"

        subprocess.run(
            [
                sys.executable,
                str(HERE / "release_notes.py"),
                "notes",
                "--repo",
                str(self.repo),
                "--tag",
                "v0.2.0",
                "--output",
                str(out),
            ],
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

        raw = out.read_bytes()
        self.assertNotIn(b"\r\n", raw)
        self.assertIn("修复中文".encode("utf-8"), raw)

    def test_check_version_cli_fails_on_mismatch(self) -> None:
        (self.repo / "Cargo.toml").write_text(
            '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.1.0"\n',
            encoding="utf-8",
        )
        proc = subprocess.run(
            [
                sys.executable,
                str(HERE / "release_notes.py"),
                "check-version",
                "--repo",
                str(self.repo),
                "--tag",
                "v0.2.0",
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(proc.returncode, 1)
        self.assertIn("不一致", proc.stderr)


class WorkflowTests(unittest.TestCase):
    def test_workflow_shape(self) -> None:
        try:
            import yaml
        except ImportError:
            self.skipTest("PyYAML 未安装")

        self.assertTrue(WORKFLOW.is_file(), f"缺少 {WORKFLOW}")
        data = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        # PyYAML 按 YAML 1.1 会把未加引号的 on 解析成布尔 True。
        triggers = data.get("on", data.get(True))
        self.assertIsInstance(triggers, dict)
        self.assertEqual(triggers["push"]["tags"], ["v[0-9]+.[0-9]+.[0-9]+*"])
        self.assertIn("workflow_dispatch", triggers)

        self.assertEqual(data["permissions"]["contents"], "read")
        job = data["jobs"]["release"]
        self.assertEqual(job["permissions"]["contents"], "write")
        self.assertIn("concurrency", data)

        steps = job["steps"]
        by_name = {step.get("name"): step for step in steps}
        checkout = by_name["Checkout repository"]
        self.assertEqual(checkout["with"]["fetch-depth"], 0)
        self.assertFalse(checkout["with"]["persist-credentials"])

        scripts = "\n".join(step.get("run", "") for step in steps)
        self.assertIn("release_notes.py check-version", scripts)
        self.assertIn("release_notes.py notes", scripts)
        self.assertIn("release_notes.py publish", scripts)
        self.assertIn("test_release_notes.py", scripts)

        # run 脚本只消费 env，不直接插值表达式，避免 tag 输入造成脚本注入。
        for step in steps:
            self.assertNotIn("${{", step.get("run", ""), step.get("name"))

        publish = by_name["Create or update GitHub release"]
        self.assertEqual(publish["env"]["GH_TOKEN"], "${{ github.token }}")
        self.assertIn("YU_RELEASE_DRY_RUN", publish["env"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
