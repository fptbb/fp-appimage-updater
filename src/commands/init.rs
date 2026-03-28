use crate::cli::InitStrategy;
use crate::initializer;
use crate::output::{print_json, print_progress, print_success, print_warning};
use crate::parser::ConfigPaths;
use anyhow::Result;
use serde::Serialize;

pub fn run(
    paths: &ConfigPaths,
    global: bool,
    app: Option<&str>,
    strategy: InitStrategy,
    force: bool,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    let result = initializer::run(paths, global, app, strategy, force)?;

    if json_output {
        #[derive(Serialize)]
        struct InitResponse {
            command: &'static str,
            created: Vec<String>,
            skipped: Vec<String>,
        }
        print_json(&InitResponse {
            command: "init",
            created: result
                .created
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            skipped: result
                .skipped
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
        })?;
    } else {
        for path in &result.created {
            print_success(&format!("Created {}", path.display()), color_output);
            print_progress(&format!("Edit: {}", path.display()), color_output);
            print_progress("Then run: fp-appimage-updater validate", color_output);
        }
        for path in &result.skipped {
            print_warning(
                &format!(
                    "Skipped existing file {} (use --force to overwrite)",
                    path.display()
                ),
                color_output,
            );
        }
        if result.created.is_empty() && result.skipped.is_empty() {
            print_progress("Nothing to initialize.", color_output);
        }
    }
    Ok(())
}
