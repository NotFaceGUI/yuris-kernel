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
#   debug   → android/app/build/outputs/apk/debug/app-debug.apk
#   release → android/app/build/outputs/apk/release/app-release.apk
#
# 环境变量(都可选;CI 与本地共用同一份脚本):
#   BUILD_TYPE=debug|release   打哪个构建类型(缺省 debug)
#   YURIS_SO_ONLY=1            只交叉编译 so 并落到 jniLibs,不跑 gradle
#   GRADLE_FLAGS=...           覆盖传给 gradle 的参数
#                              (缺省 `--offline`;CI 无缓存可传 `--no-daemon` 不带 offline)
#   GRADLE_PROJECT_ARGS=...    追加 -P 参数(如版本号注入、aapt2 覆盖)
#   YURIS_KEYSTORE_FILE / YURIS_KEYSTORE_PASSWORD / YURIS_KEY_ALIAS /
#   YURIS_KEY_PASSWORD         发布签名四件套,给 gradle 读(见 app/build.gradle.kts)
#
# 说明:本脚本只做「cargo 交叉编译 → 拷进 jniLibs → gradle 打包」,不含任何
# 游戏数据;数据在验证时用 adb push 到 app 外置私有目录(见 android/README.md)。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
BUILD_TYPE="${BUILD_TYPE:-debug}"
SO_ONLY="${YURIS_SO_ONLY:-0}"

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

case "$BUILD_TYPE" in
  debug|release) ;;
  *)
    echo "错误: BUILD_TYPE 只接受 debug / release,收到: $BUILD_TYPE" >&2
    exit 2
    ;;
esac
if [[ "$BUILD_TYPE" == "release" ]]; then
  GRADLE_TASK="assembleRelease"
  APK_PATH="android/app/build/outputs/apk/release/app-release.apk"
else
  GRADLE_TASK="assembleDebug"
  APK_PATH="android/app/build/outputs/apk/debug/app-debug.apk"
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

# Windows 上 NDK 的 `*-clang` 是 shell 脚本,CreateProcess 无法执行
# (os error 193);Rust 链接器必须用 `.cmd` 包装器。其余平台保持原样。
CC_SUFFIX=""
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) CC_SUFFIX=".cmd" ;;
esac

# 缺少 rust 目标时给一句人话,而不是让 cargo 抛 "can't find crate for `core`"。
if command -v rustup >/dev/null 2>&1; then
  installed="$(rustup target list --installed 2>/dev/null || true)"
  for abi in "${ABIS[@]}"; do
    target="$(target_of "$abi")"
    if ! grep -qx "$target" <<<"$installed"; then
      echo "错误: 未安装 Rust 目标 $target;先执行 rustup target add $target" >&2
      exit 2
    fi
  done
fi

for abi in "${ABIS[@]}"; do
  target="$(target_of "$abi")"
  cc_base="$(clang_of "$target")"
  cc="${cc_base}${CC_SUFFIX}"
  cxx="${cc_base}++${CC_SUFFIX}"
  upper="$(echo "$target" | tr '-' '_')"
  export "CC_${upper}=${cc}"
  export "CXX_${upper}=${cxx}"
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
  # r28 起 sysroot 挪进 toolchains/llvm/prebuilt/<host>/;两种布局都兼容。
  CPP_SHARED="$NDK/sysroot/usr/lib/$target/libc++_shared.so"
  if [[ ! -f "$CPP_SHARED" ]]; then
    CPP_SHARED="$TOOLCHAIN/../sysroot/usr/lib/$target/libc++_shared.so"
  fi
  cp "$CPP_SHARED" "$dst/"
  "$TOOLCHAIN/llvm-strip" --strip-unneeded "$dst/libyu_host_android.so"
  printf '==> %s: %s\n' "$abi" "$(du -h "$dst/libyu_host_android.so" | cut -f1)"
done

if [[ "$SO_ONLY" == "1" ]]; then
  echo "==> YURIS_SO_ONLY=1:so 已落到 android/app/src/main/jniLibs,跳过 gradle"
  exit 0
fi

printf '==> gradle %s\n' "$GRADLE_TASK"
if [[ -z "${JAVA_HOME:-}" ]]; then
  echo "错误: JAVA_HOME 未设置(AGP 8.13 需要 JDK 17+)" >&2
  exit 2
fi

# 离线构建复用 SDK 自带的 aapt2(AGP 默认从 Maven 拉同名制品)。
# 之前这条以绝对路径写死在 gradle.properties 里,CI 上必然失效 —— 改为
# 按 $ANDROID_HOME 探测,只有真的找到才传,联网环境不需要。
GRADLE_ARGS=()
if [[ -n "${GRADLE_PROJECT_ARGS:-}" ]]; then
  # shellcheck disable=SC2206  # 有意按空白切分,调用方负责不要放带空格的值
  GRADLE_ARGS+=(${GRADLE_PROJECT_ARGS})
fi
if [[ ! " ${GRADLE_ARGS[*]:-} " =~ aapt2FromMavenOverride ]]; then
  # Windows 上 MSYS 的 glob 会把 `aapt2` 匹配到 `aapt2.exe` 并返回**不带
  # 扩展名**的路径,AGP 拿到后无法执行(`Custom AAPT2 location does not
  # point to an AAPT2 executable`)。所以先找 `.exe`,再退回无扩展名。
  AAPT2="$(ls "$SDK"/build-tools/*/aapt2.exe 2>/dev/null | sort -V | tail -1 || true)"
  if [[ -z "$AAPT2" ]]; then
    AAPT2="$(ls "$SDK"/build-tools/*/aapt2 2>/dev/null | sort -V | tail -1 || true)"
  fi
  if [[ -n "$AAPT2" ]]; then
    GRADLE_ARGS+=("-Pandroid.aapt2FromMavenOverride=${AAPT2}")
  fi
fi

# shellcheck disable=SC2206  # GRADLE_FLAGS 同理:由调用方给短参数列表
GRADLE_OPTS=(${GRADLE_FLAGS---offline})

# 用本地 gradle(不引 wrapper jar);有 wrapper 时优先 wrapper。
if [[ -x "$REPO_ROOT/android/gradlew" ]]; then
  (cd "$REPO_ROOT/android" && ./gradlew "${GRADLE_OPTS[@]}" "${GRADLE_ARGS[@]}" "$GRADLE_TASK")
else
  gradle_bin="$(command -v gradle || true)"
  if [[ -z "$gradle_bin" ]]; then
    echo "错误: 找不到 gradle(装 Gradle 8.14+ 或生成 wrapper)" >&2
    exit 2
  fi
  (cd "$REPO_ROOT/android" && "$gradle_bin" "${GRADLE_OPTS[@]}" "${GRADLE_ARGS[@]}" "$GRADLE_TASK")
fi

if [[ ! -f "$REPO_ROOT/$APK_PATH" ]]; then
  echo "错误: gradle 报成功但产物不存在: $APK_PATH" >&2
  exit 1
fi
printf '==> 完成: %s (%s)\n' "$APK_PATH" "$(du -h "$REPO_ROOT/$APK_PATH" | cut -f1)"
