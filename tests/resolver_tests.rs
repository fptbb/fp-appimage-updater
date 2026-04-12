mod common;

use common::{run_updater_success, setup_fedora_container};
use hex::encode as hex_encode;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use zsync_rs::{
    checksum::{calc_md4, calc_sha1},
    rsum::calc_rsum_block,
};

fn build_zsync_manifest_bytes(control_url: &str, target_body: &[u8], blocksize: usize) -> Vec<u8> {
    let mut manifest = format!(
        "zsync: 0.6.2\nFilename: dummy.AppImage\nBlocksize: {blocksize}\nLength: {}\nHash-Lengths: 1,4,16\nURL: {control_url}\nSHA-1: {}\n\n",
        target_body.len(),
        hex_encode(calc_sha1(target_body))
    )
    .into_bytes();

    for chunk in target_body.chunks(blocksize) {
        let mut block = vec![0u8; blocksize];
        block[..chunk.len()].copy_from_slice(chunk);
        let rsum = calc_rsum_block(&block);
        manifest.extend_from_slice(&rsum.a.to_be_bytes());
        manifest.extend_from_slice(&rsum.b.to_be_bytes());
        manifest.extend_from_slice(&calc_md4(&block));
    }

    manifest
}

fn spawn_zsync_test_server(
    target_body: Vec<u8>,
    blocksize: usize,
) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind test server");
    let addr = listener
        .local_addr()
        .expect("failed to read test server addr");
    let zsync_hits = Arc::new(AtomicUsize::new(0));
    let range_hits = Arc::new(AtomicUsize::new(0));
    let head_hits = Arc::new(AtomicUsize::new(0));
    let target_body = Arc::new(target_body);

    let zsync_hits_thread = Arc::clone(&zsync_hits);
    let range_hits_thread = Arc::clone(&range_hits);
    let head_hits_thread = Arc::clone(&head_hits);
    let target_body_thread = Arc::clone(&target_body);
    let last_modified = "Tue, 21 Oct 2025 07:28:00 GMT";

    thread::spawn(move || {
        let target_body = target_body_thread;
        for connection in listener.incoming() {
            let Ok(mut stream) = connection else {
                continue;
            };

            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let read = stream.read(&mut buf).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request_text = String::from_utf8_lossy(&request);
            let mut lines = request_text.lines();
            let request_line = lines.next().unwrap_or("");
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("");
            let range_header = lines
                .clone()
                .find(|line| line.starts_with("Range:"))
                .map(|line| line.trim_start_matches("Range:").trim().to_string());

            let response = if method == "GET" && path == "/dummy.AppImage.zsync" {
                zsync_hits_thread.fetch_add(1, Ordering::SeqCst);
                let control_url = format!("http://{}:{}/dummy.AppImage", addr.ip(), addr.port());
                let manifest =
                    build_zsync_manifest_bytes(&control_url, target_body.as_slice(), blocksize);
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    manifest.len()
                )
                .into_bytes()
                .into_iter()
                .chain(manifest)
                .collect::<Vec<u8>>()
            } else if method == "HEAD" && path == "/dummy.AppImage" {
                head_hits_thread.fetch_add(1, Ordering::SeqCst);
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nLast-Modified: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    target_body.len(),
                    last_modified
                )
                .into_bytes()
            } else if method == "GET" && path == "/dummy.AppImage" {
                range_hits_thread.fetch_add(1, Ordering::SeqCst);
                let (start, end) = range_header
                    .as_deref()
                    .and_then(parse_range_header)
                    .unwrap_or((0, target_body.len().saturating_sub(1)));
                let start = start.min(target_body.len());
                let end = end.min(target_body.len().saturating_sub(1));
                let body = if start <= end {
                    target_body[start..=end].to_vec()
                } else {
                    Vec::new()
                };
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nLast-Modified: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len(),
                    start,
                    end,
                    target_body.len(),
                    last_modified
                )
                .into_bytes()
                .into_iter()
                .chain(body)
                .collect::<Vec<u8>>()
            } else {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            };

            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });

    (addr, zsync_hits, range_hits)
}

fn parse_range_header(header: &str) -> Option<(usize, usize)> {
    let value = header.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    Some((start, end))
}

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

    let target_body = std::fs::read("/bin/true")
        .or_else(|_| std::fs::read("/usr/bin/true"))
        .expect("Failed to read a host ELF binary for zsync testing");
    let (server_addr, zsync_hits, range_hits) = spawn_zsync_test_server(target_body, 512);
    let mock_url = format!("http://{}/dummy.AppImage", server_addr);

    let dummy_config = format!(
        r#"name: dummy-direct
strategy:
  strategy: direct
  url: {}
  check_method: last-modified
zsync: true
integration: false
"#,
        mock_url
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
        .arg("mkdir -p /root/.local/bin/AppImages && head -c 4096 /dev/zero > /root/.local/bin/AppImages/dummy-direct.AppImage")
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

    println!("Testing zsync capability with local zsync server (dummy-direct)...");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("fp-appimage-updater")
        .arg("update")
        .arg("dummy-direct")
        .status()
        .expect("Failed to execute updater for dummy-direct");

    assert!(status.success(), "Updater failed to process dummy app");
    assert_eq!(
        zsync_hits.load(Ordering::SeqCst),
        1,
        "Downloader did not request the .zsync endpoint"
    );
    assert!(
        range_hits.load(Ordering::SeqCst) >= 1,
        "Downloader did not request ranged AppImage bytes"
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
    assert_eq!(count, 1, "Fallback path did not download the full file");
}
