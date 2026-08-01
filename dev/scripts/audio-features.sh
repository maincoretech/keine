#!/usr/bin/env bash

# Prints the smallest comma-separated Cargo feature set required by a project.
# An opaque nested Hexz cannot be inspected safely here, so it deliberately
# falls back to all codecs. KEINE_AUDIO_FEATURES is an explicit CI override.
require_project_directory() {
    local project="$1"
    if [[ ! -d "$project" ]]; then
        echo "project directory does not exist: $project" >&2
        return 2
    fi
    if [[ ! -f "$project/config.yaml" && ! -f "$project/project.json" ]]; then
        echo "project manifest does not exist: expected config.yaml or project.json in $project" >&2
        return 2
    fi
}

detect_audio_features() {
    local project="$1"
    if [[ -n "${KEINE_AUDIO_FEATURES:-}" ]]; then
        printf '%s\n' "$KEINE_AUDIO_FEATURES"
        return
    fi

    local video_feature="video-ffmpeg"
    if [[ "$(uname -s)" == "Darwin" ]]; then
        video_feature="video-native"
    fi

    local wav=0 mp3=0 vorbis=0 flac=0 video=0 file lower
    while IFS= read -r -d '' file; do
        lower="$(printf '%s' "$file" | tr '[:upper:]' '[:lower:]')"
        case "$lower" in
            *.hxz)
                printf '%s\n' "audio-all,ui-sounds,$video_feature"
                return
                ;;
            *.opus) : ;;
            *.wav|*.wave) wav=1 ;;
            *.mp3) mp3=1 ;;
            *.ogg|*.oga|*.spx) vorbis=1 ;;
            *.flac) flac=1 ;;
            *.mp4|*.m4v|*.mov|*.webm|*.mkv) video=1 ;;
        esac
    done < <(find "$project" -type f -print0)

    # The engine's WebGAL K UI cues are embedded Opus assets. Custom builds
    # may still omit this feature explicitly, but normal releases keep them.
    local features=(ui-sounds)
    # ui-sounds already enables bundled-opus, so project Opus needs no second
    # feature.
    [[ "$wav" -eq 1 ]] && features+=(audio-wav)
    [[ "$mp3" -eq 1 ]] && features+=(audio-mp3)
    [[ "$vorbis" -eq 1 ]] && features+=(audio-vorbis)
    [[ "$flac" -eq 1 ]] && features+=(audio-flac)
    [[ "$video" -eq 1 ]] && features+=("$video_feature")

    local joined="" feature
    for feature in "${features[@]}"; do
        [[ -n "$joined" ]] && joined+=","
        joined+="$feature"
    done
    printf '%s\n' "$joined"
}

build_engine_for_project() {
    local project="$1"
    shift
    require_project_directory "$project" || return
    local features
    features="$(detect_audio_features "$project")"
    if [[ -n "$features" ]]; then
        echo "content features: $features"
        cargo build "$@" --no-default-features --features "$features"
    else
        echo "content features: none"
        cargo build "$@" --no-default-features
    fi
}
