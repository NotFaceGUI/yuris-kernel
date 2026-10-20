# yu-format

ybn 家族容器与结构解码:**纯字节进出,不碰 IO**。ypf 包布局归 `yu-vfs`,
执行语义(窗口字节求值/表达式)归 `yu-rt`。依赖仅 `yu-log`。

## 模块地图

| 模块 | 数据 | 内容 |
|---|---|---|
| `yscm` | `ysbin/ysc.ybn` | 命令表(下标 = opcode;按名分派的数据源) |
| `ystb` | `ysbin/yst%05d.ybn` | 脚本体四区段 → `DecodedScript`(组/12 字节参数窗口) |
| `ystl` | `ysbin/yst_list.ybn` | 脚本清单(下标 ↔ 源脚本名 + 统计) |
| `yslb` | `ysbin/ysl.ybn` | 全局标签表 |
| `ysvr` | `ysbin/ysv.ybn` | 变量声明(kind/flag/file/slot/ty/dims/初值) |
| `ystd` | `ysbin/yst.ybn` | 变量槽位表头 |
| `yscf` | `ysbin/yscfg.ybn` | 引擎配置(分辨率/扩展名优先序等) |
| `yser` | `ysbin/yse.ybn` | id ↔ 名字表 |
| `key` / `container` | — | CRC-32 + 分段异或密钥;16 字节容器头工具 |
| `codec` | — | CP932/GBK 静态码表 + auto 评分器 |

## 冻结接口(与 yu-rt 的契约)

```text
DecodedScript { script_id, groups: Vec<Group>, labels: LabelTable }
Group { command: Cow<str>, windows: Vec<Window> }   // Window = [u8; 12]
```

窗口字节的语义(参数槽 tag 等)属运行核;格式层只忠实切分。命令序号超出
ysc 表 = `fmt.ybn.opcode` 诊断,绝不落序号。

## 纪律

- 解码算法的事实来源(RxYuris / YU-RIS-Script-Editor / 旧线语料闭合)在
  `src/lib.rs` 模块文档登记;语义结论以官方产物互证为准。
- 解密密钥、字段解释一律来自数据自证,无内置兜底。

## 验证

```bash
cargo test -p yu-format
YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-format --test corpus_eris
```

进度与证据:`PROGRESS.md` 成果 B1/B2、E3。
