# AI Agent Development Guidelines

This file is intended for AI agents assisting in the development of `fp-appimage-updater`. Read this before making large sweeping changes.

## Core Design Principles

1.  **Strictly User-Space & File-Driven**: State is driven purely by the file system (no databases). App configurations live in `~/.config/fp-appimage-updater/apps/`. Desktop integrations go to `~/.local/share/applications` and `~/.local/share/icons`. Symlinks go to `~/.local/bin`.
2.  **Declarative YAML**: Apps are managed by creating/deleting YAML files, *not* via complex CLI wizard commands.
3.  **AppImage Metadata Integrity**: 
    -   Do not hardcode generic `.desktop` properties. 
    -   Use `std::process::Command` to invoke the AppImage itself with `--appimage-extract "*.desktop" "*.png" "*.svg"`.
    -   Use `rust-ini` to edit the extracted `TryExec`, `Exec`, and `Icon` absolute paths while preserving AppImage-specific arguments (like `--no-sandbox %U`).
4.  **No Unbounded Streaming Timeouts**: When using `reqwest` for large AppImage downloads, use `.connect_timeout()` rather than `.timeout()` on the `ClientBuilder`, to prevent large binary stream truncations on slow connections.
5.  **Build System**: The project uses `cargo` internally but exposes cross-platform build targets and `upx` binary compression via a `Justfile`. Always use the `Justfile` tasks (`just build-linux-x64`, `just manual-install`) to manage builds and testing environments, and update the `Justfile` if adding new target architectures.
6.  **Config Traits & YAML Syntax**: Make sure to always map YAML fields correctly to their serde properties. For example, `check_method` in the YAML is mapped to `check-method` via the `#[serde(rename_all = "kebab-case")]` or snake_case respectively. Actually, the `CheckMethod` Enum options use kebab-case (`etag`, `last-modified`), but the fields in struct use snake case like `check_method`. Always verify serde representations when parsing.

## Modular Resolution Logic
When adding a new way to check for AppImage updates:
-   Add a new file under `src/resolvers/`.
-   Implement an async `resolve` function returning `Result<Option<UpdateInfo>>`.
-   Wire it into the `StrategyConfig` enum in `src/config.rs`.
-   Dispatch it in `src/resolvers/mod.rs -> check_for_updates()`.

## Versioning policy
When version changes in the Cargo.toml, the commit message should be always exactly chore: bump version to VERSION_NUMBER in Cargo.toml just replacing the version number.