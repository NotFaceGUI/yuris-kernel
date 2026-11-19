# DIV-RT-061 · SAVE/LOAD 的 `IDNO` 图层名后缀

- 状态:**fixed**(2026-11-19)
- 影响面:`yu-rt`(`op_save` / `op_load`)、`yu-gfx` 图层捕获/恢复
- 关联:`DIV-RT-042`(存档闭环)、`DIV-GFX-071`(层名/`IDNO` 规则)

## 1. 问题

0.495 的 `SAVE` / `LOAD` 命令表新增了 `IDNO` 参数:

| 命令 | 0.495 参数(ycd 顺序) |
|---|---|
| `SAVE` | FILE, DNO, SET, ID, **IDNO**, G_INT..G_STR4 |
| `LOAD` | FILE, DNO, LET, ID, **IDNO**, G_INT..G_STR4 |

内核旧实现只读 `ID`,**忽略 `IDNO`**:`SAVE[ID="THUMB" IDNO=3]` 会抓
`THUMB` 层,而不是官方的 `THUMB.3` 层;`LOAD` 同样装错层。ERIS 语料
未使用 `IDNO`(900 帧 `SAVE` 16 次的签名里只有 DNO/SET/G_*/FILE),
所以此前没有暴露。

## 2. 官方事实(REA 0.495 `4f5eefa3…`)

### SAVE `0x44c45c`

```text
0x44c48b  movzx eax, byte ptr [0x684163]     ; ID 参数槽
0x44c492  test eax,eax
0x44c494  je   0x44c56d
0x44c49a  mov edi, dword ptr [eax*4 + 0x683fc0]  ; ID 字符串指针
0x44c4a1  movzx edx, byte ptr [0x684164]     ; IDNO 参数槽
0x44c4a8  test edx,edx
0x44c4aa  je   0x44c521                      ; IDNO 缺省 ⇒ 直接用 ID
...
0x44c4b0  mov esi, 0x5de180                  ; 临时缓冲
0x44c4b5  ... 逐字节拷贝 ID ...
0x44c4d9  mov edx, 0x808354                  ; 常量 "."
0x44c4e8  ... 追加 "." ...
0x44c4ef  ... 求串长 ...
0x44c514  call 0x46a7c0                      ; 64 位十进制转换(低/高 32 位、目标尾指针、基数 10)
0x44c51c  mov edi, 0x5de180                  ; 最终层名
0x44c52d  mov ecx, dword ptr [0x80170c]
0x44c533  call 0x4141e4                      ; 路径规范化
0x44c54f  call 0x414559                      ; 层查找
```

### LOAD `0x44708c`

`0x4470b4..0x447178` 与 SAVE **逐指令同构**:同一组参数槽
(`[0x684163]` ID / `[0x684164]` IDNO)、同一常量 `.`(`0x808354`)、
同一 `_ltoa(…,10)` 与 `0x4141e4`/`0x414559` 调用。

### 语义

- `IDNO` **在场** ⇒ 层名 = `ID` + `"."` + 十进制 `IDNO`(与
  `CG`/`CGEND`/`CGINFO` 的 `ID[.IDNO]` 规则一致);
- `IDNO` 缺省 ⇒ 层名 = `ID`;
- `IDNO` 的值不参与“是否拼接”的判断(参数存在即拼接,值 0 → `.0`);
- 手册 `file/save.html` / `file/load.html` 只写 `ID`,未写 `IDNO`;
  `IDNO` 是 0.495 命令表的新增槽(手册无页)。

## 3. 内核改动

- `handlers.rs::compose_layer_id(id, idno)`:按官方规则拼接 `ID.N`;
- `op_save` / `op_load` 增加 `"IDNO"` 参数读取(按名);
- `SAVE[ID]` 的 `GraphicsHost::capture_layer` 与 `LOAD[ID]` 的
  `restore_layer` 改用拼接后的层名;事件载荷仍保留原始 `ID`/`IDNO`。

## 4. 验证

```text
cargo test -p yu-rt --test fixture save_load_idno
  → 1 passed:captured/restored 层名均为 THUMB.3
cargo test -p yu-rt
  → 113 lib + 129 fixture + consistency 全绿
```

回归夹具:`save_load_idno_appends_dotted_suffix`
(`crates/yu-rt/tests/fixture.rs`),同时断言 SAVE 抓帧与 LOAD 装回的
层名字节。
