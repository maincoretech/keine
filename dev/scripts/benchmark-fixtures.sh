#!/usr/bin/env bash
# Generated calibration media, owned by this repository. No game/user assets.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output="$repo_root/tests/fixtures/native-benchmark/assets"
mkdir -p "$output/luts"
ffmpeg_bin="${FFMPEG_BIN:-ffmpeg}"
work="$(mktemp -d "${TMPDIR:-/tmp}/keine-bench-media.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/luts"
image() {
  "$ffmpeg_bin" -hide_banner -loglevel error -y -f lavfi -i "$2" \
    -frames:v 1 -threads 1 "$work/$1.png"
  if [[ "$3" == 1 ]]; then
    cwebp -quiet -lossless "$work/$1.png" -o "$output/$1.webp"
  else
    cwebp -quiet -q 80 "$work/$1.png" -o "$output/$1.webp"
  fi
}
image day 'testsrc2=size=1920x1080:rate=1,noise=alls=12:all_seed=17' 0
image night 'testsrc2=size=1920x1080:rate=1,hue=h=160:s=0.6,eq=brightness=-0.3,noise=alls=12:all_seed=17' 0
image portrait 'color=c=black@0:size=720x1440,format=rgba,drawbox=x=160:y=60:w=400:h=400:color=0xffddbb@1:t=fill:replace=1,drawbox=x=120:y=460:w=480:h=660:color=0x5599dd@1:t=fill:replace=1,drawbox=x=150:y=1120:w=160:h=300:color=0x444466@1:t=fill:replace=1,drawbox=x=410:y=1120:w=160:h=300:color=0x444466@1:t=fill:replace=1,noise=c0s=8:c1s=8:c2s=8:c3s=0:all_seed=17' 1
image portrait-alt 'color=c=black@0:size=720x1440,format=rgba,drawbox=x=160:y=60:w=400:h=400:color=0xffddbb@1:t=fill:replace=1,drawbox=x=120:y=460:w=480:h=660:color=0xdd9955@1:t=fill:replace=1,drawbox=x=150:y=1120:w=160:h=300:color=0x444466@1:t=fill:replace=1,drawbox=x=410:y=1120:w=160:h=300:color=0x444466@1:t=fill:replace=1,noise=c0s=8:c1s=8:c2s=8:c3s=0:all_seed=17' 1
image flake 'color=c=white@0:size=32x32,format=rgba,drawbox=x=12:y=2:w=8:h=28:color=white@1:t=fill:replace=1,drawbox=x=2:y=12:w=28:h=8:color=white@1:t=fill:replace=1' 1
image luts/cinematic "nullsrc=size=256x16,format=rgb24,geq=r='mod(X,16)*255/15':g='Y*230/15':b='floor(X/16)*255/15'" 1
for spec in 'music:220:2' 'music-alt:330:2' 'click:880:0.2' 'voice:440:4'; do
  IFS=: read -r name frequency duration <<< "$spec"
  "$ffmpeg_bin" -hide_banner -loglevel error -y -f lavfi \
    -i "sine=frequency=$frequency:sample_rate=48000:duration=$duration" \
    -ac 2 -c:a libopus -b:a 96k -threads 1 "$output/$name.opus"
done
"$ffmpeg_bin" -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=1920x1080:rate=24' \
  -f lavfi -i 'sine=frequency=660:sample_rate=48000' -t 4 \
  -c:v libx264 -preset fast -crf 24 -profile:v baseline -level:v 4.0 \
  -pix_fmt yuv420p -g 96 -threads 2 -c:a aac -b:a 96k -ac 2 \
  -map_metadata -1 -movflags +faststart "$output/movie.mp4"
