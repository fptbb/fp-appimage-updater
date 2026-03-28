# fp-appimage-updater

[![Copr build status](https://copr.fedorainfracloud.org/coprs/fptbb/fp-appimage-updater/package/fp-appimage-updater/status_image/last_build.png)](https://copr.fedorainfracloud.org/coprs/fptbb/fp-appimage-updater/package/fp-appimage-updater/)

fp-appimage-updater is a fast, single-binary CLI tool written in Rust designed to manage, update, and integrate AppImages entirely through declarative user-provided YAML configurations. Operating strictly in user-space, it is intended to be used with dotfiles and works perfectly with immutable/atomic Linux environments.

## Features
- **Data-Driven:** All apps and their update strategies are defined in YAML files.
- **Update Resolvers:** Fetch the latest version via Forge Releases (GitHub/GitLab), Direct Links (ETag/Last-Modified HTTP Headers), or Custom Shell Scripts.
- **Delta Updates:** Uses `zsync` when available to download only modified bytes.
- **Segmented Downloads:** Split large direct downloads into HTTP ranges when the server supports them. Enabled by default.
- **Parallel Operations:** `check` and `update` run multiple apps concurrently to keep large batches fast, with provider-aware caps to avoid hammering the same host.
- **Rate-Limit Cooldowns:** Apps that hit rate limits are skipped until their retry time unless you opt out.
- **GitHub Proxy Fallback:** Optional GitHub metadata proxy support can bypass GitHub API rate limits without proxying the actual download.
- **Desktop Integration:** Extracts exact `.desktop` manifests and icons directly from the AppImage using `--appimage-extract` and seamlessly inserts them into your `.local/share/applications` application menu.
- **Local Health Checks:** `doctor` checks the local configuration, required directories, and optional tooling like `zsync`.
- **Global & Local Configs:** Override storage paths, integration behaviors, symlinking, segmented downloads, rate-limit cooldowns, and GitHub proxy settings per-app or globally.

## Project Facts
- This was made for myself because I was tired of manually updating my AppImages and I wanted a tool that could do it for me automatically without deleting my config files.
- Contributions are welcome, but keep in mind that the project is intended to be simple, any bug fix is welcome, no out of scope features will be added.
- It is intentional that it will never have a repository for recipes, users must be comfortable with creating their own recipes.
- It's just a standalone binary that you can use however you want outside of the systemd service.
- It will never have a GUI, it's just a CLI tool.

## Installation

### Quick Install (Recommended)
You can quickly install the latest binary and background systemd service natively utilizing the installation script:

```bash
curl -sL fau.fpt.icu | bash
```
This will install the binary to `~/.local/bin/` and the systemd service to `~/.config/systemd/user/`. The service will run every 12 hours. If you don't want to install the systemd service, you can use the `--no-systemd` flag:

```bash
curl -sL fau.fpt.icu | bash -s -- --no-systemd
```

If you are building an immutable/atomic OS image or want to install the binary system-wide to `/usr/bin/` with the systemd service in `/usr/lib/systemd/system/`, use the `--system` flag:

```bash
curl -sL fau.fpt.icu | bash -s -- --system
```
Note that these flags can work together.

Alternatively, to uninstall it:
```bash
curl -sL fau.fpt.icu | bash -s -- --uninstall
```

### Using Pre-built Binaries
You can download the latest compiled binaries from the GitHub Releases page. Releases currently provide Linux `x64` and `ARM64` artifacts, and the release workflow also publishes build provenance attestations for those binaries. Make the binary executable and drop it into your `~/.local/bin/` folder. It works as a standalone binary without any dependencies or scheduled services. You can run it manually or integrate it into your own scripts.

### Building From Source
If you wish to compile the tool yourself from the source code, please see the [CONTRIBUTING](CONTRIBUTING.md) guidelines.

## Documentation sections:
*click to expand*
<details>
<summary>1. Directory Structure / Configuration</summary>

### The tool expects application recipes in your `~/.config/fp-appimage-updater/` folder.

```
~/.config/fp-appimage-updater/
├── config.yml                # Global behaviors (storage paths, symlinks, integration toggles)
└── apps/                     # Your applications
    ├── hayase/
    │   ├── app.yml           # Definition for Hayase
    │   └── resolver.sh       # Custom parsing script if Strategy is 'script'
    └── whatpulse.yml         # Definition using 'direct' Strategy via ETags
```

### Global Configuration Example (`config.yml`)
```yaml
storage_dir: ~/.local/bin/AppImages
symlink_dir: ~/.local/bin
naming_format: "{name}.AppImage"
manage_desktop_files: true
create_symlinks: false
segmented_downloads: true
respect_rate_limits: true
github_proxy: false
github_proxy_prefix: "https://gh-proxy.com/"
```

### App Recipe Example (`apps/whatpulse.yml`)
```yaml
name: whatpulse
strategy:
  strategy: direct
  url: "https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage"
  check_method: etag
segmented_downloads: true
```

### Update Strategies

fp-appimage-updater supports three different strategies for resolving and downloading updates.

#### 1. forge
Used for downloading from GitHub or GitLab releases.
- `repository`: The URL to the GitHub or GitLab repository.
- `asset_match`: A wildcard string to match the specific asset name in the release (e.g., `"*-amd64.AppImage"`).
- `github_proxy`: Optional per-app GitHub-only metadata proxy fallback. When enabled, `fp-appimage-updater` retries the GitHub release API through `gh-proxy.com` if the direct request is rate limited. The final download still uses the direct GitHub asset URL.
- `github_proxy_prefix`: Optional proxy base URL used when `github_proxy` is enabled. Defaults to `https://gh-proxy.com/`.
- `respect_rate_limits`: Optional per-app override that tells the updater to skip apps until the retry window expires when a rate limit is hit. Defaults to `true`.

**Example:**
```yaml
strategy:
  strategy: forge
  repository: https://github.com/hydralauncher/hydra
  asset_match: "hydralauncher-*.AppImage"
segmented_downloads: true
```

#### 2. direct
Used when the application provides a direct download URL that always points to the latest version.
- `url`: The static download URL.
- `check_method`: How to detect if the remote file has changed. Use either `etag` or `last_modified`.
- `segmented_downloads`: Optional per-app override for HTTP range downloads. When unset, the global `segmented_downloads` flag is used and defaults to `true`.

**Example:**
```yaml
strategy:
  strategy: direct
  url: "https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage"
  check_method: etag
segmented_downloads: true
```

#### 3. script
Used for complex scenarios where you need to run a custom bash script to determine the latest download URL and a local version identifier to compare against. The script must output two lines: the download URL on the first line, and the unique version string on the second line.
- `script_path`: The relative path to the local bash script.

**Example:**
```yaml
strategy:
  strategy: script
  script_path: ./resolver.sh
segmented_downloads: true
```

More examples in [examples/apps/](examples/apps/) folder.
</details>
<br />
<details>
<summary>2. Systemd Background Updates</summary>
<br />
If you installed the application using the quick install script, a systemd timer is automatically configured to run checks periodically in the background.

Since this tool is strictly designed around user-space operations, **do not use `sudo`** when interacting with its systemd services (except if you installed it system-wide, in which case you should use `sudo` and the `--system` flag instead of `--user`).

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
</details>
<br />
<details>
<summary>3. CLI Usage</summary>

### JSON Output
Add `--json` to `init`, `validate`, `doctor`, `list`, `check`, `update`, or `remove` when you want machine-readable output instead of tables and status lines.

### Initialize Configuration
Create starter configuration files for the global config or a specific app recipe:
```bash
fp-appimage-updater init --global
```

Create an app recipe scaffold with a chosen update strategy:
```bash
fp-appimage-updater init --app whatpulse --strategy direct
```

Use `--force` to overwrite existing files if needed.

### Validate Recipes
Validate all configured application recipe files:
```bash
fp-appimage-updater validate
```

Validate a single recipe by app name:
```bash
fp-appimage-updater validate whatpulse
```

This command checks that recipe files parse correctly and reports invalid files so you can fix them before running updates.

### Doctor
Run a quick health check on the local setup:
```bash
fp-appimage-updater doctor
```

This command checks:
- the config directory
- the apps directory
- the global config file
- the state directory
- whether any recipe files were parsed successfully
- whether any recipe files failed to parse
- whether `zsync` is available on `PATH`

Check the status of all your configured recipes to see if new versions are available remotely:
```bash
fp-appimage-updater check
```

Check a single app:
```bash
fp-appimage-updater check whatpulse
```

The `check` output now also reports support hints when they are available, such as direct-download range support for segmented downloads and the resolver metadata it used to compare versions.

### Update Applications
Install or update a single AppImage:
```bash
fp-appimage-updater update whatpulse
```

Update all configurations at once:
```bash
fp-appimage-updater update
```

Successful updates now include the elapsed time in seconds so you can see how long each app took to install or update.
When the updater detects a rate limit, it remembers the retry window and skips that app on the next run unless `respect_rate_limits` is disabled globally or for that app.
GitHub forge apps can optionally use `github_proxy` with a custom `github_proxy_prefix` to retry metadata lookups through a proxy without proxying the actual download URL.
Downloads are scheduled with a small provider-aware cap, so the updater keeps moving without overloading a single host.

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
</details>
