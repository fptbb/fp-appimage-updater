# fp-appimage-updater

A fast, single-binary CLI tool written in Rust designed to manage, update, and integrate AppImages entirely through declarative user-provided YAML configurations. Operating strictly in user-space, it is perfect for immutable/atomic Linux environments.

## Features
- **Data-Driven:** All apps and their update strategies are defined in YAML files.
- **Update Resolvers:** Fetch the latest version via Forge Releases (GitHub/GitLab), Direct Links (ETag/Last-Modified HTTP Headers), or Custom Shell Scripts.
- **Delta Updates:** Uses `zsync` when available to download only modified bytes.
- **Desktop Integration:** Extracts exact `.desktop` manifests and icons directly from the AppImage using `--appimage-extract` and seamlessly inserts them into your `.local/share/applications` application menu.
- **Global & Local Configs:** Override storage paths, integration behaviors, and symlinking easily per-app or globally.

## Installation

### Quick Install (Recommended)
You can quickly install the latest binary and background systemd service natively utilizing the installation script:

```bash
curl -sL https://raw.githubusercontent.com/fptbb/fp-appimage-updater/main/install.sh | bash
```

Alternatively, to uninstall it:
```bash
curl -sL https://raw.githubusercontent.com/fptbb/fp-appimage-updater/main/install.sh | bash -s -- --uninstall
```

### Using Pre-built Binaries
You can download the latest compiled binary (`fp-appimage-updater.x64`) from the GitHub Releases page. Make it executable and drop it into your `~/.local/bin/` folder.

### From Source
If you wish to compile the tool yourself from the source code, or if you want to set up the background automated daemon tasks (via `just manual-install`), please see the [CONTRIBUTING](CONTRIBUTING.md) guidelines.

### Systemd Background Updates
If you installed the application using `just manual-install`, a systemd timer is automatically configured to run checks periodically in the background.

Since this tool is strictly designed around user-space operations, **do not use `sudo`** when interacting with its systemd services. Always use the `--user` flag.

Check the background timer status:
```bash
systemctl --user status fp-appimage-updater.timer
```

View the latest background execution logs:
```bash
journalctl --user -u fp-appimage-updater.service -n 50
```

Enable or start the timer manually:
```bash
systemctl --user enable --now fp-appimage-updater.timer
```

To entirely remove the binary and the background daemons, run:
```bash
just uninstall
```

## Directory Structure
The tool expects application recipes in your `~/.config/fp-appimage-updater/` folder.

```
~/.config/fp-appimage-updater/
├── config.yml                # Global behaviors (storage paths, symlinks, integration toggles)
└── apps/                     # Your applications
    ├── hayase/
    │   ├── app.yml           # Definition for Hayase
    │   └── resolver.sh       # Custom parsing script if Strategy is 'script'
    └── whatpulse/
        └── app.yml           # Definition using 'direct' Strategy via ETags
```

### Global Configuration Example (`config.yml`)
```yaml
storage_dir: ~/.local/bin/AppImages
symlink_dir: ~/.local/bin
naming_format: "{name}.AppImage"
manage_desktop_files: true
create_symlinks: false
```

### App Recipe Example (`apps/whatpulse/app.yml`)
```yaml
name: whatpulse
strategy:
  strategy: direct
  url: "https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage"
  check_method: etag
```
More examples in `examples/apps/` folder.

## CLI Usage

### Check for Updates
Check the status of all your configured recipes to see if new versions are available remotely:
```bash
fp-appimage-updater check
```

Check a single app:
```bash
fp-appimage-updater check whatpulse
```

### Update Applications
Install or update a single AppImage:
```bash
fp-appimage-updater update whatpulse
```

Update all configurations at once:
```bash
fp-appimage-updater update
```

### List Installed Apps
Review the current integration and local versions of your defined applications:
```bash
fp-appimage-updater list
```

### Remove Applications
Remove an application's binary, symlink, extracted icons, and desktop files:
```bash
fp-appimage-updater remove whatpulse
```

Remove all installed applications at once:
```bash
fp-appimage-updater remove -a
```
