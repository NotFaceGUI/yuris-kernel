# 第三方许可与声明

<!-- 本文件由 tools/release/gen_third_party_notices.py 生成，请勿手工编辑。 -->

> 生成依据：`cargo metadata --locked`（`Cargo.lock` sha256 `4ed742692a0b75127ad737648d28e9cb7611dd8bea17c25b9ebb21ed35674af6`）；第三方 crate 共 **426** 个。
>
> 本项目自身代码以 **GPL-3.0-or-later** 发布，见 [`LICENSE`](LICENSE) 与 [`NOTICE`](NOTICE)。本文件随发行产物一并分发；许可证全文见 [`third_party/licenses/`](third_party/licenses/)。

## 1. 随仓库分发的第三方组件

| 组件 | 位置 | 许可 | 许可文本 |
| --- | --- | --- | --- |
| swash（本地 fork） | `third_party/swash/` | Apache-2.0 OR MIT | `third_party/swash/LICENSE-APACHE`、`LICENSE-MIT` |
| egui 默认字体（`epaint_default_fonts`） | crate 依赖 | OFL-1.1 AND Ubuntu-font-1.0 | `third_party/licenses/OFL-1.1.txt`、`third_party/licenses/Ubuntu-font-1.0.txt` |
| libpng（`libpng-sys` vendored） | crate 依赖 | Libpng | `third_party/licenses/Libpng.txt` |
| `yu-emote/` 子模块 | 独立仓库 | MIT OR Apache-2.0 | 见子模块 `LICENSE-*`；不属于本工作区构建 |

## 2. Rust crate 依赖清单

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| ab_glyph | 0.2.32 | Apache-2.0 | https://github.com/alexheretic/ab-glyph |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | https://github.com/alexheretic/ab-glyph |
| accesskit | 0.25.1 | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 |
| ahash | 0.8.12 | MIT OR Apache-2.0 | https://github.com/tkaitchuck/ahash |
| aho-corasick | 1.1.5 | Unlicense OR MIT | https://github.com/BurntSushi/aho-corasick |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | https://github.com/zakarumych/allocator-api2 |
| alsa | 0.9.1 | Apache-2.0/MIT | https://github.com/diwic/alsa-rs |
| alsa-sys | 0.3.1 | MIT | https://github.com/diwic/alsa-sys |
| android-activity | 0.6.1 | MIT OR Apache-2.0 | https://github.com/rust-mobile/android-activity |
| android-properties | 0.2.2 | MIT | https://github.com/miklelappo/android-properties |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | https://github.com/nical/android_system_properties |
| arboard | 3.7.0 | MIT OR Apache-2.0 | https://github.com/1Password/arboard |
| arrayref | 0.3.9 | BSD-2-Clause | https://github.com/droundy/arrayref |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | https://github.com/bluss/arrayvec |
| as-raw-xcb-connection | 1.0.1 | MIT OR Apache-2.0 | https://github.com/psychon/as-raw-xcb-connection |
| ash | 0.38.0+1.3.281 | MIT OR Apache-2.0 | https://github.com/ash-rs/ash |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | https://github.com/smol-rs/atomic-waker |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | https://github.com/cuviper/autocfg |
| bindgen | 0.72.1 | BSD-3-Clause | https://github.com/rust-lang/rust-bindgen |
| bit-set | 0.10.0 | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-set |
| bit-vec | 0.9.1 | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec |
| bitflags | 1.3.2 | MIT/Apache-2.0 | https://github.com/bitflags/bitflags |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| block2 | 0.5.1 | MIT | https://github.com/madsmtm/objc2 |
| block2 | 0.6.2 | MIT | https://github.com/madsmtm/objc2 |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | https://github.com/fitzgen/bumpalo |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| byteorder | 1.5.0 | Unlicense OR MIT | https://github.com/BurntSushi/byteorder |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | https://github.com/image-rs/byteorder-lite |
| bytes | 1.12.1 | MIT | https://github.com/tokio-rs/bytes |
| calloop | 0.13.0 | MIT | https://github.com/Smithay/calloop |
| calloop | 0.14.5 | MIT | https://github.com/Smithay/calloop |
| calloop-wayland-source | 0.3.0 | MIT | https://github.com/smithay/calloop-wayland-source |
| calloop-wayland-source | 0.4.1 | MIT | https://github.com/smithay/calloop-wayland-source |
| cc | 1.9.0 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| cesu8 | 1.1.0 | Apache-2.0/MIT | https://github.com/emk/cesu8-rs |
| cexpr | 0.6.0 | Apache-2.0/MIT | https://github.com/jethrogb/rust-cexpr |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| cfg_aliases | 0.2.2 | MIT | https://github.com/katharostech/cfg_aliases |
| clang-sys | 1.9.1 | Apache-2.0 | https://github.com/KyleMayes/clang-sys |
| clipboard-win | 5.4.1 | BSL-1.0 | https://github.com/DoumanAsh/clipboard-win |
| codespan-reporting | 0.13.1 | Apache-2.0 | https://github.com/brendanzab/codespan |
| color | 0.3.3 | Apache-2.0 OR MIT | https://github.com/linebender/color |
| color_quant | 1.1.0 | MIT | https://github.com/image-rs/color_quant.git |
| combine | 4.6.9 | MIT | https://github.com/Marwes/combine |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/concurrent-queue |
| core-foundation | 0.9.4 | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| core-graphics | 0.23.2 | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| core-graphics-types | 0.1.3 | MIT OR Apache-2.0 | https://github.com/servo/core-foundation-rs |
| core_detect | 1.0.0 | MIT/Apache-2.0 | https://github.com/thomcc/core_detect |
| coreaudio-rs | 0.11.3 | MIT/Apache-2.0 | https://github.com/RustAudio/coreaudio-rs.git |
| coreaudio-sys | 0.2.18 | MIT | https://github.com/RustAudio/coreaudio-sys.git |
| cpal | 0.15.3 | Apache-2.0 | https://github.com/rustaudio/cpal |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crunchy | 0.2.4 | MIT | https://github.com/eira-fransham/crunchy |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/cursor-icon |
| dasp_sample | 0.11.0 | MIT OR Apache-2.0 | https://github.com/rustaudio/sample.git |
| deranged | 0.5.9 | MIT OR Apache-2.0 | https://github.com/jhpratt/deranged |
| digest | 0.10.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| dispatch | 0.2.0 | MIT | http://github.com/SSheldon/rust-dispatch |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | https://github.com/yaahc/displaydoc |
| dlib | 0.5.3 | MIT | https://github.com/elinorbgr/dlib |
| document-features | 0.2.12 | MIT OR Apache-2.0 | https://github.com/slint-ui/document-features |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 | https://github.com/marcianx/downcast-rs |
| dpi | 0.1.2 | Apache-2.0 AND MIT | https://github.com/rust-windowing/winit |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | https://gitlab.com/kornelski/dunce |
| ecolor | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| egui | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui |
| egui-wgpu | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/egui-wgpu |
| egui-winit | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/egui-winit |
| either | 1.19.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/either |
| emath | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/emath |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | https://github.com/hsivonen/encoding_rs |
| epaint | 0.37.0 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/main/crates/epaint |
| epaint_default_fonts | 0.37.0 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| errno | 0.3.14 | MIT OR Apache-2.0 | https://github.com/lambda-fairy/rust-errno |
| error-code | 3.4.0 | BSL-1.0 | https://github.com/DoumanAsh/error-code |
| euclid | 0.22.14 | MIT OR Apache-2.0 | https://github.com/servo/euclid |
| fax | 0.2.7 | MIT | https://github.com/pdf-rs/fax |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | https://github.com/image-rs/fdeflate |
| fearless_simd | 0.4.1 | Apache-2.0 OR MIT | https://github.com/linebender/fearless_simd |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | https://github.com/petgraph/fixedbitset |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| foldhash | 0.1.5 | Zlib | https://github.com/orlp/foldhash |
| foldhash | 0.2.0 | Zlib | https://github.com/orlp/foldhash |
| font-types | 0.12.7 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| fontdb | 0.24.0 | MIT | https://github.com/RazrFalcon/fontdb |
| foreign-types | 0.5.0 | MIT/Apache-2.0 | https://github.com/sfackler/foreign-types |
| foreign-types-macros | 0.2.4 | MIT/Apache-2.0 | https://github.com/sfackler/foreign-types |
| foreign-types-shared | 0.3.1 | MIT/Apache-2.0 | https://github.com/sfackler/foreign-types |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| generic-array | 0.14.7 | MIT | https://github.com/fizyk20/generic-array.git |
| gethostname | 1.1.0 | Apache-2.0 | https://codeberg.org/swsnr/gethostname.rs.git |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| gif | 0.14.2 | MIT OR Apache-2.0 | https://github.com/image-rs/image-gif |
| gl_generator | 0.14.0 | Apache-2.0 | https://github.com/brendanzab/gl-rs/ |
| glifo | 0.3.0 | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| glob | 0.3.4 | MIT OR Apache-2.0 | https://github.com/rust-lang/glob |
| glow | 0.17.0 | MIT OR Apache-2.0 OR Zlib | https://github.com/grovesNL/glow |
| glutin_wgl_sys | 0.6.1 | Apache-2.0 | https://github.com/rust-windowing/glutin |
| gpu-allocator | 0.28.0 | MIT OR Apache-2.0 | https://github.com/Traverse-Research/gpu-allocator |
| guillotiere | 0.7.0 | MIT/Apache-2.0 | https://github.com/nical/guillotiere |
| half | 2.7.1 | MIT OR Apache-2.0 | https://github.com/VoidStarKat/half-rs |
| harfrust | 0.12.0 | MIT | https://github.com/harfbuzz/harfrust |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| hermit-abi | 0.5.3 | MIT OR Apache-2.0 | https://github.com/hermit-os/hermit-rs |
| hound | 3.5.1 | Apache-2.0 | https://github.com/ruuda/hound |
| icu_collections | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| icu_locale_core | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| icu_normalizer | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| icu_normalizer_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| icu_properties | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| icu_properties_data | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| icu_provider | 2.4.0 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| idna | 1.1.0 | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | https://github.com/hsivonen/idna_adapter |
| image | 0.25.10 | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| itertools | 0.13.0 | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools |
| itertools | 0.15.0 | MIT OR Apache-2.0 | https://github.com/rust-itertools/itertools |
| jni | 0.21.1 | MIT/Apache-2.0 | https://github.com/jni-rs/jni-rs |
| jni | 0.22.4 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-rs |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-rs |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 | https://github.com/jni-rs/jni-sys |
| jobserver | 0.1.35 | MIT OR Apache-2.0 | https://github.com/rust-lang/jobserver-rs |
| js-sys | 0.3.104 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys |
| khronos-egl | 6.0.0 | MIT/Apache-2.0 | https://github.com/timothee-haudebourg/khronos-egl |
| khronos_api | 3.1.0 | Apache-2.0 | https://github.com/brendanzab/gl-rs/ |
| kurbo | 0.13.1 | Apache-2.0 OR MIT | https://github.com/linebender/kurbo |
| lewton | 0.10.2 | MIT OR Apache-2.0 | https://github.com/RustAudio/lewton |
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| libloading | 0.8.9 | ISC | https://github.com/nagisa/rust_libloading/ |
| libm | 0.2.16 | MIT | https://github.com/rust-lang/compiler-builtins |
| libmimalloc-sys | 0.1.49 | MIT | https://github.com/purpleprotocol/mimalloc_rust/tree/master/libmimalloc-sys |
| libpng-sys | 1.1.11 | Libpng | https://github.com/kornelski/rust-libpng-sys.git |
| libredox | 0.1.25 | MIT | https://gitlab.redox-os.org/redox-os/libredox.git |
| libz-sys | 1.1.22 | MIT OR Apache-2.0 | https://github.com/rust-lang/libz-sys |
| linebender_resource_handle | 0.1.1 | Apache-2.0 OR MIT | https://github.com/linebender/raw_resource_handle |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys |
| linux-raw-sys | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys |
| litemap | 0.8.3 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| litrs | 1.0.1 | MIT OR Apache-2.0 | https://github.com/LukasKalbertodt/litrs |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| log | 0.4.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | https://github.com/JohnTitor/mach2 |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| memmap2 | 0.9.12 | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs |
| mimalloc | 0.1.52 | MIT | https://github.com/purpleprotocol/mimalloc_rust |
| minimal-lexical | 0.2.1 | MIT/Apache-2.0 | https://github.com/Alexhuszagh/minimal-lexical |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/moxcms.git |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT | https://github.com/hsivonen/multiversion_no_op |
| naga | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| naga-types | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| ndk | 0.8.0 | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk |
| ndk | 0.9.0 | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | https://github.com/rust-windowing/android-ndk-rs |
| ndk-sys | 0.5.0+25.2.9519653 | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | https://github.com/rust-mobile/ndk |
| nohash-hasher | 0.2.0 | Apache-2.0 OR MIT | https://github.com/paritytech/nohash-hasher |
| nom | 7.1.3 | MIT | https://github.com/Geal/nom |
| nom | 8.0.0 | MIT | https://github.com/rust-bakery/nom |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | https://github.com/jhpratt/num-conv |
| num-derive | 0.4.2 | MIT OR Apache-2.0 | https://github.com/rust-num/num-derive |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | https://github.com/illicitonion/num_enum |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | https://github.com/illicitonion/num_enum |
| num_threads | 0.1.8 | MIT OR Apache-2.0 | https://github.com/jhpratt/num_threads |
| objc-sys | 0.3.5 | MIT | https://github.com/madsmtm/objc2 |
| objc2 | 0.5.3 | MIT | https://github.com/madsmtm/objc2 |
| objc2 | 0.6.5 | MIT | https://github.com/madsmtm/objc2 |
| objc2-app-kit | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-cloud-kit | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-contacts | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-core-data | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-core-image | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-core-location | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-encode | 4.1.0 | MIT | https://github.com/madsmtm/objc2 |
| objc2-foundation | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-foundation | 0.3.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-io-surface | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-link-presentation | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-metal | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-metal | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-quartz-core | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-quartz-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-symbols | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-ui-kit | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-ui-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/madsmtm/objc2 |
| objc2-uniform-type-identifiers | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| objc2-user-notifications | 0.2.2 | MIT | https://github.com/madsmtm/objc2 |
| oboe | 0.6.1 | Apache-2.0 | https://github.com/katyo/oboe-rs |
| oboe-sys | 0.6.1 | Apache-2.0 | https://github.com/katyo/oboe-rs |
| ogg | 0.8.0 | BSD-3-Clause | https://github.com/RustAudio/ogg |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| orbclient | 0.3.55 | MIT | https://gitlab.redox-os.org/redox-os/orbclient |
| ordered-float | 5.5.0 | MIT | https://github.com/reem/rust-ordered-float |
| os_pipe | 1.2.3 | MIT | https://github.com/oconnor663/os_pipe.rs |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | https://github.com/alexheretic/owned-ttf-parser |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| peniko | 0.6.1 | Apache-2.0 OR MIT | https://github.com/linebender/peniko |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | https://github.com/petgraph/petgraph |
| pin-project | 1.1.13 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project |
| pin-project-internal | 1.1.13 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/pkg-config-rs |
| plain | 0.2.3 | MIT/Apache-2.0 | https://github.com/randomites/plain |
| png | 0.17.16 | MIT OR Apache-2.0 | https://github.com/image-rs/image-png |
| png | 0.18.1 | MIT OR Apache-2.0 | https://github.com/image-rs/image-png |
| polling | 3.11.0 | Apache-2.0 OR MIT | https://github.com/smol-rs/polling |
| pollster | 0.4.0 | Apache-2.0/MIT | https://github.com/zesterer/pollster |
| polycool | 0.4.0 | MIT OR Apache-2.0 | https://github.com/linebender/kurbo |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | https://github.com/taiki-e/portable-atomic-util |
| potential_utf | 0.1.6 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 | https://github.com/jhpratt/powerfmt |
| presser | 0.3.1 | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/presser |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | https://github.com/bkchr/proc-macro-crate |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| profiling | 1.0.18 | MIT OR Apache-2.0 | https://github.com/aclysma/profiling |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/pxfm |
| quick-error | 2.0.1 | MIT/Apache-2.0 | http://github.com/tailhook/quick-error |
| quick-xml | 0.41.0 | MIT | https://github.com/tafia/quick-xml |
| quote | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| r-efi | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | https://github.com/r-efi/r-efi |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | https://github.com/r-efi/r-efi |
| range-alloc | 0.1.5 | MIT OR Apache-2.0 | https://github.com/gfx-rs/range-alloc |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle |
| raw-window-metal | 1.1.0 | MIT OR Apache-2.0 | https://github.com/rust-windowing/raw-window-metal |
| read-fonts | 0.41.0 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| redox_syscall | 0.4.1 | MIT | https://gitlab.redox-os.org/redox-os/syscall |
| redox_syscall | 0.5.18 | MIT | https://gitlab.redox-os.org/redox-os/syscall |
| redox_syscall | 0.9.5 | MIT | https://gitlab.redox-os.org/redox-os/kernel |
| regex | 1.13.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| renderdoc-sys | 1.1.0 | MIT OR Apache-2.0 | https://github.com/ebkalderon/renderdoc-rs |
| rustc-hash | 1.1.0 | Apache-2.0/MIT | https://github.com/rust-lang-nursery/rustc-hash |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | https://github.com/rust-lang/rustc-hash |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | https://github.com/djc/rustc-version-rs |
| rustix | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix |
| rustix | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion |
| same-file | 1.0.6 | Unlicense/MIT | https://github.com/BurntSushi/same-file |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | https://github.com/alexcrichton/scoped-tls |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| sctk-adwaita | 0.10.1 | MIT | https://github.com/PolyMeilex/sctk-adwaita |
| self_cell | 1.3.0 | Apache-2.0 OR GPL-2.0-only | https://github.com/Voultapher/self_cell |
| semver | 1.0.28 | MIT OR Apache-2.0 | https://github.com/dtolnay/semver |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| sha1 | 0.10.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| shlex | 1.3.0 | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| shlex | 2.0.1 | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| simd-adler32 | 0.3.10 | MIT | https://github.com/mcountryman/simd-adler32 |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT | https://github.com/seancroach/simd_cesu8 |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | https://github.com/rusticstuff/simdutf8 |
| skrifa | 0.44.0 | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| slab | 0.4.12 | MIT | https://github.com/tokio-rs/slab |
| slotmap | 1.1.1 | Zlib | https://github.com/orlp/slotmap |
| smallvec | 1.16.3 | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| smithay-client-toolkit | 0.19.2 | MIT | https://github.com/smithay/client-toolkit |
| smithay-client-toolkit | 0.20.0 | MIT | https://github.com/smithay/client-toolkit |
| smithay-clipboard | 0.7.3 | MIT | https://github.com/smithay/smithay-clipboard |
| smol_str | 0.2.2 | MIT OR Apache-2.0 | https://github.com/rust-analyzer/smol_str |
| southstar-build | 1.0.5 | LicenseRef-NSL-1.0 OR GPL-3.0-or-later | https://github.com/nordstjernen-web/southstar-browser |
| southstar-codecs | 1.0.5 | (LicenseRef-NSL-1.0 OR GPL-3.0-or-later) AND (MIT) | https://github.com/nordstjernen-web/southstar-browser |
| spirv | 0.4.0+sdk-1.4.341.0 | Apache-2.0 | https://github.com/gfx-rs/rspirv |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | https://github.com/storyyeller/stable_deref_trait |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs |
| strict-num | 0.1.1 | MIT | https://github.com/RazrFalcon/strict-num |
| syn | 2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| syn | 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| synstructure | 0.14.0 | MIT | https://github.com/mystor/synstructure |
| termcolor | 1.4.1 | Unlicense OR MIT | https://github.com/BurntSushi/termcolor |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| tiff | 0.11.3 | MIT | https://github.com/image-rs/image-tiff |
| time | 0.3.55 | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| time-core | 0.1.9 | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| tiny-skia | 0.11.4 | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia |
| tiny-skia-path | 0.11.4 | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia/tree/master/path |
| tinystr | 0.8.5 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_edit | 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| tracing | 0.1.44 | MIT | https://github.com/tokio-rs/tracing |
| tracing-core | 0.1.36 | MIT | https://github.com/tokio-rs/tracing |
| tree_magic_mini | 3.2.2 | MIT | https://github.com/mbrubeck/tree_magic/ |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | https://github.com/harfbuzz/ttf-parser |
| type-map | 0.5.1 | MIT/Apache-2.0 | https://github.com/kardeiz/type-map |
| typenum | 1.20.1 | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| unicode-general-category | 1.1.0 | Apache-2.0 | https://github.com/yeslogic/unicode-general-category |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| unicode-properties | 0.1.4 | MIT/Apache-2.0 | https://github.com/unicode-rs/unicode-properties |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-width |
| url | 2.5.8 | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | https://github.com/hsivonen/utf8_iter |
| uuid | 1.28.0 | Apache-2.0 OR MIT | https://github.com/uuid-rs/uuid |
| vcpkg | 0.2.15 | MIT/Apache-2.0 | https://github.com/mcgoo/vcpkg-rs |
| vello_common | 0.2.0 | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| vello_cpu | 0.2.0 | Apache-2.0 OR MIT | https://github.com/linebender/vello |
| version_check | 0.9.5 | MIT/Apache-2.0 | https://github.com/SergioBenitez/version_check |
| walkdir | 2.5.0 | Unlicense/MIT | https://github.com/BurntSushi/walkdir |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/wasi-rs |
| wasm-bindgen | 0.2.127 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen |
| wasm-bindgen-futures | 0.4.77 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures |
| wasm-bindgen-macro | 0.2.127 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro |
| wasm-bindgen-macro-support | 0.2.127 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro-support |
| wasm-bindgen-shared | 0.2.127 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared |
| wayland-backend | 0.3.17 | MIT | https://github.com/smithay/wayland-rs |
| wayland-client | 0.31.15 | MIT | https://github.com/smithay/wayland-rs |
| wayland-csd-frame | 0.3.0 | MIT | https://github.com/rust-windowing/wayland-csd-frame |
| wayland-cursor | 0.31.14 | MIT | https://github.com/smithay/wayland-rs |
| wayland-protocols | 0.32.13 | MIT | https://github.com/smithay/wayland-rs |
| wayland-protocols-experimental | 20250721.0.1 | MIT | https://github.com/smithay/wayland-rs |
| wayland-protocols-misc | 0.3.12 | MIT | https://github.com/smithay/wayland-rs |
| wayland-protocols-plasma | 0.3.12 | MIT | https://github.com/smithay/wayland-rs |
| wayland-protocols-wlr | 0.3.12 | MIT | https://github.com/smithay/wayland-rs |
| wayland-scanner | 0.31.11 | MIT | https://github.com/smithay/wayland-rs |
| wayland-sys | 0.31.11 | MIT | https://github.com/smithay/wayland-rs |
| web-sys | 0.3.104 | MIT OR Apache-2.0 | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys |
| web-time | 1.1.0 | MIT OR Apache-2.0 | https://github.com/daxpedda/web-time |
| webbrowser | 1.2.4 | MIT OR Apache-2.0 | https://github.com/amodm/webbrowser-rs |
| weezl | 0.1.12 | MIT OR Apache-2.0 | https://github.com/image-rs/weezl |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-core | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-core-deps-apple | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-core-deps-emscripten | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-core-deps-wasm | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-core-deps-windows-linux-android | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-hal | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-naga-bridge | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| wgpu-types | 30.0.1 | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| winapi-util | 0.1.11 | Unlicense OR MIT | https://github.com/BurntSushi/winapi-util |
| windows | 0.54.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.54.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.1.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.45.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-targets | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_aarch64_gnullvm | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_aarch64_msvc | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_i686_gnu | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_i686_msvc | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_gnu | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_gnullvm | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_msvc | 0.42.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| winit | 0.30.13 | Apache-2.0 | https://github.com/rust-windowing/winit |
| winnow | 1.0.4 | MIT | https://github.com/winnow-rs/winnow |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/wit-bindgen |
| wl-clipboard-rs | 0.9.4 | MIT OR Apache-2.0 | https://github.com/YaLTeR/wl-clipboard-rs |
| writeable | 0.6.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| x11-dl | 2.21.0 | MIT | https://github.com/AltF02/x11-rs.git |
| x11rb | 0.13.2 | MIT OR Apache-2.0 | https://github.com/psychon/x11rb |
| x11rb-protocol | 0.13.2 | MIT OR Apache-2.0 | https://github.com/psychon/x11rb |
| xcursor | 0.3.11 | MIT | https://github.com/esposm03/xcursor-rs |
| xkbcommon-dl | 0.4.2 | MIT | https://github.com/rust-windowing/xkbcommon-dl |
| xkeysym | 0.2.1 | MIT OR Apache-2.0 OR Zlib | https://github.com/notgull/xkeysym |
| xml-rs | 0.8.29 | MIT | https://github.com/kornelski/xml-rs |
| yazi | 0.2.1 | Apache-2.0 OR MIT | https://github.com/dfrg/yazi |
| yoke | 0.8.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| yoke-derive | 0.8.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zeno | 0.3.3 | Apache-2.0 OR MIT | https://github.com/dfrg/zeno |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| zerofrom | 0.1.8 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zerotrie | 0.2.5 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zerovec | 0.11.9 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zerovec-derive | 0.11.7 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zlib-rs | 0.6.8 | Zlib | https://github.com/trifectatechfoundation/zlib-rs |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |

## 3. 版权声明

以下版权行摘自各包自带的 `LICENSE*` / `COPYING*` 文件；未列出的包请以其上游许可文件为准。

- **ahash 0.8.12** — Copyright (c) 2018 Tom Kaitchuck
- **aho-corasick 1.1.5** — Copyright (c) 2015 Andrew Gallant
- **alsa 0.9.1** — Copyright (c) 2015-2021 David Henningsson, and other contributors.
- **alsa-sys 0.3.1** — Copyright (c) 2018 diwic
- **android-properties 0.2.2** — Copyright (c) 2020 Mikhail Lappo
- **android_system_properties 0.1.6** — Copyright 2016 Nicolas Silva；Copyright (c) 2013 Nicolas Silva
- **arboard 3.7.0** — Copyright (c) 2022 The Arboard contributors
- **arrayref 0.3.9** — Copyright (c) 2015 David Roundy <roundyd@physics.oregonstate.edu>
- **as-raw-xcb-connection 1.0.1** — Copyright 2019 as-raw-xcb-connection Contributers
- **ash 0.38.0+1.3.281** — Copyright 2016 Maik Klein；Copyright (c) 2016 ASH
- **atomic-waker 1.1.2** — Copyright (c) 2016 Alex Crichton；Copyright (c) 2017 The Tokio Authors
- **autocfg 1.5.1** — Copyright (c) 2018 Josh Stone
- **bindgen 0.72.1** — Copyright (c) 2013, Jyun-Yan You
- **bit-set 0.10.0** — Copyright (c) 2026 The Rust Project Developers
- **bit-vec 0.9.1** — Copyright (c) 2023 The Rust Project Developers
- **bitflags 1.3.2** — Copyright (c) 2014 The Rust Project Developers
- **bitflags 2.13.2** — Copyright (c) 2014 The Rust Project Developers
- **block-buffer 0.10.4** — Copyright (c) 2018-2019 The RustCrypto Project Developers
- **bumpalo 3.20.3** — Copyright (c) 2019 Nick Fitzgerald
- **bytemuck 1.25.2** — Copyright (c) 2019 Daniel "Lokathor" Gee.
- **bytemuck_derive 1.12.1** — Copyright (c) 2019 Daniel "Lokathor" Gee.
- **byteorder 1.5.0** — Copyright (c) 2015 Andrew Gallant
- **byteorder-lite 0.1.0** — Copyright (c) 2015 Andrew Gallant
- **bytes 1.12.1** — Copyright (c) 2018 Carl Lerche
- **calloop 0.13.0** — Copyright (c) 2018 Victor Berger
- **calloop 0.14.5** — Copyright (c) 2018 Victor Berger
- **calloop-wayland-source 0.3.0** — Copyright (c) 2023 Kirill Chibisov
- **calloop-wayland-source 0.4.1** — Copyright (c) 2023 Kirill Chibisov
- **cc 1.9.0** — Copyright (c) 2014 Alex Crichton
- **cfg-if 1.0.5** — Copyright (c) 2014 Alex Crichton
- **cfg_aliases 0.2.2** — Copyright (c) 2020 Katharos Technology
- **color_quant 1.1.0** — Copyright (c) 2016 PistonDevelopers
- **combine 4.6.9** — Copyright (c) 2015 Markus Westerlind
- **core-foundation 0.9.4** — Copyright (c) 2012-2013 Mozilla Foundation
- **core-foundation-sys 0.8.7** — Copyright (c) 2012-2013 Mozilla Foundation
- **core-graphics 0.23.2** — Copyright (c) 2012-2013 Mozilla Foundation
- **core-graphics-types 0.1.3** — Copyright (c) 2012-2013 Mozilla Foundation
- **core_detect 1.0.0** — Copyright (c) 2017-2020 The Rust Project Developers
- **coreaudio-rs 0.11.3** — Copyright (c) 2015
- **coreaudio-sys 0.2.18** — Copyright (c) 2015
- **cpufeatures 0.2.17** — Copyright (c) 2020-2025 The RustCrypto Project Developers
- **crc32fast 1.5.2** — Copyright (c) 2018 Sam Rijs, Alex Crichton and contributors
- **crossbeam-utils 0.8.23** — Copyright (c) 2019 The Crossbeam Project Developers
- **crunchy 0.2.4** — Copyright 2017-2023 Eira Fransham.
- **crypto-common 0.1.7** — Copyright (c) 2021 RustCrypto Developers
- **cursor-icon 1.2.0** — Copyright 2023 Kirill Chibisov；Copyright (c) 2023 Kirill Chibisov
- **deranged 0.5.9** — Copyright 2024 Jacob Pratt et al.；Copyright (c) 2024 Jacob Pratt et al.
- **digest 0.10.7** — Copyright (c) 2017 Artyom Pavlov
- **dlib 0.5.3** — Copyright (c) 2015 Victor Berger
- **document-features 0.2.12** — Copyright (c) 2020 Olivier Goffart <ogoffart@sixtyfps.io>
- **downcast-rs 1.2.1** — Copyright (c) 2020 Ashish Myles and contributors
- **dpi 0.1.2** — Copyright (c) 2018 Jorge Aparicio；Copyright © 2005-2020 Rich Felker, et al.；Copyright © 1993,2004 Sun Microsystems or；Copyright © 2003-2011 David Schultz or
- **either 1.19.0** — Copyright (c) 2015
- **equivalent 1.0.3** — Copyright (c) 2016--2023
- **errno 0.3.14** — Copyright (c) 2014 Chris Wong
- **euclid 0.22.14** — Copyright (c) 2012-2013 Mozilla Foundation
- **fax 0.2.7** — Copyright © 2021 The pdf-rs contributers.
- **fearless_simd 0.4.1** — Copyright (c) 2018 Raph Levien
- **find-msvc-tools 0.1.15** — Copyright (c) 2014 Alex Crichton
- **fixedbitset 0.5.7** — Copyright (c) 2015-2017
- **flate2 1.1.10** — Copyright (c) 2014-2026 Alex Crichton
- **foldhash 0.1.5** — Copyright (c) 2024 Orson Peters
- **foldhash 0.2.0** — Copyright (c) 2024 Orson Peters
- **font-types 0.12.7** — Copyright 2019 Fontations Developers；Copyright (c) 2019 Fontations Developers
- **fontdb 0.24.0** — Copyright (c) 2020 Yevhenii Reizner
- **foreign-types 0.5.0** — Copyright (c) 2017 The foreign-types Developers
- **foreign-types-macros 0.2.4** — Copyright (c) 2017 The foreign-types Developers
- **foreign-types-shared 0.3.1** — Copyright (c) 2017 The foreign-types Developers
- **form_urlencoded 1.2.2** — Copyright (c) 2013-2016 The rust-url developers
- **futures-core 0.3.34** — Copyright (c) 2016 Alex Crichton；Copyright (c) 2017 The Tokio Authors
- **futures-task 0.3.34** — Copyright (c) 2016 Alex Crichton；Copyright (c) 2017 The Tokio Authors
- **futures-util 0.3.34** — Copyright (c) 2016 Alex Crichton；Copyright (c) 2017 The Tokio Authors
- **generic-array 0.14.7** — Copyright (c) 2015 Bartłomiej Kamiński
- **getrandom 0.3.4** — Copyright (c) 2018-2025 The rust-random Project Developers；Copyright (c) 2014 The Rust Project Developers
- **getrandom 0.4.3** — Copyright (c) 2018-2026 The rust-random Project Developers；Copyright (c) 2014 The Rust Project Developers
- **gif 0.14.2** — Copyright (c) 2015 nwin
- **glifo 0.3.0** — Copyright 2025 the Vello Authors
- **glob 0.3.4** — Copyright (c) 2014 The Rust Project Developers
- **glutin_wgl_sys 0.6.1** — Copyright 2022 Kirill Chibisov
- **gpu-allocator 0.28.0** — Copyright 2021 Traverse Research B.V.；Copyright (c) 2021 Traverse Research B.V.
- **guillotiere 0.7.0** — Copyright 2019 Nicolas Silva；Copyright (c) 2019 Nicolas Silva
- **harfrust 0.12.0** — Copyright (c) 2020 Yevhenii Reizner
- **hashbrown 0.15.5** — Copyright (c) 2016 Amanieu d'Antras
- **hashbrown 0.16.1** — Copyright (c) 2016 Amanieu d'Antras
- **hashbrown 0.17.1** — Copyright (c) 2016 Amanieu d'Antras
- **icu_collections 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **icu_locale_core 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **icu_normalizer 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **icu_normalizer_data 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **icu_properties 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **icu_properties_data 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **icu_provider 2.4.0** — Copyright © 2020-2024 Unicode, Inc.
- **idna 1.1.0** — Copyright (c) 2013-2025 The rust-url developers
- **indexmap 2.14.2** — Copyright (c) 2016--2017
- **itertools 0.13.0** — Copyright (c) 2015
- **itertools 0.15.0** — Copyright (c) 2015
- **jni 0.21.1** — Copyright (c) 2016 Prevoty, Inc. and jni-rs contributors
- **jni-sys 0.3.1** — Copyright (c) 2015 The rust-jni-sys Developers
- **jni-sys 0.4.1** — Copyright (c) 2015 The rust-jni-sys Developers
- **jobserver 0.1.35** — Copyright (c) 2014 Alex Crichton
- **js-sys 0.3.104** — Copyright (c) 2014 Alex Crichton
- **kurbo 0.13.1** — Copyright (c) 2018 Raph Levien
- **lewton 0.10.2** — Copyright (c) 2016 est31 <MTest31@outlook.com> and contributors
- **libloading 0.8.9** — Copyright © 2015, Simonas Kazlauskas
- **libm 0.2.16** — Copyright (c) 2018 Jorge Aparicio；Copyright © 2005-2020 Rich Felker, et al.；Copyright © 1993,2004 Sun Microsystems or；Copyright © 2003-2011 David Schultz or
- **libmimalloc-sys 0.1.49** — Copyright 2019 Octavian Oncescu
- **libredox 0.1.25** — Copyright (c) 2023 4lDO2
- **libz-sys 1.1.22** — Copyright (c) 2014 Alex Crichton；Copyright (c) 2020 Josh Triplett
- **litemap 0.8.3** — Copyright © 2020-2024 Unicode, Inc.
- **litrs 1.0.1** — Copyright (c) 2020 Project Developers
- **lock_api 0.4.14** — Copyright (c) 2016 The Rust Project Developers
- **log 0.4.34** — Copyright (c) 2014 The Rust Project Developers
- **mach2 0.4.3** — Copyright (c) 2019 Nick Fitzgerald, 2021 Yuki Okushi
- **memchr 2.8.3** — Copyright (c) 2015 Andrew Gallant
- **memmap2 0.9.12** — Copyright (c) 2020 Yevhenii Reizner；Copyright (c) 2015 Dan Burkert
- **mimalloc 0.1.52** — Copyright 2019 Octavian Oncescu
- **minimal-lexical 0.2.1** — Copyright (c) 2009 The Go Authors. All rights reserved.
- **miniz_oxide 0.8.9** — Copyright 2013-2014 RAD Game Tools and Valve Software；Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC；Copyright (c) 2017 Frommi；Copyright (c) 2017-2024 oyvindln
- **miniz_oxide 0.9.1** — Copyright 2013-2014 RAD Game Tools and Valve Software；Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC；Copyright (c) 2017 Frommi；Copyright (c) 2017-2024 oyvindln
- **moxcms 0.8.1** — Copyright 2024 Radzivon Bartoshyk
- **naga 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **naga-types 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **nohash-hasher 0.2.0** — Copyright 2018 Parity Technologies (UK) Ltd.
- **nom 7.1.3** — Copyright (c) 2014-2019 Geoffroy Couprie
- **nom 8.0.0** — Copyright (c) 2014-2019 Geoffroy Couprie
- **num-derive 0.4.2** — Copyright (c) 2014 The Rust Project Developers
- **num-traits 0.2.19** — Copyright (c) 2014 The Rust Project Developers
- **num_enum 0.7.6** — Copyright (c) 2018, Daniel Wagner-Hall
- **num_enum_derive 0.7.6** — Copyright (c) 2018, Daniel Wagner-Hall
- **num_threads 0.1.8** — Copyright 2021 Jacob Pratt；Copyright (c) 2021 Jacob Pratt
- **ogg 0.8.0** — Copyright (c) 2016-2017 est31 <MTest31@outlook.com> and contributors；Copyright (c) 2002-2015 Xiph.org Foundation
- **orbclient 0.3.55** — Copyright (c) 2015-2019 Jeremy Soller
- **ordered-float 5.5.0** — Copyright (c) 2015 Jonathan Reem
- **parking_lot 0.12.5** — Copyright (c) 2016 The Rust Project Developers
- **parking_lot_core 0.9.12** — Copyright (c) 2016 The Rust Project Developers
- **peniko 0.6.1** — Copyright (c) 2018 Raph Levien
- **percent-encoding 2.3.2** — Copyright (c) 2013-2025 The rust-url developers
- **petgraph 0.8.3** — Copyright (c) 2015
- **pkg-config 0.3.34** — Copyright (c) 2014 Alex Crichton
- **plain 0.2.3** — Copyright (c) 2017 Plain contributors
- **png 0.17.16** — Copyright (c) 2015 nwin
- **png 0.18.1** — Copyright (c) 2015 nwin
- **pollster 0.4.0** — Copyright (c) 2020-2021 Joshua Barretto
- **polycool 0.4.0** — Copyright (c) 2018 Raph Levien
- **potential_utf 0.1.6** — Copyright © 2020-2024 Unicode, Inc.
- **powerfmt 0.2.1** — Copyright 2023 Jacob Pratt et al.；Copyright (c) 2023 Jacob Pratt et al.
- **presser 0.3.1** — Copyright (c) 2019 Embark Studios
- **pxfm 0.1.30** — Copyright 2024 Radzivon Bartoshyk
- **quick-error 2.0.1** — Copyright (c) 2015 The quick-error Developers
- **quick-xml 0.41.0** — Copyright (c) 2016 Johann Tuffe
- **range-alloc 0.1.5** — Copyright (c) 2023 The gfx-rs developers
- **raw-window-handle 0.6.2** — Copyright (c) 2019 Osspial；Copyright (c) 2020 Osspial
- **read-fonts 0.41.0** — Copyright 2019 Fontations Developers；Copyright (c) 2019 Fontations Developers
- **redox_syscall 0.4.1** — Copyright (c) 2017 Redox OS Developers
- **redox_syscall 0.5.18** — Copyright (c) 2017 Redox OS Developers
- **redox_syscall 0.9.5** — Copyright (c) 2017 Redox OS Developers
- **regex 1.13.1** — Copyright (c) 2014 The Rust Project Developers
- **regex-automata 0.4.18** — Copyright (c) 2014 The Rust Project Developers
- **regex-syntax 0.8.11** — Copyright (c) 2014 The Rust Project Developers
- **renderdoc-sys 1.1.0** — Copyright (c) 2022 Eyal Kalderon
- **rustc_version 0.4.1** — Copyright (c) 2016 The Rust Project Developers
- **same-file 1.0.6** — Copyright (c) 2017 Andrew Gallant
- **scoped-tls 1.0.1** — Copyright (c) 2014 Alex Crichton
- **scopeguard 1.2.0** — Copyright (c) 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers
- **sctk-adwaita 0.10.1** — Copyright (c) 2022 Bartłomiej Maryńczak
- **self_cell 1.3.0** — Copyright (C) 1989, 1991 Free Software Foundation, Inc.,
- **sha1 0.10.7** — Copyright (c) 2006-2009 Graydon Hoare；Copyright (c) 2009-2013 Mozilla Foundation；Copyright (c) 2016 Artyom Pavlov
- **sha2 0.10.9** — Copyright (c) 2006-2009 Graydon Hoare；Copyright (c) 2009-2013 Mozilla Foundation；Copyright (c) 2016 Artyom Pavlov
- **shlex 1.3.0** — Copyright 2015 Nicholas Allegra (comex).；Copyright (c) 2015 Nicholas Allegra (comex).
- **shlex 2.0.1** — Copyright 2015 Nicholas Allegra (comex).；Copyright (c) 2015 Nicholas Allegra (comex).
- **skrifa 0.44.0** — Copyright 2019 Fontations Developers；Copyright (c) 2019 Fontations Developers
- **slab 0.4.12** — Copyright (c) 2019 Carl Lerche
- **slotmap 1.1.1** — Copyright (c) 2021 Orson Peters <orsonpeters@gmail.com>
- **smallvec 1.16.3** — Copyright (c) 2018 The Servo Project Developers
- **smithay-client-toolkit 0.19.2** — Copyright (c) 2018 Victor Berger
- **smithay-client-toolkit 0.20.0** — Copyright (c) 2018 Victor Berger
- **smithay-clipboard 0.7.3** — Copyright (c) 2018 Lucas Timmins & Victor Berger
- **stable_deref_trait 1.2.1** — Copyright (c) 2017 Robert Grosse
- **static_assertions 1.1.0** — Copyright (c) 2017 Nikolai Vazquez
- **strict-num 0.1.1** — Copyright (c) 2022 Yevhenii Reizner
- **synstructure 0.14.0** — Copyright 2016 Nika Layzell
- **termcolor 1.4.1** — Copyright (c) 2015 Andrew Gallant
- **tiff 0.11.3** — Copyright (c) 2018 PistonDevelopers
- **tiny-skia 0.11.4** — Copyright (c) 2011 Google Inc. All rights reserved.；Copyright (c) 2020 Yevhenii Reizner All rights reserved.
- **tiny-skia-path 0.11.4** — Copyright (c) 2011 Google Inc. All rights reserved.；Copyright (c) 2020 Yevhenii Reizner All rights reserved.
- **tinystr 0.8.5** — Copyright © 2020-2024 Unicode, Inc.
- **tinyvec 1.13.3** — Copyright (c) 2019 Daniel "Lokathor" Gee.
- **tracing 0.1.44** — Copyright (c) 2019 Tokio Contributors
- **tracing-core 0.1.36** — Copyright (c) 2019 Tokio Contributors
- **tree_magic_mini 3.2.2** — Copyright (c) 2017 Aaron Hancock
- **ttf-parser 0.25.1** — Copyright (c) 2018 Yevhenii Reizner
- **type-map 0.5.1** — Copyright 2017-NOW Actix Team；Copyright (c) 2022 Jacob Brown；Copyright (c) 2017-NOW Actix Team
- **typenum 1.20.1** — Copyright 2014 Paho Lurie-Gregg；Copyright (c) 2014 Paho Lurie-Gregg
- **unicode-ident 1.0.27** — Copyright © 1991-2023 Unicode, Inc.
- **unicode-properties 0.1.4** — Copyright (c) 2015 The Rust Project Developers
- **unicode-segmentation 1.13.3** — Copyright (c) 2015 The Rust Project Developers
- **unicode-width 0.2.2** — Copyright (c) 2015 The Rust Project Developers
- **url 2.5.8** — Copyright (c) 2013-2025 The rust-url developers
- **uuid 1.28.0** — Copyright (c) 2014 The Rust Project Developers；Copyright (c) 2018 Ashley Mannix, Christopher Armstrong, Dylan DPC, Hunar Roop Kahlon
- **vcpkg 0.2.15** — Copyright (c) 2017 Jim McGrath
- **vello_common 0.2.0** — Copyright 2020 the Vello Authors
- **vello_cpu 0.2.0** — Copyright 2020 the Vello Authors
- **version_check 0.9.5** — Copyright (c) 2017-2018 Sergio Benitez
- **walkdir 2.5.0** — Copyright (c) 2015 Andrew Gallant
- **wasm-bindgen 0.2.127** — Copyright (c) 2014 Alex Crichton
- **wasm-bindgen-futures 0.4.77** — Copyright (c) 2014 Alex Crichton
- **wasm-bindgen-macro 0.2.127** — Copyright (c) 2014 Alex Crichton
- **wasm-bindgen-macro-support 0.2.127** — Copyright (c) 2014 Alex Crichton
- **wasm-bindgen-shared 0.2.127** — Copyright (c) 2014 Alex Crichton
- **wayland-backend 0.3.17** — Copyright (c) 2015 Elinor Berger
- **wayland-client 0.31.15** — Copyright (c) 2015 Elinor Berger
- **wayland-csd-frame 0.3.0** — Copyright (c) 2023 Kirill Chibisov
- **wayland-cursor 0.31.14** — Copyright (c) 2015 Elinor Berger
- **wayland-protocols 0.32.13** — Copyright (c) 2015 Elinor Berger
- **wayland-protocols-experimental 20250721.0.1** — Copyright (c) 2015 Elinor Berger
- **wayland-protocols-misc 0.3.12** — Copyright (c) 2015 Elinor Berger
- **wayland-protocols-plasma 0.3.12** — Copyright (c) 2015 Elinor Berger
- **wayland-protocols-wlr 0.3.12** — Copyright (c) 2015 Elinor Berger
- **wayland-scanner 0.31.11** — Copyright (c) 2015 Elinor Berger
- **wayland-sys 0.31.11** — Copyright (c) 2015 Elinor Berger
- **web-sys 0.3.104** — Copyright (c) 2014 Alex Crichton
- **web-time 1.1.0** — Copyright 2023 dAxpeDDa；Copyright (c) 2023 dAxpeDDa
- **webbrowser 1.2.4** — Copyright (c) 2015-2022 Amod Malviya
- **wgpu 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-core 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-core-deps-apple 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-core-deps-emscripten 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-core-deps-wasm 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-core-deps-windows-linux-android 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-hal 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-naga-bridge 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **wgpu-types 30.0.1** — Copyright (c) 2025 The gfx-rs developers
- **winapi-util 0.1.11** — Copyright (c) 2017 Andrew Gallant
- **wl-clipboard-rs 0.9.4** — Copyright (c) 2019 Ivan Molodetskikh
- **writeable 0.6.4** — Copyright © 2020-2024 Unicode, Inc.
- **x11rb 0.13.2** — Copyright 2019 x11rb Contributers
- **x11rb-protocol 0.13.2** — Copyright 2019 x11rb Contributers
- **xcursor 0.3.11** — Copyright (c) 2020 Samuele Esposito
- **xkbcommon-dl 0.4.2** — Copyright (c) 2023 Kirill Chibisov
- **xkeysym 0.2.1** — Copyright 2022-2023 John Nunley；Copyright (c) 2022-2023 John Nunley
- **xml-rs 0.8.29** — Copyright (c) 2014 Vladimir Matveev
- **yazi 0.2.1** — Copyright (c) 2020 Chad Brokaw
- **yoke 0.8.4** — Copyright © 2020-2024 Unicode, Inc.
- **yoke-derive 0.8.4** — Copyright © 2020-2024 Unicode, Inc.
- **zeno 0.3.3** — Copyright (c) 2020 Chad Brokaw
- **zerocopy 0.8.63** — Copyright 2023 The Fuchsia Authors；Copyright 2019 The Fuchsia Authors.
- **zerocopy-derive 0.8.63** — Copyright 2023 The Fuchsia Authors；Copyright 2019 The Fuchsia Authors.
- **zerofrom 0.1.8** — Copyright © 2020-2024 Unicode, Inc.
- **zerofrom-derive 0.1.8** — Copyright © 2020-2024 Unicode, Inc.
- **zerotrie 0.2.5** — Copyright © 2020-2024 Unicode, Inc.
- **zerovec 0.11.9** — Copyright © 2020-2024 Unicode, Inc.
- **zerovec-derive 0.11.7** — Copyright © 2020-2024 Unicode, Inc.

## 4. 上游 NOTICE 文件

### cfg_aliases 0.2.2 — `NOTICES.md`

```text
# 3rd Party Notices

The `cfg_aliases!` macro uses a lot of the code from [`tectonic_cfg_support::target_cfg!`] macro which is under the following license:

[`tectonic_cfg_support::target_cfg!`]: https://github.com/tectonic-typesetting/tectonic/blob/f2439b936470ad27bdf92882064bc4702ee01899/cfg_support/src/lib.rs#L166

    tectonic_cfg_support is licensed under the MIT License.

    Permission is hereby granted, free of charge, to any person obtaining a copy
    of this software and associated documentation files (the “Software”), to deal
    in the Software without restriction, including without limitation the rights
    to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    copies of the Software, and to permit persons to whom the Software is
    furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in all
    copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
    SOFTWARE.
---
```

## 5. 说明

- SPDX 表达式中的 `OR` 表示双许可；本项目按宽松项使用。
- `LicenseRef-NSL-1.0 OR GPL-3.0-or-later` 的包按 GPL-3.0-or-later 选项使用，不适用 NSL 条款。
- 表中没有自带许可文件的包，以 crate 元数据中的 `license` 字段为准；完整依赖图固定在 `Cargo.lock`。
- 清单覆盖工作区全部依赖（含开发/测试与平台条件依赖），单个发行包实际链接的子集可能更小。
- 本文件是发布合规清单，不构成法律意见；如用作商业分发，请自行复核。
