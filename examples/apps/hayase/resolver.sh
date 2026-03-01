#!/usr/bin/env bash

set -e

# Fetch the YAML definition
YAML_DATA=$(curl -sL "https://api.hayase.watch/files/latest-linux.yml")

# Use yq or grep to extract version - let's write it generically for systems without yq
VERSION=$(echo "$YAML_DATA" | grep "^version:" | awk '{print $2}')
FILENAME=$(echo "$YAML_DATA" | grep "'linux-hayase-.*-linux.AppImage'" || echo "$YAML_DATA" | grep 'path:' | awk '{print $2}')
# fallback using basic sed
if [ -z "$FILENAME" ]; then
   FILENAME="linux-hayase-${VERSION}-linux.AppImage"
fi

DOWNLOAD_URL="https://api.hayase.watch/files/${FILENAME}"

# The updater expects "DOWNLOAD_URL" then "VERSION" on consecutive lines of stdout:
echo "$DOWNLOAD_URL"
echo "$VERSION"
