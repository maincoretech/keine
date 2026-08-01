#!/usr/bin/env bash
set -euo pipefail

project="${1:-projects/test-project}"
name="${2:-Kēne}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
source "$root/dev/scripts/audio-features.sh"
bundle="$root/target/bundle/macos/$name.app"
version="$(awk '
    /^\[workspace.package\]$/ { workspace = 1; next }
    /^\[/ { workspace = 0 }
    workspace && /^version = / { gsub(/version = |"/, ""); print; exit }
' "$root/Cargo.toml")"

cd "$root"
build_engine_for_project "$project" --release
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources/project"
cp target/release/keine "$bundle/Contents/MacOS/keine"
cp -R "$project"/. "$bundle/Contents/Resources/project/"
cp "$root/assets/icons/keine.icns" "$bundle/Contents/Resources/keine.icns"

sed -e "s/__NAME__/$name/g" -e "s/__VERSION__/$version/g" > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>launch</string>
<key>CFBundleIdentifier</key><string>io.github.maincoretech.keine</string>
<key>CFBundleName</key><string>__NAME__</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>keine.icns</string>
<key>CFBundleShortVersionString</key><string>__VERSION__</string>
</dict></plist>
PLIST

cat > "$bundle/Contents/MacOS/launch" <<'LAUNCH'
#!/usr/bin/env bash
launcher_dir="$(cd "$(dirname "$0")" && pwd)"
cd "$launcher_dir/../Resources/project"
exec "$launcher_dir/keine" .
LAUNCH
chmod +x "$bundle/Contents/MacOS/keine" "$bundle/Contents/MacOS/launch"
echo "$bundle"
