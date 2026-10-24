# DIV-SND-004 · ACM WAVE(CODEC=2)

状态:**fixed(no-manual;2026-10-24 B6 收口)**

## 事实(2026-10-24 REA 复核,字节/调用点级)

- 手册 `sound/sound.html` 原文:「特殊コーデック形式の WAVE ファイルには
  対応していません」——**该句过时**:0.495 对非 PCM WAVE 完整走 ACM。
- `yu-ris.exe` 导入 `MSACM32.DLL::{acmStreamOpen, acmStreamSize,
  acmFormatSuggest, acmStreamPrepareHeader, acmStreamConvert,
  acmStreamUnprepareHeader, acmStreamClose}`;7 个导入的全部调用点都在
  WAVE 装载函数 `0x422e04` 内(调用点 `0x423086..0x4232a4`,IAT 跳板
  `0x47f86e..0x47f892`)。
- 分支(反编译 `0x422e04`):`fmt tag == 1` 走 PCM 直解(`+0x107=0`);
  **其余全部 tag(含 3=float、0xFFFE=extensible)走 ACM**(`+0x107=2`):
  `acmFormatSuggest(NULL, src, dst=PCM, cbwfx=0x12,
  ACM_FORMATSUGGESTF_WFORMATTAG)` → `acmStreamOpen(…, fdwOpen=
  ACM_STREAMOPENF_NONREALTIME)` → `acmStreamSize(data, &out, 0)` →
  `PrepareHeader` → 一次 `acmStreamConvert(…, 0)` → `UnprepareHeader` →
  `StreamClose`;TOTALTIME = `round(输出字节*1000/(nBlockAlign*
  nSamplesPerSec))`。任一步非 0 = 装载失败(负错误码返回,层保持旧
  元数据,台账 N0-8)。
- 源格式修正:`cbSize == 0` 时官方改写为 `0x12`(含 chunk 不足 18 字节
  的零填充情形),逐字镜像。
- 「转换后的 PCM 格式未取证」的旧缺口已闭合:目标格式由宿主 ACM 的
  `acmFormatSuggest` 决定(只钉 PCM tag,声道/采样率随源,位深由驱动
  建议)——内核调用同一 API,同机行为 1:1。

## 裁决(B6 实现)

`crates/yu-sound/src/acm.rs` 新增 win32 ACM 转换(全仓第一个第一方
unsafe 点,FFI `msacm32`;crate 级 `forbid` 放宽为 `deny` 并模块级
`allow`):`decode_wav` 对 `fmt tag != 1` 一律走官方同序列 ACM 链,
`CODEC=2`。非 Windows 构建无 MSACM32,按官方「win32 程序」形态对齐
为装载失败。测试 `acm_wave_roundtrip_through_system_adpcm` 用系统
MS-ADPCM 编码器做真实往返(CODEC=2 文件 → 解码 → PCM)。

## 手册勘误

手册「不支持」句与 0.495 实际行为矛盾,以引擎为准(RE 锚点见上);
实现处代码注释带「手册无页(2026-10-24 政策)」标注。
