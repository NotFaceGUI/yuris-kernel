# ERIS 官方示例测试工程(内核基准语料)

> 用途: yuris-kernel 的**官方核心**测试语料 —— 由官方编译器/打包器产出的真实
> `.ybn` 与 `.ypf`,给 yu-format / yu-vfs / yu-rt 当字节级与行为级基准。
> 全部步骤在本机实测通过(2026-10-19);来源为官方 SDK 0.495 的 ERIS 示例工程。

## 1. 语料位置

```
<work-root>/projects/<side-repo>/workbench/project-eris/
├── yu-ris.exe  system/  yscfg.dat  エンジン設定.exe   ← 官方引擎与配置
├── data/                            ← 工作副本(散盘;含编译产物 data/ysbin/)
└── data.ypf                         ← 官方 YSPac 打出的基础包(发布形态)
```

- 编译产物: `data/ysbin/` = **137 个脚本 YSTB**(`yst00000..yst00136.ybn`)+
  7 张元数据表(`ysc.ybn`=`YSCM`、`yscfg`=`YSCF`、`yse`=`YSER`、`ysl`=`YSLB`、
  `yst`=`YSTD`、`yst_list`=`YSTL`、`ysv`=`YSVR`),全部版本字段 495。
- 脚本源文件名做过 **ASCII 化**(非日文系统上官方编译器读 CP932 文件名会损坏):
  映射清单 `<work-root>/projects/<side-repo>/workbench/rename-manifest.json`。

## 2. 从零复现(四步)

### 2.1 编译(官方 YSCom,引擎侧协议)

官方编译器不能独立运行,必须由"引擎角色"驱动:

- 共享内存 `"YU-RISCompilerFileMapObject"`(≥0x200):`+0x80` = 引擎版本(0x1EF=495),
  `+0x84` = **项目路径(必须以 `\` 结尾)**;`+0x00` state0(编译器附着)、`+0x04` state1(启动器确认)、
  `+0x08/+0x0C` 完成位、`+0x10` 条目数、`+0x14` 总字节数。
- `CreateProcess(YSCom.exe ys)`,CWD=项目根(注意:`Start-Process`/ShellExecuteEx **不传 argv**,
  会让 YSCom 静默退出);编译器按 `system\ysconfig.txt` 的 `PROJECTFOLDER` 找源码。
- 编译器生成第二个映射 `"YU-RISCompilerFileMapObject2"` 由启动器创建(大小=+0x14),
  条目为 `{u32 名字长度(含 NUL、带路径), 名字, u32 文件大小, 数据}`;
  数据为 **加密段形态**的 YSTB(直接可当磁盘 `.ybn` 写入)。
- 落地工具: `workbench/yscom_harvest_rs`(Rust,零依赖;`--le` 可用 Locale Emulator 转日文环境)。

### 2.2 运行(调试模式)

`data/config/projectconfig.txt` 设 `SYSTEMMODE[DEBUGMODE=1]` → 引擎每次启动**重新编译**
(界面上有进度条,约 30-40 秒;失败时回落到磁盘 `ysbin`),随后进标题。**实测通过。**

### 2.3 打包(官方 YSPac,发布形态)

见 `docs/formats/ypf.md` §5:进 `data/` 全选其**内容**(不是 `data` 文件夹本身)粘贴进 YSPac →
`全ファイル圧縮` → 输出 `data.ypf` 放到**游戏根目录**。

> 关键差异:打包 `data` 文件夹会得到 `data\ysbin\...` 前缀,引擎按项目目录相对
> (`ysbin\...`)查找 → 找不到;必须打"内容"。这是发布模式黑屏/退出的直接原因。

### 2.4 发布模式启动(独立运行,无编译)

- `SYSTEMMODE[DEBUGMODE=0]`(发布模式;引擎走"单包挂载"分支);
- 目录形态:`yu-ris.exe` + `system/` + `yscfg.dat` + `data.ypf`(散盘 `data/` 可不存在);
- **实测**:官方引擎从 `data.ypf` 读 `ysbin`,直接出标题画面,**无编译进度条**。

## 3. 该语料能回答的问题(内核对齐用)

| 问题 | 用哪份数据 |
|---|---|
| `.ybn` 磁盘形态(明文头 + 分段异或) | `data/ysbin/*.ybn`(见 `docs/sdk-eris-analysis.md` §8) |
| YSCM/YSLB/YSVR/YSTD/YSTL 布局 | 对应 7 张元数据表(未加密) |
| `.ypf` 索引/哈希/压缩布局 | `data.ypf`(见 `docs/formats/ypf.md`) |
| 编译产物条目命名规律 | 编译映射条目(`ysbin\yst%05d.ybn`)与 `data.ypf` 索引 |
| 引擎启动链与包发现规则 | 引擎反编译(`FUN_0045e3b8`/`FUN_0045eda0`) |

## 4. 注意事项

- `data.ypf` 为官方 YSPac 产出,可直接作为 yu-vfs `YpfSource` 的真实样本(1407 条目)。
- 打包时如遇非 ASCII 文件名(如 4 张 `cgsys/.../tip_*.png`),系统 ACP 非 932 时会被
  YSPac 跳过或损坏——语料需先 ASCII 化(同 §1 的 rename 清单做法)。
- 引擎配置 `yscfg.dat` 属官方分发包的一部分,发布时随包携带(官方「配布時の注意点」)。
