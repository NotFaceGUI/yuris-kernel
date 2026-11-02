# DIV-VFS-001 · 资源名编码不一致时的唯一同名回退

- **状态**: fixed(兼容路径,默认开启,可关闭)
- **范围**: `TextureStore` 解析的图像资产(软/硬两个后端共用);音频/影片
  /语音等直接走 VFS 的路径不在本回退内。
- **发现**: 2026-11-01,`project-eris` 的 CG 鉴赏界面 5 张装饰图
  (`tip_s_／`、`tip_／`、`tip_％`、`tip_鑑賞数`、`tip_鑑賞率`)预载失败。

## 现象与证据

- 包内条目名是 **GBK** 字节,例如 `cgsys\extra\cgmode\tip_s_\xa3\xaf.png`
  (GBK 解码 = `tip_s_／.png`);索引里的 `nameHash` 与 GBK 字节的
  MurmurHash2 相等(实测 `0x4c662ebb`)。
- 脚本字面量是 **CP932**:`tip_s_\x81\x5e`(见 `ES.CGMODE.DEFINE` 编译产物)。
- 官方查找按原始字节哈希 + `_mbsicmp`,调用链无代码页转换;字符串字面量
  处理(opcode 77 `0x42694c`)只做反斜杠转义。因此这份 GBK 包在官方引擎
  里同样按字节匹配不上。
- 官方能显示这些图,是因为官方 SDK 样本是散盘 `data/`(Unicode 文件名),
  文件系统层按 CP932→UTF-16 访问;本仓库 `DirSource` 已对齐(0.479 SDK
  1424 文件、0 skipped)。

## 处置

`crates/yu-render/core/src/texture.rs`:

1. **原始字节命中优先**,行为与官方一致;
2. 非 ASCII key 且原始候选全部 miss 时,对每个扩展名候选做“同名 Unicode”
   查找:
   - 脚本 key 按工程 codec 解码(通常 CP932);
   - 同目录文件名按工程 codec 与另一 codec(CP932/GBK)各解一次;
   - 仅当 **Unicode 全名(含扩展名)相等且唯一** 时采用该条目的真实字节
     路径读取;两个候选都等价则判歧义,保持 `asset.missing`(不猜);
3. 命中计入 `TextureStore::name_fallback_hits()`,默认以 `log::info!`
   留痕(`[asset] name-encoding fallback: … -> …`);
4. 开关:`YURIS_ASSET_NAME_FALLBACK=off` 关闭回退。

## 与官方的关系

这是**有意的兼容性分叉**:官方在同样的 GBK 名包上会 miss;本内核在
“唯一同名”前提下把图读出来。分叉只影响“坏包”场景,正常 CP932/ASCII 包
的原始字节命中路径完全不变,也不回写 YPF 索引、不改脚本/存档字节语义。

## 限制

- 只覆盖图像资产;非 ASCII 名的音频/影片/语音仍按字节查找,若本地重打包
  同样需要处理,应在 VFS 层另行设计(需评估对脚本/存档查找的影响)。
- `encode∘decode` 对 CP932 的 398 条别名不保字节;本回退只做**比较**,
  不把匹配结果重新编码写入,因此不会改写包内字节。
- 歧义策略是“宁可 miss 也不猜”,避免两张不同图被静默互换。
