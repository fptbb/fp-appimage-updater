use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::InitStrategy;
use crate::config::GlobalConfig;
use crate::parser::ConfigPaths;

pub struct InitResult {
    pub created: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

pub fn run(
    paths: &ConfigPaths,
    create_global: bool,
    app_name: Option<&str>,
    strategy: InitStrategy,
    force: bool,
) -> Result<InitResult> {
    let should_create_global = create_global || app_name.is_none();
    let mut created = Vec::new();
    let mut skipped = Vec::new();

    if should_create_global {
        let global_path = paths.global_config_path();
        let content = serde_yaml::to_string(&GlobalConfig::default())?;
        write_file(&global_path, &content, force, &mut created, &mut skipped)?;
    }

    if let Some(app_name) = app_name {
        let app_path = paths.apps_dir().join(format!("{app_name}.yml"));
        let content = app_template(app_name, strategy);
        write_file(&app_path, &content, force, &mut created, &mut skipped)?;

        if matches!(strategy, InitStrategy::Script) {
            let script_dir = paths.apps_dir().join(app_name);
            let script_path = script_dir.join("resolver.sh");
            let script_content = script_template();
            write_file(
                &script_path,
                script_content,
                force,
                &mut created,
                &mut skipped,
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(0o755);
                let _ = fs::set_permissions(script_path, perms);
            }
        }
    }

    Ok(InitResult { created, skipped })
}

fn write_file(
    path: &Path,
    content: &str,
    force: bool,
    created: &mut Vec<PathBuf>,
    skipped: &mut Vec<PathBuf>,
) -> Result<()> {
    if path.exists() && !force {
        skipped.push(path.to_path_buf());
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid path: {}", path.display()))?;
    fs::create_dir_all(parent)?;

    if path.exists() && force && path.is_dir() {
        bail!("Cannot overwrite directory with file: {}", path.display());
    }

    fs::write(path, content)?;
    created.push(path.to_path_buf());
    Ok(())
}

fn app_template(name: &str, strategy: InitStrategy) -> String {
    match strategy {
        InitStrategy::Direct => format!(
            "name: {name}\nstrategy:\n  strategy: direct\n  url: \"https://example.org/{name}.AppImage\"\n  check_method: etag\n"
        ),
        InitStrategy::Forge => format!(
            "name: {name}\nstrategy:\n  strategy: forge\n  repository: \"https://github.com/OWNER/REPO\"\n  asset_match: \"*-x86_64.AppImage\"\n"
        ),
        InitStrategy::Script => format!(
            "name: {name}\nstrategy:\n  strategy: script\n  script_path: ./{name}/resolver.sh\n"
        ),
    }
}

fn script_template() -> &'static str {
    "#!/usr/bin/env bash\nset -euo pipefail\n\necho \"https://example.org/my-app.AppImage\"\necho \"1.0.0\"\n"
}
