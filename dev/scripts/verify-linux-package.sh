#!/usr/bin/env bash
# Runs inside the clean Ubuntu 24.04 container, not on the build host.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt_options=(-o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
apt-get "${apt_options[@]}" update
# Only host runtime libraries and checking tools: no FFmpeg, Rust or C/C++ SDK.
apt-get "${apt_options[@]}" install -y --no-install-recommends \
  python3 binutils unzip libasound2t64 libudev1 libfontconfig1 \
  libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-cursor0 \
  libwayland-egl1 libx11-6 libxcb1
mkdir /tmp/package
if [[ -d /input ]]; then
  cp -a /input/. /tmp/package/
else
  unzip -q /input -d /tmp/package
fi
python3 /checks/verify-linux-abi.py /tmp/package
unset LD_LIBRARY_PATH LD_PRELOAD
for executable in keine editor; do
  binary="/tmp/package/$executable"
  if [[ "$executable" == editor && ! -e "$binary" ]]; then continue; fi
  test -x "$binary"
  dependencies="$(ldd "$binary")"
  printf '%s\n' "$dependencies"
  if grep -q 'not found' <<< "$dependencies"; then
    echo "Unresolved runtime library in final package: $executable" >&2
    exit 1
  fi
done
# An unrelated cwd and fresh user-data root cannot pick up build-tree assets.
cd /tmp
timeout 30s /tmp/package/keine --version
project=/native-smoke
# The Hakutaku adapter opens a snapshot file; a directory is an authoring project.
if [[ -f /tmp/package/game.haku ]]; then project=/tmp/package/game.haku; fi
timeout 60s /tmp/package/keine validate "$project" | tee /tmp/validation.log
grep -q '^project valid' /tmp/validation.log
echo 'Final Linux package: loader and project validation passed on Ubuntu 24.04.'
# This gate does not claim a graphical desktop, GPU, audio or performance verdict.
