#!/usr/bin/env bash

# Prints the canonical release feature set. Project images and standalone
# audio are validated by the Rust publisher; this helper only selects the
# platform video backend early enough for CI to install its native runtime.
detect_shipping_features() {
    local project="$1"
    if [[ ! -d "$project" ]]; then
        echo "project directory does not exist: $project" >&2
        return 2
    fi
    if [[ ! -f "$project/config.yaml" && ! -f "$project/project.json" ]]; then
        echo "project manifest does not exist: expected config.yaml or project.json in $project" >&2
        return 2
    fi

    local video=0 file lower
    while IFS= read -r -d '' file; do
        lower="$(printf '%s' "$file" | tr '[:upper:]' '[:lower:]')"
        case "$lower" in
            *.mp4|*.m4v|*.mov|*.webm|*.mkv) video=1 ;;
        esac
    done < <(find "$project" -type f -print0)

    local features="ui-sounds"
    if [[ "$video" -eq 1 ]]; then
        if [[ "$(uname -s)" == "Darwin" ]]; then
            features+=",video-native"
        else
            features+=",video-ffmpeg"
        fi
    fi
    printf '%s\n' "$features"
}
