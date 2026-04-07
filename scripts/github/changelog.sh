#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck disable=SC1091
source "$root_dir/scripts/ci/lib.sh"

ci_resolve_changelog_context "https://github.com/${GITHUB_REPOSITORY}"
ci_build_changelog_body

{
    echo 'notes<<CHANGELOG_EOF'
    printf '%s\n' "$CI_CHANGELOG_BODY"
    echo 'CHANGELOG_EOF'
} >> "$GITHUB_OUTPUT"
