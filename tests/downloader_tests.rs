use fp_appimage_updater::downloader::{ElfMachineArch, detect_elf_machine_arch_from_bytes};

fn elf_header_for_machine(machine: u16) -> [u8; 20] {
    let mut header = [0u8; 20];
    header[..4].copy_from_slice(b"\x7FELF");
    header[4] = 2;
    header[5] = 1;
    header[18..20].copy_from_slice(&machine.to_le_bytes());
    header
}

#[test]
fn detects_x86_64_elf_machine() {
    let arch =
        detect_elf_machine_arch_from_bytes(&elf_header_for_machine(62)).expect("missing arch");
    assert_eq!(arch, ElfMachineArch::X86_64);
}

#[test]
fn detects_aarch64_elf_machine() {
    let arch =
        detect_elf_machine_arch_from_bytes(&elf_header_for_machine(183)).expect("missing arch");
    assert_eq!(arch, ElfMachineArch::AArch64);
}

#[test]
fn skips_non_elf_files() {
    let err = detect_elf_machine_arch_from_bytes(b"not an elf")
        .expect_err("expected non-elf file to fail");
    assert!(format!("{:#}", err).contains("not an ELF executable"));
}
