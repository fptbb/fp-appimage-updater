#!/usr/bin/env bash
set -euo pipefail

BASE="https://cdn.kde.org/ci-builds/graphics/krita/master/linux/"
HTML="$(curl -fsSL "$BASE")"

FILE="$(printf '%s\n' "$HTML" | grep -oE 'krita-[^"]+x86_64\.AppImage' | head -n1)"
VERSION="$(printf '%s' "$FILE" | sed -E 's/^krita-([^ ]+)-x86_64\.AppImage$/\1/')"

echo "${BASE}${FILE}"
echo "$VERSION"
