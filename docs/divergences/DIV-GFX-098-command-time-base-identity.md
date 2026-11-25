# DIV-GFX-098 · 中间层同帧 `CGEND` 后 MERGE 链基面身份不一致,合成结果被丢弃

- 状态: fixed
- 优先级: 高(立绘换表情/耳朵动作时整帧露出旧差分:腮红消失、眼睛开合错误)
- 发现日期 / 发现方式: 2026-11-25 / `YU_GFX_CMD_TRACE` 连续命令流 +
  `YU_PASS_TRACE` 逐帧回放,`<game-c>` 实机

## 复现

- 游戏位置: `<game-c>` 任意对话场景,角色待机、无输入
- 锚点: 耳朵动作切显示层(`CG SPL602 E=1 A=256`)的那一帧,角色露出
  “睁眼、无腮红”的旧差分;动作结束回 `SPL601` 后恢复

## 命令流(`<game-c>`,s159 `es_sprite_cgl.yst`)

```text
pc51  CG      ID=SPLBUF602 FILE="" Z=1            ← 清目标
pc52  CGACT   ID=sp.tmpA  ID2=SPLBUF602 COPY2=1
pc54  CG      ID=sp.tmpB  FILE=...a_tou_4a_01     ← 眼睛差分基图
pc58  CG      ID=sp.tmpC  FILE=...a_tou_4a1701    ← 腮红差分
pc59  CGACT   ID=sp.tmpC  ID2=sp.tmpB X=0 Y=0 MERGE=1
pc60  CGEND   ID=sp.tmpC
pc63  GOSUB   es.SP.PRO.LOADCG.MERGE (ID=SPLBUF602, PSTR2=sp.tmpB)
      → CGACT ID=sp.tmpB ID2=SPLBUF602 X=0 Y=0 MERGE=1
pc64  CGEND   ID=sp.tmpB                          ← 消费者命令之后才删
```

`YU_PASS_TRACE` 同一帧全部发射:

```text
seq 2903 COPY  sp.tmpA → SPLBUF602
seq 2904 IMG   sp.tmpC → sp.tmpB   Merge
seq 2905 IMG   SPLBUF602 → SPLBUF602 RectPaint3
seq 2906 IMG   sp.tmpB → SPLBUF602 Merge
```

## 官方行为

`CGACT` 在命令体内立即执行(`FUN_0042f809`,EVID-T-15):`MERGE` 读上层
`sp.tmpC` 当前表面、写入下层 `sp.tmpB` 表面;随后 `CGEND sp.tmpC` 只释放
对象,不撤销已经写进 `sp.tmpB` 的像素。`MERGE sp.tmpB → SPLBUF602`
读到的是**合成后的** `sp.tmpB`。

## 内核行为(修前)

1. 生产者 `MERGE tmpC→tmpB` 的回放发生在帧末,此时 `tmpB` 已被 `CGEND`,
   `frame.layers` 里没有它;
2. wgpu `apply_image` 计算目标基面 token 时,只对**原地**动作使用命令时刻
   纹理,非原地且活层缺失时退化成 `None`(hash 常量 5);
3. 消费者 `MERGE tmpB→SPLBUF602` 的源 token 使用命令时刻 `source_texture`
   (`Asset(..._01)`,hash 1+名字);
4. 消费者在 `apply_image` 开头 `sync(src)`:`sync_base` 发现两者 token
   不同 ⇒ **清掉 `contents[tmpB]`**(刚合好的腮红结果);
5. `resolve_source(tmpB)` 退回命令时刻基图资产 `_01` —— 只有眼睛差分,
   没有腮红。

软后端没有这个分歧:它按帧内 `texture_overrides` 链式传递,同一序列输出
正确(回归测试的 soft 像素即参考值)。

## 根因

帧末回放下「命令时刻基面身份」在**生产端与消费端不对称**:消费端按
`ImageOp.source_texture`(命令时刻纹理)算 token,生产端对已删目标按
`None` 算 token。同一份层基面被哈希成两个值,内容被误判为“基面变了”。

## 修法

`crates/yu-render/wgpu/src/plan.rs::apply_image`:非原地动作的目标基面
token 不再退化成 `None`,改用 `ImageOp.target_base.texture`(命令时刻
记录的基面,与消费者 `source_texture` 同源)。原地动作仍用
`source_texture`。

## 回归

- `crates/yu-render/wgpu/tests/wgpu.rs::
  merge_chain_keeps_command_time_base_when_intermediate_is_deleted`
  - 修前: `gpu=[0,255,0,255] soft=[255,0,0,255] skips=[]`
    (GPU 取到中间层裸基图,软后端正确);
  - 修后: 通过;同一采样点 GPU/soft 误差 ≤2(alpha 混合量化容差)。
- `cargo test -p yu-backend-wgpu --test wgpu` → 40 passed;
- `cargo test -p yu-gfx --tests` → 各套件全绿。

## 取证工具(可保留)

- `YU_GFX_CMD_TRACE=1`:逐条打印 Graphics/Window/Movie 域命令
  (`[gfxcmd] f= s= pc= 名称 槽:值 …`),默认关闭、OnceLock 只读一次;
- `YU_PASS_TRACE=1`(既有):打印实际发射的 `[texop]` 回放序;
- 两者组合可在不对帧号、不依赖 F9 的情况下复现“随机动作”前后的完整
  写入顺序。

## 残余

- `ImageOp` 只捕获 `source_texture`;`source2/source3`(过渡的 `to`
  与遮罩)仍按活层解析,若同帧被 `CGEND` 需要同样的命令时刻纹理字段,
  单列后续。
