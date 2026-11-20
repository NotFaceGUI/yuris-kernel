# DIV-GFX-090 · ONMOUSE 对隐形层(`E=0`)全透明像素穿透采样下层可见内容

- 状态: fixed(2026-11-20,主人实机确认 hover/点击恢复)
- 优先级: 高(0.479 两个商业样本的存/读档界面槽位完全无法交互)
- 发现日期 / 发现方式: 2026-11-20 / 主人实机:存档页槽位 hover 无高亮、
  点击无效;同游戏标题按钮正常。

## 复现

- 0.479 商业样本 A/B 进入 Save 界面,鼠标悬停槽位:官方 exe 金色边框高亮、
  可点击;本内核无任何反应(其他游戏正常)。
- 锚点:`es_Button.yst` 的 `es.BT.MOUSECHECK`:
  `CGINFO[ID="BT.MASK"… ONMOUSE=1 LET=@col]` → `@over=(@col>=@BT.MAP.A(@no))`;
  游戏脚本(`s99` pc83/84 等)对每个槽位设
  `es.BT.MAP.CG.SET "saveload/btn_plate_back_mask"` +
  `es.BT.MAP.A.SET 1`。

## 官方行为(真值层级 1 + 3)

- 官方实机该界面 hover 高亮(主人提供截图);
- 手册 `cg/cginfo.html` 例[8]:ONMOUSE 返回"该坐标图层的 RGB 值",
  命中矩形外为 `-1`;
- **实测资产**:`cgsys/saveload/btn_plate_mask.png` 与
  `btn_plate_back_mask.png` 是 **100% 全透明占位图**(YCG method 1
  两段 zlib,解出 544×133×4 全零;1067 条目的 `cgsys.ypf` 中仅此一份,
  无覆盖包)。`btn_plate_off/back_off` 同样全零;`btn_plate_over/on`
  才是带边框的真图(15.7% 非零)。
- 官方脚本把 mask 层建为 `E=0`(不可见,只参与命中查询)。

由「官方 hover 正常 + mask 全零 + `MAP.A=1`」推出:官方对这块隐形层
读到的 `@col ≥ 1`。非零的唯一来源是该位置**下方合成出的可见内容**
(槽位边框由背景层 `back_save` 绘制)。即官方 ONMOUSE 对隐形层读到的是
**穿透后的结果**,不是该层自身位图。

## 内核行为

`GfxState::query_onmouse` 直接采样查询层自身的纹理:mask 层全零 ⇒
`@col=0` ⇒ `0>=1` 假 ⇒ `@over` 恒假 ⇒ 点击分支永不进入。

## 修法

`query_onmouse` 在**隐形层**(`visible=false`)且采样到全透明像素
`(0,0,0,0)` 时,调用 `sample_beneath` 近似官方穿透:

- 取**绘制序严格在查询层之前**(`(z, seq) < (top.z, top.seq)`)、同坐标
  命中矩形的**可见**图像层,按 `(z, seq)` 降序(最近在下者先);
- 返回首个非零像素;`RECTPAINT` 实色层(`fill`)命中且非全零时直接取实色;
- 快照层按源资产采样(与主路径同口径:区域原点 + 显示尺寸映射)。

边界:

- `COLOR` 不走穿透(语料钉值:es_Button pc2020 期望 `0x00808080`);
- 只对 ONMOUSE/ONMOUSE2/ONMOUSE3 生效,且仅在「隐形 + 全透明」时触发。

全语料核验无回归:kotoneiro mask 全不透明、v555-2 号样本 mask 为 RGB
无 alpha(不透明),均不进入该路径。

## 残余

- 官方若确为**屏幕合成采样**,本实现的逐层近似在多层半透明叠加下有
  差异(本用例只需非零判定,不影响);等 B7 宿主合成面落地后改为直接
  采样屏幕表面。
- 穿透目标无层可命中时保持原值 0(与旧行为一致)。

## 回归

- 主人实机(0.479 商业样本 A/B):存档/读档界面槽位 hover 高亮、点击
  进入保存流程正常。
- `cargo test -p yu-gfx`(含 `info_query` 30 项)全绿;
  `cargo build --workspace --all-targets` 通过。
