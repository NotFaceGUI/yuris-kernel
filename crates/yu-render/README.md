# yu-render

渲染实现与共享契约的目录索引。这里不是单个 crate,而是三层:

| 路径 | crate | 定位 |
|---|---|---|
| [`core/`](core) | `yu-render-core` | 两后端共享的**数据契约与平台设施**,不含后端行为 |
| [`soft/`](soft) | `yu-backend-soft` | CPU 参考实现;确定性逐字节可复现 |
| [`wgpu/`](wgpu) | `yu-backend-wgpu` | GPU 主后端;桌面与 Android 上屏 |

依赖方向只有一条:

```text
yu-gfx(语义层,CompositorFrame)
   ├─ yu-render-core(契约:Surface / RenderReport / TextureStore / TextComposer)
   ├─ yu-backend-soft ──┐
   └─ yu-backend-wgpu ──┴─ 互为实现,不互为真值
```

- **`yu-gfx` 决定画什么**;两个后端只执行显示列表,不得修改语义。
- soft 与 wgpu 是两条独立实现,共享契约但不共享像素算法;pass 顺序、
  可执行/跳过计数、跳过原因必须逐项对齐,像素允许舍入差。
- `core` 里出现后端特有行为即越界;像素算法留在各自后端。

边界与证据:`docs/architecture/backends.md`、`docs/render-parity.md`。
