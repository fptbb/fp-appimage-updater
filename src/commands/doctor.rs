use crate::commands::helpers::ExecutionContext;
use crate::doctor;
use crate::output::{
    DoctorCheck, DoctorResponse, DoctorStatus, print_doctor_human, print_json, print_progress,
};
use anyhow::Result;

pub fn run(ctx: &ExecutionContext) -> Result<()> {
    let checks = doctor::run(ctx.paths, ctx.global_config, ctx.client)
        .into_iter()
        .map(|check| DoctorCheck {
            name: check.name,
            status: match check.status {
                doctor::DoctorStatus::Ok => DoctorStatus::Ok,
                doctor::DoctorStatus::Warn => DoctorStatus::Warn,
            },
            detail: check.detail,
        })
        .collect::<Vec<_>>();

    if ctx.json_output {
        print_json(&DoctorResponse {
            command: "doctor",
            checks,
        })?;
    } else {
        print_doctor_human(&checks, ctx.color_output);
        if !ctx.app_config_errors.is_empty() {
            print_progress(
                &format!(
                    "Tip: run `{} validate` for detailed parse errors.",
                    crate::cli::current_bin_name()
                ),
                ctx.color_output,
            );
        }
    }
    Ok(())
}
