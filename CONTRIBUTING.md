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
- **`cli.rs` & `main.rs`**: Implements the `clap` CLI surface and ties the modules together.

## Pull Request Guidelines
1. Fork the repository and create your feature branch: `git checkout -b my-new-feature`
2. Ensure your code complies and matches formatting checks (ideally running `cargo check` or `cargo clippy`).
3. Compile the local test binary using `just build-linux-x64` before invoking your local test binary against a live AppImage update via `./build/fp-appimage-updater.x64 update`.
4. Commit your changes logically.
5. Push to the branch and submit a Pull Request.
