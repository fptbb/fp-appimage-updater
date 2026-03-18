#!/usr/bin/env bash
set -e

REPO="fptbb/fp-appimage-updater"
APP_NAME="fp-appimage-updater"

INSTALL_SYSTEMD=true
USE_PRERELEASE=false
SCOPE="auto"

for arg in "$@"; do
    if [ "$arg" = "--no-systemd" ]; then
        INSTALL_SYSTEMD=false
    elif [ "$arg" = "--pre-release" ]; then
        USE_PRERELEASE=true
    elif [ "$arg" = "--system" ]; then
        SCOPE="system"
    elif [ "$arg" = "--user" ]; then
        SCOPE="user"
    elif [ "$arg" = "uninstall" ] || [ "$arg" = "--uninstall" ]; then
        echo "Uninstalling $APP_NAME..."

        # disables user-wide active timers gracefully
        systemctl --user disable --now "${APP_NAME}.timer" 2>/dev/null || true

        # cleans user-wide paths
        rm -f "$HOME/.local/bin/$APP_NAME"
        rm -f "$HOME/.config/systemd/user/${APP_NAME}.service"
        rm -f "$HOME/.config/systemd/user/${APP_NAME}.timer"
        systemctl --user daemon-reload 2>/dev/null || true

        # skips system-wide cleanup entirely on immutable systems or non-root executions
        if [ -w "/usr/bin" ] && [ -w "/usr/lib/systemd/system" ]; then
            systemctl disable --now "${APP_NAME}.timer" 2>/dev/null || true
            
            SYSTEM_PATHS=(
                "/usr/bin/$APP_NAME"
                "/usr/local/bin/$APP_NAME"
                "/usr/lib/systemd/system/${APP_NAME}.service"
                "/usr/lib/systemd/system/${APP_NAME}.timer"
                "/etc/systemd/system/${APP_NAME}.service"
                "/etc/systemd/system/${APP_NAME}.timer"
            )

            for target in "${SYSTEM_PATHS[@]}"; do
                if [ -f "$target" ]; then
                    rm -f "$target"
                fi
            done
            
            systemctl daemon-reload 2>/dev/null || true
        else
            echo "note: skipped system-wide cleanup (read-only filesystem or requires root)"
        fi

        echo "Uninstallation complete!"
        echo "Note: AppImage binaries and configs in ~/.config/fp-appimage-updater were left intact."
        exit 0
    fi
done

# resolves target directories and validates system writability
if [ "$SCOPE" = "auto" ] || [ "$SCOPE" = "system" ]; then
    if [ -w "/usr/bin" ] && [ -w "/usr/lib/systemd/system" ]; then
        SCOPE="system"
        BIN_DIR="/usr/bin"
        SYSTEMD_DIR="/usr/lib/systemd/system"
        SYSTEMCTL_CMD="systemctl"
    else
        if [ "$SCOPE" = "system" ]; then
            echo "error: failed to write to /usr/bin or /usr/lib/systemd/system."
            echo "you must be on an immutable system or lack sufficient privileges."
            exit 1
        else
            echo "warning: system paths are read-only. you must be on an immutable system."
            echo "falling back to user-wide installation."
            SCOPE="user"
        fi
    fi
fi

if [ "$SCOPE" = "user" ]; then
    BIN_DIR="$HOME/.local/bin"
    SYSTEMD_DIR="$HOME/.config/systemd/user"
    SYSTEMCTL_CMD="systemctl --user"
fi

echo "Starting $SCOPE-wide installation of $APP_NAME..."

# detects architecture
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

# fetches release version
if [ "$USE_PRERELEASE" = "true" ]; then
    echo "Fetching latest release version from GitHub (including pre-releases)..."
    VERSION=$(curl -sL "https://api.github.com/repos/$REPO/releases?per_page=1" \
        | grep '"tag_name":' | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')
    RELEASE_KIND="release"
else
    echo "Fetching latest stable release version from GitHub..."
    VERSION=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name":' | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')
    RELEASE_KIND="release"
fi

if [ -z "$VERSION" ]; then
    echo "Error: Could not determine latest release version. Maybe API rate limited?"
    exit 1
fi

echo "Found latest ${RELEASE_KIND}: $VERSION"

# downloads binary
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${APP_NAME}.${TARGET_ARCH}"
echo "Downloading binary for $TARGET_ARCH from $DOWNLOAD_URL..."

mkdir -p "$BIN_DIR"
curl -sL --fail --progress-bar "$DOWNLOAD_URL" -o "$BIN_DIR/$APP_NAME"
chmod +x "$BIN_DIR/$APP_NAME"

if [ "$INSTALL_SYSTEMD" = true ]; then
    # downloads systemd instances
    echo "Setting up background systemd services in $SYSTEMD_DIR..."
    mkdir -p "$SYSTEMD_DIR"

    SERVICE_URL="https://github.com/${REPO}/releases/download/${VERSION}/${APP_NAME}.service"
    TIMER_URL="https://github.com/${REPO}/releases/download/${VERSION}/${APP_NAME}.timer"

    curl -sL --fail "$SERVICE_URL" -o "$SYSTEMD_DIR/${APP_NAME}.service"
    curl -sL --fail "$TIMER_URL" -o "$SYSTEMD_DIR/${APP_NAME}.timer"

    # adjusts ExecStart path to match installation directory
    sed -i "s|%h/.local/bin|$BIN_DIR|g" "$SYSTEMD_DIR/${APP_NAME}.service"

    # enables and starts systemd services
    if [ "$SYSTEMCTL_CMD" = "systemctl" ]; then
        $SYSTEMCTL_CMD daemon-reload 2>/dev/null || true
        $SYSTEMCTL_CMD enable "${APP_NAME}.timer" 2>/dev/null || true
    else
        $SYSTEMCTL_CMD daemon-reload
        $SYSTEMCTL_CMD enable --now "${APP_NAME}.timer"
    fi

    echo ""
    echo "Installation complete!"
    echo "Background updates are scheduled via systemd (${APP_NAME}.timer)."
else
    echo ""
    echo "Installation complete!"
    echo "Systemd service installation was skipped (--no-systemd specified)."
fi

# verifies if target directory is in current path
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "warning: $BIN_DIR is not in your PATH."
    if [ "$SCOPE" = "user" ]; then
        echo "add 'export PATH=\"$BIN_DIR:\$PATH\"' to your shell configuration."
    else
        echo "ensure $BIN_DIR is added to the system-wide path variables."
    fi
fi