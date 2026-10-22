# DIV-SND-004 · ACM WAVE（CODEC=2）未实现

状态：**open（Unimplemented）**

## 事实

- 手册 `sound/sound.html` 原文：“特殊コーデック形式の WAVE ファイルには
  対応していません”。
- 0.495 `yu-ris.exe` 导入 `MSACM32.DLL::acmStreamOpen / acmStreamSize /
  acmFormatSuggest / acmStreamPrepareHeader / acmStreamConvert /
  acmStreamUnprepareHeader / acmStreamClose`；WAVE 路径 `0x422e04` 在
  `fmt tag != 1` 时走 ACM 转换，并把 `+0x107` 写 2（对外 CODEC=2）。
- 转换后的 PCM 格式/重采样参数未逐项取证。

## 裁决

`yu-sound` 只解普通 PCM/float WAVE 与 OGG；ACM WAVE 返回
`snd.codec.unsupported`（Unimplemented），不猜转换链。

## 解除条件

用 ACM 样本 + 官方引擎取得 `acmFormatSuggest` 的目标格式与输出采样，
或取得官方手册对 ACM WAVE 的明确支持说明。
