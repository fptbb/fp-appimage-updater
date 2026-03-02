mod common;

use common::{setup_fedora_container, run_updater_success};

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
async fn test_cli_check() {
    let container = setup_fedora_container().await;
    
    // Command: check
    let stdout = run_updater_success(&container, &["check"]).await;
    
    // Check shouldn't panic and should iterate through apps
    assert!(stdout.contains("Update available for winboat") || stdout.contains("winboat is up to date"));
    assert!(stdout.contains("whatpulse"));
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
