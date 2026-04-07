#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"
# shellcheck disable=SC1091
source "$root_dir/scripts/ci/lib.sh"

export EVENT_NAME="${CI_PIPELINE_SOURCE:-}"
export PRERELEASE="${PRERELEASE:-false}"
export FORCE_RELEASE="${FORCE_RELEASE:-false}"
ci_resolve_release_meta

if [[ "$CI_SHOULD_RELEASE" != "true" ]]; then
    echo "Not a valid release attempt. Exiting."
    exit 0
fi

export EVENT_NAME="${CI_PIPELINE_SOURCE:-}"
export PRERELEASE="${PRERELEASE:-false}"
export CATEGORIZE="${CATEGORIZE:-false}"
export SHOW_AUTHORS="${SHOW_AUTHORS:-false}"
export INCLUDE_COMMITS="${INCLUDE_COMMITS:-true}"
ci_resolve_changelog_context "${CI_PROJECT_URL}/-"
ci_build_changelog_body

{
    printf '%s\n' "$CI_CHANGELOG_BODY"
} > release_notes.md

echo "Creating release $CI_TAG..."

package_registry_url="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/fp-appimage-updater/${CI_TAG}"
export PACKAGE_REGISTRY_URL="$package_registry_url"

while IFS= read -r file; do
    if [[ -f "$file" ]]; then
        filename="$(basename "$file")"
        echo "Uploading $filename to Package Registry..."
        curl --fail --silent --show-error --header "JOB-TOKEN: ${CI_JOB_TOKEN}" --upload-file "$file" "${package_registry_url}/${filename}"
    fi
done < <(ci_gitlab_release_files)

release_args=(release-cli create --name "Release $CI_TAG" --description "./release_notes.md" --tag-name "$CI_TAG" --ref "$CI_COMMIT_SHA")
while IFS= read -r asset_link; do
    release_args+=(--assets-link "$asset_link")
done < <(ci_gitlab_release_asset_links)
"${release_args[@]}"

if [[ -n "${GITLAB_API_TOKEN:-}" ]]; then
    echo "Locking tag $CI_TAG..."
    curl --request POST --header "PRIVATE-TOKEN: $GITLAB_API_TOKEN" \
        --url "${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/protected_tags" \
        --data "name=$CI_TAG&create_access_level=40" || echo "Failed to lock tag, check API token permissions."
else
    echo "GITLAB_API_TOKEN not set, skipping tag protection."
fi

if [[ "${PRERELEASE:-false}" == "true" ]]; then
    echo "Pre-release detected, skipping COPR webhook."
else
    if [[ -z "${COPR_WEBHOOK_UUID:-}" ]]; then
        echo "Error: COPR_WEBHOOK_UUID is not set."
        exit 1
    fi

    copr_webhook_url="https://copr.fedorainfracloud.org/webhooks/custom/226828/${COPR_WEBHOOK_UUID}/fp-appimage-updater/"

    echo "Triggering COPR webhook..."
    curl --fail --silent --show-error --request POST "${copr_webhook_url}"
fi
