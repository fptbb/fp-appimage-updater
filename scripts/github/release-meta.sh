#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "$root_dir/scripts/ci/lib.sh"

ci_resolve_release_meta

{
    echo "should_release=${CI_SHOULD_RELEASE}"
    echo "tag=${CI_TAG}"
    echo "draft=${CI_DRAFT}"
    echo "prerelease=${CI_PRERELEASE}"
} >> "$GITHUB_OUTPUT"
