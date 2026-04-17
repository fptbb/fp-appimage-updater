use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use zip::ZipArchive;

pub fn extract_zip_asset(
    zip_path: &Path,
    dest_path: &Path,
    inner_asset_match: Option<&str>,
) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("Failed to open zip file {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read zip archive {}", zip_path.display()))?;

    let mut asset_index = None;

    if let Some(pattern) = inner_asset_match {
        let matcher = glob::Pattern::new(pattern)
            .with_context(|| format!("Invalid inner_asset_match pattern: {}", pattern))?;
        
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            if file.is_file() && matcher.matches(file.name()) {
                asset_index = Some(i);
                break;
            }
        }
    } else {
        // Heuristic: look for .AppImage or any ELF file
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            if file.is_file() {
                if file.name().ends_with(".AppImage") {
                    asset_index = Some(i);
                    break;
                }
            }
        }

        if asset_index.is_none() {
             for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                if file.is_file() {
                    let mut header = [0u8; 4];
                    if file.read_exact(&mut header).is_ok() && &header == b"\x7fELF" {
                        asset_index = Some(i);
                        break;
                    }
                }
            }
        }
    }

    let index = asset_index.ok_or_else(|| {
        let mut available_names = Vec::new();
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                available_names.push(file.name().to_string());
            }
        }
        anyhow!(
            "Could not find a suitable asset in zip {}. Available files: {}",
            zip_path.display(),
            available_names.join(", ")
        )
    })?;

    let mut asset_file = archive.by_index(index)?;
    let mut out_file = fs::File::create(dest_path)
        .with_context(|| format!("Failed to create destination file {}", dest_path.display()))?;
    
    io::copy(&mut asset_file, &mut out_file)
        .with_context(|| format!("Failed to extract asset from zip to {}", dest_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest_path, perms)?;
    }

    Ok(())
}

pub fn is_zip_file(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else { return false };
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() { return false }
    &magic == b"PK\x03\x04"
}
