# DIV-SND-002 · 负 START/RESTART(延迟播放)

状态:**fixed(Likely 标注;2026-10-24 B6 收口)**

## 事实(全部静态字节级取证)

手册 `sound/sound.html`:`START`/`RESTART` 为负值时是「遅延再生」。
0.495 处理体把负值交给 `0x423f9c`(OGG seek)/`0x424424`(WAVE seek):

- `delay_bytes += |param_ms| * bytes_per_sec(+0x198/+0x19c) / 1000`(64 位),
  累加进延迟累加器 `+0x190/+0x194`;
- 累加后按 `block align(+0x1c4)` **向下取整**(`__allrem` 减余数);
- 本次播放起始字节 `+0x17c` 保留负值;WAVE 侧数据填充位置 `+0x184`
  从字节 0 重灌(负 param 先钳 0)。

即:延迟 = **按字节累加的静音区间**,数据从 0 开始播。

## 裁决(B6 实现)

`yu-sound` 以 ms 域等价实现(`crates/yu-sound/src/state.rs` 的
`Layer::delay_ms`):PLAY 起点或循环边界遇到负 START/RESTART 时,
位置钉 0、记 `delay = |param|` ms;推进时钟只烧延迟不推进位置
(TIME 冻结),混音端把延迟窗口渲染为静音(设备路径
`render_and_advance` 先渲染后推进,静音在本块内可见)。
换算上 `|param|*bps/1000` 再按 block align 取整回到 ms 与 `|param|`
只差亚毫秒截断,故直接以 ms 存储。

**Likely 项(动态样本未到,保持标注)**:
- 延迟期间 `SOUNDINFO[TIME]` 冻结在 0;
- 延迟期间 `SOUNDINFO[PLAY]` 保持 1(state 2 不变);
- 延迟窗口内混音输出静音。

`+0x190` 在混音线程的消费点未逐行取证(引擎侧该区间行为只能间接推断);
动态逐帧曲线仍归 `DIV-SND-003` 的复现步骤校准。正 START/RESTART 与
单测(手册例 3/4)不受影响。

## 解除条件(Likely → Confirmed)

取得官方引擎延迟播放期间逐帧 `TIME/PLAY/LOOPCOUNT` 曲线
(`DIV-SND-003` 复现步骤),或在混音线程定位 `+0x190` 消费点。
