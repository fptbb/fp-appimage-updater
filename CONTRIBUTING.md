# Contributing to fp-appimage-updater

Thank you for your interest in contributing to `fp-appimage-updater`! This document provides guidelines for setting up your environment and contributing to the project.

## Development Environment setup

This project is written in Rust. You will need:
- `rustc` and `cargo` installed (preferably via `rustup`).
- `just` (optional, but recommended) for running build commands defined in the `Justfile`.
- `upx` (optional) if you want to test binary compression locally.

### Building
The project exclusively uses `just` to orchestrate builds, cross-compilation, compressing, and deployment.

To list all available commands:
```bash
just --list
```

To compile natively and build the application:
```bash
just build-linux-x64
```

To install the binary and active background systemd timers to your `~/.local` user-space:
```bash
just manual-install
```

To clean your work directory:
```bash
just clean
```

## Project Architecture Overview

The codebase is split into specific single-responsibility modules under `src/`:
- **`config.rs`**: Defines the data models for the global configuration and per-app YAML configurations (`serde`).
- **`parser.rs`**: Handles parsing YAML files out of the XDG standard directories (`~/.config/fp-appimage-updater/`).
- **`state.rs`**: Manages the local JSON cache tracking versions, ETags, Last-Modified, and file paths.
- **`resolvers/`**: Contains the logic to check for updates without downloading. Divided into:
  - `forge.rs`: For GitHub/GitLab releases API.
  - `direct.rs`: For HTTP HEAD checks (ETag/Last-Modified).
  - `script.rs`: For invoking external shell scripts returning URLs.
- **`downloader.rs`**: Implements async downloading using `reqwest` and wraps `zsync` for binary diffs.
- **`integrator.rs`**: Manages the AppImage extraction logic (`--appimage-extract`), `.desktop` parsing and rewriting, icon moving, and `chmod`/symlinking.
- **`disintegrator.rs`**: Handles the clean uninstallation of apps and removal of their desktop footprints.
- **`self_updater.rs`**: Checks the GitHub releases API and replaces the running binary in-place when a newer version is available.
- **`cli.rs` & `main.rs`**: Implements the `clap` CLI surface and ties the modules together.

## Pull Request Guidelines
1. Fork the repository and create your feature branch: `git checkout -b my-new-feature`
2. Ensure your code complies and matches formatting checks (ideally running `cargo check` or `cargo clippy`).
3. Build the local binary with `just build-linux-x64` and smoke-test it against a real config: `./build/fp-appimage-updater.x64 check` / `./build/fp-appimage-updater.x64 update`, also, you can give it a go on the automated tests using `just test`, it requires docker to be installed and uses testcontainers.
4. Commit your changes logically.
5. Push to the branch and submit a Pull Request.

## Release Process

All releases are published from the GitHub Actions **Build & Release** workflow, triggered manually via `workflow_dispatch`.

### Pre-release

1. Set `version` in `Cargo.toml` to the version you are working toward (e.g. `1.1.0`).
2. Commit and push.
3. Go to **Actions → Build & Release → Run workflow**, check *Publish as pre-release*.

The workflow refuses to create an RC if the stable tag for that version already exists.
When testing is complete, run a stable release with the same version number.

### Installing a pre-release

```bash
# Via install script
curl -sL https://fau.fpt.icu/install.sh | bash -s -- --pre-release

# Via the self-update command
fp-appimage-updater self-update --pre-release
```

With `--pre-release`, it resolves the **most recently published** release (stable or RC).

### Stable Release

1. Bump `version` in `Cargo.toml`.
2. Commit and push to `main`.
3. Go to **Actions → Build & Release → Run workflow**

The workflow fails immediately if the version tag already exists - bump the version and retry.