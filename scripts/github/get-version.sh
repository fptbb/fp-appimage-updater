#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "$root_dir/scripts/ci/lib.sh"

ci_read_cargo_version
echo "version=v${CI_VERSION}" >> "$GITHUB_OUTPUT"
