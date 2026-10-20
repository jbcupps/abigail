//! Regression boundaries for RUSTSEC-2026-0367, -0368, and -0369.
//! Upstream contract: zip-rs/zip2 v9.0.5, src/read/extract.rs and
//! src/format/functions.rs. Every destination and outside sentinel lives inside
//! one temporary directory, including deliberately escaping test symlinks.

use std::fs;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn writer() -> ZipWriter<Cursor<Vec<u8>>> {
    ZipWriter::new(Cursor::new(Vec::new()))
}

fn archive(writer: ZipWriter<Cursor<Vec<u8>>>) -> ZipArchive<Cursor<Vec<u8>>> {
    ZipArchive::new(writer.finish().expect("finish fixture ZIP")).expect("read fixture ZIP")
}

#[test]
fn ordinary_installer_archive_preserves_all_payload_bytes() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("installed");
    let executable: &[u8] = b"MZ\0\xff\x01\x02sample executable payload\0";
    let config: &[u8] = b"{\"family\":\"local\",\"enabled\":true}\n";
    let mut writer = writer();
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in [
        ("Abigail.exe", executable),
        ("resources/hive-daemon.exe", executable),
        ("resources/defaults.json", config),
    ] {
        writer.start_file(name, options).unwrap();
        writer.write_all(content).unwrap();
    }
    archive(writer).extract(&destination).unwrap();
    assert_eq!(
        fs::read(destination.join("Abigail.exe")).unwrap(),
        executable
    );
    assert_eq!(
        fs::read(destination.join("resources/hive-daemon.exe")).unwrap(),
        executable
    );
    assert_eq!(
        fs::read(destination.join("resources/defaults.json")).unwrap(),
        config
    );
}

#[test]
fn parent_traversal_entry_cannot_overwrite_outside_destination() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("installed");
    let sentinel = root.path().join("outside.txt");
    fs::write(&sentinel, b"keep this family record").unwrap();
    let mut writer = writer();
    writer
        .start_file("../outside.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"replacement").unwrap();
    assert!(archive(writer).extract(&destination).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep this family record");
}

#[test]
#[cfg(any(unix, windows))]
fn escaping_symlink_targets_are_rejected_before_materializing_a_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside.txt");
    fs::write(&outside, b"private sentinel").unwrap();
    let absolute_target = outside.to_str().unwrap().to_owned();
    for (case, target) in [
        ("relative", "../outside.txt"),
        ("absolute", &absolute_target),
    ] {
        let destination = root.path().join(case);
        let mut writer = writer();
        writer
            .add_symlink("exposed", target, SimpleFileOptions::default())
            .unwrap();
        assert!(archive(writer).extract(&destination).is_err(), "{case}");
        assert!(
            fs::symlink_metadata(destination.join("exposed")).is_err(),
            "{case} must not leave a usable escaping link"
        );
        assert_eq!(fs::read(&outside).unwrap(), b"private sentinel");
    }
}

#[test]
#[cfg(unix)]
fn preexisting_symlink_directory_cannot_redirect_extracted_payloads() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("installed");
    let outside = root.path().join("outside");
    fs::create_dir_all(&destination).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("record.txt"), b"preserved").unwrap();
    symlink(&outside, destination.join("resources")).unwrap();
    let mut writer = writer();
    writer
        .start_file("resources/record.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"overwritten").unwrap();
    assert!(archive(writer).extract(&destination).is_err());
    assert_eq!(fs::read(outside.join("record.txt")).unwrap(), b"preserved");
}

#[test]
#[cfg(unix)]
fn apparently_internal_link_cannot_escape_through_an_existing_link() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("installed");
    let outside = root.path().join("outside.txt");
    fs::create_dir_all(&destination).unwrap();
    fs::write(&outside, b"private record").unwrap();
    symlink(&outside, destination.join("pivot")).unwrap();
    let mut writer = writer();
    writer
        .add_symlink("new-link", "pivot", SimpleFileOptions::default())
        .unwrap();
    assert!(archive(writer).extract(&destination).is_err());
    assert!(fs::symlink_metadata(destination.join("new-link")).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"private record");
}

#[test]
#[cfg(unix)]
fn extracted_symlink_permissions_never_change_its_outside_target() {
    use std::os::unix::fs::PermissionsExt;
    use zip::read::{ExtractOptions, SymlinkExtractAction};

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("installed");
    let outside = root.path().join("outside.txt");
    fs::write(&outside, b"private record").unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::metadata(&outside).unwrap().permissions().mode();
    let mut writer = writer();
    writer
        .add_symlink(
            "permission-link",
            outside.to_str().unwrap(),
            SimpleFileOptions::default().unix_permissions(0o777),
        )
        .unwrap();
    // Explicitly permit this fixture's escaping link to isolate permission
    // safety from the default confinement rejection. Production uses extract().
    archive(writer)
        .extract_with_options(
            &destination,
            ExtractOptions::default().symlink_action(SymlinkExtractAction::ExtractNoRestrictions),
        )
        .unwrap();
    assert!(fs::symlink_metadata(destination.join("permission-link"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::metadata(&outside).unwrap().permissions().mode(), before);
    assert_eq!(fs::read(&outside).unwrap(), b"private record");
}

#[test]
#[cfg(unix)]
fn bulk_extraction_strips_privileged_bits_from_raw_external_attributes() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("installed");
    let mut writer = writer();
    // unix_permissions() alone masks these bits while writing. Raw external
    // attributes model an untrusted archive that really carries SUID/SGID.
    writer
        .start_file(
            "installer-helper",
            SimpleFileOptions::default()
                .unix_permissions(0o755)
                .external_attributes(0o106755 << 16),
        )
        .unwrap();
    writer.write_all(b"non-executable fixture bytes").unwrap();
    writer
        .add_directory(
            "shared/",
            SimpleFileOptions::default()
                .unix_permissions(0o775)
                .external_attributes(0o043775 << 16),
        )
        .unwrap();
    archive(writer).extract(&destination).unwrap();
    for name in ["installer-helper", "shared"] {
        let mode = fs::metadata(destination.join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7000, 0, "privileged bits survived on {name}");
    }
    assert_eq!(
        fs::read(destination.join("installer-helper")).unwrap(),
        b"non-executable fixture bytes"
    );
}
