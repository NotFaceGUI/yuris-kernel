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
| `host` | `SaveHost`:`yu-cli` Engine 每帧 drain 事件后喂入 |

与 `yu-sound` 的差别:`yu-rt` 在 P4 轮冻结,故**不加 `yu-rt` 宿主
trait**,由 Engine 在 `Vm::take_events()` 之后驱动(事实记录容忍一帧
延迟,见 `yu-rt/docs/domain-event-contracts.md` §6)。

## 本轮硬边界(全部挂号,不猜)

| 项 | 处置 | 依据 |
|---|---|---|
| `SAVE[FILE]` 刷盘 | **拒绝**(`sv.write.refused`),不写半忠实存档 | `SET/LET` 的变量引用身份在事件层丢失(DIV-RT-042);块的 `var_id` 与整组数组内容拿不到 |
| `LOAD[LET/ID]` 值写回 | 容器读侧照常(槽表/解析);写回挂账 | 同上 + 图层恢复归 H-P4-02 |
| `SAVEINFO` 查询 | 挂账(`REA 0x44e1c8` 未取证,语料 0 次) | 手册页 + 语料实测 |
| `FILEACT[EXEC]` | 拒绝(`sv.exec.refused`) | 无引擎语义取证 + 宿主安全策略(不执行外部程序) |
| `FILEACT` 图层导出(BMP/PNG/JPG) | 挂账 H-P4-02(→ P2/集成轮) | 需要 gfx |
| `FILEINFO[EXIST]` | **不双写**(内核 J3 已实现) | REA `0x44286c` |
| SNP 压缩形态 | 不实现(本语料 7 文件全 zlib level 1) | 旧线双语料对照;0.495 产物 `78 01` 头 |
| FILEACT 落盘 | 仅在**显式可写 save 根**下允许 | 默认只读观察,保护语料工程目录 |

## 验证

```bash
cargo test -p yu-save        # 单元:预约/拒绝/越界/槽表/路径越界
cargo test -p yu-format --test yssd_roundtrip   # 官方 .sd 逐字节 roundtrip
```

语料:`YURIS_SAMPLE_ERIS_ROOT` 指向 ERIS 工程根(或 `YURIS_SAVE_CORPUS`
直接指向 `.sd` 快照目录)。**注意**:`<project>/save/` 是官方 exe 的
活动写盘位——对拍前先快照到 `target/tmp`,不要拿移动靶当 golden。
