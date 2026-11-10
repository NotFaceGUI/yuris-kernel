# yu-save · 存档/文件域宿主

YU-RIS 存档与文件域(`SAVE` / `LOAD` / `SAVEEND` / `SAVEINFO` /
`FILEACT` / `FILEINFO`)的宿主侧状态机。容器字节级事实在
[`yu-format::yssd`](../yu-format)(官方 0.495 产物 parse → rebuild
**逐字节一致**,269 块全量对账);本 crate 只消费 `DomainCall` 事件、
维护槽位表(盘上事实)并挂号一切未取证处。

## 分层

| 模块 | 内容 |
|---|---|
| `error` | `sv.*` 稳定诊断码表(唯一性单测守卫) |
| `state` | `SaveState`:槽表(盘上事实)+ 事件事实消费 + 诊断队列 |
| `host` | `SaveHost`:`yu-rt` 事件**命令时刻**同步喂入(B1 裁 1-A/裁 2-A) |

| `fs` | `SaveFs` 平台端口(`NativeSaveFs` 默认实现;宿主可注入内存实现) |

与 `yu-sound` 的差别:`yu-rt` 在 P4 轮冻结,当时未加宿主 trait;B1 起改为
`SaveHost` 同步契约(命令时刻快照),不再靠 drain 后补喂(见
`yu-rt/docs/domain-event-contracts.md` §6)。

## 本轮硬边界(全部挂号,不猜)

| 项 | 处置 | 依据 |
|---|---|---|
| `SAVE[FILE]` 刷盘 | **已实现**(`flush` 按 DNO 升序写预约时的字节快照;默认**可写**,只读为显式构造态,拒绝时挂号 `sv.write.refused`) | B1 裁 1-A/裁 2-A;REA `0x44d73d`/`0x44e1a0` |
| `LOAD[LET/ID]` 值写回 | 容器读侧照常(槽表/解析);图层恢复挂账 H-P4-02 | DIV-RT-042 + H-P4-02 |
| `SAVEINFO` | **官方 no-op**(`0x44e1c8` 守卫后 `xor eax,eax; pop ecx; ret`;VM 分派为 `HandlerKind::Noop`,不再进入存档域) | REA 2026-11-10(`44e1e1 33 c0 59 c3`);`fixture::saveinfo_and_var_are_official_noops` |
| `FILEACT[EXEC]` | 拒绝(`sv.exec.refused`) | 无引擎语义取证 + 宿主安全策略(不执行外部程序) |
| `FILEACT` 图层导出(BMP/PNG/JPG) | 挂账 H-P4-02(→ P2/集成轮) | 需要 gfx |
| `FILEINFO[EXIST]` | **不双写**(内核 J3 已实现) | REA `0x44286c` |
| SNP 压缩形态 | 可读可写(`BlockCodec::Snappy`,按盘上既有存档 → 引擎代号 → zlib 判定) | 0.495 产物 `78 01` 头;CLI `--save-codec` |
| FILEACT 落盘 | 仅在**显式可写 save 根**下允许 | 保护语料工程目录 |

## 验证

```bash
cargo test -p yu-save        # 单元:预约/拒绝/越界/槽表/路径越界
cargo test -p yu-format --test yssd_roundtrip   # 官方 .sd 逐字节 roundtrip
```

语料:`YURIS_SAMPLE_ERIS_ROOT` 指向 ERIS 工程根(或 `YURIS_SAVE_CORPUS`
直接指向 `.sd` 快照目录)。**注意**:`<project>/save/` 是官方 exe 的
活动写盘位——对拍前先快照到 `target/tmp`,不要拿移动靶当 golden。
