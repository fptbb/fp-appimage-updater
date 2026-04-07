#!/usr/bin/env bash
set -euo pipefail

ci_read_cargo_version() {
    CI_VERSION="$(grep -m1 -E '^version\s*=' Cargo.toml | cut -d'"' -f2 || true)"
    if [[ -z "${CI_VERSION:-}" ]]; then
        echo "Error: Could not extract version from Cargo.toml" >&2
        return 1
    fi
}

ci_resolve_release_meta() {
    ci_read_cargo_version
    CI_BASE_TAG="v${CI_VERSION}"
    CI_SHOULD_RELEASE="false"
    CI_TAG=""
    CI_DRAFT=""
    CI_PRERELEASE=""
    
    if [[ "${EVENT_NAME:-}" == "pull_request" ]]; then
        return 0
    fi
    
    if [[ "${EVENT_NAME:-}" == "push" || "${EVENT_NAME:-}" == "merge_request_event" ]]; then
        CI_HEAD_MESSAGE="$(git log -1 --pretty=%s | tr -d '\r')"
        CI_EXPECTED_MESSAGE="chore: bump version to ${CI_VERSION} in Cargo.toml"
        
        if [[ "${CI_HEAD_MESSAGE}" == "${CI_EXPECTED_MESSAGE}" ]]; then
            if git ls-remote --tags origin | grep -q "refs/tags/${CI_BASE_TAG}$"; then
                echo "Stable tag ${CI_BASE_TAG} already exists. Bump the version in Cargo.toml before pushing a new release commit."
                exit 1
            fi
            
            CI_SHOULD_RELEASE="true"
            CI_TAG="${CI_BASE_TAG}"
            CI_DRAFT="false"
            CI_PRERELEASE="false"
        fi
        return 0
    fi
    
    if [[ "${EVENT_NAME:-}" == "web" && "${FORCE_RELEASE:-false}" != "true" ]]; then
        echo "Manual pipeline run without force_release. Skipping release creation."
        return 0
    fi
    
    if [[ "${PRERELEASE:-false}" == "true" ]]; then
        if git ls-remote --tags origin | grep -q "refs/tags/${CI_BASE_TAG}$"; then
            echo "Stable tag ${CI_BASE_TAG} already exists. Bump the version in Cargo.toml before creating a new pre-release."
            exit 1
        fi
        
        CI_HIGHEST_RC="$(git ls-remote --tags origin \
            | grep -oP "refs/tags/${CI_BASE_TAG}-RC\K[0-9]+" \
        | sort -n | tail -1 || true)"
        if [[ -z "${CI_HIGHEST_RC:-}" ]]; then
            CI_NEXT_RC=1
        else
            CI_NEXT_RC=$((CI_HIGHEST_RC + 1))
        fi
        CI_TAG="${CI_BASE_TAG}-RC${CI_NEXT_RC}"
    else
        CI_TAG="${CI_BASE_TAG}"
        if git ls-remote --tags origin | grep -q "refs/tags/${CI_TAG}$"; then
            echo "Tag ${CI_TAG} already exists. Bump the version in Cargo.toml before releasing."
            exit 1
        fi
    fi
    
    export CI_SHOULD_RELEASE="true"
    export CI_DRAFT="true"
    export CI_PRERELEASE="${PRERELEASE:-false}"
}

ci_resolve_changelog_context() {
    local compare_url_prefix="$1"
    
    CI_HEAD_SHA="$(git rev-parse --short HEAD)"
    if [[ "${EVENT_NAME:-}" == "workflow_dispatch" || "${EVENT_NAME:-}" == "web" ]]; then
        if [[ "${PRERELEASE:-false}" == "true" ]]; then
            CI_PREV_TAG="$(git describe --tags --abbrev=0 2>/dev/null || true)"
        else
            CI_PREV_TAG="$(git tag --sort=-creatordate | grep -v -- '-RC' | head -1 || true)"
        fi
    else
        CI_PREV_TAG="$(git tag --sort=-creatordate | grep -v -- '-RC' | head -1 || true)"
    fi
    
    if [[ -z "${CI_PREV_TAG:-}" ]]; then
        CI_BASE_SHA="$(git rev-list --max-parents=0 HEAD --abbrev-commit)"
        CI_COMPARE_URL="${compare_url_prefix}/compare/${CI_BASE_SHA}...${CI_HEAD_SHA}"
        CI_RANGE_SPEC="HEAD"
    else
        CI_BASE_SHA="$(git rev-parse --short "${CI_PREV_TAG}")"
        CI_COMPARE_URL="${compare_url_prefix}/compare/${CI_PREV_TAG}...${CI_HEAD_SHA}"
        CI_RANGE_SPEC="${CI_PREV_TAG}..HEAD"
    fi
}

ci_build_changelog_body() {
    local format log_content key header_key prefix
    log_content=""
    
    if [[ "${INCLUDE_COMMITS:-true}" == "true" ]]; then
        if [[ "${SHOW_AUTHORS:-false}" == "false" ]]; then
            format='- %s (%h)'
        else
            format='- %s (%h) by %an'
        fi
        
        if [[ "${CATEGORIZE:-false}" == "true" ]]; then
            declare -A commit_groups=()
            while IFS= read -r line; do
                prefix="$(echo "$line" | sed -nE 's/^- ([^: ]+):.*/\1/p' | tr '[:upper:]' '[:lower:]')"
                if [[ -z "$prefix" ]]; then
                    prefix="other"
                fi
                commit_groups["$prefix"]="${commit_groups["$prefix"]-}${line}"$'\n'
            done < <(git log --pretty=format:"$format" "$CI_RANGE_SPEC")
            
            for key in "${!commit_groups[@]}"; do
                header_key="$(tr '[:lower:]' '[:upper:]' <<< "${key:0:1}")${key:1}"
                log_content+=$'### '"${header_key}"$'\n'"${commit_groups[$key]}"$'\n'
            done
        else
            log_content="$(git log --pretty=format:"$format" "$CI_RANGE_SPEC")"
        fi
    fi
    
    CI_CHANGELOG_BODY=$'## What'\''s Changed\n'
    CI_CHANGELOG_BODY+=$'**Full Changelog**: ['"${CI_BASE_SHA}...${CI_HEAD_SHA}"']('"${CI_COMPARE_URL}"')\n\n'
    CI_CHANGELOG_BODY+="${log_content}"
}

ci_github_release_files() {
    cat <<'EOF'
build/fp-appimage-updater.x64
build/fp-appimage-updater.ARM
build/checksums.txt
systemd/fp-appimage-updater.service
systemd/fp-appimage-updater.timer
EOF
}

ci_gitlab_release_files() {
    cat <<'EOF'
build/fp-appimage-updater.x64
build/fp-appimage-updater.ARM
build/checksums.txt
build/fp-appimage-updater.x64.bundle
build/fp-appimage-updater.ARM.bundle
systemd/fp-appimage-updater.service
systemd/fp-appimage-updater.timer
EOF
}

ci_gitlab_release_asset_links() {
    cat <<EOF
{"name":"fp-appimage-updater.x64","url":"${PACKAGE_REGISTRY_URL}/fp-appimage-updater.x64","direct_asset_path":"/bin/fp-appimage-updater.x64"}
{"name":"checksums.txt","url":"${PACKAGE_REGISTRY_URL}/checksums.txt","direct_asset_path":"/bin/checksums.txt"}
{"name":"fp-appimage-updater.x64.bundle","url":"${PACKAGE_REGISTRY_URL}/fp-appimage-updater.x64.bundle","direct_asset_path":"/bin/fp-appimage-updater.x64.bundle"}
{"name":"fp-appimage-updater.ARM","url":"${PACKAGE_REGISTRY_URL}/fp-appimage-updater.ARM","direct_asset_path":"/bin/fp-appimage-updater.ARM"}
{"name":"fp-appimage-updater.ARM.bundle","url":"${PACKAGE_REGISTRY_URL}/fp-appimage-updater.ARM.bundle","direct_asset_path":"/bin/fp-appimage-updater.ARM.bundle"}
{"name":"fp-appimage-updater.service","url":"${PACKAGE_REGISTRY_URL}/fp-appimage-updater.service","direct_asset_path":"/systemd/fp-appimage-updater.service"}
{"name":"fp-appimage-updater.timer","url":"${PACKAGE_REGISTRY_URL}/fp-appimage-updater.timer","direct_asset_path":"/systemd/fp-appimage-updater.timer"}
EOF
}
