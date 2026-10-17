#!/usr/bin/env bash
# Build only the Engine: no Editor, publisher identity, game content or video.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
mkdir -p "$repo_root/target"
icon_dir="$(mktemp -d "$repo_root/target/android-icons.XXXXXX")"
trap 'rm -rf -- "$icon_dir"' EXIT
# The generator requires a fresh output, beneath this disposable directory.
cargo run --locked -p keine-media --features icons --example icons --target-dir target/icon-tool -- src/assets/icons/keine.png "$icon_dir/derived"
export KEINE_APP_ICON_DIR="$icon_dir/derived"
cargo ndk -t arm64-v8a -P 26 -o target/android/jniLibs \
    rustc --locked -p keine --lib --crate-type cdylib \
    --target-dir target/android --release \
    --no-default-features --features ui-sounds -- \
    -C link-arg=-Wl,-z,max-page-size=16384 \
    -C link-arg=-Wl,-z,common-page-size=16384

engine_version="$(cargo metadata --locked --format-version 1 --filter-platform aarch64-linux-android \
    --no-default-features --features ui-sounds | python3 dev/scripts/package_notices.py --android)"
# Recreate the APK so replacing the large .so cannot leave old ZIP allocation
# holes behind. This only cleans Gradle outputs, not the native Cargo cache.
gradle --no-daemon -p dev/android -PengineVersion="$engine_version" \
    -PengineIconDir="$KEINE_APP_ICON_DIR/android" clean assembleDebug assembleRelease lintDebug
