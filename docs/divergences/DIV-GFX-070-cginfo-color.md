# DIV-GFX-070 · `CGINFO[COLOR]` 未实现,按钮占位图重绘判定永远拿不到像素

- 状态: fixed
- 优先级: 中
- 发现日期 / 发现方式: 2026-10-30 / 0.481 商业包运行期诊断
  (`UNIMP [gfx] gfx.info.unsupported … CGINFO[COLOR] 需要像素/未分类语义` ×7)

## 复现

- 游戏位置: 0.481 商业包,多包入口 `pac/`(base 7 + update 2)。
- 锚点: `data\script\eris\es_button.yst` s0 pc2023 / pc2087 / pc2143 /
  pc2187 / pc2243 / pc2273(引导期 7 条,同一形态)。
- 站点字节(s0 pc2023):
  ```text
  CGINFO  w0 ID    = $0x5d4
          w1 COLOR = 1
          w2 LET   = @0x5d2
          w3 SET   = 0
          w4 SET2  = 0
  ```

## 官方行为

- **取值形态由语料自证**(s0 pc2024 条件窗口):
  ```text
  read @0x5d0 == 1  &&  read @0x5d1 == 1  &&  read @0x5d2 == 0x00808080
  ```
  `@0x5d0/@0x5d1` 是同一层的 `CGINFO[SX]/[SY]`,`@0x5d2` 是 `CGINFO[COLOR]`;
  命中即 `CGACT[RECTPAINT3]` 重绘。`0x00808080` = `128 | 128<<8 | 128<<16`
  = 50% 灰 ⇒ **COLOR 的返回值就是 `r | g<<8 | b<<16`**,与 `ONMOUSE` 同打包。
- **引擎**:像素采样体 `0x43ddc0..0x43de86`(0.495 `yu-ris.exe`,
  sha256 `4f5eefa3…`)的坐标对来自 `[obj+0x58]/[obj+0x5c]` 或
  `[obj+0x60]/[obj+0x64]`,由全局 `[0x68417d]` 切换(鼠标坐标 / 显式
  `SET/SET2`);显式分支缺省走 `XOR ECX,ECX; XOR EDX,EDX`(坐标 0,0);
  越界 `MOV ECX,-1`(`0x43de4b`)。

## 内核行为

`CGINFO` 的 flag 分支只列了 `ONMOUSE/ONMOUSE2/ONMOUSE3`,其余一律
`INFO_UNSUPPORTED`。`query_onmouse` 本身**已经**支持 `set/set2` 覆盖坐标
(只是没有 flag 走到它),所以 COLOR 只需要接上分支。

## 修法

- `crates/yu-gfx/src/state.rs`:`"COLOR"` 分支走同一采样器,坐标取
  `SET/SET2`(缺省 0,0,不继承鼠标);`query_onmouse` 的返回分支对 COLOR
  落 `rgb`。
- 顺带补两处**尺寸来源**(COLOR/ONMOUSE 都需要命中矩形):
  1. `query_onmouse` 的尺寸优先级补上「VM 回填的资产尺寸」(`asset_sizes`),
     与 `query_layer_size` 同一张表;
  2. 资产**不存在**时按 `0×0` 回填而不是报错 —— 官方 `CGINFO[SX/SY]` 直接
     读图层 `+0x78/+0x7c`,装载失败的层矩形保持 0。
- `crates/yu-rt/src/vm/handlers.rs`:`CGINFO` 的资产元数据回退从「只认
  SX/SY」扩到 `SX/SY/COLOR/ONMOUSE*`;像素查询在尺寸回填后**重试原查询**。

## 回归

- 对拍用例: `crates/yu-gfx/tests/info_query.rs::cginfo_color_samples_pixel_at_set_coordinates`
  (1×1 灰 → `0x00808080`;越界 → `-1`)。
- 验证命令与原始结果:

```text
$ cargo test --release -p yu-gfx --test info_query
test cginfo_color_samples_pixel_at_set_coordinates ... ok
test result: ok. 8 passed

$ yu-cli run <0.481 商业包根> --frames 1800 --log-level info
  gfx.info.unsupported : 51 → 0
  gfx.info.asset_size  : 15 → 0
  帧像素 sha256        : 未变(ad3cff2b…)
```

## 残余

- 按钮系统的 `*_bt3/_bt4` 层名(如 `cgsys/main/button/btn_skip_bt4`)在包里
  **没有同名文件**(同目录只有 `_on/_na/_mask`),因此这些层的尺寸恒 0、
  `CGINFO[COLOR]` 恒 `-1`,重绘分支不进入。属按钮重绘流水线
  (`CGACT[RECTPAINT3]` + `BT.OFF.TMP` 复制)尚未对齐,另案跟进。
