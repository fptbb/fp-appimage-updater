mod common;

use common::{setup_fedora_container, run_updater_success};
use std::process::Command;

#[tokio::test]
async fn test_direct_resolver() {
    let container = setup_fedora_container().await;
    let container_id = container.id();
    
    // Update whatpulse (direct strategy)
    println!("Testing direct strategy (whatpulse)...");
    
    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("fp-appimage-updater")
        .arg("update")
        .arg("whatpulse")
        .status()
        .expect("Failed to execute updater for whatpulse");
        
    assert!(status.success(), "Updater failed to process direct resolver");

    // Verify it downloaded to the mapped AppImages directory
    let _ls_output = run_updater_success(&container, &["--help"]).await; // Just testing it runs basically
    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("ls")
        .arg("/root/.local/bin/AppImages/whatpulse.AppImage")
        .status()
        .expect("Failed check");
        
    assert!(check_file.success(), "Direct AppImage was not downloaded to expected path");
}

#[tokio::test]
async fn test_forge_resolver() {
    let container = setup_fedora_container().await;
    let container_id = container.id();
    
    println!("Testing forge strategy (winboat)...");
    
    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("fp-appimage-updater")
        .arg("update")
        .arg("winboat")
        .status()
        .expect("Failed to execute updater for winboat");
        
    assert!(status.success(), "Updater failed to process forge resolver");

    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("ls")
        .arg("/root/.local/bin/AppImages/winboat.AppImage")
        .status()
        .expect("Failed check");
        
    assert!(check_file.success(), "Forge AppImage was not downloaded to expected path");
}

#[tokio::test]
async fn test_script_resolver() {
    let container = setup_fedora_container().await;
    let container_id = container.id();
    
    println!("Testing script strategy (hayase)...");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("fp-appimage-updater")
        .arg("update")
        .arg("hayase")
        .status()
        .expect("Failed to execute updater for hayase");
        
    assert!(status.success(), "Updater failed to process script resolver");

    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("ls")
        .arg("/root/.local/bin/AppImages/hayase.AppImage")
        .status()
        .expect("Failed check");
        
    assert!(check_file.success(), "Script AppImage was not downloaded to expected path");
}
