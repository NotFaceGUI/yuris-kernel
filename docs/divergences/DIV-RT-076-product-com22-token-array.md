# DIV-RT-076 · `PRODUCT[COM=22]` token 输出数组的兜底定位(待取证)

- 状态:**挂账(待取证,2026-11-24)**
- 优先级:高(启动链;定位错会让标签表为空、主任务提前 END)
- 发现日期 / 发现方式:2026-11-24 / `<v555-2>` desktop 实机闪退复现
- 影响面:`crates/yu-rt/src/other/handlers/product.rs`(`scenario_token_vars`)
- 关联:`DIV-RT-066`(v555 运行面)、`DIV-RT-074`(LET 系统变量)
- 样本:`<v555-2>`(`es_scenario_label.yst` SYSTEM@82)

## 1. 现象

`PRODUCT[COM=22]` 要把标签 token 写进一对 `STR[1024] + INT[1024]` 数组。
官方证据只确认了 `PINT2/PINT3` 槽号(实测 2918/2919),但 `<v555-2>` 运行时
`PINT2/PINT3` **未回填**。若只按 `PINT2/PINT3` 定位,数组找不到 ⇒ 标签表
为空 ⇒ 主任务提前 END ⇒ `main_finished && live_tasks()==0` ⇒ 桌面宿主退出
(用户观察为“窗口闪退”)。

一次实测日志:

```text
UNIMP [vm] rt.cmd.unimplemented @ es_scenario_label.yst pc82:
  PRODUCT[COM=22] 未定位到 token 输出数组(PINT2/PINT3 未回填,且 ysv
  无唯一 STR[1024]+INT[1024] 对);文本/类型码未写,只返回 token 数
... exit diag: main_finished=true stuck="主任务已 END:引擎不再推进"
```

## 2. 当前实现(保留兜底,挂账)

`scenario_token_vars` 的候选顺序:

1. `PINT` / `PINT-4` 的 `STR[1024]`,配 `+1/+4/+5` 的 `INT[1024]`;
2. `PINT2/PINT3` 指向的 `STR[]`/`INT[]`;
3. ysv 中**唯一**相邻的 `STR[1024]+INT[1024]` 对。

第 2/3 条有文档/样本依据;第 1 条的 `+1/+4/+5` 是让 `<v555-2>` 跑通的
形状启发式,**尚无 REA/trace 证据**,因此保留而不是删除。

## 3. 风险

- 不同作品的数组槽布局可能不同,启发式可能写错数组(静默错数据);
- 兜底命中时不会触发 `report_unimplemented_variant`,问题更隐蔽;
- 目前只有 `<v555-2>` 的实机验证(能进标题循环),未逐作品对拍。

## 4. 待取证

- v555 `PRODUCT COM=22` 处理体:读取哪个参数、数组槽号如何传入
  (PINT/PINT2/PINT3 的真实语义);
- 22 个 v555 样本各跑一次 COM=22,记录三个 PINT 值与 ysv 槽号;
- 拿到证据后:若官方只认 PINT2/PINT3,再收紧候选并补回归;
  若官方也有形状规则,把规则写进本档案与代码注释。

## 5. 回归

- `<v555-2>` desktop debug:`--mode desktop --debug-ui` 连续 45s 不自行退出
  (2026-11-24 修复后实跑;日志无 `PRODUCT` UNIMP);
- 待补:单元/合成回归覆盖 PINT 兜底与“唯一相邻数组对”两条路径。
