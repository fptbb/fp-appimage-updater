# AI Agent Development Guidelines for fp-appimage-updater

This document provides essential context and instructions for AI agents assisting with the development of `fp-appimage-updater`.

## 1. Project Overview & Architecture

`fp-appimage-updater` is a CLI tool written in Rust designed to manage, check, and update AppImages in a user-space, declarative manner.

### Core Architecture
- **Parser (`src/parser.rs`)**: Loads YAML application recipes and global configuration.
- **State (`src/state.rs`)**: Manages a JSON cache of installed AppImages and their metadata.
- **Resolvers (`src/resolvers/`)**: Strategies to find the latest version of an AppImage:
    - `Forge`: GitHub/GitLab repository releases.
    - `Direct`: Direct URL with versioning via ETag or Last-Modified.
    - `Script`: External script execution for custom resolution.
- **Update Engine (`src/update/engine/`)**: Orchestrates the update process using a worker pool for parallel downloads and heuristics for retry logic.
- **Integrator/Disintegrator (`src/integrator.rs`, `src/disintegrator.rs`)**: Manages desktop entry integration (`.desktop` files), icons, and binary symlinks in `~/.local/bin`.
- **Downloader (`src/downloader/`)**: Handles HTTP downloads with progress reporting using `ureq`.

## 2. Technical Standards & Conventions

### Language & Tooling
- **Rust Edition**: 2024.
- **CLI Parsing**: `pico-args`.
- **Serialization**: `serde` with `serde_yaml` and `serde_json`.
- **HTTP Client**: `ureq` 3.x for core logic (Agent-based). **Avoid `reqwest`** except in tests.
- **Error Handling**: `anyhow::Result` for all high-level operations.

### Naming & Style
- **YAML Fields**: `kebab-case` (e.g., `check-method`).
- **Rust Enums**: `CamelCase` (e.g., `StrategyConfig`).
- **Variables/Functions**: `snake_case`.
- **File Names**: `snake_case.rs`.

### Coding Principles
- **User-Space Only**: Never touch `/usr/bin` or system-wide configurations. Use `~/.local/bin`, `~/.config/fp-appimage-updater/`, and `~/.local/share/applications`.
- **Declarative State**: The truth is the YAML file. If a YAML file is removed, the app should be removed (via `remove` command).
- **Atomic Operations**: Always download to a temporary file (`.part`) and move to the final destination upon success.
- **AppImage Extraction**: To extract metadata from an AppImage, execute the binary with `--appimage-extract` to pull out `.desktop` and icon files. Use `rust-ini` to patch paths in the extracted `.desktop` file.

## 3. Best Practices for Models

### Design Patterns
- **Resolvers**: When adding a new resolution strategy, implement it in `src/resolvers/`, add a variant to `StrategyConfig` in `src/config.rs`, and wire it in `src/resolvers/mod.rs`.
- **Engine**: The update engine is highly parallel. Ensure any shared state is handled via the `UpdateEvent` channel or thread-safe primitives.
- **Desktop Files**: When updating a `.desktop` file, preserve AppImage-specific flags (e.g., `--no-sandbox`) and update the `Exec` and `Icon` keys to absolute paths.

### Performance & Safety
- **Binary Compression**: The project uses `upx` in release builds (via `Justfile`).
- **Memory Safety**: Use standard Rust ownership patterns. Avoid `unsafe` unless strictly necessary for OS-level integration (e.g., in `libc` calls).
- **Rate Limiting**: Respect GitHub/GitLab API rate limits using the `RateLimitInfo` structures provided in the resolvers.

## 4. Testing Workflow

The project uses a sophisticated Docker-based integration testing suite.

- **Tools**: `testcontainers`, `tokio`, `wiremock`.
- **Procedure**:
    1. The binary MUST be built before running tests (`just build-linux-x64`).
    2. Tests spin up a Fedora container, copy the binary into it, and execute it against local mocks or real networks.
    3. Use `tests/common/mod.rs` for container setup utilities.
- **Requirement**: Any new feature or bug fix MUST include a corresponding test case in `tests/`.

## 5. Maintenance Commands

- **Version Bump**: When updating `Cargo.toml`, use the exact commit message: `chore: bump version to X.Y.Z in Cargo.toml`.
- **Build**: Use `just` for building and installation:
    - `just build`: Standard release build.
    - `just manual-install`: Installs the local build for testing.
- **Formatting**: Always run `cargo fmt` before suggesting code.

## 6. Prohibited Actions
- **Do not** introduce system-wide dependencies that aren't available in standard user-space environments.
- **Do not** use `std::process::exit` in library modules; always return `Result`.
- **Do not** add excessive comments for obvious code; focus on the "why" for complex logic.
