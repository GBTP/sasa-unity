#!/usr/bin/env bash
# 构建 sasa-unity 原生库并部署到 Unity 项目。
#
#   ./build.sh ios        仅 iOS（静态库）
#   ./build.sh android    仅 Android（arm64 + armv7）
#   ./build.sh macos      仅 macOS（arm64 + x64）
#   ./build.sh all        全部
#
# 依赖：rustup、对应平台的 target、Android NDK（cargo-ndk）
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_DIR="$CRATE_DIR/../Anoawa/Assets/Plugins/Sasa"

build_ios() {
    echo "==> iOS (aarch64-apple-ios)"
    cargo build --release --target aarch64-apple-ios
    mkdir -p "$PLUGIN_DIR/iOS"
    cp "target/aarch64-apple-ios/release/libsasa_unity.a" "$PLUGIN_DIR/iOS/"
}

build_android() {
    echo "==> Android (arm64-v8a + armeabi-v7a)"
    cargo ndk -t arm64-v8a -t armeabi-v7a -o "$PLUGIN_DIR/Android" build --release
}

build_macos() {
    echo "==> macOS (aarch64 + x86_64)"
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    mkdir -p "$PLUGIN_DIR/macOS"
    lipo -create \
        "target/aarch64-apple-darwin/release/libsasa_unity.dylib" \
        "target/x86_64-apple-darwin/release/libsasa_unity.dylib" \
        -output "$PLUGIN_DIR/macOS/libsasa_unity.dylib"
}

cd "$CRATE_DIR"

case "${1:-all}" in
    ios)     build_ios ;;
    android) build_android ;;
    macos)   build_macos ;;
    all)     build_ios; build_android; build_macos ;;
    *)       echo "用法: $0 {ios|android|macos|all}" >&2; exit 1 ;;
esac

echo "==> 完成，产物已复制到 $PLUGIN_DIR"
