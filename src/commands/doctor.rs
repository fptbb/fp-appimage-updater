use crate::config::GlobalConfig;
use crate::doctor;
use crate::output::{
    DoctorCheck, DoctorResponse, DoctorStatus, print_doctor_human, print_json, print_progress,
};
use crate::parser::{AppConfigLoadError, ConfigPaths};
use anyhow::Result;
use ureq::Agent;

pub fn run(
    paths: &ConfigPaths,
    global_config: &GlobalConfig,
    client: &Agent,
    app_count: usize,
    error_count: usize,
    app_config_errors: &[AppConfigLoadError],
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    let checks = doctor::run(paths, global_config, client, app_count, error_count)
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

    if json_output {
        print_json(&DoctorResponse {
            command: "doctor",
            checks,
        })?;
    } else {
        print_doctor_human(&checks, color_output);
        if !app_config_errors.is_empty() {
            print_progress(
                "Tip: run `fp-appimage-updater validate` for detailed parse errors.",
                color_output,
            );
        }
    }
    Ok(())
}
