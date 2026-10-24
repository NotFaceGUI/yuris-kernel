# H-P2-05 · DIALOG / SAVE / 存档界面(交接,不在性能会话实现)

> **2026-10-24 全部销账**:
> - **§1(H-P1-02 + H-P1-04)→ B2 落地**:`Engine::pending_dialogs/answer_dialog`
>   透出 + 桌面 egui 模态(`host-desktop/src/ui.rs::dialog_modal`);验收 =
>   symptom-ledger S5(fixed(B2))+ `yu-cli tests/dialog.rs` + host-desktop
>   lib 5 用例。语料事实修正:菜单「終了する」走脚本自绘框,原生 DIALOG 的
>   入口是**窗口关闭**(`es_Scenario:481/1461` → `ES.ENDCONF.DIALOG`)。
> - **§2(DIV-RT-042 SAVE 写盘)→ B1 落地**(2026-10-24,`ArgValue.reference`
>   + `SaveHost` 同步契约;coverage-matrix §5 已记)。
> - **§3(H-P1-08 存档界面不可见)→ P8 + B1 落地**(除零根因 P7 续;
>   合成/行文本 = DIV-GFX-058/059;存档落盘 = B1;「背景透出上一亮帧」
>   残余另立 DIV-RT-050,归并行 CGS 会话)。
> 本文件保留为交接记录,不再作为挂账依据。

> 来源:2026-10-23 性能与质量修复会话(P2 语义只登记)。
> 这三项**不得**为了“画面好看”改游戏语义;需要内核/宿主契约或官方 trace。
> 真值层级见 `AGENTS.md` §0。

## 1. H-P1-02 + H-P1-04 · DIALOG 透出与呈现

- 现状(证据:`docs/symptom-ledger.md` S5):
  - 标题「終了する」/窗口关闭确认走**原生 `DIALOG`**;
  - VM 侧 `yu_rt::vm::Vm::pending_dialogs` / `answer_dialog` 已就绪;
  - `Engine` 未透出 ⇒ 宿主看不到,画面变暗后静默挂起;
  - 宿主兜底:第二次关闭请求强制退出(不是引擎语义)。
- 需要的 API(H-P1-02):
  ```rust
  pub fn pending_dialogs(&self) -> &[yu_rt::dialog::DialogRequest];
  pub fn answer_dialog(&mut self, id: u64, button: u8, text: Option<&[u8]>)
      -> Result<(), yu_log::Diag>;
  ```
- 呈现(H-P1-04):egui 模态,按钮集合取 `DialogButtons`;`INPUT=1` 走文本框;
  写回 `LET/LETSTR` 由内核完成,宿主不解释返回值。
- 验收场景:标题「終了する」出现确认框;选择“いいえ”后 VM 继续;
  “はい”走脚本 `END` 语义。**不要**把 S3b 的脚本自绘确认框重复实现。

## 2. DIV-RT-042 · SAVE 写盘被拒

- 现状(P4 实测):`sv.write.refused`(Recoverable)出现;`SAVE` 落盘被
  `yu-save` 域宿主按契约拒绝,同时 `sv.arg.missing` 多次。
- 契约缺口:`SAVE[SET]` / `LOAD[LET]` / `FILEINFO[LET]` 的 `DomainCall`
  只带求值结果,丢失变量引用身份(跨 crate 契约变更)。
- 处置:必须由主人裁决是否扩展 `DomainCall`(加 `VarRef` 身份)或改为
  宿主侧只读观察;在此之前**不伪造写盘成功**,也不放宽 `sv.write.refused`。
- 参考:`crates/yu-rt/docs/divergences/README.md` §DIV-RT-042。

## 3. H-P1-08 · 存档界面不可见

- 现状(`docs/symptom-ledger.md` S4):
  - 点「セーブ」后 ops 158→196,新增 `CGS100`/`SLTEMP` 的 `rectpaint2`、
    8 个按钮拷贝;`skipped=0`,但像素 SHA 完全不变(`f0c2706d`);
  - 随后 `es_number.yst:SYSTEM@56` 整数除 0(Fatal),主任务 END。
- 需要 P2(gfx)取证:
  1. `MODE 2`(`rectpaint2`)的可见性/写掩码;
  2. `SLTEMP` 之后是否有缺失的合成/COPY;
  3. offscreen 组到主组的上屏路径。
- P4 侧:`yu-save` 的槽位 UI 是否依赖 `SAVEINFO` 写回;`SAVE` 被拒后
  UI 是否应保持不可见(官方行为未取证)。
- 验收:headless `cases/symptom-ingame-save.json` 从“像素不变”变为
  “出现存档界面”;桌面宿主不再因 `es_number:56` 直接关窗。
  **不要把除零 Fatal 改成跳过**(DIV-RT-003)。

## 4. 关联

- 除零取证:见 `crates/yu-rt/docs/divergences/README.md` §DIV-RT-003
  (本会话追加 `@1118` 扫描结果与下一步)。
- 性能会话只保证:上述路径的**诊断可见、进程不崩、语义不变**;
  不实现 DIALOG/SAVE/存档 UI。
