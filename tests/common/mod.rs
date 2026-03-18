use std::process::{Command, Output};
use testcontainers::{runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};

pub async fn setup_fedora_container() -> ContainerAsync<GenericImage> {
    // 1. Start a Fedora container, using the host network for faster downloads
    let image = GenericImage::new("fedora", "latest")
        .with_cmd(["sleep", "infinity"])
        .with_network("host");
        
    let container = image.start().await.expect("Failed to start Fedora container");

    let container_id_for_install = container.id();
    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id_for_install)
        .arg("dnf")
        .arg("install")
        .arg("-y")
        .arg("zsync")
        .arg("python3")
        .status()
        .expect("Failed to execute dnf install");
    assert!(status.success(), "Failed to install zsync and python3 in test container");

    // 2. Get the host path to the compiled binary
    let binary_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("build/fp-appimage-updater.x64");
        
    if !binary_path.exists() {
        panic!("Release binary not found at {:?}. Please run Justfile or install.sh first.", binary_path);
    }

    // 3. Copy the binary to the container
    let container_id = container.id();
    
    // Copy the binary into /usr/local/bin
    let status = Command::new("docker")
        .arg("cp")
        .arg(&binary_path)
        .arg(format!("{}:/usr/local/bin/fp-appimage-updater", container_id))
        .status()
        .expect("Failed to execute docker cp for binary");
    assert!(status.success(), "Failed to copy binary into container");

    // Make sure it's executable
    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("chmod")
        .arg("+x")
        .arg("/usr/local/bin/fp-appimage-updater")
        .status()
        .expect("Failed to chmod binary");
    assert!(status.success(), "Failed to chmod binary in container");

    // 4. Setup Config directory
    let status = Command::new("docker")
        .arg("exec")
        .arg(container_id)
        .arg("mkdir")
        .arg("-p")
        .arg("/root/.config/fp-appimage-updater/")
        .status()
        .expect("Failed to create config dir");
    assert!(status.success(), "Failed to create config dir in container");

    // Copy examples/apps
    let config_path = format!("{}/examples/apps", env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("docker")
        .arg("cp")
        .arg(&config_path)
        .arg(format!("{}:/root/.config/fp-appimage-updater/apps", container_id))
        .status()
        .expect("Failed to copy apps config");
    assert!(status.success(), "Failed to copy apps config into container");
    
    // Also copy config.yml
    let global_config_path = format!("{}/examples/config.yml", env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("docker")
        .arg("cp")
        .arg(&global_config_path)
        .arg(format!("{}:/root/.config/fp-appimage-updater/config.yml", container_id))
        .status()
        .expect("Failed to copy global config");
    assert!(status.success(), "Failed to copy global config into container");

    // Make sure the resolver scripts are executable
    // (hayase resolver.sh needs executable bit)
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

/// Helper function to easily run updater commands inside the container
pub async fn run_updater_cmd(container: &ContainerAsync<GenericImage>, args: &[&str]) -> Output {
    Command::new("docker")
        .arg("exec")
        .arg(container.id())
        .arg("fp-appimage-updater")
        .args(args)
        .output()
        .expect("Failed to execute updater command inside container")
}

/// Prints output and asserts that the command succeeded
pub async fn run_updater_success(container: &ContainerAsync<GenericImage>, args: &[&str]) -> String {
    let output = run_updater_cmd(container, args).await;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    if !output.status.success() {
        panic!("Command {:?} failed.\nSTDOUT:\n{}\nSTDERR:\n{}", args, stdout, stderr);
    }
    
    stdout
}

#[allow(dead_code)]
pub fn write_file_in_container(
    container: &ContainerAsync<GenericImage>,
    path: &str,
    content: &str,
) {
    let command = format!(
        "mkdir -p \"$(dirname '{path}')\" && cat > '{path}' <<'EOF'\n{content}\nEOF"
    );
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
