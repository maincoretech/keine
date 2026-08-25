#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture_dir="$repo_root/dev/fixtures/video"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/keine-video-fixtures.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

ffmpeg_bin="${FFMPEG_BIN:-ffmpeg}"
command -v "$ffmpeg_bin" >/dev/null
mkdir -p "$fixture_dir"

seed="$fixture_dir/playback.mp4"
test -f "$seed"

# Keep the established AVFoundation-compatible playback fixture as the seed.
# Stream-copy isolates the no-audio contract from encoder-version changes.
"$ffmpeg_bin" -hide_banner -loglevel error -y \
    -i "$seed" -map 0:v:0 -c copy -map_metadata -1 -movflags +faststart \
    "$work_dir/no-audio.mp4"

# Repeat the generated seed and encode one four-second GOP. Level 3.0 is
# explicit because some AVFoundation versions reject libx264's lower inferred
# level even when its dimensions and bitrate satisfy that level.
"$ffmpeg_bin" -hide_banner -loglevel error -y \
    -stream_loop 3 -i "$seed" -t 4 -map 0:v:0 -map 0:a:0 \
    -c:v libx264 -profile:v baseline -level:v 3.0 -pix_fmt yuv420p -threads 1 \
    -g 96 -keyint_min 96 -sc_threshold 0 \
    -c:a aac -b:a 48k -ar 48000 -ac 2 \
    -map_metadata -1 -movflags +faststart "$work_dir/long-gop.mp4"

# The default MP4 muxer layout writes `moov` after media payload. Stream-copy
# keeps this fixture tiny while preserving the canonical H.264/AAC streams.
"$ffmpeg_bin" -hide_banner -loglevel error -y \
    -i "$seed" -map 0 -c copy -map_metadata -1 \
    "$work_dir/tail-moov.mp4"

# A partial `ftyp`/`moov` prefix is a deterministic damaged-header input. It
# must be rejected before either backend can produce a frame.
dd if="$seed" of="$work_dir/damaged-header.mp4" \
    bs=128 count=1 status=none

for fixture in no-audio.mp4 long-gop.mp4 tail-moov.mp4 damaged-header.mp4; do
    install -m 0644 "$work_dir/$fixture" "$fixture_dir/$fixture"
done

echo "generated video fixtures with $($ffmpeg_bin -version | sed -n '1p')"
