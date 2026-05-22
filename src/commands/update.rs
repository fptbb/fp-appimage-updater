use crate::commands::helpers::ExecutionContext;
use anyhow::Result;

pub fn run(
    ctx: &mut ExecutionContext,
    app_name: Option<&str>,
    show_all: bool,
    debug_download_url: Option<&str>,
    debug_version: Option<&str>,
) -> Result<()> {
    let forced_update = match (debug_download_url, debug_version) {
        (Some(download_url), Some(version)) => {
            if app_name.is_none() {
                anyhow::bail!("debug downgrade requires specifying an app name");
            }
            Some(crate::update::ForcedUpdateInfo {
                download_url: download_url.to_string(),
                version: version.to_string(),
            })
        }
        (None, None) => None,
        _ => {
            anyhow::bail!("debug downgrade requires both --debug-download-url and --debug-version")
        }
    };

    crate::update::run(ctx, app_name, show_all, forced_update)
}
