mod common;

use common::{setup_fedora_container, run_updater_success};
use serde_json::Value;

#[tokio::test]
async fn test_cli_list() {
    let container = setup_fedora_container().await;
    
    // Command: list
    let stdout = run_updater_success(&container, &["list"]).await;
    
    // Check that our configured apps are printed in the table
    assert!(stdout.contains("winboat"));
    assert!(stdout.contains("whatpulse"));
    assert!(stdout.contains("hayase"));
    assert!(stdout.contains("hydra-launcher"));
    assert!(stdout.contains("curseforge"));
}

#[tokio::test]
async fn test_cli_list_json() {
    let container = setup_fedora_container().await;

    let stdout = run_updater_success(&container, &["--json", "list"]).await;
    let payload: Value = serde_json::from_str(&stdout).expect("List output was not valid JSON");

    assert_eq!(payload["command"], "list");
    assert!(payload["apps"].is_array());
    assert!(!payload["apps"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_cli_check() {
    let container = setup_fedora_container().await;
    
    // Command: check
    let stdout = run_updater_success(&container, &["check"]).await;

    let lower = stdout.to_lowercase();

    // Check should print the new structured status output and include a summary.
    assert!(lower.contains("check results"));
    assert!(lower.contains("winboat"));
    assert!(lower.contains("whatpulse"));
    assert!(lower.contains("update available") || lower.contains("up to date"));
    assert!(lower.contains("summary:"));
}

#[tokio::test]
async fn test_cli_remove() {
    let container = setup_fedora_container().await;
    
    // Update one app first so we have something to remove
    run_updater_success(&container, &["update", "whatpulse"]).await;
    
    // Removing practically via cli
    let stdout = run_updater_success(&container, &["remove", "--all"]).await;
    
    // Assert there's output that uninstalls elements
    assert!(stdout.to_lowercase().contains("removed") || stdout.contains("Removing") || stdout.contains("Not installed"));
}

#[tokio::test]
async fn test_cli_update_json() {
    let container = setup_fedora_container().await;

    let stdout = run_updater_success(&container, &["--json", "update", "whatpulse"]).await;
    let payload: Value = serde_json::from_str(&stdout).expect("Update output was not valid JSON");

    assert_eq!(payload["command"], "update");
    assert!(payload["apps"].is_array());
    assert_eq!(payload["apps"].as_array().unwrap().len(), 1);
}
