# DIV-SV-EXEC · `FILEACT[EXEC]` 被宿主安全策略拒绝

- 状态: deferred(master) —— 是否提供显式开关待主人拍板
- 优先级: 中
- 发现日期 / 发现方式: 2026-11-25 v555 启动日志普查

## 现象

`<v555-1>` 启动更新检查脚本 `アップデートファイルチェック.txt`
（s? pc15/17）在用户确认后执行 `FILEACT[EXEC]` 拉起 `updchk`。
内核日志：

```text
INFO [save] sv.exec.refused @ sv@frame85 pc0:
FILEACT[EXEC] 宿主安全策略拒绝外部程序(官方会启动子进程;本内核不执行 EXE)
```

## 官方行为

- 手册 `FILEACT` 有 `EXEC` / `EXEC2` / `PARAM` 参数，语义是启动外部程序。
- 商业 v555 更新检查依赖它：脚本先 `PRODUCT[COM=56/57]` 判版本，再
  `FILEACT[EXEC]` 启动 `updchk`。
- 官方 exe 是 Win32 进程，`FILEACT[EXEC]` 成功后子进程独立运行；引擎本身
  不等待返回值（脚本只用返回值判断启动是否成功）。

## 内核处置

- **不执行外部程序**：内核是跨平台运行时，执行脚本指定的任意 EXE 属于
  宿主安全边界（存档/回放/汉化补丁都不可信），且没有官方
  `CreateProcess` 参数构造的逐字节证据。
- 诊断码 `sv.exec.refused` 保留；级别从 `Unimplemented` 降为 **Info**
  （2026-11-25）——它不是「待实现语义」，而是**已决定的宿主策略**，
  不应再计入 Unimplemented 台账。
- 影响：v555 的自动更新检查不生效（可接受）；游戏主流程继续。

## 待主人拍板

1. 保持现状（Info + 拒绝），把本条标为 `fixed`（宿主策略）；
2. 或加显式 `--allow-exec` / 工程配置白名单（仅允许工程根下的 updater），
   需要补 `CreateProcessA` 参数构造与返回值语义的 REA 取证；
3. 或标 `unsupported-by-official` 不适用（官方支持，不能用该终态）。

在拍板前按 `deferred(master)` 挂账，不作为 `Unimplemented` 终态。
