#!/usr/bin/env bash
set -e

REPO="fptbb/fp-appimage-updater"
APP_NAME="fp-appimage-updater"
BIN_DIR="$HOME/.local/bin"
SYSTEMD_USER_DIR="$HOME/.config/systemd/user"

INSTALL_SYSTEMD=true

for arg in "$@"; do
    if [ "$arg" = "--no-systemd" ]; then
        INSTALL_SYSTEMD=false
    elif [ "$arg" = "uninstall" ] || [ "$arg" = "--uninstall" ]; then
        echo "Uninstalling $APP_NAME..."
        systemctl --user disable --now "${APP_NAME}.timer" 2>/dev/null || true
        echo "Removing systemd units..."
        rm -f "$SYSTEMD_USER_DIR/${APP_NAME}.service"
        rm -f "$SYSTEMD_USER_DIR/${APP_NAME}.timer"
        systemctl --user daemon-reload
        echo "Removing binary..."
        rm -f "$BIN_DIR/$APP_NAME"
        echo "Uninstallation complete!"
        echo "Note: AppImage binaries and configs in ~/.config/fp-appimage-updater were left intact."
        exit 0
    fi
done

echo "Starting installation of $APP_NAME..."

# 1. Detect Architecture
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64)
        TARGET_ARCH="x64"
        ;;
    aarch64|arm64)
        TARGET_ARCH="ARM"
        ;;
    *)
        echo "Error: Unsupported architecture $ARCH"
        exit 1
        ;;
esac

# 2. Fetch latest release version
echo "Fetching latest release version from GitHub..."
VERSION=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | head -n 1 | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$VERSION" ]; then
    echo "Error: Could not determine latest release version. Maybe API rate limited?"
    exit 1
fi

echo "Found latest version: $VERSION"

# 3. Download the binary
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${APP_NAME}.${TARGET_ARCH}"
echo "Downloading binary for $TARGET_ARCH from $DOWNLOAD_URL..."

mkdir -p "$BIN_DIR"
curl -sL --fail --progress-bar "$DOWNLOAD_URL" -o "$BIN_DIR/$APP_NAME"
chmod +x "$BIN_DIR/$APP_NAME"

if [ "$INSTALL_SYSTEMD" = true ]; then
    # 4. Create systemd instances
    echo "Setting up background systemd services..."
    mkdir -p "$SYSTEMD_USER_DIR"

    cat << EOF > "$SYSTEMD_USER_DIR/${APP_NAME}.service"
[Unit]
Description=FP AppImage Updater Service
Documentation=https://github.com/$REPO
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=$BIN_DIR/$APP_NAME update
# Give generous limits for downloading massive AppImages
TimeoutStartSec=3600

[Install]
WantedBy=default.target
EOF

    cat << EOF > "$SYSTEMD_USER_DIR/${APP_NAME}.timer"
[Unit]
Description=FP AppImage Updater Background Timer
Documentation=https://github.com/$REPO

[Timer]
# Run 15 minutes after boot, and then every 12 hours
OnBootSec=15min
OnUnitActiveSec=12h
Persistent=true

[Install]
WantedBy=timers.target
EOF

    # 5. Enable and start systemd services
    systemctl --user daemon-reload
    systemctl --user enable --now "${APP_NAME}.timer"

    echo ""
    echo "Installation complete!"
    echo "Make sure '$BIN_DIR' is in your PATH."
    echo "Background updates are scheduled via systemd (${APP_NAME}.timer)."
else
    echo ""
    echo "Installation complete!"
    echo "Make sure '$BIN_DIR' is in your PATH."
    echo "Systemd service installation was skipped (--no-systemd specified)."
fi
