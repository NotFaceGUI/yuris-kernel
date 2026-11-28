//! FFmpeg 静态库的定位与 C 封装编译。
//!
//! 库由 `tools/android/build-ffmpeg.sh` 交叉编译到
//! `third_party/ffmpeg/android/<abi>/{include,lib}`;该目录不入库。
//! 换位置时用 `YURIS_FFMPEG_ANDROID_DIR` 指向新的根(其下仍有按 ABI 分的子目录)。

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-env-changed=YURIS_FFMPEG_ANDROID_DIR");

    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("android") {
        // 非 Android 目标不编译 C 封装、不链接 libav*(桌面走
        // `yu-movie-ffmpeg` 子进程后端)。
        return;
    }
    let abi = if target.starts_with("aarch64") {
        "arm64-v8a"
    } else if target.starts_with("x86_64") {
        "x86_64"
    } else {
        panic!("yu-movie-libav: 不支持的 Android 目标 {target}(只做 arm64-v8a / x86_64)");
    };

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let root = std::env::var_os("YURIS_FFMPEG_ANDROID_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../../third_party/ffmpeg/android"));
    let dir = root.join(abi);
    let include = dir.join("include");
    let libdir = dir.join("lib");
    let probe = libdir.join("libavcodec.a");
    // 静态库内容变了(改了 build-ffmpeg.sh 的组件表)必须重链 so;cargo
    // 默认只盯 shim.c 与清单,不会知道 .a 被替换。逐个文件盯住,覆盖
    // `install` 原地覆盖(目录 mtime 可能不变)的情况。
    println!("cargo:rerun-if-changed={}", libdir.display());
    for lib in ["avformat", "avcodec", "swresample", "swscale", "avutil"] {
        println!(
            "cargo:rerun-if-changed={}",
            libdir.join(format!("lib{lib}.a")).display()
        );
    }
    if !probe.is_file() {
        panic!(
            "yu-movie-libav: 缺少 {}(目标 {target})。\n\
             先在 Linux/WSL 里跑 tools/android/build-ffmpeg.sh {abi},\n\
             或用 YURIS_FFMPEG_ANDROID_DIR 指向已构建的根目录。",
            probe.display()
        );
    }

    cc::Build::new()
        .file("src/shim.c")
        .include(&include)
        .warnings(true)
        .compile("yuris_movie_shim");

    println!("cargo:rustc-link-search=native={}", libdir.display());
    // 顺序有意义:封装(shim)在 cc 里先登记,这里再按依赖从高到低列库。
    for lib in ["avformat", "avcodec", "swresample", "swscale", "avutil"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=dl");
}
