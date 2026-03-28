use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub command: &'static str,
    pub apps: Vec<ListApp>,
}

#[derive(Debug, Serialize)]
pub struct ListApp {
    pub name: String,
    pub strategy: String,
    pub local_version: Option<String>,
    pub integration: bool,
    pub symlink: bool,
}

#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub command: &'static str,
    pub apps: Vec<CheckApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckApp {
    pub name: String,
    pub status: CheckStatus,
    pub local_version: Option<String>,
    pub remote_version: Option<String>,
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limited_until: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    UpToDate,
    UpdateAvailable,
    RateLimited,
    Error,
}

#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    pub command: &'static str,
    pub apps: Vec<UpdateApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateApp {
    pub name: String,
    pub status: UpdateStatus,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limited_until: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Updated,
    UpToDate,
    RateLimited,
    Error,
}

#[derive(Debug, Serialize)]
pub struct RemoveResponse {
    pub command: &'static str,
    pub apps: Vec<RemoveApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RemoveApp {
    pub name: String,
    pub status: RemoveStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveStatus {
    Removed,
    Error,
    NotFound,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub command: &'static str,
    pub apps: Vec<ValidateApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidateApp {
    pub name: Option<String>,
    pub file: String,
    pub status: ValidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidateStatus {
    Valid,
    Invalid,
}

#[derive(Debug, Serialize)]
pub struct DoctorResponse {
    pub command: &'static str,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ok,
    Warn,
}
