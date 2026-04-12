use crate::config::GlobalConfig;
use crate::state::AppState;
use anyhow::{Context, Result, anyhow};
use regex_lite::Regex;
use ureq::Agent;

use super::{CheckResult, UpdateInfo, dedupe_capabilities, rate_limit_info_from_headers};

pub use crate::state::ForgePlatform as ForgeHost;

#[derive(Debug, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
}

#[derive(Debug, Clone)]
pub enum AssetMatcher {
    Glob(glob::Pattern),
    Regex { pattern: String, regex: Regex },
}

impl AssetMatcher {
    pub fn from_config(
        asset_match: &str,
        asset_match_regex: Option<&str>,
        repository: &str,
    ) -> Result<Self> {
        if let Some(regex_pattern) = asset_match_regex {
            let anchored = format!("^(?:{})$", regex_pattern);
            let regex = Regex::new(&anchored).with_context(|| {
                format!(
                    "Invalid asset_match_regex pattern '{}' for {}",
                    regex_pattern, repository
                )
            })?;
            return Ok(Self::Regex {
                pattern: regex_pattern.to_string(),
                regex,
            });
        }

        let pattern = glob::Pattern::new(asset_match).with_context(|| {
            format!(
                "Invalid asset_match pattern '{}' for {}",
                asset_match, repository
            )
        })?;

        Ok(Self::Glob(pattern))
    }

    pub fn matches(&self, asset_name: &str) -> bool {
        match self {
            Self::Glob(pattern) => pattern.matches(asset_name),
            Self::Regex { regex, .. } => regex.is_match(asset_name),
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Glob(pattern) => pattern.as_str(),
            Self::Regex { pattern, .. } => pattern,
        }
    }
}

#[derive(Debug, Clone)]
struct ForgeRepoInfo {
    account: String,
    repository: String,
    repo_path: String,
    project_path: String,
}

fn repository_origin(repository: &str) -> Result<String> {
    let (scheme, rest) = repository
        .split_once("://")
        .context("Repository URL did not contain a scheme")?;
    let (host, _) = rest
        .split_once('/')
        .context("Repository URL did not contain a repository path")?;
    Ok(format!("{scheme}://{host}"))
}

fn repository_path(repository: &str) -> Result<&str> {
    let (_, rest) = repository
        .split_once("://")
        .context("Repository URL did not contain a scheme")?;
    let (_, path) = rest
        .split_once('/')
        .context("Repository URL did not contain a repository path")?;
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        Err(anyhow!(
            "Repository URL did not contain an account and repository: {}",
            repository
        ))
    } else {
        Ok(path)
    }
}

fn repository_host(repository: &str) -> Result<&str> {
    let (_, rest) = repository
        .split_once("://")
        .context("Repository URL did not contain a scheme")?;
    let (host, _) = rest
        .split_once('/')
        .context("Repository URL did not contain a repository path")?;
    Ok(host)
}

fn cached_forge_platform(state: Option<&AppState>, repository: &str) -> Option<ForgeHost> {
    state
        .filter(|state| state.forge_repository.as_deref() == Some(repository))
        .and_then(|state| state.forge_platform)
}

pub fn forge_platform_from_swagger_title(title: &str) -> Option<ForgeHost> {
    match title.trim() {
        "Gitea API" => Some(ForgeHost::Gitea),
        "Forgejo API" => Some(ForgeHost::Forgejo),
        "GitHub API" => Some(ForgeHost::GitHub),
        "GitLab API" => Some(ForgeHost::GitLab),
        _ => None,
    }
}

fn detect_forge_platform(client: &Agent, repository: &str) -> Result<ForgeHost> {
    let swagger_url = format!("{}/swagger.v1.json", repository_origin(repository)?);
    let response = client
        .get(&swagger_url)
        .call()
        .with_context(|| format!("Failed to reach forge swagger endpoint for {}", repository))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to detect forge platform for {} from {}: {}",
            repository,
            swagger_url,
            response.status()
        ));
    }

    let resp: serde_json::Value = response.into_body().read_json().with_context(|| {
        format!(
            "Failed to parse forge swagger metadata for {} from {}",
            repository, swagger_url
        )
    })?;
    let title = resp["info"]["title"]
        .as_str()
        .context("Forge swagger response did not contain info.title")?;

    forge_platform_from_swagger_title(title).ok_or_else(|| {
        anyhow!(
            "Unsupported forge API title '{}' returned by {}",
            title,
            swagger_url
        )
    })
}

fn forge_host(client: &Agent, repository: &str, state: Option<&AppState>) -> Result<ForgeHost> {
    if let Some(platform) = cached_forge_platform(state, repository) {
        return Ok(platform);
    }

    match repository_host(repository)? {
        "github.com" => Ok(ForgeHost::GitHub),
        "gitlab.com" => Ok(ForgeHost::GitLab),
        _ => detect_forge_platform(client, repository),
    }
}

pub fn resolve(
    client: &Agent,
    repository: &str,
    asset_match: &str,
    asset_match_regex: Option<&str>,
    state: Option<&AppState>,
    github_proxy: bool,
    github_proxy_prefixes: &[String],
    global_config: &GlobalConfig,
) -> Result<CheckResult> {
    let matcher = AssetMatcher::from_config(asset_match, asset_match_regex, repository)?;
    let host = forge_host(client, repository, state)?;
    let url = release_api_url_with_config(host, repository, global_config)?;

    let response = client
        .get(&url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .with_context(|| {
            format!(
                "Failed to reach {} release API for {}",
                host_name(host),
                repository
            )
        })?;

    let status = response.status().as_u16();
    if status == 403 || status == 429 {
        let Some(rate_limit) = rate_limit_info_from_headers(response.headers()) else {
            return Err(anyhow!(
                "Rate limited by {} but no rate-limit headers were returned",
                repository
            ));
        };
        if host == ForgeHost::GitHub && github_proxy {
            return resolve_via_github_proxy(
                client,
                repository,
                &matcher,
                state,
                github_proxy_prefixes,
                global_config,
            )
            .map_err(|e| {
                anyhow::Error::from(rate_limit.clone()).context(format!(
                    "{} {}",
                    rate_limit.short_message(),
                    e
                ))
            });
        }

        if host == ForgeHost::GitHub {
            let html_url = github_release_page_url(repository)?;
            let html = client
                .get(&html_url)
                .call()
                .with_context(|| {
                    format!(
                        "{} GitHub release page fallback failed for {}",
                        rate_limit.short_message(),
                        repository
                    )
                })?
                .into_body()
                .read_to_string()
                .with_context(|| {
                    format!("Failed to read GitHub release page for {}", repository)
                })?;

            return resolve_from_github_release_page(
                client,
                repository,
                &matcher,
                &html,
                state,
                global_config,
            )
            .map_err(|e| {
                anyhow::Error::from(rate_limit.clone()).context(format!(
                    "{} {}",
                    rate_limit.short_message(),
                    e
                ))
            });
        }

        return Err(anyhow::Error::from(rate_limit).context(format!(
            "{} release API rate limited for {}",
            host_name(host),
            repository
        )));
    }

    if !response.status().is_success() {
        return Err(anyhow!(
            "{} release API returned {} for {}",
            host_name(host),
            response.status(),
            repository
        ));
    }

    let resp: serde_json::Value = response.into_body().read_json().with_context(|| {
        format!(
            "Failed to parse {} release metadata for {}",
            host_name(host),
            repository
        )
    })?;
    let version = resp["tag_name"].as_str().with_context(|| {
        format!(
            "Release metadata for {} did not contain tag_name",
            repository
        )
    })?;
    let assets = release_assets(host, &resp, repository)?;

    resolve_from_release_assets(
        client,
        repository,
        &matcher,
        version,
        &assets,
        state,
        host,
        github_proxy,
        github_proxy_prefixes,
        global_config,
    )
}

fn host_name(host: ForgeHost) -> &'static str {
    match host {
        ForgeHost::GitHub => "GitHub",
        ForgeHost::GitLab => "GitLab",
        ForgeHost::Gitea => "Gitea",
        ForgeHost::Forgejo => "Forgejo",
    }
}

fn forge_repo_path(repository: &str, _host: ForgeHost) -> Result<&str> {
    repository_path(repository)
}

fn forge_repo_info(repository: &str, host: ForgeHost) -> Result<ForgeRepoInfo> {
    let repo_path = forge_repo_path(repository, host)?;
    let mut parts = repo_path.split('/');
    let account = parts
        .next()
        .context("Repository URL did not contain an account segment")?
        .to_string();
    let repository = parts
        .next_back()
        .context("Repository URL did not contain a repository segment")?
        .to_string();

    Ok(ForgeRepoInfo {
        account,
        repository,
        repo_path: repo_path.to_string(),
        project_path: repo_path.replace('/', "%2F"),
    })
}

fn encoded_gitlab_project_path(repository: &str) -> Result<String> {
    Ok(repository_path(repository)?.replace('/', "%2F"))
}

pub fn release_api_url(host: ForgeHost, repository: &str) -> Result<String> {
    match host {
        ForgeHost::GitHub => Ok(repository
            .replace("https://github.com/", "https://api.github.com/repos/")
            + "/releases/latest"),
        ForgeHost::GitLab => Ok(format!(
            "https://gitlab.com/api/v4/projects/{}/releases/permalink/latest",
            encoded_gitlab_project_path(repository)?
        )),
        ForgeHost::Gitea | ForgeHost::Forgejo => Ok(format!(
            "{}/api/v1/repos/{}/releases/latest",
            repository_origin(repository)?,
            repository_path(repository)?
        )),
    }
}

fn render_forge_template(template: &str, repo: &ForgeRepoInfo) -> String {
    template
        .replace("{account}", &repo.account)
        .replace("{repository}", &repo.repository)
        .replace("{repo_path}", &repo.repo_path)
        .replace("{project_path}", &repo.project_path)
}

fn release_api_url_with_template(
    host: ForgeHost,
    repository: &str,
    template: Option<&str>,
) -> Result<String> {
    if let Some(template) = template {
        let repo = forge_repo_info(repository, host)?;
        return Ok(render_forge_template(template, &repo));
    }

    release_api_url(host, repository)
}

pub fn release_api_url_with_config(
    host: ForgeHost,
    repository: &str,
    global_config: &GlobalConfig,
) -> Result<String> {
    match host {
        ForgeHost::GitHub => release_api_url_with_template(
            host,
            repository,
            global_config.github_release_api_url.as_deref(),
        ),
        ForgeHost::GitLab => release_api_url_with_template(
            host,
            repository,
            global_config.gitlab_release_api_url.as_deref(),
        ),
        ForgeHost::Gitea | ForgeHost::Forgejo => release_api_url(host, repository),
    }
}

fn github_release_web_url(repository: &str) -> Result<String> {
    if repository.starts_with("https://github.com/") {
        Ok(repository.to_string())
    } else {
        Err(anyhow!(
            "Only github.com is currently supported for the repository base URL, got {}",
            repository
        ))
    }
}

fn github_release_web_url_with_template(
    repository: &str,
    template: Option<&str>,
) -> Result<String> {
    if let Some(template) = template {
        let repo = forge_repo_info(repository, ForgeHost::GitHub)?;
        return Ok(render_forge_template(template, &repo));
    }

    github_release_web_url(repository)
}

pub fn github_release_web_url_with_config(
    repository: &str,
    global_config: &GlobalConfig,
) -> Result<String> {
    github_release_web_url_with_template(
        repository,
        global_config.github_release_web_url.as_deref(),
    )
}

fn github_release_page_url(repository: &str) -> Result<String> {
    if repository.starts_with("https://github.com/") {
        Ok(format!("{}/releases/latest", repository))
    } else {
        Err(anyhow!(
            "Only github.com is currently supported for the release page fallback, got {}",
            repository
        ))
    }
}

pub fn github_proxy_release_url(repository: &str, github_proxy_prefix: &str) -> Result<String> {
    Ok(format!(
        "{}{}",
        github_proxy_prefix,
        release_api_url(ForgeHost::GitHub, repository)?
    ))
}

fn github_proxy_release_url_with_config(
    repository: &str,
    github_proxy_prefix: &str,
    global_config: &GlobalConfig,
) -> Result<String> {
    Ok(format!(
        "{}{}",
        github_proxy_prefix,
        release_api_url_with_config(ForgeHost::GitHub, repository, global_config)?
    ))
}

pub fn sanitize_github_proxy_url(url: &str, github_proxy_prefix: &str) -> String {
    url.strip_prefix(github_proxy_prefix)
        .unwrap_or(url)
        .to_string()
}

pub fn validate_github_proxy_metadata(
    repository: &str,
    resp: &serde_json::Value,
    github_proxy_prefix: &str,
) -> Result<()> {
    let repo_path = forge_repo_path(repository, ForgeHost::GitHub)?;
    let expected_api_prefix = format!("https://api.github.com/repos/{}/releases/", repo_path);
    let expected_web_prefix = format!("https://github.com/{}/", repo_path);

    if let Some(api_url) = resp["url"].as_str() {
        let api_url = sanitize_github_proxy_url(api_url, github_proxy_prefix);
        if !api_url.starts_with(&expected_api_prefix) {
            return Err(anyhow!(
                "GitHub proxy returned metadata for a different repository: {}",
                api_url
            ));
        }
    }

    if let Some(html_url) = resp["html_url"].as_str() {
        let html_url = sanitize_github_proxy_url(html_url, github_proxy_prefix);
        if !html_url.starts_with(&expected_web_prefix) {
            return Err(anyhow!(
                "GitHub proxy returned metadata for a different repository: {}",
                html_url
            ));
        }
    }

    Ok(())
}

fn validate_github_proxy_metadata_with_config(
    repository: &str,
    resp: &serde_json::Value,
    github_proxy_prefix: &str,
    global_config: &GlobalConfig,
) -> Result<()> {
    let api_url = release_api_url_with_config(ForgeHost::GitHub, repository, global_config)?;
    let expected_api_prefix = api_url
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{}/", prefix))
        .unwrap_or_else(|| api_url.clone());
    let web_url = github_release_web_url_with_config(repository, global_config)?;
    let expected_web_prefix = format!("{}/", web_url.trim_end_matches('/'));

    if let Some(api_url) = resp["url"].as_str() {
        let api_url = sanitize_github_proxy_url(api_url, github_proxy_prefix);
        if !api_url.starts_with(&expected_api_prefix) {
            return Err(anyhow!(
                "GitHub proxy returned metadata for a different repository: {}",
                api_url
            ));
        }
    }

    if let Some(html_url) = resp["html_url"].as_str() {
        let html_url = sanitize_github_proxy_url(html_url, github_proxy_prefix);
        if !html_url.starts_with(&expected_web_prefix) {
            return Err(anyhow!(
                "GitHub proxy returned metadata for a different repository: {}",
                html_url
            ));
        }
    }

    Ok(())
}

fn validate_release_download_url(
    host: ForgeHost,
    release_web_url: &str,
    version: &str,
    download_url: &str,
) -> Result<()> {
    let expected = match host {
        ForgeHost::GitHub | ForgeHost::Gitea | ForgeHost::Forgejo => format!(
            "{}/releases/download/",
            release_web_url.trim_end_matches('/')
        ),
        ForgeHost::GitLab => format!(
            "{}/-/releases/{}/downloads/",
            release_web_url.trim_end_matches('/'),
            version
        ),
    };

    if download_url.starts_with(&expected) {
        Ok(())
    } else {
        Err(anyhow!(
            "{} returned a download URL for a different repository: {}",
            host_name(host),
            download_url
        ))
    }
}

pub fn release_assets(
    host: ForgeHost,
    resp: &serde_json::Value,
    repository: &str,
) -> Result<Vec<ReleaseAsset>> {
    match host {
        ForgeHost::GitHub | ForgeHost::Gitea | ForgeHost::Forgejo => {
            let assets = resp["assets"].as_array().with_context(|| {
                format!(
                    "Release metadata for {} did not contain an assets array",
                    repository
                )
            })?;

            Ok(assets
                .iter()
                .filter_map(|asset| {
                    Some(ReleaseAsset {
                        name: asset["name"].as_str()?.to_string(),
                        download_url: asset["browser_download_url"].as_str()?.to_string(),
                    })
                })
                .collect())
        }
        ForgeHost::GitLab => {
            let links = resp["assets"]["links"].as_array().with_context(|| {
                format!(
                    "Release metadata for {} did not contain an assets.links array",
                    repository
                )
            })?;

            Ok(links
                .iter()
                .filter_map(|asset| {
                    let name = asset["name"].as_str()?.to_string();
                    let download_url = asset["direct_asset_url"]
                        .as_str()
                        .or_else(|| asset["url"].as_str())?
                        .to_string();
                    Some(ReleaseAsset { name, download_url })
                })
                .collect())
        }
    }
}

fn resolve_from_release_assets(
    client: &Agent,
    repository: &str,
    matcher: &AssetMatcher,
    version: &str,
    assets: &[ReleaseAsset],
    state: Option<&AppState>,
    host: ForgeHost,
    github_proxy: bool,
    github_proxy_prefixes: &[String],
    global_config: &GlobalConfig,
) -> Result<CheckResult> {
    let release_web_url = match host {
        ForgeHost::GitHub => github_release_web_url_with_config(repository, global_config)?,
        ForgeHost::GitLab => {
            if let Some(template) = global_config.gitlab_release_web_url.as_deref() {
                let repo = forge_repo_info(repository, host)?;
                render_forge_template(template, &repo)
            } else {
                format!("https://gitlab.com/{}", forge_repo_path(repository, host)?)
            }
        }
        ForgeHost::Gitea | ForgeHost::Forgejo => repository.trim_end_matches('/').to_string(),
    };
    for asset in assets {
        if matcher.matches(&asset.name) {
            let download_url = if host == ForgeHost::GitHub && github_proxy {
                let Some(github_proxy_prefix) = github_proxy_prefixes.first() else {
                    return Err(anyhow!(
                        "GitHub proxy is enabled for {} but no proxy prefixes were configured",
                        repository
                    ));
                };
                let sanitized = sanitize_github_proxy_url(&asset.download_url, github_proxy_prefix);
                validate_release_download_url(host, &release_web_url, version, &sanitized)?;
                sanitized
            } else {
                validate_release_download_url(
                    host,
                    &release_web_url,
                    version,
                    &asset.download_url,
                )?;
                asset.download_url.clone()
            };
            return build_check_result(
                client,
                repository,
                version.to_string(),
                download_url,
                state,
                Some(repository.to_string()),
                Some(host),
            );
        }
    }

    let available_assets = assets
        .iter()
        .map(|asset| asset.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    Err(anyhow!(
        "No matching asset found for repository {} with pattern '{}'. Available assets: {}",
        repository,
        matcher.description(),
        if available_assets.is_empty() {
            "<none>".to_string()
        } else {
            available_assets
        }
    ))
}

fn resolve_via_github_proxy(
    client: &Agent,
    repository: &str,
    matcher: &AssetMatcher,
    state: Option<&AppState>,
    github_proxy_prefixes: &[String],
    global_config: &GlobalConfig,
) -> Result<CheckResult> {
    let mut last_error = None;
    let mut last_rate_limit = None;

    for github_proxy_prefix in github_proxy_prefixes {
        match resolve_via_single_github_proxy(
            client,
            repository,
            matcher,
            state,
            github_proxy_prefix,
            global_config,
        ) {
            Ok(result) => return Ok(result),
            Err(err) => {
                if err.downcast_ref::<super::RateLimitInfo>().is_some() {
                    last_rate_limit = Some(err.context(format!(
                        "GitHub proxy {} rate limited for {}",
                        github_proxy_prefix, repository
                    )));
                } else {
                    last_error = Some(err.context(format!(
                        "GitHub proxy {} failed for {}",
                        github_proxy_prefix, repository
                    )));
                }
            }
        }
    }

    Err(last_rate_limit.or(last_error).unwrap_or_else(|| {
        anyhow!(
            "GitHub proxy is enabled for {} but no proxy prefixes were configured",
            repository
        )
    }))
}

fn resolve_via_single_github_proxy(
    client: &Agent,
    repository: &str,
    matcher: &AssetMatcher,
    state: Option<&AppState>,
    github_proxy_prefix: &str,
    global_config: &GlobalConfig,
) -> Result<CheckResult> {
    let proxy_url =
        github_proxy_release_url_with_config(repository, github_proxy_prefix, global_config)?;
    let response = client
        .get(&proxy_url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .with_context(|| format!("Failed to reach GitHub proxy for {}", repository))?;

    if !response.status().is_success() {
        if let Some(rate_limit) = rate_limit_info_from_headers(response.headers()) {
            return Err(anyhow::Error::from(rate_limit).context(format!(
                "GitHub proxy returned {} for {}",
                response.status(),
                repository
            )));
        }
        return Err(anyhow!(
            "GitHub proxy returned {} for {}",
            response.status(),
            repository
        ));
    }

    let resp: serde_json::Value = response.into_body().read_json().with_context(|| {
        format!(
            "Failed to parse proxied release metadata for {}",
            repository
        )
    })?;
    validate_github_proxy_metadata_with_config(
        repository,
        &resp,
        github_proxy_prefix,
        global_config,
    )?;
    let version = resp["tag_name"].as_str().with_context(|| {
        format!(
            "Release metadata for {} did not contain tag_name",
            repository
        )
    })?;
    let assets = release_assets(ForgeHost::GitHub, &resp, repository)?;

    resolve_from_release_assets(
        client,
        repository,
        matcher,
        version,
        &assets,
        state,
        ForgeHost::GitHub,
        true,
        &[github_proxy_prefix.to_string()],
        global_config,
    )
}

fn resolve_from_github_release_page(
    client: &Agent,
    repository: &str,
    matcher: &AssetMatcher,
    html: &str,
    state: Option<&AppState>,
    global_config: &GlobalConfig,
) -> Result<CheckResult> {
    let release_web_url = github_release_web_url_with_config(repository, global_config)?;
    let Some((version, download_url)) =
        find_release_asset_in_html_with_base_and_matcher(html, &release_web_url, matcher)
    else {
        return Err(anyhow!(
            "Rate limited for {} and no matching asset was found on the release page for pattern '{}'",
            repository,
            matcher.description()
        ));
    };

    build_check_result(
        client,
        repository,
        version,
        download_url,
        state,
        Some(repository.to_string()),
        Some(ForgeHost::GitHub),
    )
}

fn build_check_result(
    client: &Agent,
    repository: &str,
    version: String,
    download_url: String,
    state: Option<&AppState>,
    forge_repository: Option<String>,
    forge_platform: Option<ForgeHost>,
) -> Result<CheckResult> {
    let mut capabilities = Vec::new();
    let mut segmented_downloads = Some(false);

    if let Ok(head_resp) = client.head(&download_url).call()
        && let Some(range_header) = head_resp
            .headers()
            .get("Accept-Ranges")
            .and_then(|value| value.to_str().ok())
        && range_header.trim().eq_ignore_ascii_case("bytes")
    {
        capabilities.push("segmented_downloads".to_string());
        segmented_downloads = Some(true);
    }

    dedupe_capabilities(&mut capabilities);

    if version.is_empty() {
        return Err(anyhow!(
            "Release metadata for {} did not contain a version",
            repository
        ));
    }

    let update = if state.and_then(|s| s.local_version.as_deref()) == Some(version.as_str()) {
        None
    } else {
        Some(UpdateInfo {
            download_url,
            version,
            new_etag: None,
            new_last_modified: None,
        })
    };

    Ok(CheckResult {
        update,
        capabilities,
        segmented_downloads,
        forge_repository,
        forge_platform,
    })
}

pub fn find_release_asset_in_html(
    html: &str,
    repo_path: &str,
    pattern: &glob::Pattern,
) -> Option<(String, String)> {
    let matcher = AssetMatcher::Glob(pattern.clone());
    find_release_asset_in_html_with_matcher(html, repo_path, &matcher)
}

pub fn find_release_asset_in_html_with_matcher(
    html: &str,
    repo_path: &str,
    matcher: &AssetMatcher,
) -> Option<(String, String)> {
    let release_web_url = format!("https://github.com/{}", repo_path.trim_start_matches('/'));
    find_release_asset_in_html_with_base_and_matcher(html, &release_web_url, matcher)
}

pub fn find_release_asset_in_html_with_base(
    html: &str,
    release_web_url: &str,
    pattern: &glob::Pattern,
) -> Option<(String, String)> {
    let matcher = AssetMatcher::Glob(pattern.clone());
    find_release_asset_in_html_with_base_and_matcher(html, release_web_url, &matcher)
}

pub fn find_release_asset_in_html_with_base_and_matcher(
    html: &str,
    release_web_url: &str,
    matcher: &AssetMatcher,
) -> Option<(String, String)> {
    let absolute_needle = format!(
        "{}/releases/download/",
        release_web_url.trim_end_matches('/')
    );
    let relative_base = release_web_url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/').map(|(_, path)| format!("/{}", path)))
        .unwrap_or_else(|| release_web_url.trim_end_matches('/').to_string());
    let relative_needle = format!("{}/releases/download/", relative_base.trim_end_matches('/'));

    for needle in [absolute_needle, relative_needle] {
        let mut search_start = 0usize;

        while let Some(relative_idx) = html[search_start..].find(&needle) {
            let start = search_start + relative_idx;
            let after = &html[start + needle.len()..];
            let tag_end = after.find('/')?;
            let version = &after[..tag_end];
            let after_tag = &after[tag_end + 1..];
            let file_end = after_tag
                .find(|c: char| c == '"' || c == '\'' || c == '?' || c == '<' || c.is_whitespace())
                .unwrap_or(after_tag.len());
            let asset_name = &after_tag[..file_end];

            if matcher.matches(asset_name) {
                let download_url = format!(
                    "{}/releases/download/{}/{}",
                    release_web_url.trim_end_matches('/'),
                    version,
                    asset_name
                );
                return Some((version.to_string(), download_url));
            }

            search_start = start + needle.len();
        }
    }

    None
}
