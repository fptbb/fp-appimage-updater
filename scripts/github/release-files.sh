#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "$root_dir/scripts/ci/lib.sh"

{
    echo 'files<<RELEASE_FILES_EOF'
    ci_github_release_files
    echo 'RELEASE_FILES_EOF'
} >> "$GITHUB_OUTPUT"
