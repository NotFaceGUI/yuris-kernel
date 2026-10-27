#!/usr/bin/env bash
# 本地预演 .github/workflows/release.yml 的脚本步骤。
#
# 在临时 git 仓库里跑：tag 校验 -> Cargo 版本校验 -> notes 生成 ->
# publish dry-run。不访问 GitHub、不创建 tag、不改动当前工作区。
#
# 用法：
#   bash tools/release/rehearse_workflow.sh
#   PY=python bash tools/release/rehearse_workflow.sh   # Windows Git Bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"

# Windows Git Bash 可能把 python3 解析成应用商店空壳（command -v 能查到但执行失败），
# 所以这里探测真正可用的 Python，而不是只检查命令是否存在。
if [ -n "${PY:-}" ]; then
  if ! "$PY" -c 'import tomllib' >/dev/null 2>&1; then
    echo "PY=$PY 不可用（需要 Python 3.11+）" >&2
    exit 1
  fi
else
  for candidate in python3 python; do
    if "$candidate" -c 'import tomllib' >/dev/null 2>&1; then
      PY="$candidate"
      break
    fi
  done
  if [ -z "${PY:-}" ]; then
    echo "找不到可用的 Python 3.11+；可用 PY=... 指定" >&2
    exit 1
  fi
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
git -c init.defaultBranch=main init -q "$tmp"
cd "$tmp"
git config user.email release@example.invalid
git config user.name "Release Test"
git config commit.gpgsign false
git config tag.gpgsign false
git config core.autocrlf false

printf '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.2.0"\n' > Cargo.toml
git add Cargo.toml
git commit -q -m "feat: 初始功能"
git tag v0.1.0

printf 'one\n' > a.txt
git add a.txt
git commit -q -m "fix(gfx): 修复渲染"
printf 'two\n' > b.txt
git add b.txt
git commit -q -m "feat(ui)!: 重做布局" -m "BREAKING CHANGE: 布局 API 改了"
git tag v0.2.0

# --- 以下与 .github/workflows/release.yml 的步骤一一对应 ---
INPUT_TAG=v0.2.0
[[ "$INPUT_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]
git rev-parse --verify --quiet "refs/tags/$INPUT_TAG^{commit}" >/dev/null

"$PY" "$repo_root/tools/release/release_notes.py" check-version --tag "$INPUT_TAG"

mkdir -p .release
"$PY" "$repo_root/tools/release/release_notes.py" notes \
  --tag "$INPUT_TAG" --output .release/notes.md

grep -q "完整变更" .release/notes.md
grep -q "### 破坏性变更" .release/notes.md
grep -q "### 修复 (fix)" .release/notes.md

YU_RELEASE_DRY_RUN=1 "$PY" "$repo_root/tools/release/release_notes.py" publish \
  --tag "$INPUT_TAG" --notes-file .release/notes.md

echo "===== generated notes ====="
cat .release/notes.md
echo "===== rehearsal OK ====="
