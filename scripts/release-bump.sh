#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
spec_file="$root_dir/copr.spec"

# shellcheck disable=SC1091
source "$root_dir/scripts/ci/lib.sh"

cd "$root_dir"
ci_read_cargo_version
version="$CI_VERSION"

current_spec_version="$(sed -n 's/^Version:[[:space:]]*\(.*\)$/\1/p' "$spec_file" | head -n1 | tr -d '[:space:]')"
current_spec_release="$(sed -n 's/^Release:[[:space:]]*\([0-9][0-9]*\).*$/\1/p' "$spec_file" | head -n1)"
today="$(LC_ALL=C date '+%a %b %d %Y')"
spec_note="- Update to version ${version}"
release_number=1

if [[ "$current_spec_version" == "$version" ]]; then
    release_number=$(( ${current_spec_release:-0} + 1 ))
fi

spec_entry="* ${today} fp-appimage-updater Maintainer - ${version}-${release_number}"

if [[ "$current_spec_version" != "$version" ]]; then
    sed -i -E "s/^Version:[[:space:]].*/Version:        ${version}/" "$spec_file"
    echo "Updated copr.spec version to ${version}"
fi

if [[ "$current_spec_release" != "$release_number" ]]; then
    sed -i -E "s/^Release:[[:space:]].*/Release:        ${release_number}%{?dist}/" "$spec_file"
    echo "Updated copr.spec release to ${release_number}"
fi

tmp_file="$(mktemp)"
awk -v entry="$spec_entry" -v note="$spec_note" '
    BEGIN { inserted = 0 }
    /^%changelog$/ && !inserted {
        print
        print entry
        print note
        inserted = 1
        next
    }
    { print }
    END {
        if (!inserted) {
            print "%changelog"
            print entry
            print note
        }
    }
' "$spec_file" > "$tmp_file"
mv "$tmp_file" "$spec_file"
echo "Updated copr.spec changelog"

git -C "$root_dir" add Cargo.toml copr.spec
if git -C "$root_dir" diff --cached --quiet; then
    echo "No release changes to commit."
    exit 0
fi

# git -C "$root_dir" commit -m "chore: bump version to ${version} in Cargo.toml"
