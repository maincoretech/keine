#!/usr/bin/env bash
# Shared app signing; publisher identities are unrelated to Apple certificates.
set -euo pipefail
[[ $# -eq 1 && -d "$1/Contents" ]] || { echo 'usage: sign-macos.sh <app-bundle>' >&2; exit 2; }
app="$1"
identity="${KEINE_CODESIGN_IDENTITY:--}"
profile="${KEINE_NOTARY_PROFILE:-}"
if [[ -n "$profile" && "$identity" == '-' ]]; then
    echo 'notarization requires KEINE_CODESIGN_IDENTITY (Developer ID Application)' >&2
    exit 2
fi
if [[ "$identity" == '-' ]]; then
    codesign --force --sign - "$app/Contents/MacOS/"*
    codesign --force --sign - "$app"
else
    codesign --force --options runtime --timestamp --sign "$identity" "$app/Contents/MacOS/"*
    codesign --force --options runtime --timestamp --sign "$identity" "$app"
fi
codesign --verify --deep --strict "$app"
if [[ -n "$profile" ]]; then
    scratch="$(mktemp -d "${TMPDIR:-/tmp}/keine-notary.XXXXXX")"
    trap 'rm -rf -- "$scratch"' EXIT
    ditto -c -k --keepParent "$app" "$scratch/app.zip"
    xcrun notarytool submit "$scratch/app.zip" --keychain-profile "$profile" --wait
    xcrun stapler staple "$app"
    xcrun stapler validate "$app"
    spctl --assess --type execute "$app"
fi
