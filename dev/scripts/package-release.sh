#!/usr/bin/env bash
set -euo pipefail

project="${1:-projects/test-project}"
output="${2:-target/release-package}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
source "$root/dev/scripts/audio-features.sh"

cd "$root"
require_project_directory "$project"
if [[ -z "${HEXZ_PASSWORD:-}" ]]; then
    echo "HEXZ_PASSWORD must be set" >&2
    exit 2
fi
if ! command -v hexz >/dev/null 2>&1; then
    echo "hexz CLI is required; install maincoretech/hexz_k with its cli feature" >&2
    exit 2
fi

case "$output" in
    target/*) ;;
    *)
        echo "release output must stay under target/: $output" >&2
        exit 2
        ;;
esac

# Release packages are the native project form: the packaged loader reads
# config.yaml from inside the archive. LetsGal (project.json) projects need
# the native conversion defined in docs/project-and-assets-spec.md first.
if [[ ! -f "$project/config.yaml" ]]; then
    echo "release packaging requires a native project with config.yaml at its root" >&2
    echo "($project has no config.yaml; LetsGal project.json conversion is not implemented yet)" >&2
    exit 2
fi

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/project"
cp -R "$project"/. "$staging/project/"

# Runtime state and generated caches must never enter the encrypted artifact.
# The staging copy regenerates `.keine/compiled/program.bin` below, so any
# source-project `.keine` contents are dropped as well.
find "$staging/project" -type d \( -name saves -o -name imported_assets -o -name .keine \) \
    -prune -exec rm -rf {} +
find "$staging/project" -type f \( -name '.DS_Store' -o -name '*.meta' \) -delete

# Pin `compiled_program` in the staged config. The compiler must parse source
# scenes, so it runs with the policy neutralized to `auto`; the packaged
# config is then set to `require`, making startup fail loudly if the compiled
# artifact is missing.
set_compiled_policy() {
    local config="$1" policy="$2"
    awk -v policy="$policy" '
        /^compiled_program:/ { print "compiled_program: " policy; seen = 1; next }
        { print }
        END { if (!seen) print "compiled_program: " policy }
    ' "$config" > "$config.tmp" && mv "$config.tmp" "$config"
}
set_compiled_policy "$staging/project/config.yaml" auto

release_features="$(detect_audio_features "$staging/project")"
HEXZ_PASSWORD="$HEXZ_PASSWORD" \
    KEINE_AUDIO_FEATURES="$release_features" \
    build_engine_for_project "$staging/project" --release --locked

# Reuse the just-built engine for `cargo compiler` so the release pipeline
# performs a single release build. The compiler emits
# `.keine/compiled/program.bin` inside the staging copy, which `hexz pack`
# ships in the archive.
engine_bin="$root/target/release/keine"
if [[ -f "$root/target/release/keine.exe" ]]; then
    engine_bin="$root/target/release/keine.exe"
fi
"$engine_bin" compiler "$staging/project"
set_compiled_policy "$staging/project/config.yaml" require

rm -rf "$output"
mkdir -p "$output"
cp "$root/assets/icons/keine-256.png" "$output/keine.png"
hexz pack "$staging/project" "$output/game.hxz" \
    --compression zstd --encrypt --block-size 65536

if [[ -f target/release/keine.exe ]]; then
    cp target/release/keine.exe "$output/keine.exe"
    if [[ ",$release_features," == *,video-ffmpeg,* ]]; then
        if [[ -z "${VCPKG_ROOT:-}" ]]; then
            echo "VCPKG_ROOT is required to bundle the Windows FFmpeg runtime" >&2
            exit 2
        fi
        vcpkg_root="$VCPKG_ROOT"
        if command -v cygpath >/dev/null 2>&1; then
            vcpkg_root="$(cygpath -u "$vcpkg_root")"
        fi
        ffmpeg_bin="$vcpkg_root/installed/x64-windows/bin"
        if ! compgen -G "$ffmpeg_bin/*.dll" >/dev/null; then
            echo "Windows FFmpeg runtime DLLs were not found in $ffmpeg_bin" >&2
            exit 2
        fi
        cp "$ffmpeg_bin"/*.dll "$output/"
    fi
    cat > "$output/run.bat" <<'BAT'
@echo off
"%~dp0keine.exe" "%~dp0game.hxz"
BAT
else
    cp target/release/keine "$output/keine"
    cat > "$output/run.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
exec "$root/keine" "$root/game.hxz"
SH
    chmod +x "$output/keine" "$output/run.sh"
fi

printf '%s\n' "$root/$output"
