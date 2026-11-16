#!/usr/bin/env bash
# iOS 宿主构建:C ABI 静态库(device + simulator)+ 可选 Xcode 工程/模拟器构建。
#
# 用法:
#   tools/ios/build.sh                    # release 静态库,不跑 xcodebuild
#   BUILD_TYPE=debug tools/ios/build.sh
#   YURIS_IOS_XCODE=1 tools/ios/build.sh  # xcodegen + 模拟器 app 构建
#   YURIS_IOS_DEVICE=1 YURIS_IOS_XCODE=1 tools/ios/build.sh
#                                         # 真机 app 构建(需 DEVELOPMENT_TEAM)
#
# 前置:
#   - macOS + Xcode(Windows/Linux 直接报错退出;Windows 上只能做宿主侧 cargo test)
#   - rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
#   - xcodegen(仅 YURIS_IOS_XCODE=1):brew install xcodegen
#
# 产物:
#   dist/ios/iphoneos/libyu_host_ios.a
#   dist/ios/iphonesimulator/libyu_host_ios.a
#   ios/YuKernel.xcodeproj(YURIS_IOS_XCODE=1 时生成,gitignore)
#
# 环境变量:
#   BUILD_TYPE=debug|release       缺省 release
#   YURIS_IOS_XCODE=1              生成 Xcode 工程并跑 xcodebuild
#   YURIS_IOS_DEVICE=1             真机构建(缺省模拟器)
#   YURIS_IOS_SIM_NAME=iPhone 15   模拟器机型(缺省 iPhone 15)
#   DEVELOPMENT_TEAM=XXXXXXXXXX    真机签名 Team ID(也可在 Xcode 里设)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "错误: iOS 构建需要 macOS + Xcode(当前 $(uname -s))。" >&2
  echo "      Windows 上可先跑: cargo test -p yu-host-ios(soft 会话 + C ABI 导出)" >&2
  exit 2
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "错误: 找不到 cargo(先装 Rust)" >&2
  exit 2
fi

# 允许从任意目录调用:后面的 target/ 相对路径都以仓库根为基准。
cd "$REPO_ROOT"

BUILD_TYPE="${BUILD_TYPE:-release}"
case "$BUILD_TYPE" in
  debug)
    PROFILE="debug"
    CARGO_FLAGS=()
    XCODE_CONFIG="Debug"
    ;;
  release)
    PROFILE="release"
    CARGO_FLAGS=(--release)
    XCODE_CONFIG="Release"
    ;;
  *)
    echo "错误: BUILD_TYPE 只接受 debug / release,收到: $BUILD_TYPE" >&2
    exit 2
    ;;
esac

rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

echo "==> 构建 device(arm64)"
cargo build -p yu-host-ios "${CARGO_FLAGS[@]}" --target aarch64-apple-ios
echo "==> 构建 simulator(arm64)"
cargo build -p yu-host-ios "${CARGO_FLAGS[@]}" --target aarch64-apple-ios-sim
echo "==> 构建 simulator(x86_64,Intel Mac)"
cargo build -p yu-host-ios "${CARGO_FLAGS[@]}" --target x86_64-apple-ios

mkdir -p "$REPO_ROOT/dist/ios/iphoneos" "$REPO_ROOT/dist/ios/iphonesimulator"
cp "target/aarch64-apple-ios/$PROFILE/libyu_host_ios.a" \
  "$REPO_ROOT/dist/ios/iphoneos/libyu_host_ios.a"
lipo -create \
  "target/aarch64-apple-ios-sim/$PROFILE/libyu_host_ios.a" \
  "target/x86_64-apple-ios/$PROFILE/libyu_host_ios.a" \
  -output "$REPO_ROOT/dist/ios/iphonesimulator/libyu_host_ios.a"

echo "静态库:"
ls -lh "$REPO_ROOT/dist/ios/iphoneos/libyu_host_ios.a" \
  "$REPO_ROOT/dist/ios/iphonesimulator/libyu_host_ios.a"

if [[ "${YURIS_IOS_XCODE:-0}" != "1" ]]; then
  echo "跳过 xcodebuild(设 YURIS_IOS_XCODE=1 可生成工程并构建 app)"
  exit 0
fi

if ! command -v xcodegen >/dev/null 2>&1; then
  echo "错误: 未安装 xcodegen(brew install xcodegen)" >&2
  exit 3
fi
(cd "$REPO_ROOT/ios" && xcodegen generate)

if [[ "${YURIS_IOS_DEVICE:-0}" == "1" ]]; then
  TEAM_ARGS=()
  if [[ -n "${DEVELOPMENT_TEAM:-}" ]]; then
    TEAM_ARGS=(DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM")
  fi
  echo "==> xcodebuild(真机)"
  # macOS 自带 bash 3.2:空数组在 `set -u` 下直接展开会报 unbound variable,
  # 用 `${arr[@]+"${arr[@]}"}` 的兼容写法。
  xcodebuild \
    -project "$REPO_ROOT/ios/YuKernel.xcodeproj" \
    -scheme YuKernel \
    -configuration "$XCODE_CONFIG" \
    -destination 'generic/platform=iOS' \
    -allowProvisioningUpdates \
    ${TEAM_ARGS[@]+"${TEAM_ARGS[@]}"} \
    build
else
  echo "==> xcodebuild(模拟器 ${YURIS_IOS_SIM_NAME:-iPhone 15})"
  xcodebuild \
    -project "$REPO_ROOT/ios/YuKernel.xcodeproj" \
    -scheme YuKernel \
    -configuration "$XCODE_CONFIG" \
    -sdk iphonesimulator \
    -destination "platform=iOS Simulator,name=${YURIS_IOS_SIM_NAME:-iPhone 15}" \
    build
fi
