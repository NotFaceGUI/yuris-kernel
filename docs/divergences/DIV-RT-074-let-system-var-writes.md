# DIV-RT-074 · `LET` 系统变量写侧(`_MOUSE_L` / `_DROPFILE_NUM` / `_INT` / `_PINT`)

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/vm/handlers/variables.rs`、
  `crates/yu-rt/src/other/{mod.rs,sysvar.rs,handlers/product.rs}`
- 关联: `DIV-RT-007`(系统变量写入挂号)、`DIV-RT-065`(GOSUB 参数组)、
  `DIV-RT-017`(系统变量 ABI)
- 样本: v555 2.1MB 族 / `<v555-1>` / `<v555-pc>`

## 1. 现象

`LET` 的按钮族分支(`_KEY_*`/`_MOUSE_*`)只求值 RHS、不写计数器;
`_DROPFILE_NUM`、`_INT`、`_PINT` 落 `DIV-RT-007` 挂号。v555 语料有
`<v555-1>` s70 pc410/pc502 `LET @147 = 3`(`@147 = _MOUSE_L`),
官方会写 64 位输入计数表。

## 2. 证据

### 2.1 `_MOUSE_L`(id 0x93)

`b02a4dc04ae LET 0x443988` 的 int 分支:

```text
0x4440b5  cmp esi, 0x93
0x4440c1  cmp esi, 0x113
0x4440c7  jle 0x4441e5          ; 0x93..0x113 共用
0x4441e5  movzx eax, byte ptr [esi + 0x667c8d] ; id → 表下标
...
          call FUN_00444750      ; 写 64 位计数表
```

`FUN_00444750` 按 `DAT_006638c0[index]`(当前物理 down 状态)钳制:

- down=0:负数保留,零/正数写 0;
- down=1:正数保留,零/负数写 0。

内核 `InputState::Counter::write` 已按 0.495 `FUN_00447014` 实现同一
钳制;本修复把按钮族写入接到 `set_counter(name)`。

### 2.2 `_DROPFILE_NUM`(id 0x0f)

同一 LET 处理体 int 分支:

```text
if (iVar11 == 0xf) { DAT_008724ac = uVar16; ... }
```

读侧使用同一全局。语料以 `PRODUCT[COM=21/24]` 枚举后读取。
内核在 `other::State` 增加 `drop_file_num: u32` 镜像;枚举时同步写入,
`LET` 写低 32 位,`_DROPFILE_NUM` 读该镜像。

### 2.3 `_INT`(id 0x32)

同一处理体:

```text
0x444538  ...
0x444545  cmp byte ptr [ecx + 1], 1   ; 目标变量类型必须是 int
0x444597  error 0x1a6c6               ; 否则
          ... 按 DAT_00661260 操作符表写变量表
```

`_INT(槽号[,元素])` 是动态变量写入;夹具验证标量目标按元素 0 处理,
数组目标按给定元素写。**槽号范围**:上述片段只证据了目标变量类型检查
(0x1a6c6),`<1000` 是否合法未取证;内核不设下界(仅按 u16 变量 id
宽度拒绝越界),找不到目标按未声明报错。

### 2.4 `_PINT`(id 0x35)

同一处理体:

```text
iVar4 = *(int *)(task + 0x144 + task[0x140] * 4); // 当前 GOSUB 帧
uVar15 = *(char *)(iVar4 + 700);                  // 参数个数
if (index >= uVar15) error;                        // 0x18b1e 族
*(u64 *)(iVar4 + 0x10 + index * 8) = value;        // 按操作符表写
```

内核写当前任务 `other.gosub_frames[task].last().args_int[n-1]`;不在
GOSUB 帧内报 recoverable,不写。**缺参数号**(`_PINT = v`)的官方缺参
行为未取证,内核显式报 recoverable,不默认 1。

## 3. 门控

- `_MOUSE_L` 等按钮族写入是通用输入语义(0.495 也有同一 `Counter::write`
  与 ERIS `@_MOUSE_L=5` 用法),不按版本门控。
- `_DROPFILE_NUM` / `_INT` / `_PINT` 只由 `other` 运行面(精确 555)声明,
  写入分支用 `self.other.is_some()` 门控;旧版仍保持原挂号路径。

## 4. 回归

- `fixture::variables::let_mouse_button_writes_the_input_counter`
- `fixture::other::let_dropfile_num_writes_the_sysvar_mirror`
- `fixture::other::let_int_writes_the_target_variable_slot`
- `fixture::other::let_pint_writes_the_current_gosub_frame_arg`
- `fixture::other::script_statics_declare_unreachable_int_at_load`
  (不可达 `INT` 的装载期声明)
- `state::official_save_flush_preserves_all_blocks`(ignored,env-gated)
- `cargo test -p yu-rt --lib` 158 passed;
- `cargo test -p yu-rt --test fixture` 156 passed / 7 ignored。

## 5. 残余

- `_DROPFILE_NUM` 的复合赋值码:官方 id 0x0f 分支直接写低 32 位,
  未逐操作符取证;当前按直接赋值实现,语料只出现普通赋值。
- `_INT` 的动态槽表映射(`DAT_00867...`)与 `_STR` 写侧未在本轮展开;
  `_INT` 读侧已有 `read_int`,本轮补了标量元素 0 的读取。
- `_PINT` 写侧的操作符表逐个 op 由共享 `compound` 复用;未单独对拍
  0..8 的边界溢出语义。
