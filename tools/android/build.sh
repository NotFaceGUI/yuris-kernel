#!/usr/bin/env bash
# Android so 构建 + APK 打包(一条命令)。
#
# 用法:
#   tools/android/build.sh [abi ...]     # 默认 arm64-v8a x86_64
#   tools/android/build.sh arm64-v8a     # 只打真机
#
# 前置:
#   - Rust 目标已装: rustup target add aarch64-linux-android x86_64-linux-android
#   - ANDROID_HOME / ANDROID_NDK_HOME 指向 SDK / NDK(r27 起)
#   - JDK 17+ 在 JAVA_HOME(AGP 8.13 要求)
#
# 产物:
#   android/app/build/outputs/apk/debug/app-debug.apk
#
# 说明:本脚本只做「cargo 交叉编译 → 拷进 jniLibs → gradle 打包」,不含任何
# 游戏数据;数据在验证时用 adb push 到 app 外置私有目录(见 android/README.md)。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"

if [[ -z "$SDK" ]]; then
  echo "错误: ANDROID_HOME / ANDROID_SDK_ROOT 未设置" >&2
  exit 2
fi
if [[ -z "$NDK" ]]; then
  # 取 SDK 下最高版本 NDK。
  NDK="$(ls -d "$SDK"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
fi
if [[ -z "$NDK" || ! -d "$NDK" ]]; then
  echo "错误: 找不到 NDK(设 ANDROID_NDK_HOME,或把 NDK 装进 \$ANDROID_HOME/ndk/)" >&2
  exit 2
fi

HOST_TAG="windows-x86_64"
case "$(uname -s)" in
  Linux*) HOST_TAG="linux-x86_64" ;;
  Darwin*) HOST_TAG="darwin-x86_64" ;;
esac
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
if [[ ! -d "$TOOLCHAIN" ]]; then
  echo "错误: NDK 工具链不存在: $TOOLCHAIN" >&2
  exit 2
fi

# API 级别与 app/build.gradle.kts 的 minSdk 对齐。
API="${ANDROID_API:-24}"

ABIS=("$@")
if [[ ${#ABIS[@]} -eq 0 ]]; then
  ABIS=(arm64-v8a x86_64)
fi

target_of() {
  case "$1" in
    arm64-v8a) echo "aarch64-linux-android" ;;
    x86_64) echo "x86_64-linux-android" ;;
    *)
      echo "错误: 不支持的 ABI: $1(支持 arm64-v8a / x86_64)" >&2
      return 1
      ;;
  esac
}

clang_of() {
  case "$1" in
    aarch64-linux-android) echo "$TOOLCHAIN/aarch64-linux-android${API}-clang" ;;
    x86_64-linux-android) echo "$TOOLCHAIN/x86_64-linux-android${API}-clang" ;;
  esac
}

for abi in "${ABIS[@]}"; do
  target="$(target_of "$abi")"
  cc="$(clang_of "$target")"
  upper="$(echo "$target" | tr '-' '_')"
  export "CC_${upper}=${cc}"
  export "CXX_${upper}=${cc}++"
  export "AR_${upper}=${TOOLCHAIN}/llvm-ar"
  export "CARGO_TARGET_$(echo "$target" | tr 'a-z-' 'A-Z_')_LINKER=${cc}"
  printf '==> cargo build %s (%s)\n' "$target" "$abi"
  # cpal 在 Android 上走 oboe(C++ 实现):so 必须链 libc++_shared,否则
  # 运行时 dlopen 报 `cannot locate symbol "__cxa_pure_virtual"`。
  RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-lc++_shared" \
    cargo build -p yu-host-android --release --target "$target" \
      --manifest-path "$REPO_ROOT/Cargo.toml"
done

for abi in "${ABIS[@]}"; do
  target="$(target_of "$abi")"
  dst="$REPO_ROOT/android/app/src/main/jniLibs/$abi"
  mkdir -p "$dst"
  cp "$REPO_ROOT/target/$target/release/libyu_host_android.so" "$dst/"
  # oboe 依赖的动态 C++ 运行库必须随 APK 分发。
  cp "$NDK/sysroot/usr/lib/$target/libc++_shared.so" "$dst/"
  "$TOOLCHAIN/llvm-strip" --strip-unneeded "$dst/libyu_host_android.so"
  printf '==> %s: %s\n' "$abi" "$(du -h "$dst/libyu_host_android.so" | cut -f1)"
done

printf '==> gradle assembleDebug\n'
if [[ -z "${JAVA_HOME:-}" ]]; then
  echo "错误: JAVA_HOME 未设置(AGP 8.13 需要 JDK 17+)" >&2
  exit 2
fi
# 用本地 gradle(不引 wrapper jar);有 wrapper 时优先 wrapper。
if [[ -x "$REPO_ROOT/android/gradlew" ]]; then
  (cd "$REPO_ROOT/android" && ./gradlew --offline assembleDebug)
else
  gradle_bin="$(command -v gradle || true)"
  if [[ -z "$gradle_bin" ]]; then
    echo "错误: 找不到 gradle(装 Gradle 8.14+ 或生成 wrapper)" >&2
    exit 2
  fi
  (cd "$REPO_ROOT/android" && "$gradle_bin" --offline assembleDebug)
fi

echo "==> 完成: android/app/build/outputs/apk/debug/app-debug.apk"
