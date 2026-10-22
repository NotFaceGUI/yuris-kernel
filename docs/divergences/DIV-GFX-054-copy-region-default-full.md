# DIV-GFX-054 · COPY 区域未指定坐标应取整幅,而不是 0

- 状态: **fixed(AU1,2026-10-22)**
- 优先级: 中(所有“只给部分坐标”的切片复制)
- 发现日期 / 方式: 2026-10-22 / `eris_dump` 的 copy region dump

## 现象

按钮切片命令是:

```text
CGACT[ID="BT.OFF.TMP" ID2=$off COPY2=1 X=0 X2=@sx-1]
```

即只给 `X/X2`,不给 `Y/Y2`。旧实现:

```rust
CopyRegion::Partial {
    x: x.unwrap_or(0),
    y: y.unwrap_or(0),
    x2: x2.unwrap_or(0),
    y2: y2.unwrap_or(0),
}
```

⇒ 区域变成 `(0,0)-(sx-1,0)`,**只复制首行**。

dump 里的实际输出就是 `{"x":0,"y":0,"x2":-1,"y2":0}`。

## 证据

- 真实脚本:`es_Button.yst:2381/2435/2490/2537/2584` 全部只给 `X/X2`;
- 官方 title 按钮是完整高度(不是 1px 横线);
- 手册 `cg/cgact.html` 例 29/30 只给部分坐标时仍是整层截图;
- `j2-micro-copy` 显式给全四个坐标,行为不受影响(EVID-T-11)。

## 修法

```rust
pub enum CopyRegion {
    Full,
    Partial { x: Option<i64>, y: Option<i64>, x2: Option<i64>, y2: Option<i64> },
}

impl CopyRegion {
    /// 缺省:x0/y0 = 0,x1 = 宽−1,y1 = 高−1。显式负值/越界由调用方裁剪。
    pub fn resolve(&self, width: u32, height: u32) -> (i64, i64, i64, i64) { ... }
}
```

两端(soft `apply_copy_op`、wgpu `plan_copy` / `resolve_cpu_source`)统一用
`resolve()`,不再各自 destructure。

## 回归

- `crates/yu-backend-soft/tests/render.rs::copy_slices_full_height_after_source_layer_is_gone`
  (X/X2 only + 源层删除,两条一起验证);
- JSON 序列化改为 `null` 表示“未指定”(`json::copy_region`);
- golden 与 `cases/*.json` 已重生成。
