#!/usr/bin/env bash
set -euo pipefail

project="${1:-projects/test-project}"
name="${2:-Kēne}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
bundle_parent="$root/target/bundle/macos"
bundle="$bundle_parent/$name.app"
package_output="target/bundle/macos/.$name.package"
package_dir="$root/$package_output"
version="$(awk '
    /^\[workspace.package\]$/ { workspace = 1; next }
    /^\[/ { workspace = 0 }
    workspace && /^version = / { gsub(/version = |"/, ""); print; exit }
' "$root/Cargo.toml")"

case "$name" in
    ""|"."|".."|*/*|*'&'*|*'<'*|*'>'*|*'\'*|*$'\n'*|*$'\r'*)
        echo "invalid app name: $name" >&2
        exit 2
        ;;
esac
cd "$root"
mkdir -p "$bundle_parent"
cargo bundle "$project" --output "$package_output"

staging="$(mktemp -d "$bundle_parent/.$name.app.staging.XXXXXX")"
backup="$bundle_parent/.$name.app.backup"
cleanup() {
    [[ -n "${staging:-}" && -e "$staging" ]] && rm -rf -- "$staging"
    [[ -e "$package_dir" ]] && rm -rf -- "$package_dir"
}
trap cleanup EXIT

mkdir -p "$staging/Contents/MacOS" "$staging/Contents/Resources"
cp "$package_dir/keine" "$staging/Contents/MacOS/keine"
cp "$package_dir/game.haku" "$staging/Contents/Resources/game.haku"
cp -R "$package_dir/data" "$staging/Contents/Resources/data"
cp "$root/assets/icons/keine.icns" "$staging/Contents/Resources/keine.icns"

sed -e "s/__NAME__/$name/g" -e "s/__VERSION__/$version/g" > "$staging/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>launch</string>
<key>CFBundleIdentifier</key><string>moe.maincore.keine</string>
<key>CFBundleName</key><string>__NAME__</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>keine.icns</string>
<key>CFBundleShortVersionString</key><string>__VERSION__</string>
</dict></plist>
PLIST

cat > "$staging/Contents/MacOS/launch" <<'LAUNCH'
#!/usr/bin/env bash
launcher_dir="$(cd "$(dirname "$0")" && pwd)"
exec "$launcher_dir/keine" "$launcher_dir/../Resources/game.haku"
LAUNCH
chmod +x "$staging/Contents/MacOS/keine" "$staging/Contents/MacOS/launch"

if [[ -e "$backup" ]]; then
    echo "stale bundle backup blocks publication: $backup" >&2
    exit 1
fi
had_previous=0
if [[ -e "$bundle" ]]; then
    mv "$bundle" "$backup"
    had_previous=1
fi
if ! mv "$staging" "$bundle"; then
    [[ "$had_previous" -eq 1 ]] && mv "$backup" "$bundle"
    exit 1
fi
staging=""
[[ "$had_previous" -eq 1 ]] && rm -rf -- "$backup"
echo "$bundle"
