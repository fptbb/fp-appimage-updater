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
    let _ls_output = run_updater_success(&container, &["--help"]).await;
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

#[tokio::test]
async fn test_zsync_download_with_mock_server() {
    let container = setup_fedora_container().await;
    let container_id = container.id();
    
    // 1. Start WireMock Container
    use testcontainers::core::ContainerPort;
    use testcontainers::core::WaitFor;
    use testcontainers::runners::AsyncRunner;

    let wiremock_image = testcontainers::GenericImage::new("wiremock/wiremock", "latest")
        .with_exposed_port(ContainerPort::Tcp(8080))
        .with_wait_for(WaitFor::http(
            testcontainers::core::wait::HttpWaitStrategy::new("/__admin/mappings")
                .with_expected_status_code(200_u16)
        ));
    let mock_container = wiremock_image.start().await.expect("Failed to start wiremock");
    let mock_port = mock_container.get_host_port_ipv4(8080).await.expect("Failed to get wiremock port");
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    // 2. Configure WireMock Stubs via REST API
    let client = reqwest::Client::new();
    
    // Stub: HEAD /dummy.AppImage
    let head_stub = serde_json::json!({
        "request": { "method": "HEAD", "url": "/dummy.AppImage" },
        "response": {
            "status": 200,
            "headers": {
                "Content-Length": "20",
                "Last-Modified": "Wed, 21 Oct 2026 07:28:00 GMT",
                "Content-Type": "application/octet-stream"
            }
        }
    });
    client.post(format!("{}/__admin/mappings", mock_url))
        .json(&head_stub).send().await.expect("Failed to configure HEAD stub");

    // Stub: GET /dummy.AppImage
    let get_stub = serde_json::json!({
        "request": { "method": "GET", "url": "/dummy.AppImage" },
        "response": {
            "status": 200,
            "body": "new_appimage_content",
            "headers": { "Content-Type": "application/octet-stream" }
        }
    });
    client.post(format!("{}/__admin/mappings", mock_url))
        .json(&get_stub).send().await.expect("Failed to configure GET stub");

    // Stub: GET /dummy.AppImage.zsync
    let zsync_stub = serde_json::json!({
        "request": { "method": "GET", "url": "/dummy.AppImage.zsync" },
        "response": {
            "status": 200,
            "body": "zsync_content",
            "headers": { "Content-Type": "application/octet-stream" }
        }
    });
    client.post(format!("{}/__admin/mappings", mock_url))
        .json(&zsync_stub).send().await.expect("Failed to configure ZSYNC stub");

    // 3. Configure fp-appimage-updater apps/dummy-direct.yml dynamically
    // Since our fedora container runs on --network host, it can reach localhost:{mock_port}
    let dummy_config = format!(r#"name: dummy-direct
strategy:
  strategy: direct
  url: http://localhost:{}/dummy.AppImage
  check_method: last-modified
zsync: true
integration: false
"#, mock_port);

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg(format!("echo '{}' > /root/.config/fp-appimage-updater/apps/dummy-direct.yml", dummy_config))
        .status()
        .expect("Failed to override dummy config");

    // 4. Create dummy "old" state
    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg("mkdir -p /root/.local/bin/AppImages && echo 'old content' > /root/.local/bin/AppImages/dummy-direct.AppImage")
        .status()
        .expect("Failed to create old AppImage file");
        
    let state_json = serde_json::json!({
        "apps": {
            "dummy-direct": {
                "version": "old_version",
                "file_path": "/root/.local/bin/AppImages/dummy-direct.AppImage"
            }
        }
    });

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg(format!("mkdir -p /root/.local/share/fp-appimage-updater /root/.local/state/fp-appimage-updater && echo '{}' | tee /root/.local/share/fp-appimage-updater/cache.json /root/.local/state/fp-appimage-updater/cache.json", state_json))
        .status()
        .expect("Failed to create state file");
    
    // 5. Test Updater
    println!("Testing zsync capability with WireMock server (dummy-direct)...");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("fp-appimage-updater")
        .arg("update")
        .arg("dummy-direct")
        .status()
        .expect("Failed to execute updater for dummy-direct");
        
    assert!(status.success(), "Updater failed to process dummy app");

    // 6. Verify Zsync request explicitly via WireMock's API
    let count_query = serde_json::json!({
        "method": "GET",
        "url": "/dummy.AppImage.zsync"
    });
    let res = client.post(format!("{}/__admin/requests/count", mock_url))
        .json(&count_query).send().await.expect("Failed to query request count");
    
    let stats: serde_json::Value = res.json().await.expect("Failed to parse count response");
    let count = stats["count"].as_i64().expect("Count was not a number");

    // Ensure it hits .zsync exactly once
    assert_eq!(count, 1, "Downloader did not request the .zsync endpoint (actual: {})", count);
}
