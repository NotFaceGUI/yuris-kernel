# DIV-GFX-053 · 复制源层同帧被 `CGEND` 删除导致快照丢失

- 状态: **fixed(AU1,2026-10-22)**
- 优先级: 高(ERIS 全部按钮状态快照)
- 发现日期 / 方式: 2026-10-22 / `gfx_dump` copy log 与 `frame()` pass 数不一致

## 现象

`eris_dump` 的 copy log 有 69 条,但 `frame()` 只输出 1 条:

```text
copy log (69):
  copy 271 -> :/W0/CGS100
  copy 272 -> :/W0/BT.OFFES.GAMEMAIN.TIP.CLICK=0=1
  copy 273 -> :/W0/BT.OVERES.GAMEMAIN.TIP.CLICK=0=1
  ...
  copy 339 -> :/W0/BT.OFFES.GAMEMAIN.BTN.MENU=0=1

frame 1: passes=14 ops=17        ← 68 条被丢弃
```

## 根因

`GfxState::frame()` 的可达性检查:

```rust
let source_live = match &c.source {
    CopySource::Layer(id) | CopySource::Group(id) => self.nodes.contains_key(id),
};
if dest_live && source_live { ... }
```

ERIS 按钮构造流程(`es_Button.yst:2377-2682`):

```text
CG[ID="BT.OFF.TMP" Z=1 FILE=$BT.OFF(@mLC)]      ← 一张多状态合图
CG[ID=$off Z=... FILE=""]                        ← 每个按钮状态建一个空层
CGINFO[ID="BT.OFF.TMP" SX=1 LET=@sx];@sx/=4
CGACT[ID="BT.OFF.TMP" ID2=$off COPY2=1 X=0 X2=@sx-1]   ← 切片复制
... (over/on/onover/na 同理,共 68 条) ...
CGEND[ID="BT.OFF.TMP"]                           ← 全部按钮建完后删源层
```

官方在**命令到达时**立即执行复制(EVID-T-15),所以源层随后被删不影响结果;
本内核把纹理 pass 留到帧末回放,那时 `nodes` 里已经没有 `BT.OFF.TMP`。

## 修法

`CopyOp` 增加 `source_texture: Option<TextureRef>`:在 `cgact_copy` 处理命令时
记下源层**当时**的 `TextureRef`(Asset/Snapshot/SolidBlack1x1);
`frame()` 的可达性改为“目标仍引用快照 && (源层还在 || 有命令时刻纹理)”。

两端 `layer_surface` / `layer_image` 在找不到图层节点时回退到
`copy.source_texture`;都没有才报 `gfx.copy.source.gone`。

## 回归

- 首屏 census:`texture pass 12 → 80`(69 copy + 11 image)、`ops 17 → 85`;
- `crates/yu-backend-soft/tests/render.rs::copy_slices_full_height_after_source_layer_is_gone`;
- `crates/yu-gfx/tests/golden/eris_first_screen_display.json` 已按新 census 重生成;
- 600 帧 soft/wgpu 0 差异,title 稳定帧 `a5a618d0…2483` 不变。

## 关联

同批修掉的 `DIV-GFX-054`(COPY 区域缺省坐标应为整幅)是同一段 dump 暴露的:
旧实现把未指定的 `Y/Y2` 填 0,只复制首行。
