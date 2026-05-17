# Justfile for fp-appimage-updater
# Requirements: cargo, upx

APP_NAME := "fp-appimage-updater"
BUILD_DIR := "build"

_default:
    @just --list

build-all: prepare build-linux-x64 build-linux-x64-musl build-linux-arm build-linux-arm-musl

prepare:
    #!/usr/bin/env bash
    mkdir -p {{BUILD_DIR}}

alias do := dev

dev it commands:
    #!/usr/bin/env bash
    cargo watch -x "run -- {{commands}}"

clean-root:
    #!/usr/bin/env bash
    rm -f {{APP_NAME}} {{APP_NAME}}.x64 {{APP_NAME}}.x64-v3 {{APP_NAME}}.x64-musl {{APP_NAME}}.ARM {{APP_NAME}}.ARM-musl

build-linux-x64: prepare
    #!/usr/bin/env bash
    echo "Building Linux x64 (glibc)..."
    cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.28
    if command -v upx >/dev/null; then \
        echo "Compressing Linux x64..."; \
        upx --best --lzma target/x86_64-unknown-linux-gnu/release/{{APP_NAME}}; \
    fi
    mv target/x86_64-unknown-linux-gnu/release/{{APP_NAME}} {{BUILD_DIR}}/{{APP_NAME}}.x64
    echo "Done: {{BUILD_DIR}}/{{APP_NAME}}.x64"

build-linux-x64-v3: prepare
    #!/usr/bin/env bash
    echo "Building Linux x64-v3 (Host glibc)..."
    RUSTFLAGS="-C target-cpu=x86-64-v3" cargo build --release
    if command -v upx >/dev/null; then \
        echo "Compressing Linux x64-v3..."; \
        upx --best --lzma target/release/{{APP_NAME}}; \
    fi
    mv target/release/{{APP_NAME}} {{BUILD_DIR}}/{{APP_NAME}}.x64-v3
    echo "Done: {{BUILD_DIR}}/{{APP_NAME}}.x64-v3"

build-linux-x64-musl: prepare
    #!/usr/bin/env bash
    echo "Building Linux x64 (musl)..."
    cargo zigbuild --release --target x86_64-unknown-linux-musl
    if command -v upx >/dev/null; then \
        echo "Compressing Linux x64 musl..."; \
        upx --best --lzma target/x86_64-unknown-linux-musl/release/{{APP_NAME}}; \
    fi
    mv target/x86_64-unknown-linux-musl/release/{{APP_NAME}} {{BUILD_DIR}}/{{APP_NAME}}.x64-musl
    echo "Done: {{BUILD_DIR}}/{{APP_NAME}}.x64-musl"

build-linux-arm: prepare
    #!/usr/bin/env bash
    echo "Building Linux ARM64 (glibc)..."
    cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28
    if command -v upx >/dev/null; then \
        echo "Compressing Linux ARM64..."; \
        upx --best --lzma target/aarch64-unknown-linux-gnu/release/{{APP_NAME}}; \
    fi
    mv target/aarch64-unknown-linux-gnu/release/{{APP_NAME}} {{BUILD_DIR}}/{{APP_NAME}}.ARM
    echo "Done: {{BUILD_DIR}}/{{APP_NAME}}.ARM"

build-linux-arm-musl: prepare
    #!/usr/bin/env bash
    echo "Building Linux ARM64 (musl)..."
    cargo zigbuild --release --target aarch64-unknown-linux-musl
    if command -v upx >/dev/null; then \
        echo "Compressing Linux ARM64 musl..."; \
        upx --best --lzma target/aarch64-unknown-linux-musl/release/{{APP_NAME}}; \
    fi
    mv target/aarch64-unknown-linux-musl/release/{{APP_NAME}} {{BUILD_DIR}}/{{APP_NAME}}.ARM-musl
    echo "Done: {{BUILD_DIR}}/{{APP_NAME}}.ARM-musl"

clean: clean-root
    #!/usr/bin/env bash
    rm -rf {{BUILD_DIR}}
    cargo clean

manual-install: build-linux-x64
    #!/usr/bin/env bash
    echo "Installing binary to ~/.local/bin..."
    mkdir -p ~/.local/bin
    cp build/fp-appimage-updater.x64 ~/.local/bin/fp-appimage-updater
    chmod +x ~/.local/bin/fp-appimage-updater
    echo "Installing systemd user service..."
    mkdir -p ~/.config/systemd/user
    cp systemd/fp-appimage-updater.service ~/.config/systemd/user/
    cp systemd/fp-appimage-updater.timer ~/.config/systemd/user/
    systemctl --user daemon-reload
    systemctl --user enable --now fp-appimage-updater.timer
    echo "Installation complete! Make sure '~/.local/bin' is in your PATH."

uninstall:
    #!/usr/bin/env bash
    echo "Uninstalling fp-appimage-updater..."
    -systemctl --user disable --now fp-appimage-updater.timer 2>/dev/null || true
    echo "Removing systemd units..."
    rm -f ~/.config/systemd/user/fp-appimage-updater.service
    rm -f ~/.config/systemd/user/fp-appimage-updater.timer
    systemctl --user daemon-reload
    echo "Removing binary..."
    rm -f ~/.local/bin/fp-appimage-updater
    echo "Uninstallation complete."
    echo "Note: AppImage binaries and configs in ~/.config/fp-appimage-updater were left intact."

test:
    #!/usr/bin/env bash
    cargo test -- --nocapture

clippy:
    #!/usr/bin/env bash
    cargo clippy --workspace --all-targets -- -D warnings

docs:
    #!/usr/bin/env bash
    set -e
    cargo watch -x "doc --document-private-items" &
    WATCH_PID=$!
    trap "kill $WATCH_PID 2>/dev/null" EXIT INT TERM
    echo "Docs rebuilding automatically. Serving at http://localhost:8080"
    python3 -m http.server 8080 --directory target/doc/

clean-test-images:
    docker rm -f $(docker ps -aq --filter ancestor=fedora) 2>/dev/null || true
    docker rmi fedora:latest 2>/dev/null || true

release-bump:
    #!/usr/bin/env bash
    bash scripts/release-bump.sh
