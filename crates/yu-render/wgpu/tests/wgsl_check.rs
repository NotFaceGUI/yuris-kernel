//! WGSL 语法自检:新着色器必须能被 naga 解析(否则管线创建只会报
//! "ShaderModule is invalid")。

#[test]
fn shaders_parse() {
    for (name, source) in [
        ("transform", include_str!("../src/shaders/transform.wgsl")),
        ("composite", include_str!("../src/shaders/composite.wgsl")),
        ("resample", include_str!("../src/shaders/resample.wgsl")),
    ] {
        let module = naga::front::wgsl::parse_str(source);
        if let Err(err) = module {
            panic!("{name}.wgsl 解析失败: {err:?}");
        }
    }
}
