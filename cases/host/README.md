# `cases/host/` · 桌面宿主复现脚本(P1)

这里的 `*.json` **不是** `yu-cli case` 的用例,而是 `yu-desktop --script`
的场景脚本(格式见 `crates/yu-platform/host-desktop/src/script.rs`)。

`yu-cli case` 只读 `cases/` 顶层(`list_cases` 不递归),所以本目录不会
进入 `case --all` 门禁;真正进用例门禁的症状回归是 `cases/symptom-*.json`。

| 脚本 | 剧本 | 驱动方式 |
|---|---|---|
| `symptom-1-title-settings-back.json` | 标题 → 設定 → 戻る | 宿主指针 |
| `symptom-2-enter-story.json` | 标题 → スタート(进本篇) | 宿主指针 |
| `symptom-3-ingame-buttons.json` | 本篇右键菜单 / オート / スキップ / 設定 | 宿主指针 |
| `symptom-4-save-load.json` | 本篇 セーブ / ロード | 宿主指针 |
| `symptom-5-close-twice.json` | 窗口关闭两次 | `WindowEvent::CloseRequested` 同路径 |
| `symptom-6-stress.json` | resize / 最小化 / 还原 / 失焦 / 连点 / 滚轮 | 真 winit 调用 + 宿主失焦路径 |
| `symptom-7-idle.json` | 长时间挂机(桌面 60s 快速验证) | 无输入 |

运行(证据产物只写 `target/tmp`):

```bash
cargo build --release -p yu-host-desktop
target/release/yu-desktop.exe <project-eris> --backend wgpu --debug-ui \
    --script cases/host/symptom-5-close-twice.json \
    --log-file target/tmp/p1/close-twice.jsonl \
    --shot target/tmp/p1/close-twice.png
```

`script.frames` 在未给 `--frames` 时作为宿主帧上限,跑完自动退出。
