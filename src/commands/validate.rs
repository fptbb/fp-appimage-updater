use crate::commands::helpers::ExecutionContext;
use crate::output::{
    ValidateApp, ValidateResponse, ValidateStatus, print_json, print_validate_human,
};
use crate::validator;
use anyhow::Result;

pub fn run(ctx: &ExecutionContext, app_name: Option<&str>) -> Result<()> {
    let (apps, error) = validator::validate_app_configs(ctx.paths, app_name)?;
    let results = apps
        .into_iter()
        .map(|app| ValidateApp {
            name: app.app_name,
            file: app.file,
            status: match app.status {
                validator::ValidationStatus::Valid => ValidateStatus::Valid,
                validator::ValidationStatus::Invalid => ValidateStatus::Invalid,
            },
            error: app.error,
        })
        .collect::<Vec<_>>();

    if ctx.json_output {
        print_json(&ValidateResponse {
            command: "validate",
            apps: results,
            error,
        })?;
    } else {
        print_validate_human(&results, error.as_deref(), ctx.color_output);
    }
    Ok(())
}
