//! 全局纹理过滤与扩展名探测序。
//!
//! 旧线“固定 Linear”是已知 ❌;本测试钉死:过滤来自宿主/引擎设置,
//! gfx 不默认 Linear。

mod support;

use support::*;
use yu_gfx::{
    probe_order_from_codes, FilterMode, FilterSource, GfxConfig, GfxState, ImageFormat, MipFilter,
    TextureFilter, Viewport,
};

#[test]
fn default_filter_is_not_linear_and_is_marked_placeholder() {
    let gfx = state();
    let f = gfx.frame();
    assert_eq!(f.filter.mag, FilterMode::Point);
    assert_eq!(f.filter.min, FilterMode::Point);
    assert_eq!(f.filter.mip, MipFilter::None);
    assert_eq!(f.filter.source, FilterSource::Placeholder);
}

#[test]
fn host_can_set_engine_filter() {
    let mut gfx = GfxState::new(table(), GfxConfig::new(Viewport::new(800, 600)));
    gfx.set_filter(TextureFilter::engine_settings(
        FilterMode::Linear,
        FilterMode::Anisotropic,
        MipFilter::Linear,
    ));
    let f = gfx.frame();
    assert_eq!(f.filter.mag, FilterMode::Linear);
    assert_eq!(f.filter.min, FilterMode::Anisotropic);
    assert_eq!(f.filter.mip, MipFilter::Linear);
    assert_eq!(f.filter.source, FilterSource::EngineSettings);
}

#[test]
fn filter_codes_match_engine_settings() {
    assert_eq!(FilterMode::from_code(0), Some(FilterMode::Point));
    assert_eq!(FilterMode::from_code(1), Some(FilterMode::Linear));
    assert_eq!(FilterMode::from_code(2), Some(FilterMode::Anisotropic));
    assert_eq!(FilterMode::from_code(3), None);
    assert_eq!(MipFilter::from_code(-1), Some(MipFilter::None));
    assert_eq!(MipFilter::from_code(0), Some(MipFilter::Point));
    assert_eq!(MipFilter::from_code(1), Some(MipFilter::Linear));
    assert_eq!(MipFilter::from_code(2), Some(MipFilter::Anisotropic));
    assert_eq!(MipFilter::from_code(3), None);
}

#[test]
fn probe_order_from_eris_yscfg() {
    let order = probe_order_from_codes(&[1, 2, 3, 4, 5, 0, 0, 0]);
    assert_eq!(
        order,
        vec![
            ImageFormat::Png,
            ImageFormat::Bmp,
            ImageFormat::Jpg,
            ImageFormat::Gif,
            ImageFormat::Other(5)
        ]
    );
    assert_eq!(ImageFormat::Png.extension(), Some(".png"));
    assert_eq!(ImageFormat::Bmp.extension(), Some(".bmp"));
    assert_eq!(ImageFormat::Jpg.extension(), Some(".jpg"));
    assert_eq!(ImageFormat::Gif.extension(), Some(".gif"));
}

#[test]
fn default_probe_order_is_manual_order() {
    let f = state().frame();
    assert_eq!(
        f.image_probe_order,
        vec![
            ImageFormat::Png,
            ImageFormat::Bmp,
            ImageFormat::Jpg,
            ImageFormat::Gif
        ]
    );
}

#[test]
fn filter_is_global_not_layer_specific() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("L")),
                arg(&table, "CG", "FILE", s("l")),
                arg(&table, "CG", "MIPMAP", i(1)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    // MIPMAP 逐层参数不在本版语义内:原样留档,不改变全局 filter。
    assert_eq!(f.filter.mag, FilterMode::Point);
    let l = f
        .layers
        .iter()
        .find(|l| l.id.display() == ":/W0/L")
        .unwrap();
    assert!(l.extra.contains_key("MIPMAP"));
}
