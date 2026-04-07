#!/usr/bin/env bash
# fp-appimage-updater install script
# GitLab page: https://gitlab.com/fpsys/fp-appimage-updater
# Run from terminal:
#   curl -sL fau.fpt.icu/i | bash
#   curl -sL fau.fpt.icu/i | bash -s -- [OPTIONS]
#   curl -sL fau.fpt.icu/i | sudo bash -s -- --system
set -euo pipefail

main() {
    export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=/run/user/${UID}/bus}"
    
    REPO="fpsys/fp-appimage-updater"
    APP_NAME="fp-appimage-updater"
    
    INSTALL_SYSTEMD=true
    SCOPE="auto"
    
    for arg in "$@"; do
        if [ "$arg" = "--help" ] || [ "$arg" = "-h" ]; then
            echo "Usage: curl -sL fau.fpt.icu/i | bash -s -- [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --system         Force system-wide installation (/usr/bin/)"
            echo "  --user           Force user-wide installation (~/.local/bin/)"
            echo "  --no-systemd     Skip systemd background timer installation"
            echo "  --uninstall      Remove fp-appimage-updater from the system"
            echo "  -h, --help       Show this help message"
            exit 0
            elif [ "$arg" = "--no-systemd" ]; then
            INSTALL_SYSTEMD=false
            elif [ "$arg" = "--system" ]; then
            SCOPE="system"
            elif [ "$arg" = "--user" ]; then
            SCOPE="user"
            elif [ "$arg" = "uninstall" ] || [ "$arg" = "--uninstall" ]; then
            echo -e "\e[34m[INFO]\e[0m Uninstalling $APP_NAME..."
            
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
                echo -e "\e[33m[WARN]\e[0m Skipped system-wide cleanup (read-only filesystem or requires root)"
            fi
            
            echo -e "\e[32m[SUCCESS]\e[0m Uninstallation complete!"
            echo "Note: AppImage binaries and configs in ~/.config/fp-appimage-updater were left intact."
            exit 0
        fi
    done
    
    # Systemd Reality Check
    if [ "$INSTALL_SYSTEMD" = true ]; then
        if ! command -v systemctl >/dev/null 2>&1; then
            echo -e "\e[33m[WARN]\e[0m systemctl not found. Disabling systemd background timers installation."
            INSTALL_SYSTEMD=false
        fi
    fi
    
    # Native Package Manager Detection
    check_package_manager() {
        local bin_path="$1"
        if [ ! -f "$bin_path" ]; then return 0; fi
        
        local managed=false
        if command -v rpm >/dev/null 2>&1 && rpm -qf "$bin_path" >/dev/null 2>&1; then
            echo -e "\e[31m[ERROR]\e[0m $APP_NAME is managed by RPM (dnf/zypper/COPR). Please use your package manager to update."
            managed=true
        fi
        
        if [ "$managed" = true ]; then
            exit 1
        fi
    }
    check_package_manager "/usr/bin/$APP_NAME"
    check_package_manager "/usr/local/bin/$APP_NAME"
    
    # Immutable Distro Check
    is_immutable() {
        if [ -d "/run/ostree-booted" ]; then
            return 0
        fi
        # If not root and /usr/bin is strictly read-only
        if [ ! -w "/usr/bin" ] && [ "$EUID" -ne 0 ]; then
            if ! touch /usr/bin/.fau_writetest 2>/dev/null; then
                return 0
            fi
            rm -f /usr/bin/.fau_writetest 2>/dev/null || true
        fi
        return 1
    }
    
    # Strict Scope Resolver
    if [ "$SCOPE" = "system" ]; then
        if is_immutable; then
            echo -e "\e[31m[ERROR]\e[0m You requested --system, but the system is immutable or /usr/bin is read-only. Aborting."
            exit 1
            elif [ ! -w "/usr/bin" ] || [ ! -w "/usr/lib/systemd/system" ]; then
            echo -e "\e[31m[ERROR]\e[0m You requested --system but lack permissions. Run 'curl ... | sudo bash -s -- --system'. Aborting."
            exit 1
        else
            BIN_DIR="/usr/bin"
            SYSTEMD_DIR="/usr/lib/systemd/system"
            SYSTEMCTL_CMD="systemctl"
        fi
        elif [ "$SCOPE" = "auto" ]; then
        if is_immutable || [ ! -w "/usr/bin" ] || [ ! -w "/usr/lib/systemd/system" ]; then
            SCOPE="user"
            BIN_DIR="$HOME/.local/bin"
            SYSTEMD_DIR="$HOME/.config/systemd/user"
            SYSTEMCTL_CMD="systemctl --user"
        else
            SCOPE="system"
            BIN_DIR="/usr/bin"
            SYSTEMD_DIR="/usr/lib/systemd/system"
            SYSTEMCTL_CMD="systemctl"
        fi
    else
        BIN_DIR="$HOME/.local/bin"
        SYSTEMD_DIR="$HOME/.config/systemd/user"
        SYSTEMCTL_CMD="systemctl --user"
    fi
    
    # Cleanup duplicate conflicting installs
    if [ "$SCOPE" = "user" ]; then
        if [ -f "/usr/bin/$APP_NAME" ]; then
            echo -e "\e[33m[WARN]\e[0m Found conflicting system-wide installation. Attempting to clean up..."
            if [ -w "/usr/bin" ] && [ -w "/usr/lib/systemd/system" ]; then
                systemctl disable --now "${APP_NAME}.timer" 2>/dev/null || true
                rm -f "/usr/bin/$APP_NAME" "/usr/local/bin/$APP_NAME"
                rm -f "/usr/lib/systemd/system/${APP_NAME}.service" "/usr/lib/systemd/system/${APP_NAME}.timer"
                rm -f "/etc/systemd/system/${APP_NAME}.service" "/etc/systemd/system/${APP_NAME}.timer"
                systemctl daemon-reload 2>/dev/null || true
                elif command -v sudo >/dev/null 2>&1; then
                sudo systemctl disable --now "${APP_NAME}.timer" 2>/dev/null || true
                sudo rm -f "/usr/bin/$APP_NAME" "/usr/local/bin/$APP_NAME"
                sudo rm -f "/usr/lib/systemd/system/${APP_NAME}.service" "/usr/lib/systemd/system/${APP_NAME}.timer"
                sudo rm -f "/etc/systemd/system/${APP_NAME}.service" "/etc/systemd/system/${APP_NAME}.timer"
                sudo systemctl daemon-reload 2>/dev/null || true
            else
                echo -e "\e[31m[ERROR]\e[0m Insufficient privileges to safely remove system-wide deployment."
            fi
        fi
        elif [ "$SCOPE" = "system" ]; then
        if [ -f "$HOME/.local/bin/$APP_NAME" ]; then
            echo -e "\e[33m[WARN]\e[0m Found conflicting user-wide installation. Cleaning up..."
            systemctl --user disable --now "${APP_NAME}.timer" 2>/dev/null || true
            rm -f "$HOME/.local/bin/$APP_NAME"
            rm -f "$HOME/.config/systemd/user/${APP_NAME}.service" "$HOME/.config/systemd/user/${APP_NAME}.timer"
            systemctl --user daemon-reload 2>/dev/null || true
        fi
    fi
    
    echo -e "\e[34m[INFO]\e[0m Starting $SCOPE-wide installation of $APP_NAME..."
    
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
            echo -e "\e[31m[ERROR]\e[0m Unsupported architecture $ARCH"
            exit 1
        ;;
    esac
    
    # fetches release version
    extract_tag_name() {
        tr -d '\n' | sed -nE 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/p'
    }
    
    download_asset() {
        PRIMARY_URL="$1"
        FALLBACK_URL="$2"
        OUTPUT_PATH="$3"
        
        if ! curl -sL --fail --progress-bar "$PRIMARY_URL" -o "$OUTPUT_PATH"; then
            echo -e "\e[33m[WARN]\e[0m Primary asset URL failed, falling back to GitLab job artifact..."
            if [ -n "$FALLBACK_URL" ]; then
                curl -sL --fail --progress-bar "$FALLBACK_URL" -o "$OUTPUT_PATH"
            fi
        fi
    }
    
    echo -e "\e[34m[INFO]\e[0m Fetching latest stable release version from GitLab..."
    VERSION=$(curl -sL "https://gitlab.com/api/v4/projects/${REPO//\//%2F}/releases/permalink/latest" | extract_tag_name)
    RELEASE_KIND="release"
    
    if [ -z "$VERSION" ]; then
        echo -e "\e[31m[ERROR]\e[0m Could not determine latest release version. Maybe API rate limited?"
        exit 1
    fi
    
    echo -e "\e[34m[INFO]\e[0m Found latest ${RELEASE_KIND}: $VERSION"
    
    WORK_DIR=$(mktemp -d)
    trap 'rm -rf "$WORK_DIR"' EXIT
    
    # downloads binary
    DOWNLOAD_URL="https://gitlab.com/${REPO}/-/releases/${VERSION}/downloads/bin/${APP_NAME}.${TARGET_ARCH}"
    FALLBACK_DOWNLOAD_URL="https://gitlab.com/${REPO}/-/jobs/artifacts/main/raw/build/${APP_NAME}.${TARGET_ARCH}?job=build-and-compress"
    CHECKSUMS_URL="https://gitlab.com/${REPO}/-/releases/${VERSION}/downloads/bin/checksums.txt"
    FALLBACK_CHECKSUMS_URL="https://gitlab.com/${REPO}/-/jobs/artifacts/main/raw/build/checksums.txt?job=build-and-compress"
    
    echo -e "\e[34m[INFO]\e[0m Downloading binary and checking checksums..."
    download_asset "$DOWNLOAD_URL" "$FALLBACK_DOWNLOAD_URL" "$WORK_DIR/${APP_NAME}.${TARGET_ARCH}"
    
    if curl -sL --fail "$CHECKSUMS_URL" -o "$WORK_DIR/checksums.txt" || curl -sL --fail "$FALLBACK_CHECKSUMS_URL" -o "$WORK_DIR/checksums.txt"; then
        echo -e "\e[34m[INFO]\e[0m Validating checksum..."
        pushd "$WORK_DIR" >/dev/null
        if ! sha256sum --ignore-missing -c checksums.txt 2>/dev/null | grep -q 'OK'; then
            if ! sha256sum -c checksums.txt --ignore-missing; then
                echo -e "\e[31m[ERROR]\e[0m Checksum validation failed! The downloaded binary is corrupted or compromised."
                exit 1
            fi
        fi
        popd >/dev/null
    else
        echo -e "\e[33m[WARN]\e[0m checksums.txt not found in release assets. Skipping cryptographic validation."
    fi
    
    mkdir -p "$BIN_DIR"
    mv "$WORK_DIR/${APP_NAME}.${TARGET_ARCH}" "$BIN_DIR/$APP_NAME"
    chmod +x "$BIN_DIR/$APP_NAME"
    
    if [ "$INSTALL_SYSTEMD" = true ]; then
        # downloads systemd instances
        echo -e "\e[34m[INFO]\e[0m Setting up background systemd services in $SYSTEMD_DIR..."
        mkdir -p "$SYSTEMD_DIR"
        
        SERVICE_URL="https://gitlab.com/${REPO}/-/releases/${VERSION}/downloads/systemd/${APP_NAME}.service"
        TIMER_URL="https://gitlab.com/${REPO}/-/releases/${VERSION}/downloads/systemd/${APP_NAME}.timer"
        FALLBACK_SERVICE_URL="https://gitlab.com/${REPO}/-/jobs/artifacts/main/raw/systemd/${APP_NAME}.service?job=build-and-compress"
        FALLBACK_TIMER_URL="https://gitlab.com/${REPO}/-/jobs/artifacts/main/raw/systemd/${APP_NAME}.timer?job=build-and-compress"
        
        download_asset "$SERVICE_URL" "$FALLBACK_SERVICE_URL" "$SYSTEMD_DIR/${APP_NAME}.service"
        download_asset "$TIMER_URL" "$FALLBACK_TIMER_URL" "$SYSTEMD_DIR/${APP_NAME}.timer"
        
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
        echo -e "\e[32m[SUCCESS]\e[0m Installation complete!"
        echo "Background updates are scheduled via systemd (${APP_NAME}.timer)."
    else
        echo ""
        echo -e "\e[32m[SUCCESS]\e[0m Installation complete!"
        echo "Systemd service installation was skipped (--no-systemd specified)."
    fi
    
    # verifies if target directory is in current path
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        echo ""
        echo -e "\e[33m[WARN]\e[0m $BIN_DIR is not in your PATH."
        if [ "$SCOPE" = "user" ]; then
            if [[ "$SHELL" == *"zsh"* ]]; then
                echo -e "Run this command: \e[34mecho 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc\e[0m"
            else
                echo -e "Run this command: \e[34mecho 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc\e[0m"
            fi
        else
            echo "Ensure $BIN_DIR is added to the system-wide path variables."
        fi
    fi
}

main "$@"
