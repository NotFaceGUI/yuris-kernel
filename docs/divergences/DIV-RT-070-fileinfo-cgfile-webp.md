# DIV-RT-070 · FILEINFO[CGFILE] 的版本门控与 WebP 扩展名

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/vm/handlers/file_io.rs`
- 关联: `DIV-GFX-092`(WebP 资产解码)、
  `DIV-GFX-024`(`CGINFO[SX/SY]` 资产尺寸回退)、
  `docs/formats/v555-semantic-census-2026-11-23.md` §2.3
- 样本: `<v555-1>`、`<v555-pc>`(扩展名表证据取 `b02a4dc04ae`)

## 1. 现象

`FILEINFO[CGFILE=基名 EXIST=1 LET=@x]` 对**纯 WebP** 资产返回 `0`,
脚本据此判定 CG/按钮不存在;渲染端本身能解码同一资产。

失败链:

1. 脚本按 BT 按钮宏 `es_buttonque.yst` 的约定,用无扩展名基名查询
   `FILEINFO[CGFILE]`;
2. 内核 `cg_file_exists` 只按 `yscfg.image_ext_order` 的码
   1..5(`png/bmp/jpg/gif/psb`)拼候选,没有 `.webp`;
3. 包内资产是 `基名.webp` ⇒ 全部 miss ⇒ 写 `0`;
4. 按钮层/可选层不被创建,UI 缺件。

同时,该分支外层还有 `yscm.version == 555` 门控;0.495 实际也读
`CGFILE`,门控没有语义依据。

## 2. 证据

### 2.1 0.495 已读 `CGFILE`

`0x44286c`(handler 入口):

- 参数 0/1/2/3(FOLDER/FILE/CGFILE/SOUNDFILE)分别把 `ebx` 置 1/2/3/4;
  kind 2(CGFILE)在 `0x4428db-0x4428ed` 把 `ebx=3`;
- EXIST 分支 `0x44298a: cmp ebx,3; je 0x442c7f`;
- `0x442c7f` 先试原路径(追加空串 `0x80795c`),再按
  `0x8079ec ".png"` / `0x8079f4 ".bmp"` / `0x8079fc ".jpg"` /
  `0x807a04 ".gif"` / `0x807a0c ".psb"` 拼名,
  用 `0x4436b5` 查存在性;命中写 `LET=1`,否则写 `0`。

⇒ `yscm.version == 555` 门控错误。

### 2.2 v555 的扩展名表含 `.webp`

`b02a4dc04ae 0x43eca8`(FILEINFO)对 kind 2(CGFILE)置 `cVar5='\x03'`,
EXIST 分支与 FILE/SOUNDFILE 一样调用 `FUN_0043f770`(`0x43f770`)。
该函数按图像扩展名表拼名,表实测:

```text
0x878a00 ".png"  0x878a08 ".bmp"  0x878a10 ".jpg"
0x878a18 ".gif"  0x878a20 ".psb"  0x878a28 ".webp"
```

0.495 同表止于 `.psb`(见 §2.1),所以 `.webp` 是 v555 新增项。

## 3. 修法

`crates/yu-rt/src/vm/handlers/file_io.rs`:

1. 删除 CGFILE 分支的 `self.tables().boot.yscm.version == 555` 门控;
2. `cg_file_exists` 在 yscfg 扩展名序之后**通用追加 `.webp`**
   (不按版本判断;旧语料不含 WebP,追加不改变旧行为);
3. 已带扩展名的键仍先做 exact 匹配,不再拼后缀。

不做 `SemanticProfile` 新字段:CGFILE 探测是两代共有语义,
`.webp` 是通用扩展名能力,没有需要按版本切换的分支
(与主人 2026-11-23 口径一致)。

## 4. 回归

- 新增 `crates/yu-rt/tests/fixture/file_save.rs::
  fileinfo_cgfile_probes_webp_without_version_gate`:
  夹具 `ysc` 头 = 495,覆盖
  - 纯 WebP(基名 + `.webp`)→ 1;
  - 不存在 → 0(仍写 LET);
  - 显式 `.webp` → 1(不重复拼接);
  - 旧扩展名序(png)→ 1。
- `cargo test -p yu-rt --test fixture`(145 passed / 7 ignored);
- `cargo test -p yu-rt --lib`(153 passed)。
