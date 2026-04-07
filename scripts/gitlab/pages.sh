#!/bin/sh
set -eu

root_dir="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"

mkdir -p "$root_dir/public"
cp "$root_dir/install.sh" "$root_dir/public/install.sh"
cp "$root_dir/install-github.sh" "$root_dir/public/install-github.sh"
printf '/ /install.sh 301\n' > "$root_dir/public/_redirects"
