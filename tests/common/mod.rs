use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::Duration;
use testcontainers::{ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner};

const TEST_IMAGE_TAG: &str = "fp-appimage-updater-test-fedora:latest";
static BUILD_TEST_IMAGE_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
static DOCKER_START_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn ensure_test_image() -> Result<(), String> {
    BUILD_TEST_IMAGE_RESULT
        .get_or_init(|| {
            let dockerfile = format!(
                "{}/tests/common/fedora.Dockerfile",
                env!("CARGO_MANIFEST_DIR")
            );
            let context_dir = format!("{}/tests/common", env!("CARGO_MANIFEST_DIR"));
            let status = Command::new("docker")
                .arg("build")
                .arg("-t")
                .arg(TEST_IMAGE_TAG)
                .arg("-f")
                .arg(&dockerfile)
                .arg(&context_dir)
                .status()
                .map_err(|e| format!("Failed to build cached Fedora test image: {}", e))?;
            if status.success() {
                Ok(())
            } else {
                Err("Failed to build cached Fedora test image".to_string())
            }
        })
        .clone()
}

async fn acquire_docker_start_lock() -> tokio::sync::MutexGuard<'static, ()> {
    DOCKER_START_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

pub async fn setup_fedora_container() -> ContainerAsync<GenericImage> {
    ensure_test_image().expect("Failed to build cached Fedora test image");
    let _guard = acquire_docker_start_lock().await;

    let image = GenericImage::new("fp-appimage-updater-test-fedora", "latest")
        .with_cmd(["sleep", "infinity"])
        .with_network("host");

    let container = image
        .start()
        .await
        .expect("Failed to start Fedora container");

    let binary_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("build/fp-appimage-updater.x64");

    if !binary_path.exists() {
        panic!(
            "Release binary not found at {:?}. Please run Justfile or install.sh first.",
            binary_path
        );
    }

    let container_id = container.id();

    let status = Command::new("docker")
        .arg("cp")
        .arg(&binary_path)
        .arg(format!(
            "{}:/usr/local/bin/fp-appimage-updater",
            container_id
        ))
        .status()
        .expect("Failed to execute docker cp for binary");
    assert!(status.success(), "Failed to copy binary into container");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("chmod")
        .arg("+x")
        .arg("/usr/local/bin/fp-appimage-updater")
        .status()
        .expect("Failed to chmod binary");
    assert!(status.success(), "Failed to chmod binary in container");

    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("mkdir")
        .arg("-p")
        .arg("/root/.config/fp-appimage-updater/")
        .status()
        .expect("Failed to create config dir");
    assert!(status.success(), "Failed to create config dir in container");

    let config_path = format!("{}/examples/apps", env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("docker")
        .arg("cp")
        .arg(&config_path)
        .arg(format!(
            "{}:/root/.config/fp-appimage-updater/apps",
            container_id
        ))
        .status()
        .expect("Failed to copy apps config");
    assert!(
        status.success(),
        "Failed to copy apps config into container"
    );

    let global_config_path = format!("{}/examples/config.yml", env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("docker")
        .arg("cp")
        .arg(&global_config_path)
        .arg(format!(
            "{}:/root/.config/fp-appimage-updater/config.yml",
            container_id
        ))
        .status()
        .expect("Failed to copy global config");
    assert!(
        status.success(),
        "Failed to copy global config into container"
    );

    let _status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("chmod")
        .arg("+x")
        .arg("/root/.config/fp-appimage-updater/apps/hayase/resolver.sh")
        .status()
        .unwrap_or_else(|_| Default::default());

    container
}

#[allow(dead_code)]
pub async fn setup_wiremock_container() -> ContainerAsync<GenericImage> {
    let _guard = acquire_docker_start_lock().await;

    use testcontainers::core::ContainerPort;
    use testcontainers::runners::AsyncRunner;

    let wiremock_image = testcontainers::GenericImage::new("wiremock/wiremock", "latest")
        .with_exposed_port(ContainerPort::Tcp(8080))
        .with_startup_timeout(Duration::from_secs(180));

    let container = wiremock_image
        .start()
        .await
        .expect("Failed to start wiremock");

    wait_for_wiremock_ready(&container).await;
    container
}

async fn wait_for_wiremock_ready(container: &ContainerAsync<GenericImage>) {
    let host_port = container
        .get_host_port_ipv4(8080)
        .await
        .expect("Failed to get wiremock port");
    let base_url = format!("http://127.0.0.1:{}", host_port);
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);

    loop {
        match client
            .get(format!("{}/__admin/mappings", base_url))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return,
            Ok(_) | Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Ok(resp) => panic!(
                "Wiremock became reachable but returned unexpected status {}",
                resp.status()
            ),
            Err(err) => panic!("Wiremock did not become ready in time: {}", err),
        }
    }
}

pub async fn run_updater_cmd(container: &ContainerAsync<GenericImage>, args: &[&str]) -> Output {
    Command::new("docker")
        .arg("exec")
        .arg(container.id())
        .arg("fp-appimage-updater")
        .args(args)
        .output()
        .expect("Failed to execute updater command inside container")
}

pub async fn run_updater_success(
    container: &ContainerAsync<GenericImage>,
    args: &[&str],
) -> String {
    let output = run_updater_cmd(container, args).await;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        panic!(
            "Command {:?} failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
            args, stdout, stderr
        );
    }

    stdout
}

#[allow(dead_code)]
pub fn write_file_in_container(
    container: &ContainerAsync<GenericImage>,
    path: &str,
    content: &str,
) {
    let command =
        format!("mkdir -p \"$(dirname '{path}')\" && cat > '{path}' <<'EOF'\n{content}\nEOF");
    let status = Command::new("docker")
        .arg("exec")
        .arg(container.id())
        .arg("sh")
        .arg("-lc")
        .arg(command)
        .status()
        .expect("Failed to write file in container");

    assert!(status.success(), "Failed to write file in container");
}
