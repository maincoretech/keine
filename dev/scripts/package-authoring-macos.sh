#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo 'usage: package-authoring-macos.sh <editor-binary> <engine-binary> <output-directory>' >&2
    exit 2
fi

editor_binary="$1"
engine_binary="$2"
output_dir="$3"
[[ -f "$editor_binary" && -x "$editor_binary" ]] || { echo 'missing Editor executable' >&2; exit 2; }
[[ -f "$engine_binary" && -x "$engine_binary" ]] || { echo 'missing Engine executable' >&2; exit 2; }
case "$(basename "$output_dir")" in
    ''|'.'|'..') echo 'invalid output directory' >&2; exit 2 ;;
esac
if [[ -e "$output_dir" || -L "$output_dir" ]]; then
    echo 'output directory already exists; choose a fresh location' >&2
    exit 2
fi
output_parent="$(dirname "$output_dir")"
mkdir -p "$output_parent"
output_dir="$(cd "$output_parent" && pwd -P)/$(basename "$output_dir")"
editor_app="$output_dir/Kēne Editor.app"
engine_app="$output_dir/Kēne Engine.app"

repo_root="$(cd "$(dirname "$0")/../.." && pwd -P)"
version="$(awk '
    /^\[workspace.package\]$/ { workspace = 1; next }
    /^\[/ { workspace = 0 }
    workspace && /^version = / { gsub(/version = |"/, ""); print; exit }
' "$repo_root/Cargo.toml")"
[[ -n "$version" ]] || { echo 'workspace version not found' >&2; exit 2; }

staging="$(mktemp -d "$output_parent/.keine-authoring.XXXXXX")"
cleanup() { [[ ! -e "$staging" ]] || rm -rf -- "$staging"; }
trap cleanup EXIT

make_app() {
    local name="$1" identifier="$2" executable="$3" source="$4"
    local bundle="$staging/$name.app"
    mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
    cp "$source" "$bundle/Contents/MacOS/$executable"
    chmod +x "$bundle/Contents/MacOS/$executable"
    cp "$repo_root/assets/icons/keine.icns" "$bundle/Contents/Resources/keine.icns"
    printf '%s\n' \
        '<?xml version="1.0" encoding="UTF-8"?>' \
        '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
        '<plist version="1.0"><dict>' \
        "<key>CFBundleExecutable</key><string>$executable</string>" \
        "<key>CFBundleIdentifier</key><string>$identifier</string>" \
        "<key>CFBundleName</key><string>$name</string>" \
        '<key>CFBundlePackageType</key><string>APPL</string>' \
        '<key>CFBundleIconFile</key><string>keine.icns</string>' \
        "<key>CFBundleShortVersionString</key><string>$version</string>" \
        '</dict></plist>' > "$bundle/Contents/Info.plist"
    plutil -lint "$bundle/Contents/Info.plist" >/dev/null
}

make_app 'Kēne Editor' 'moe.maincore.keine-editor' 'editor' "$editor_binary"
make_app 'Kēne Engine' 'moe.maincore.keine-engine' 'keine' "$engine_binary"
mv "$staging" "$output_dir"
staging=""
echo "$editor_app"
echo "$engine_app"
