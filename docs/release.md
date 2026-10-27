# 发布流程（tag → GitHub Release）

`.github/workflows/release.yml` 在推送版本 tag 时创建 GitHub Release，Release 正文
由 `tools/release/release_notes.py` 从 conventional commits 生成。更新日志按
`AGENTS.md` 的提交规范分组，不依赖 GitHub 自动生成的 release notes。

## 触发方式

| 方式 | 条件 |
|---|---|
| 推送 tag | `v[0-9]+.[0-9]+.[0-9]+*`（例如 `v0.1.0`、`v0.1.0-rc.1`） |
| 手动运行 | Actions → `release` → Run workflow，填写已存在的 tag；可勾选 `dry_run` |

工作流依次执行：

1. checkout 该 tag，`fetch-depth: 0`（更新日志需要完整历史与旧 tag）；
2. 校验 tag 与 `Cargo.toml` 的 `[workspace.package].version` 完全一致；
3. 运行发布工具自测（`tools/release/test_release_notes.py`）；
4. 生成 `.release/notes.md`；
5. `gh release create --verify-tag`；Release 已存在时改为 `gh release edit`
   （可重复运行，不会因为重跑失败）。

`dry_run=true` 时只跑 1–4 步并打印将执行的动作，不调用 `gh`。

## 版本纪律

发布前先把 `Cargo.toml` 的 `[workspace.package].version` 改成 tag 对应的版本并
提交，再打 tag：

```bash
# 例：准备 v0.2.0
# 1. 修改 Cargo.toml: [workspace.package] version = "0.2.0"
# 2. 提交后打 tag 并推送（是否执行由主人决定）
git tag v0.2.0
git push origin v0.2.0
```

tag 与 Cargo.toml 不一致时 workflow 在发布前直接失败，避免出现版本号与代码
不一致的 Release。

提交信息必须符合 `AGENTS.md` §4.1。生成器识别的类型：

| type | 分组 |
|---|---|
| `feat` | 新功能 |
| `fix` | 修复 |
| `perf` | 性能 |
| `refactor` | 重构 |
| `docs` | 文档 |
| `test` | 测试 |
| `build` | 构建 |
| `ci` | CI |
| `chore` | 杂项 |
| `style` | 格式 |
| `revert` | 回滚 |
| 其他 / 非 conventional | 其他变更 |

`!` 或提交正文里的 `BREAKING CHANGE:` / `BREAKING-CHANGE:` 会进入「破坏性变更」
分组并排在所有分组之前；合并提交（`--no-merges`）不计入日志。

## 本地验证（不碰 GitHub、不创建 tag）

发布逻辑全部在 `tools/release/release_notes.py`，workflow 只是薄壳，因此可以在
本地用同一份代码完整预演：

```bash
# 1. 发布工具回归测试（临时 git 仓库 + fake gh，无网络）
python tools/release/test_release_notes.py -v

# 1b. 按 workflow 的脚本步骤做端到端预演（临时仓库，无网络）
bash tools/release/rehearse_workflow.sh

# 2. tag 与 Cargo.toml 版本校验
python tools/release/release_notes.py check-version --tag v0.1.0

# 3. 预览更新日志
#    tag 尚不存在时用 --to HEAD 预览；已存在的 tag 可省略 --to
python tools/release/release_notes.py notes \
  --tag v0.1.0 --to HEAD --output .release/notes.md

#    只预览某一段提交
python tools/release/release_notes.py notes \
  --tag v0.1.0 --from <old-ref> --to HEAD --output .release/notes.md

# 4. 发布路径 dry-run（打印动作，不调用 gh）
YU_RELEASE_DRY_RUN=1 python tools/release/release_notes.py publish \
  --tag v0.1.0 --notes-file .release/notes.md
```

输出文件固定 UTF-8 + LF；Windows Git Bash、WSL Ubuntu 与 ubuntu-latest 的行为一致。
`.release/` 已加入 `.gitignore`。

可选的 workflow 静态检查：

```bash
# YAML / 表达式 / shell 检查（需自行安装 actionlint）
actionlint .github/workflows/release.yml

# 只解析 workflow 与事件，不启动容器
act -l -W .github/workflows/release.yml

# 容器级完整执行（需 Docker，且 tag 已推到远端；dry_run 不会创建 Release）
act workflow_dispatch -W .github/workflows/release.yml \
  --input tag=v0.1.0 --input dry_run=true \
  -P ubuntu-latest=catthehacker/ubuntu:act-latest
```

`act` 的容器执行会运行 `actions/checkout` 并拉取远端仓库，因此本地未推送的 tag
不能用于这一步；没有 Docker 守护进程时只能做 `act -l` 解析检查。脚本级测试
（第 1–4 步）不依赖 Docker，是本地验证的主路径。

## 设计取舍

- **不用 `generate_release_notes`**：GitHub 自动日志按它自己的规则排序，不按
  `AGENTS.md` 的类型分组；本仓库的提交规范比自动日志更严格。
- **上一个 tag 用 `git describe` 取最近可达 tag，而不是版本号最大者**：维护分支
  从旧 tag 拉出热修时，最近可达 tag 才是正确的比较基线。
- **tag 与 Cargo.toml 强校验**：Release 标题就是 tag，版本错位会误导使用者。
- **先 notes、后 create**：notes 生成失败时不会留下空 Release；重跑时已存在则
  edit，保持幂等。
- **只写 Release，不构建产物**：当前发布不含二进制；后续若加产物，复用同一份
  `.release/notes.md`，用 `gh release upload` 追加附件即可。

## 仓库设置

工作流在 job 级声明 `permissions: contents: write`。如果仓库的
Settings → Actions → General → Workflow permissions 被设为只读，`GITHUB_TOKEN`
仍然拿不到写权限，`gh release create/edit` 会失败；需要放开写权限，或改用具备
`contents: write` 的 token。

## 常见失败

| 现象 | 原因 / 处理 |
|---|---|
| `tag 与 Cargo.toml ... 不一致` | 先改 `[workspace.package].version` 并提交，再打 tag |
| `tag 'vX.Y.Z' 不存在` | 手动运行时 tag 还没推到远端；先 push tag |
| `gh release view` 报 401/403 | `GITHUB_TOKEN` 权限不足，见上一节 |
| Release 已存在 | 正常路径：workflow 会 `edit` 而不是报错 |
| 首次发布没有「完整变更」 | 仓库还没有上一个 `v*` tag，日志覆盖全部历史 |
