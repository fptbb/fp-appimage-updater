use anyhow::Result;
use glob::glob;
use serde_yaml::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::parser::ConfigPaths;

#[derive(Debug)]
pub struct ValidationResult {
    pub app_name: Option<String>,
    pub file: String,
    pub status: ValidationStatus,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum ValidationStatus {
    Valid,
    Invalid,
}

pub fn validate_app_configs(
    paths: &ConfigPaths,
    target_app: Option<&str>,
) -> Result<(Vec<ValidationResult>, Option<String>)> {
    let apps_dir = paths.apps_dir();
    if !apps_dir.exists() {
        return Ok((Vec::new(), target_app.map(app_not_found_error)));
    }

    let files = app_config_files(&apps_dir)?;
    let mut results = Vec::new();
    let mut target_found = target_app.is_none();

    for file in files {
        let result = validate_file(&file);
        let matches_target = target_app.is_some_and(|target| {
            result.app_name.as_deref() == Some(target)
        });

        if target_app.is_some() {
            if matches_target {
                target_found = true;
                results.push(result);
            }
        } else {
            results.push(result);
        }
    }

    let error = if target_found {
        None
    } else {
        target_app.map(app_not_found_error)
    };

    Ok((results, error))
}

fn app_config_files(apps_dir: &Path) -> Result<Vec<PathBuf>> {
    let pattern = format!("{}/**/*.yml", apps_dir.display());
    let mut files = Vec::new();

    for entry in glob(&pattern)? {
        if let Ok(path) = entry {
            files.push(path);
        }
    }

    Ok(files)
}

fn validate_file(path: &Path) -> ValidationResult {
    let file = path.display().to_string();
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            return ValidationResult {
                app_name: None,
                file,
                status: ValidationStatus::Invalid,
                error: Some(format!("Failed to read file: {}", error)),
            };
        }
    };

    let inferred_name = infer_name_from_yaml(&content);

    match serde_yaml::from_str::<AppConfig>(&content) {
        Ok(app) => ValidationResult {
            app_name: Some(app.name),
            file,
            status: ValidationStatus::Valid,
            error: None,
        },
        Err(error) => ValidationResult {
            app_name: inferred_name,
            file,
            status: ValidationStatus::Invalid,
            error: Some(error.to_string()),
        },
    }
}

fn infer_name_from_yaml(content: &str) -> Option<String> {
    let value: Value = serde_yaml::from_str(content).ok()?;
    let map = value.as_mapping()?;
    map.get(&Value::String("name".to_string()))?
        .as_str()
        .map(ToOwned::to_owned)
}

fn app_not_found_error(name: &str) -> String {
    format!("App '{}' not found in configuration.", name)
}

#[cfg(test)]
mod tests {
    use super::infer_name_from_yaml;

    #[test]
    fn infer_name_from_valid_yaml() {
        let yaml = "name: whatpulse\nstrategy:\n  strategy: direct\n  url: https://example.org/x.AppImage\n  check_method: etag\n";
        assert_eq!(infer_name_from_yaml(yaml).as_deref(), Some("whatpulse"));
    }
}
