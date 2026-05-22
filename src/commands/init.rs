use crate::cli::InitStrategy;
use crate::commands::helpers::ExecutionContext;
use crate::initializer;
use crate::output::{print_json, print_progress, print_success, print_warning};
use anyhow::Result;
use serde::Serialize;

pub fn run(
    ctx: &ExecutionContext,
    global: bool,
    app: Option<&str>,
    strategy: InitStrategy,
    force: bool,
) -> Result<()> {
    let result = initializer::run(ctx.paths, global, app, strategy, force)?;

    if ctx.json_output {
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
            print_success(&format!("Created {}", path.display()), ctx.color_output);
            print_progress(&format!("Edit: {}", path.display()), ctx.color_output);
            print_progress(
                &format!("Then run: {} validate", crate::cli::current_bin_name()),
                ctx.color_output,
            );
        }
        for path in &result.skipped {
            print_warning(
                &format!(
                    "Skipped existing file {} (use --force to overwrite)",
                    path.display()
                ),
                ctx.color_output,
            );
        }
        if result.created.is_empty() && result.skipped.is_empty() {
            print_progress("Nothing to initialize.", ctx.color_output);
        }
    }
    Ok(())
}
