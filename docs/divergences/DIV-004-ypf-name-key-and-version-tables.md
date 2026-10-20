# DIV-004 · 名字 XOR key 与按版本切换的长度交换表(按开源参考实现)

- 状态: **fixed(V2:版本分支删除,统一为引擎单一布局)**
- 优先级: 中
- 发现日期 / 发现方式: 2026-10-19,会话 A 重启后对照开源实现
  `YU-RIS-Script-Editor/core/ypf.py`(未声明许可证 → 仅借鉴判据/算法,未复制代码)。

## 事实

### 1. 名字 XOR key 逐游戏不同

- 官方 0.495 包: key = `0xFF`,即索引里的名字是**逐字节取反**(本机真包
  1,407/1,407 条 nameHash 命中,实证)。
- 参考实现给出的推导: `name_key = enc[nameLen - 4] ^ '.'`(取名字第 4 个末字节,
  与 `.` 异或);名字短于 4 字节时回退 `0xFF`。
  —— 该推导**本机无第二把 key 的真样本**,未验证。

### 2. 长度交换表(已定:与版本无关)

**REA 复核 `FUN_00462a10`(0.495,唯一写入者)**:函数先把 0..255 填成恒等表,
再做**恰好 12 对交换**,随后生成脚本密钥;**函数内没有任何版本分支**。
开源参考实现的分段(10 对/8 对/12 对)与引擎字节不符,已删除。

### 3. meta 长度(已定:恒 22 字节)

REA `FUN_0045e3b8` 的步长是 `nameLen + 5 + 0x16`(即 +27),meta 恒 22 字节;
开源参考实现的 `0x12 + extra` 分支已删除。

## 内核行为

`crates/yu-vfs/src/ypf.rs`(任务 E · V2):

- `name_len_table(_version)` / `meta_len(_version)` 恒返回单一表 / 22 字节;
  版本参数仅保留调用兼容。
- 版本接受区间收敛为 `234..=495`(低于 234 拒绝;高于 495 视为未验证布局,
  Unimplemented)。
- `detect_name_key` 仍用 nameHash 择优(顺序 `0xFF` → 参考推导 → 全量),
  但命中数 == 条目数时立即返回(V8);`is_verified()` 只认
  `index_start == 0x20 && name_key == 0xFF`(V6)。

## 待验(缺样本)

- [ ] 换 key 的真实包(`<game-a>` 一类的 Whirlpool 系作品):需确认引擎是否
      真的存在非 0xFF 的 key;当前按"统一 0xFF + 哈希自检"处理。
- [x] 10 对 / 8 对表:`FUN_00462a10` 证明不存在版本分支,分支已删除。
- [x] 18/26 字节 meta:同上,已删除。

## 回归

```text
$ YURIS_SAMPLE_ERIS_YPF=<official 0.495 data.ypf> cargo test -p yu-vfs --test real_sample -- --nocapture
index: start=0x20 key=0xff meta=22 bytes=79647 consumed=79647 closed=true nameHash=1407/1407 compressed=152
test result: ok. 4 passed
```
