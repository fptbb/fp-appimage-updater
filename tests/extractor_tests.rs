use fp_appimage_updater::extractor::{extract_zip_asset, is_zip_file};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[test]
fn test_is_zip_file() {
    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    let file = File::create(&zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    zip.start_file("test.txt", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"hello").unwrap();
    zip.finish().unwrap();

    assert!(is_zip_file(&zip_path));

    let not_zip_path = dir.path().join("not_zip.txt");
    let mut not_zip = File::create(&not_zip_path).unwrap();
    not_zip.write_all(b"not a zip").unwrap();

    assert!(!is_zip_file(&not_zip_path));
}

#[test]
fn test_extract_zip_asset_by_name() {
    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    let file = File::create(&zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    
    zip.start_file("wrong.txt", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"wrong").unwrap();
    
    zip.start_file("target.AppImage", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"target content").unwrap();
    
    zip.finish().unwrap();

    let dest_path = dir.path().join("extracted.AppImage");
    extract_zip_asset(&zip_path, &dest_path, Some("target.AppImage")).unwrap();

    let content = std::fs::read_to_string(&dest_path).unwrap();
    assert_eq!(content, "target content");
}

#[test]
fn test_extract_zip_asset_heuristic_appimage() {
    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    let file = File::create(&zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    
    zip.start_file("something.txt", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"something").unwrap();
    
    zip.start_file("MyApp.AppImage", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"appimage content").unwrap();
    
    zip.finish().unwrap();

    let dest_path = dir.path().join("extracted.AppImage");
    extract_zip_asset(&zip_path, &dest_path, None).unwrap();

    let content = std::fs::read_to_string(&dest_path).unwrap();
    assert_eq!(content, "appimage content");
}

#[test]
fn test_extract_zip_asset_heuristic_elf() {
    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    let file = File::create(&zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    
    zip.start_file("data.bin", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"random data").unwrap();
    
    let mut elf_content = b"\x7fELF".to_vec();
    elf_content.extend_from_slice(&[0u8; 16]);
    zip.start_file("my-binary", SimpleFileOptions::default()).unwrap();
    zip.write_all(&elf_content).unwrap();
    
    zip.finish().unwrap();

    let dest_path = dir.path().join("extracted-binary");
    extract_zip_asset(&zip_path, &dest_path, None).unwrap();

    let content = std::fs::read(&dest_path).unwrap();
    assert_eq!(content, elf_content);
}
