# yu-movie-android

Android **外置** `MOVIE` 后端:把解码交给设备系统媒体栈。

- 容器拆包:`AMediaExtractor`(fd 输入;整段影片先落 app cache 临时文件)。
- 视频解码:`AMediaCodec` **ByteBuffer 输出**(不传 Surface);按输出格式的
  `color-format`/`stride`/`slice-height`/crop 转 RGBA,兼容 I420/NV12。
  Surface/ImageReader 在高通 UBWC(`0x7fa30c06`)上拿不到 U/V CPU 指针,
  因此不走那条路。
- 音频解码:`AMediaCodec` ByteBuffer → i16 → 立体声降混,交给 `yu-sound`。
- 设备能力:`probe_video_codecs()` 对固定 MIME 表逐项
  `AMediaCodec_createDecoderByType`(结果由 Android 壳的
  `nativeMovieCodecs` 展示);硬件/软件标记由 Java `MediaCodecList` 补足。

## 使用

宿主构造 `MovieHostFactory`,塞进 `EngineOptions.movie_external_factory`:

```rust
options.movie_backend = MovieBackend::Auto; // 缺省
options.movie_external_factory = Some(MovieHostFactory::new(move || {
    Box::new(AndroidMovieHost::new(cache_dir.clone()))
}));
```

`auto` 下引擎先试外置,`open` 失败回退内置 pl_mpeg;`external` 不静默回退。

## 限制

- Android 官方保证格式不含 MPEG-PS;官方 MPEG-1 语料走内置回退。
- AVI/MPEG-2 是否可解取决于设备 extractor/decoder。
- 影片整段字节会多落一份临时文件(extractor 只接 fd/URI),`close`/Drop 删除。
- 运行时解码错误只能结束播放并记日志(`MovieHost::frame` 没有错误通道);
  `open` 阶段失败才触发回退。
- 颜色转换固定 BT.601 limited range(与内置 pl_mpeg 同口径);BT.709 的
  H.264/MP4 可能有轻微色偏,后续按 `color-standard` 选择矩阵。
- 非 Android 构建只编译纯转换函数(`convert`),用于单测。
