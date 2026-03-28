mod common;

use common::{run_updater_success, setup_fedora_container, write_file_in_container};
use std::process::Command;

#[tokio::test]
async fn test_direct_resolver() {
    let container = setup_fedora_container().await;
    let container_id = container.id();

    println!("Testing direct strategy (whatpulse)...");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("fp-appimage-updater")
        .arg("update")
        .arg("whatpulse")
        .status()
        .expect("Failed to execute updater for whatpulse");

    assert!(
        status.success(),
        "Updater failed to process direct resolver"
    );

    let _ls_output = run_updater_success(&container, &["--help"]).await;
    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("ls")
        .arg("/root/.local/bin/AppImages/whatpulse.AppImage")
        .status()
        .expect("Failed check");

    assert!(
        check_file.success(),
        "Direct AppImage was not downloaded to expected path"
    );
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

    assert!(
        check_file.success(),
        "Forge AppImage was not downloaded to expected path"
    );
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

    assert!(
        status.success(),
        "Updater failed to process script resolver"
    );

    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("ls")
        .arg("/root/.local/bin/AppImages/hayase.AppImage")
        .status()
        .expect("Failed check");

    assert!(
        check_file.success(),
        "Script AppImage was not downloaded to expected path"
    );
}

#[tokio::test]
async fn test_zsync_download_with_mock_server() {
    let container = setup_fedora_container().await;
    let container_id = container.id();

    let mock_container = common::setup_wiremock_container().await;
    let mock_port = mock_container
        .get_host_port_ipv4(8080)
        .await
        .expect("Failed to get wiremock port");
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    let client = reqwest::Client::new();

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
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&head_stub)
        .send()
        .await
        .expect("Failed to configure HEAD stub");

    let get_stub = serde_json::json!({
        "request": { "method": "GET", "url": "/dummy.AppImage" },
        "response": {
            "status": 200,
            "body": "new_appimage_content",
            "headers": { "Content-Type": "application/octet-stream" }
        }
    });
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&get_stub)
        .send()
        .await
        .expect("Failed to configure GET stub");

    let zsync_stub = serde_json::json!({
        "request": { "method": "GET", "url": "/dummy.AppImage.zsync" },
        "response": {
            "status": 200,
            "body": "zsync_content",
            "headers": { "Content-Type": "application/octet-stream" }
        }
    });
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&zsync_stub)
        .send()
        .await
        .expect("Failed to configure ZSYNC stub");

    // Keep this stub minimal so the test exercises request wiring, not zsync parsing.
    let fake_zsync_script = r#"#!/bin/sh
target=""
url=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o)
            target="$2"
            shift 2
            ;;
        -i)
            shift 2
            ;;
        *)
            url="$1"
            shift
            ;;
    esac
done

python3 - "$url" "$target" <<'PY'
import pathlib
import sys
import urllib.request

url = sys.argv[1]
target = sys.argv[2]

if url:
    urllib.request.urlopen(url).read()

if target:
    pathlib.Path(target).write_bytes(b"fake zsync output\n")
PY
"#;
    write_file_in_container(&container, "/tmp/fake-bin/zsync", fake_zsync_script);

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("chmod")
        .arg("+x")
        .arg("/tmp/fake-bin/zsync")
        .status()
        .expect("Failed to chmod fake zsync");
    assert!(status.success(), "Failed to make fake zsync executable");

    let dummy_config = format!(
        r#"name: dummy-direct
strategy:
  strategy: direct
  url: http://localhost:{}/dummy.AppImage
  check_method: last-modified
zsync: true
integration: false
"#,
        mock_port
    );

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg(format!(
            "echo '{}' > /root/.config/fp-appimage-updater/apps/dummy-direct.yml",
            dummy_config
        ))
        .status()
        .expect("Failed to override dummy config");

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

    println!("Testing zsync capability with WireMock server (dummy-direct)...");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-lc")
        .arg("PATH=/tmp/fake-bin:$PATH fp-appimage-updater update dummy-direct")
        .status()
        .expect("Failed to execute updater for dummy-direct");

    assert!(status.success(), "Updater failed to process dummy app");

    let count_query = serde_json::json!({
        "method": "GET",
        "url": "/dummy.AppImage.zsync"
    });
    let res = client
        .post(format!("{}/__admin/requests/count", mock_url))
        .json(&count_query)
        .send()
        .await
        .expect("Failed to query request count");

    let stats: serde_json::Value = res.json().await.expect("Failed to parse count response");
    let count = stats["count"].as_i64().expect("Count was not a number");

    assert_eq!(
        count, 1,
        "Downloader did not request the .zsync endpoint (actual: {})",
        count
    );
}

#[tokio::test]
async fn test_segmented_download_with_mock_server() {
    let container = setup_fedora_container().await;
    let container_id = container.id();

    let mock_container = common::setup_wiremock_container().await;
    let mock_port = mock_container
        .get_host_port_ipv4(8080)
        .await
        .expect("Failed to get wiremock port");
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    let client = reqwest::Client::new();
    let full_body = "abcdefghijklmnop";

    let head_stub = serde_json::json!({
        "request": {
            "method": "HEAD",
            "url": "/segmented.AppImage"
        },
        "response": {
            "status": 200,
            "headers": {
                "Content-Length": "16",
                "Last-Modified": "Wed, 21 Oct 2026 07:28:00 GMT",
                "Accept-Ranges": "bytes",
                "Content-Type": "application/octet-stream"
            }
        }
    });
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&head_stub)
        .send()
        .await
        .expect("Failed to configure segmented HEAD stub");

    let probe_stub = serde_json::json!({
        "request": {
            "method": "GET",
            "url": "/segmented.AppImage",
            "headers": {
                "Range": { "equalTo": "bytes=0-0" }
            }
        },
        "response": {
            "status": 206,
            "body": "a",
            "headers": {
                "Content-Type": "application/octet-stream",
                "Content-Range": "bytes 0-0/16",
                "Content-Length": "1",
                "Accept-Ranges": "bytes"
            }
        }
    });
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&probe_stub)
        .send()
        .await
        .expect("Failed to configure probe stub");

    for (range, body) in [
        ("bytes=0-3", "abcd"),
        ("bytes=4-7", "efgh"),
        ("bytes=8-11", "ijkl"),
        ("bytes=12-15", "mnop"),
    ] {
        let content_range = format!("bytes {}/16", range.trim_start_matches("bytes="));
        let stub = serde_json::json!({
            "request": {
                "method": "GET",
                "url": "/segmented.AppImage",
                "headers": {
                    "Range": { "equalTo": range }
                }
            },
            "response": {
                "status": 206,
                "body": body,
                "headers": {
                    "Content-Type": "application/octet-stream",
                    "Content-Range": content_range,
                    "Content-Length": body.len().to_string(),
                    "Accept-Ranges": "bytes"
                }
            }
        });
        client
            .post(format!("{}/__admin/mappings", mock_url))
            .json(&stub)
            .send()
            .await
            .expect("Failed to configure segmented chunk stub");
    }

    let dummy_config = format!(
        r#"name: segmented-direct
strategy:
  strategy: direct
  url: http://localhost:{}/segmented.AppImage
  check_method: last-modified
segmented_downloads: true
integration: false
"#,
        mock_port
    );

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg(format!(
            "echo '{}' > /root/.config/fp-appimage-updater/apps/segmented-direct.yml",
            dummy_config
        ))
        .status()
        .expect("Failed to write segmented app config");

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg("mkdir -p /root/.local/bin/AppImages && echo 'old content' > /root/.local/bin/AppImages/segmented-direct.AppImage")
        .status()
        .expect("Failed to create old segmented file");

    let state_json = serde_json::json!({
        "apps": {
            "segmented-direct": {
                "version": "old_version",
                "file_path": "/root/.local/bin/AppImages/segmented-direct.AppImage"
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

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-lc")
        .arg("fp-appimage-updater update segmented-direct")
        .status()
        .expect("Failed to execute updater for segmented-direct");

    assert!(
        status.success(),
        "Updater failed to process segmented download app"
    );

    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("cat")
        .arg("/root/.local/bin/AppImages/segmented-direct.AppImage")
        .output()
        .expect("Failed to inspect segmented AppImage file");

    assert!(
        check_file.status.success(),
        "Failed to read segmented AppImage file"
    );
    assert_eq!(
        String::from_utf8_lossy(&check_file.stdout),
        full_body,
        "Segmented download did not reconstruct the expected file"
    );

    let count_query = serde_json::json!({
        "method": "GET",
        "url": "/segmented.AppImage"
    });
    let res = client
        .post(format!("{}/__admin/requests/count", mock_url))
        .json(&count_query)
        .send()
        .await
        .expect("Failed to query segmented request count");

    let stats: serde_json::Value = res
        .json()
        .await
        .expect("Failed to parse segmented count response");
    let count = stats["count"]
        .as_i64()
        .expect("Segmented count was not a number");
    assert_eq!(
        count, 5,
        "Segmented download did not use the expected range requests"
    );
}

#[tokio::test]
async fn test_segmented_download_falls_back_to_full_http() {
    let container = setup_fedora_container().await;
    let container_id = container.id();

    let mock_container = common::setup_wiremock_container().await;
    let mock_port = mock_container
        .get_host_port_ipv4(8080)
        .await
        .expect("Failed to get wiremock port");
    let mock_url = format!("http://127.0.0.1:{}", mock_port);

    let client = reqwest::Client::new();

    let probe_stub = serde_json::json!({
        "request": {
            "method": "HEAD",
            "url": "/fallback.AppImage",
        },
        "response": {
            "status": 200,
            "headers": {
                "Content-Length": "16",
                "Last-Modified": "Wed, 21 Oct 2026 07:28:00 GMT",
                "Content-Type": "application/octet-stream"
            }
        }
    });
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&probe_stub)
        .send()
        .await
        .expect("Failed to configure probe fallback stub");

    let full_stub = serde_json::json!({
        "request": { "method": "GET", "url": "/fallback.AppImage" },
        "response": {
            "status": 200,
            "body": "fallback_content",
            "headers": {
                "Content-Type": "application/octet-stream"
            }
        }
    });
    client
        .post(format!("{}/__admin/mappings", mock_url))
        .json(&full_stub)
        .send()
        .await
        .expect("Failed to configure fallback GET stub");

    let dummy_config = format!(
        r#"name: fallback-direct
strategy:
  strategy: direct
  url: http://localhost:{}/fallback.AppImage
  check_method: last-modified
segmented_downloads: true
integration: false
"#,
        mock_port
    );

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg(format!(
            "echo '{}' > /root/.config/fp-appimage-updater/apps/fallback-direct.yml",
            dummy_config
        ))
        .status()
        .expect("Failed to write fallback app config");

    Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-c")
        .arg("mkdir -p /root/.local/bin/AppImages && echo 'old content' > /root/.local/bin/AppImages/fallback-direct.AppImage")
        .status()
        .expect("Failed to create old fallback file");

    let state_json = serde_json::json!({
        "apps": {
            "fallback-direct": {
                "version": "old_version",
                "file_path": "/root/.local/bin/AppImages/fallback-direct.AppImage"
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
        .expect("Failed to create fallback state file");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("sh")
        .arg("-lc")
        .arg("fp-appimage-updater update fallback-direct")
        .status()
        .expect("Failed to execute updater for fallback-direct");

    assert!(
        status.success(),
        "Updater failed to process fallback segmented app"
    );

    let check_file = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("cat")
        .arg("/root/.local/bin/AppImages/fallback-direct.AppImage")
        .output()
        .expect("Failed to inspect fallback AppImage file");

    assert!(
        check_file.status.success(),
        "Failed to read fallback AppImage file"
    );
    assert_eq!(
        String::from_utf8_lossy(&check_file.stdout),
        "fallback_content",
        "Fallback download did not reconstruct the expected file"
    );

    let count_query = serde_json::json!({
        "method": "GET",
        "url": "/fallback.AppImage"
    });
    let res = client
        .post(format!("{}/__admin/requests/count", mock_url))
        .json(&count_query)
        .send()
        .await
        .expect("Failed to query fallback request count");

    let stats: serde_json::Value = res
        .json()
        .await
        .expect("Failed to parse fallback count response");
    let count = stats["count"]
        .as_i64()
        .expect("Fallback count was not a number");
    assert_eq!(
        count, 1,
        "Fallback path did not download the full file"
    );
}
