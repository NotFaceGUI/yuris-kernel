#!/usr/bin/env bash
# 为 Android 构建影片软解用的 FFmpeg 静态库(libavformat/avcodec/swscale/swresample)。
#
# 背景:YU-RIS 5xx 的 OP/ED 是 ASF(WMV)容器 + VC-1 Advanced + WMA,
# Android 系统媒体栈(MediaExtractor/MediaCodec)不支持 ASF/VC-1,
# 必须自带软解。见 docs/divergences/DIV-GFX-074-movie-family.md。
#
# 只开影片需要的组件(按需增删见 COMPONENTS),关掉网络/编码器/滤镜,
# 生成的静态库按 ABI 落在:
#   third_party/ffmpeg/android/<abi>/{include,lib}
# 该目录不入库(见 .gitignore);APK 构建前先跑一次本脚本。
#
# 用法(在 Linux / WSL 里跑;Windows 上用 WSL):
#   tools/android/build-ffmpeg.sh [abi ...]     # 缺省 arm64-v8a x86_64
#
# 环境变量:
#   ANDROID_NDK_HOME     Linux 版 NDK 根(缺省 $HOME/yuris-build/android-ndk-r28c)
#   YURIS_FFMPEG_SRC     ffmpeg 源码 tar.xz(缺省 third_party/ffmpeg/src/ffmpeg-7.1.1.tar.xz,
#                        不存在则下载到该位置并校验 sha256)
#   YURIS_FFMPEG_BUILD   构建缓存目录(缺省 $HOME/.cache/yuris-ffmpeg)
#   API_LEVEL            对齐 app minSdk(缺省 24)
#
# 许可:仅启用 LGPL 组件(未开 --enable-gpl);本仓库整体 GPL-3.0-or-later,
# 静态链接合法,分发时随源码提供即可。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-$HOME/yuris-build/android-ndk-r28c}}"
SRC_TAR="${YURIS_FFMPEG_SRC:-$REPO_ROOT/third_party/ffmpeg/src/ffmpeg-7.1.1.tar.xz}"
BUILD_ROOT="${YURIS_FFMPEG_BUILD:-$HOME/.cache/yuris-ffmpeg}"
OUT_ROOT="$REPO_ROOT/third_party/ffmpeg/android"
API="${API_LEVEL:-24}"

FFMPEG_VERSION="7.1.1"
FFMPEG_SHA256="733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"

if [[ ! -d "$NDK" ]]; then
  echo "错误: 找不到 Linux 版 NDK: $NDK" >&2
  echo "     设置 ANDROID_NDK_HOME 指向包含 toolchains/llvm 的 NDK 根。" >&2
  exit 2
fi
HOST_TAG="linux-x86_64"
case "$(uname -s)" in
  Darwin*) HOST_TAG="darwin-x86_64" ;;
esac
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG"
if [[ ! -d "$TOOLCHAIN" ]]; then
  echo "错误: NDK 工具链不存在: $TOOLCHAIN" >&2
  exit 2
fi

# ---- 影片需要的组件(按需增删;每多一个解码器都会进 APK) --------------
# 容器:YU-RIS 语料实际出现过 MPEG-PS / ASF;其余按"看起来像视频"兜底。
# 容器:YU-RIS 实际用 MPEG-PS / ASF;其余按「视频文件可能是什么」兜底,
# 宁可多带也不让以后某部游戏再返工。
DEMUXERS="asf,avi,mpegps,mpegts,mov,matroska,flv,ogg,rm"
# 视频:H.26x / MPEG / VC-1 / WMV / VPx / AV1 / RealVideo / 老 AVI 编码。
VCODECS="vc1,wmv3,wmv1,wmv2,mpeg1video,mpeg2video,mpeg4,msmpeg4v2,msmpeg4v3,h264,hevc,h263,flv1,av1,vp8,vp9,vp6,vp6a,vp6f,rv10,rv20,rv30,rv40,mjpeg,theora,cinepak,indeo2,indeo3,indeo4,indeo5,msrle,msvideo1,svq1,svq3,huffyuv,ffv1,rawvideo"
# 音频:WMA(含 Pro)/MPEG 音频/AAC/AC-3/DTS/FLAC/Vorbis/Opus/RealAudio/老游戏 ADPCM。
ACODECS="wmav1,wmav2,wmapro,mp1,mp2,mp3,aac,ac3,eac3,dca,truehd,flac,alac,vorbis,opus,ra_144,ra_288,cook,sipr,atrac3,atrac3p,amrnb,amrwb,gsm,gsm_ms,adpcm_ms,adpcm_ima_wav,adpcm_ima_qt,adpcm_yamaha,pcm_s16le,pcm_s16be,pcm_u8"
PARSERS="vc1,mpegvideo,mpeg4video,h264,hevc,vp9,vp8,vp3,av1,rv30,rv40,h263,flv,aac,ac3,dca,mp3,mpegaudio,opus,vorbis,flac,theora,mjpeg"
BSFS="h264_mp4toannexb,hevc_mp4toannexb,aac_adtstoasc,vc1_annexb,extract_extradata,mpeg4_unpack_bframes"

fetch_source() {
  if [[ -f "$SRC_TAR" ]]; then
    return 0
  fi
  mkdir -p "$(dirname "$SRC_TAR")"
  echo "==> 下载 FFmpeg ${FFMPEG_VERSION}: $FFMPEG_URL"
  curl -fL --retry 3 -o "$SRC_TAR.part" "$FFMPEG_URL"
  echo "$FFMPEG_SHA256  $SRC_TAR.part" | sha256sum -c -
  mv "$SRC_TAR.part" "$SRC_TAR"
}

extract_source() {
  local src_dir="$BUILD_ROOT/ffmpeg-${FFMPEG_VERSION}"
  if [[ -f "$src_dir/configure" ]]; then
    return 0
  fi
  mkdir -p "$BUILD_ROOT"
  echo "==> 解包到 $src_dir"
  tar -C "$BUILD_ROOT" -xf "$SRC_TAR"
}

abi_to_arch() {
  case "$1" in
    arm64-v8a) echo "aarch64" ;;
    x86_64) echo "x86_64" ;;
    *) echo "错误: 不支持的 ABI: $1(支持 arm64-v8a / x86_64)" >&2; return 1 ;;
  esac
}

abi_to_target() {
  case "$1" in
    arm64-v8a) echo "aarch64-linux-android" ;;
    x86_64) echo "x86_64-linux-android" ;;
  esac
}

build_abi() {
  local abi="$1"
  local arch target cpu build_dir out_dir
  arch="$(abi_to_arch "$abi")"
  target="$(abi_to_target "$abi")"
  case "$arch" in
    aarch64) cpu="armv8-a" ;;
    x86_64) cpu="x86-64" ;;
  esac
  build_dir="$BUILD_ROOT/build-$abi"
  out_dir="$OUT_ROOT/$abi"

  local cc="$TOOLCHAIN/bin/${target}${API}-clang"
  local cxx="$TOOLCHAIN/bin/${target}${API}-clang++"
  if [[ ! -x "$cc" ]]; then
    echo "错误: 找不到 NDK clang: $cc" >&2
    exit 2
  fi

  echo "==> FFmpeg $abi ($arch / API $API)"
  mkdir -p "$build_dir"
  pushd "$build_dir" >/dev/null

  local extra=()
  if [[ "$arch" == "x86_64" ]]; then
    # 不装 nasm/yasm 也能构建;x86 侧只损失一部分 SIMD 性能(模拟器用)。
    extra+=(--disable-x86asm)
  fi

  "$BUILD_ROOT/ffmpeg-${FFMPEG_VERSION}/configure" \
    --prefix="$out_dir" \
    --target-os=android --arch="$arch" --cpu="$cpu" \
    --enable-cross-compile --enable-pic \
    --cc="$cc" --cxx="$cxx" \
    --ar="$TOOLCHAIN/bin/llvm-ar" --ranlib="$TOOLCHAIN/bin/llvm-ranlib" \
    --nm="$TOOLCHAIN/bin/llvm-nm" --strip="$TOOLCHAIN/bin/llvm-strip" \
    --sysroot="$TOOLCHAIN/sysroot" \
    --disable-shared --enable-static \
    --disable-programs --disable-doc --disable-debug \
    --disable-network --disable-autodetect \
    --disable-avdevice --disable-avfilter --disable-postproc \
    --disable-encoders --disable-muxers --disable-devices --disable-filters \
    --disable-hwaccels --disable-vulkan \
    --disable-zlib --disable-bzlib --disable-lzma --disable-libxml2 \
    --disable-iconv --disable-sdl2 --disable-xlib \
    --disable-everything \
    --enable-protocol=file \
    --enable-demuxer="$DEMUXERS" \
    --enable-decoder="$VCODECS,$ACODECS" \
    --enable-parser="$PARSERS" \
    --enable-bsf="$BSFS" \
    --enable-swscale --enable-swresample \
    "${extra[@]}"

  make -j"$(nproc)" >/dev/null
  make install >/dev/null
  # 只保留静态库;二进制工具已被 --disable-programs 关掉。
  find "$out_dir/lib" -maxdepth 1 -name '*.la' -delete
  popd >/dev/null

  echo "    -> $out_dir"
  du -sh "$out_dir/lib"/*.a | sed 's/^/    /'
}

fetch_source
extract_source

ABIS=("$@")
if [[ ${#ABIS[@]} -eq 0 ]]; then
  ABIS=(arm64-v8a x86_64)
fi
for abi in "${ABIS[@]}"; do
  build_abi "$abi"
done

echo "==> 完成。APK 构建前确保 third_party/ffmpeg/android/<abi> 存在。"
