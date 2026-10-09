#!/usr/bin/env bash
# Validate an existing package; this script never builds or installs an SDK.
set -euo pipefail
checks="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
test -e "${1:?usage: verify-linux-package.sh PACKAGE [FIXTURE]}"
input="$(realpath "$1")"
test -d "${2:-$checks/../../tests/fixtures/native-smoke}"
fixture="$(realpath "${2:-$checks/../../tests/fixtures/native-smoke}")"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
if [[ -d "$input" ]]; then
  package="$input"
else
  package="$work/package"
  mkdir "$package"
  unzip -q "$input" -d "$package"
fi
unset LD_LIBRARY_PATH LD_PRELOAD
python3 "$checks/verify-linux-abi.py" "$package" --runtime
# An unrelated cwd cannot pick up build-tree assets.
cd "$work"
timeout 30s "$package/keine" --version
project="$fixture"
# The Hakutaku adapter opens a snapshot file; a directory is an authoring project.
if [[ -f "$package/game.haku" ]]; then project="$package/game.haku"; fi
timeout 60s "$package/keine" validate "$project" | tee "$work/validation.log"
grep -q '^project valid' "$work/validation.log"
echo 'Final Linux package: loader and project validation passed.'
# This gate does not claim a graphical desktop, GPU, audio or performance verdict.
