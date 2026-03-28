use super::{CheckResult, UpdateInfo, capability_from_header_value, dedupe_capabilities};
use crate::config::CheckMethod;
use crate::state::AppState;
use anyhow::{Result, anyhow};
use ureq::Agent;

pub fn resolve(
    client: &Agent,
    url: &str,
    check_method: &CheckMethod,
    state: Option<&AppState>,
) -> Result<CheckResult> {
    let resp = client.head(url).call()?;
    let mut capabilities = Vec::new();

    if let Some(capability) = capability_from_header_value(
        "segmented_downloads",
        resp.headers()
            .get("Accept-Ranges")
            .and_then(|value| value.to_str().ok()),
    ) {
        capabilities.push(capability);
    }

    let (mut new_etag, mut new_last_modified) = (None, None);
    let mut is_new = false;

    match check_method {
        CheckMethod::Etag => {
            if let Some(etag) = resp.headers().get("ETag") {
                let etag_str = etag.to_str().unwrap_or("").trim_matches('"').to_string();
                new_etag = Some(etag_str.clone());
                if state.and_then(|s| s.etag.as_ref()) != Some(&etag_str) {
                    is_new = true;
                }
                capabilities.push("etag".to_string());
            } else {
                return Err(anyhow!(
                    "ETag check requested but server did not return ETag"
                ));
            }
        }
        CheckMethod::LastModified => {
            if let Some(lm) = resp.headers().get("Last-Modified") {
                let lm_str = lm.to_str().unwrap_or("").to_string();
                new_last_modified = Some(lm_str.clone());
                if state.and_then(|s| s.last_modified.as_ref()) != Some(&lm_str) {
                    is_new = true;
                }
                capabilities.push("last_modified".to_string());
            } else {
                return Err(anyhow!("Last-Modified check requested but missing"));
            }
        }
    }

    if is_new || state.is_none() || state.unwrap().local_version.is_none() {
        let pseudo_version = match check_method {
            CheckMethod::Etag => new_etag.clone().unwrap(),
            CheckMethod::LastModified => new_last_modified.clone().unwrap(),
        };

        dedupe_capabilities(&mut capabilities);

        Ok(CheckResult {
            update: Some(UpdateInfo {
                download_url: url.to_string(),
                version: pseudo_version,
                new_etag,
                new_last_modified,
            }),
            capabilities,
        })
    } else {
        dedupe_capabilities(&mut capabilities);

        Ok(CheckResult {
            update: None,
            capabilities,
        })
    }
}
