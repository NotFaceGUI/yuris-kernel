# DIV-GFX-081 · 采样 bind group 缓存钉住历史层表面导致 D3D12 OOM

**状态**:`fixed`(2026-11-08)

## 症状

0.494 商业包(`kotoneiro`)按「标题 → 新游戏 → 推进到立绘 → TITLE →
再新游戏」往返,第二轮后差分/表情切换时:

- 角色立绘整层消失(只剩背景);
- 或表情消失、立绘还在;
- `[spl]` 探针报 `SPLBUFb601/701 = 1x1(decl 1332x3351)`;
- 桌面 wgpu 日志出现
  `[wgpu] 分配失败 transition surface 1332x3351: Out of Memory`,
  之后同尺寸 `asset texture` 也创建失败。

窗口截图证据:`target/tmp/debug-ui/probe11`(立绘消失窗口)、
`target/tmp/ktn/play3.out`(`transition surface 1332x3351` OOM,
engine frame≈19452)。

## 官方语义(对齐目标)

官方 `CGACT` 在命令到达时把结果写进**层表面**并替换层对象的表面指针
(`FUN_0042f809` 尾部 `*(iVar4+0x90) = local_68`)。旧表面在无人引用时
立即释放;引擎没有「按纹理句柄缓存的采样 bind group」这一层,所以
**表面生命周期结束 = 显存回收**。

## 我们的实现与分歧

内核为了跨帧复用命令日志,采用「常驻层表面 + 每帧重放命令序」:
过渡/差分纯写动作每帧写入一张**新**纹理(`surfaces.acquire`),旧
`Content` 被替换后释放。

`PipelineCache::ensure_bind_group` 为性能按 `(texture_id, filter)` 缓存
采样 bind group。旧实现只在两类生命周期事件里 `forget_texture`:

- `TargetSet` 的屏幕/离屏/合成/快照目标重建;
- `TextureCache` 的资产/临时上传 evict 或 `remove`。

**图层表面(`LayerSurfaces`)不在其中。** 每次差分/眨眼换一张表面纹理,
屏幕 pass 都会为它建一个 bind group;表面被替换后 bind group 仍留在
缓存里,其 `wgpu::TextureView` 钉住底层 D3D12 纹理 ⇒ 显存不随
`target_bytes/live_bytes` 统计增长,而是随「换过多少张表面」无限增长。
长会话(实测 engine frame≈19452)后 `create_texture(1332x3351)` 返回
`Out of Memory`,过渡 op 被跳过;目标是 `CG[FILE=""]` 的 1×1 占位层,
于是 `SPLBUFb` 退化 1×1,`SIZE2` 把 1 个透明像素拉伸到 643×825 ⇒
立绘/表情整层消失。

## 修复

帧末在 `encode_passes` **之后**按存活纹理集合剪枝 bind group 缓存:

- `LayerSurfaces::live_texture_ids`:各层当前 `content` / 文本基面 `raw`;
- `TargetSet::live_texture_ids`:屏幕/离屏/合成/快照/零拷贝采样窗;
- `TextureCache::live_texture_ids`:全部已上传资产/影片/临时纹理;
- `TextUploads::live_texture_ids`:文本层与屏幕文本覆盖纹理;
- `PipelineCache::retain_textures`:丢弃不在集合内的 `bind_groups` /
  `scale_groups`。

必须在 encode 之后:本帧规划期的临时表面 bind group 在编码时仍被命令
缓冲引用;提前剪会丢绘制。

## 取证与门禁

```text
cargo test -p yu-backend-wgpu
  → 13 + 15 + 39 + 1 + 1 passed
crates/yu-render/wgpu/tests/surfaces.rs::
  rewritten_transition_surfaces_do_not_pin_bind_groups
  → 保留 retain:pass;注释掉 retain:FAILED(bind group 随帧数线性增长)

桌面 wgpu 10 轮标题循环,22500 帧(YURIS_RES_PROBE=1):
  → 0 次 OOM;bg 45..70 有界(旧实现随历史表面增长)
  → [res] assets=509MB/57files/evicted=202 live=22/111MB snap=71
          bg=49 vram=224MB;avg 59.5fps
  → 越过旧故障点(engine frame 19452)无退化
```

`[res]` 新增 `bg=` 字段(采样 bind group 缓存数),作为后续长会话回归
的现场指标。

## 残留

- 该修复只回收「采样 bind group」这一条隐藏引用链;若未来新增其它按
  `texture_id` 缓存 GPU 对象的表,必须加入同一存活集合。
- 官方在过渡期仍是每帧新建表面(官方也在命令时刻分配),我们的
  `surfaces.acquire` 仍会产生大表面 churn;本轮不改成 in-place,
  以保持命令时刻像素语义(EVID-T-15)。
