#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

tap_repo_url="${HOMEBREW_TAP_REPO_URL:-}"
tap_push_token="${HOMEBREW_TAP_TOKEN:-}"
tap_branch="${HOMEBREW_TAP_BRANCH:-main}"
tap_formula_path="${HOMEBREW_TAP_FORMULA_PATH:-Formula/fp-appimage-updater.rb}"
tap_formula_name="${HOMEBREW_TAP_FORMULA_NAME:-fp-appimage-updater}"
tap_package_name="${HOMEBREW_TAP_PACKAGE_NAME:-fp-appimage-updater}"
tap_description="${HOMEBREW_TAP_DESCRIPTION:-Declarative AppImage updater and integrator}"
tap_homepage="${HOMEBREW_TAP_HOMEPAGE:-${CI_PROJECT_URL:-https://gitlab.com/fpsys/fp-appimage-updater}}"
tap_license="${HOMEBREW_TAP_LICENSE:-MIT}"
tap_class_name="$(printf '%s' "$tap_formula_name" | awk -F '[-_]' '{
    for (i = 1; i <= NF; i++) {
        printf "%s", toupper(substr($i, 1, 1)) substr($i, 2)
    }
}')"

if [[ -z "$tap_repo_url" ]]; then
    echo "Error: HOMEBREW_TAP_REPO_URL is not set."
    exit 1
fi

if [[ -z "$tap_push_token" ]]; then
    echo "Error: HOMEBREW_TAP_TOKEN is not set."
    exit 1
fi

if [[ -z "${CI_TAG:-}" && -z "${CI_VERSION:-}" ]]; then
    fallback_version="$(grep -m1 -E '^version\s*=' Cargo.toml | cut -d'"' -f2 || true)"
    if [[ -z "$fallback_version" ]]; then
        echo "Error: Unable to determine release version from CI_TAG, CI_VERSION, or Cargo.toml."
        exit 1
    fi
    export CI_VERSION="$fallback_version"
    export CI_TAG="v${fallback_version}"
fi

checksums_file="build/checksums.txt"
if [[ ! -f "$checksums_file" ]]; then
    echo "Error: Missing $checksums_file. The tap recipe cannot be updated."
    exit 1
fi

x64_sha256="$(awk '$2 == "fp-appimage-updater.x64" { print $1 }' "$checksums_file")"
arm_sha256="$(awk '$2 == "fp-appimage-updater.ARM" { print $1 }' "$checksums_file")"

if [[ -z "$x64_sha256" || -z "$arm_sha256" ]]; then
    echo "Error: Could not read both architecture checksums from $checksums_file."
    exit 1
fi

tap_release_tag="${CI_TAG}"
tap_version="${CI_VERSION:-${tap_release_tag#v}}"
package_registry_url="${PACKAGE_REGISTRY_URL:-${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/${tap_package_name}/${tap_release_tag}}"

tap_workdir="$(mktemp -d)"
trap 'rm -rf "$tap_workdir"' EXIT

git init -b "$tap_branch" "$tap_workdir" >/dev/null
cd "$tap_workdir"
git remote add origin "$tap_repo_url"
git config user.name "${HOMEBREW_TAP_GIT_NAME:-GitLab CI}"
git config user.email "${HOMEBREW_TAP_GIT_EMAIL:-ci@gitlab.local}"

git_auth=(-c "http.extraHeader=PRIVATE-TOKEN: ${tap_push_token}")

if git "${git_auth[@]}" ls-remote --exit-code --heads origin "$tap_branch" >/dev/null 2>&1; then
    git "${git_auth[@]}" fetch --depth 1 origin "$tap_branch"
    git checkout -B "$tap_branch" "origin/$tap_branch" >/dev/null
fi

mkdir -p "$(dirname "$tap_formula_path")"
cat > "$tap_formula_path" <<EOF
class ${tap_class_name} < Formula
  desc "${tap_description}"
  homepage "${tap_homepage}"
  license "${tap_license}"
  version "${tap_version}"

  on_macos do
    odie "${tap_formula_name} is Linux-only."
  end

  if OS.linux?
    if Hardware::CPU.intel?
      url "${package_registry_url}/fp-appimage-updater.x64"
      sha256 "${x64_sha256}"
    elsif Hardware::CPU.arm?
      url "${package_registry_url}/fp-appimage-updater.ARM"
      sha256 "${arm_sha256}"
    else
      odie "${tap_formula_name} is only available for x86_64 and arm64 Linux."
    end
  end

  def install
    if Hardware::CPU.intel?
      bin.install "fp-appimage-updater.x64" => "${tap_formula_name}"
    elsif Hardware::CPU.arm?
      bin.install "fp-appimage-updater.ARM" => "${tap_formula_name}"
    else
      odie "${tap_formula_name} is only available for x86_64 and arm64 Linux."
    end

    chmod 0755, bin/"${tap_formula_name}"
    generate_completions_from_executable(bin/"${tap_formula_name}", "completion")
  end

  test do
    output = shell_output("#{bin}/${tap_formula_name} --help")
    assert_match "${tap_formula_name}", output
  end
end
EOF

if ! git status --porcelain -- "$tap_formula_path" | grep -q .; then
    echo "Homebrew tap formula is already up to date."
    exit 0
fi

git add "$tap_formula_path"
git commit -m "chore: update Homebrew tap for ${CI_TAG}" >/dev/null
git "${git_auth[@]}" push -u origin "$tap_branch"

echo "Homebrew tap recipe synced successfully."
