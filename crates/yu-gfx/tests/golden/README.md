# yu-gfx golden 说明

- `eris_first_screen_display.json`:真实 ERIS 首屏(第 2 次 tick 后)的
  `json::to_display_json`。样本来自官方 YSCom 0.495 编译产物。
- `synthetic_group_layers_display.json`:合成 fixture(组/过渡/模式)。

## 样本身份(2026-10-20 实测)

| 文件 | sha256 |
|---|---|
| `save/data/ysbin/ysc.ybn` | `741d4caf8c40846b07b20d05f3e9777bf65ec9251146b71c07235d7b23539e08` |
| `save/data/ysbin/yscfg.ybn` | `f451d3a70741fde290c428d73cd109fb122d332bc11e800f92417e69f763f4a3` |

## 生成 / 更新

```bash
export TEMP="<side-repo>/workbench/tmp"; TMP="$TEMP"
export YURIS_SAMPLE_ERIS_YSBIN="<side-repo>/workbench/project-eris/save/data/ysbin"
export YURIS_REQUIRE_SAMPLES=1

UPDATE_GFX_GOLDEN=1 cargo test -p yu-gfx --test eris_first_screen --test display_list_golden
cargo test -p yu-gfx
```

`UPDATE_GFX_GOLDEN=1` 只写文件、不做断言,是唯一生成入口;默认模式逐字节比较。
更新前必须先审阅 census 断言(`tests/eris_first_screen.rs`),不得用更新掩盖差异。
