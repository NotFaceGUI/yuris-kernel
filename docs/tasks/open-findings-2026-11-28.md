# 提交审查挂账清单(2026-11-28)

> 来源:`95ab539`(2xx 支持)提交审查。已定性但仍需取证/裁定/补测的项
> 记录在此;已完成的快修见文末。**每项只写事实与下一步,不写猜测结论。**

## 挂账(按优先级)

| ID | 等级 | 位置 | 定性(事实) | 下一步 |
|---|---|---|---|---|
| P1 | 高(推送阻塞) | `0ff9d13`(perf 裁剪)+ `crates/yu-gfx/tests/eris_first_screen.rs` | 带 `YURIS_SAMPLE_ERIS_YSBIN` 跑 `yu-gfx --test eris_first_screen`:layers 199 正常,但 passes **74 vs golden 83**(golden 停在 `bb784df`,perf 裁剪后未更新);debug 构建 `snapshot_reads` 索引与全扫描不一致(`{269:291,271:294}` vs 空),裁剪/丢弃日志项后索引未同步 | 定位裁剪后仍应发射的 pass 集,更新 golden 并用像素/屏幕快照确认;裁剪路径统一走 `rebuild_log_indices` 或逐项 `forget_snapshot_read` |
| A2 | 中 | `crates/yu-rt/src/vm/handlers/variables.rs` `encode_scriptpos` / `scriptpos_wire_to_pc` | `code_offset_at` 失败时静默回退组下标:V2 下该分支理论不可达(exec 脚本必在缓存、pc 指向有效指令),但一旦触发会写出官方读不了的 pc;V5 的 pc 单位本身未取证,现按原值透传 | 对 V2 改为显式失败/诊断;补 `pc == group_count`(脚本尾)的 code 段末尾偏移映射;V5 待官方存档取证 |
| A3 | 中 | `crates/yu-save/src/profile.rs` `0x00C0` | 容器版本 `0xC0` 横跨 0.450–0.466(A=0x5F0)与 0.289(A=0x2B0),静态档案只能选一个;现靠盘上大学覆盖。**全新 2xx 目录无官方 `.sd` 时**会按 0x5F0 写缩略图,0.289 官方 LOAD 硬校验 A=0x2B0 | 裁定:`0xC0` 静态 `image_layout` 是否改为 `None`(跳过缩略图 + recoverable 诊断,失败闭合),或保留并记录已知限制 |
| B1 | 中 | `crates/yu-gfx/src/state.rs` `CG` 缺层分支 | 建层判据已收窄为**仅 `A` 在场**(0.495 首屏 483 条 `E/X/Y/MODE` 探测不得建层,已由 golden 钉住);`CG[ID A=0]` 建层仍只有 2xx 脚本行为证据,没有官方处理体 REA | 对 0.292/0.295 官方 exe 的 `CG` 处理体做 REA,确认缺层 + `A` 时是否建节点 |
| B3 | 中 | `crates/yu-save/src/state.rs` `refresh_slots` | `learned_btype0_var_id` 取最后一个文件、`learned_image_layout` 取第一个可判定块,结果依赖 `read_dir` 顺序;目录混有官方档与内核旧档时不确定 | 遍历前按文件名排序;var_id 做显式选举(有全 0 官方特征优先 Zero);refresh 无证据时清回 `None` |
| B4 | 低 | `crates/yu-render/core/src/texture.rs` `decode_bmp32_bgra` | 32bpp BI_RGB 一律按第 4 字节 alpha 解码;任意非 0 alpha 会全信。真实样本是 0/255 分布,但“第 4 字节未使用但残留垃圾”的资产会被解成半透明 | 收集更多样本;必要时加分布判据(仅 0/255 或双峰显著才启用 alpha) |
| B5 | 低 | `crates/yu-format/src/yssd.rs` `parse` | `+0x0C == 0` 一律尝试旧式记录容器;现有结构自检很严(id 升序、自描述长度、精确到文件尾),误判概率低 | 保持;如遇到误判样本,再加版本/魔数旁证 |
| C3 | 低 | `crates/yu-format/src/yssd.rs` `YssdEntry` | `compressed` 与 `raw` 二选一的不变量只靠注释 | 改为 `Payload` enum,或在 parse/write 处 `debug_assert` 恰有一个 |
| C4 | 低 | `crates/yu-rt/src/sysvar_abi.rs` | 对齐代价魔数(改型 ≤4、基线 +64)有文档,但缺“单槽 dims 变化必须派生到正确族”的正例;前缀 `ty != 0` 截断规则缺 sanity 检查 | 补正例与前缀长度合理性断言;记录边界样本 |
| D3 | 低 | `docs/divergences/DIV-GFX-099` | 文档已更正后端“读源层”事实;但 ID2 滤镜仍缺**像素级**回归(现只有 op 元数据与手工帧) | soft 后端补单测:源层有像素、目标空白,断言旋转后目标非空且源不变 |
| D6 | 低 | `crates/yu-vfs/tests/real_sample.rs` | 新增真实包用例缺样本时 `skip()` 打印后 return,按 AGENTS 属“空跑通过” | 统一到 `YURIS_REQUIRE_SAMPLES=1` 口径(与 `yssd_legacy.rs` 一致) |
| E2 | 低 | `crates/yu-rt/tests/fixture/variables.rs` | SCRIPTPOS 测试的 GOSUB 帧 `return_script` 都是同一脚本,未覆盖跨脚本换算 | 补跨脚本 GOSUB 帧用例 |
| E3 | 低 | 存档交换 | 0.289 官方档 pc=1698 ↔ 组 61 目前是手工取证 | 做 `YURIS_SAMPLE_*` 门控测试:解官方档 DNO10,断言 `pc == code_offset_at(pc_covering(pc))` 且 DNO4 文本对齐 |

## 本轮已修(不再挂账)

- **A1 路径逃逸**:`confined_path` / `save_root_path` 拒绝绝对/盘符/UNC
  形式(目标不存在时 `join` 会替换根);补了根外真实文件的回归用例。
- **B2 边距别名**:`LEFT/RIGHT/TOP/BOTTOM` 只在 `RESIZE` 生效;
  `cgact_margin_aliases_are_resize_only` 覆盖。
- **CG 建层判据收窄**:从“任一状态参数”收为**仅 `A`**;0.495 ERIS 首屏
  483 条 `CG[ID E/X/Y/MODE]` 探测恢复不建层(271 → 199),并新增反例断言。
- **C1 布局优先级重复**:`build_container` 统一走
  `effective_image_layout`。
- **D2**:补 `DIV-RT-083-packed-0263-exe.md`(0.263 加壳残余)。
- **D4/D5**:V263 注释自相矛盾、DIV-RT-017 占位空行已清理。
- **测试环境脆弱**:`confined_path_accepts_cp932_projected_root` 依赖
  路径含中文,原来用纯 ASCII 目录名会失去意义;改用含汉字的目录名。
