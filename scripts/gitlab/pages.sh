#!/bin/sh
set -eu

root_dir="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"

mkdir -p "$root_dir/public"
cp "$root_dir/install.sh" "$root_dir/public/i"
cp "$root_dir/install-github.sh" "$root_dir/public/ig"
cp "$root_dir/build/schema-global.json" "$root_dir/public/schema-global.json"
cp "$root_dir/build/schema-recipe.json" "$root_dir/public/schema-recipe.json"

cat > "$root_dir/public/_redirects" <<'EOF'
/ https://gitlab.com/fpsys/fp-appimage-updater 301
/:file https://gitlab.com/fpsys/fp-appimage-updater/-/blob/main/:file 301
/* https://gitlab.com/fpsys/fp-appimage-updater/-/blob/main/:splat 301
EOF
