# CONTEXT · 术语权威(种子版)

本文件是本仓库唯一术语表。新增条目必须附证据出处;与旧接手线用词冲突时,
以旁线 SDK 逆向笔记为准并在此登记勘误。

## 核心术语

| 术语 | 含义 |
|---|---|
| 命令(Command) | YSCM 层的语句(GO/IF/LET…),按游戏 ysc 表以**名字**识别 |
| opcode | 表达式栈机的操作码(30 种),与命令是两层概念,不得混用 |
| 命令组(Group) | YSTB 执行单元,PC = 组下标 |
| 窗口(Window) | 命令组的参数记录(12 字节),处理器经参数槽取其求值结果 |
| 参数槽(param slot) | 窗口内 tag.B0 标识的具名槽位 |
| 共享池 | content 与 part4 的合并区 |
| 对拍 | 与官方引擎行为的差异比对;不是 diff/golden 比对的同义词 |
| 真值 | 官方引擎可观测行为;见 README 立场一节 |

## 待扩充

宏系统、scenario、任务链/vsync 链等域的术语,待对应子系统实装时
从旁线笔记(format-vm-opcodes / script-keywords / architecture)逐条核对后补入。
