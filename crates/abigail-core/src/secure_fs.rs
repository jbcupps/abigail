use crate::error::{CoreError, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    // Allow trusted absolute paths from the app/runtime, but reject parent
    // traversal so callers cannot smuggle `..` through relative inputs.
    for component in path.components() {
        match component {
            Component::CurDir
            | Component::Normal(_)
            | Component::RootDir
            | Component::Prefix(_) => {}
            Component::ParentDir => {
                return Err(CoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Path '{}' contains disallowed traversal", path.display()),
                )));
            }
        }
    }

    #[cfg(windows)]
    validate_windows_file_path(path)?;

    let parent = path.parent().ok_or_else(|| {
        CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Path '{}' has no parent directory", path.display()),
        ))
    })?;

    std::fs::create_dir_all(parent)?;

    let temp_path = unique_temp_path(path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }

    let write_result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file_atomic(&temp_path, path)?;
        sync_parent(parent)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }

    write_result
}

pub fn write_string_atomic(path: &Path, content: &str) -> Result<()> {
    write_bytes_atomic(path, content.as_bytes())
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("abigail.tmp");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        suffix
    ))
}

#[cfg(not(windows))]
fn replace_file_atomic(src: &Path, dest: &Path) -> Result<()> {
    std::fs::rename(src, dest)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file_atomic(src: &Path, dest: &Path) -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    // Rust's Windows canonicalize returns extended-length drive/UNC paths.
    // The temporary file and parent already exist, while the destination may
    // be a first write. Canonicalizing the parent and joining only the leaf
    // keeps both MoveFileExW arguments independent of MAX_PATH and OS opt-in.
    let src_absolute = std::fs::canonicalize(src)?;
    let dest_parent = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let dest_leaf = dest.file_name().ok_or_else(|| {
        CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Path '{}' has no file name", dest.display()),
        ))
    })?;
    let dest_absolute = std::fs::canonicalize(dest_parent)?.join(dest_leaf);
    let src_wide = path_to_wide(&src_absolute);
    let dest_wide = path_to_wide(&dest_absolute);
    unsafe {
        MoveFileExW(
            PCWSTR(src_wide.as_ptr()),
            PCWSTR(dest_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|e| {
            CoreError::Io(std::io::Error::other(format!(
                "Atomic replace failed for '{}' -> '{}': {}",
                src.display(),
                dest.display(),
                e
            )))
        })?;
    }
    Ok(())
}

#[cfg(windows)]
fn validate_windows_file_path(path: &Path) -> Result<()> {
    use std::path::Prefix;

    let supported = match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            path.is_absolute()
                && matches!(
                    prefix.kind(),
                    Prefix::Disk(_)
                        | Prefix::UNC(_, _)
                        | Prefix::VerbatimDisk(_)
                        | Prefix::VerbatimUNC(_, _)
                )
        }
        Some(Component::RootDir) => false,
        _ => true,
    };
    if !supported {
        return Err(CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Windows file path '{}' must be a normal relative path or an absolute drive/UNC path",
                path.display()
            ),
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn path_to_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(not(windows))]
fn sync_parent(parent: &Path) -> Result<()> {
    let dir = OpenOptions::new().read(true).open(parent)?;
    dir.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn sync_parent(_parent: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = std::env::temp_dir().join("abigail_secure_fs_atomic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("value.bin");

        write_bytes_atomic(&path, b"first").unwrap();
        write_bytes_atomic(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_replaces_long_windows_path_without_os_opt_in() {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        // Own a unique directory so concurrent test processes cannot delete
        // each other's files. Revalidate its resolved identity before cleanup.
        struct TestDirectory {
            path: PathBuf,
            canonical_path: PathBuf,
            canonical_parent: PathBuf,
            created_at: u64,
        }
        impl Drop for TestDirectory {
            fn drop(&mut self) {
                let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
                    return;
                };
                if !metadata.is_dir()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                    || metadata.creation_time() != self.created_at
                {
                    return;
                }
                let Ok(resolved) = std::fs::canonicalize(&self.path) else {
                    return;
                };
                if resolved != self.canonical_path
                    || resolved.parent() != Some(self.canonical_parent.as_path())
                {
                    return;
                }
                let _ = std::fs::remove_dir_all(&resolved);
            }
        }
        let temp_parent = std::env::temp_dir();
        let canonical_parent = std::fs::canonicalize(&temp_parent).unwrap();
        let root_path = unique_temp_path(&temp_parent.join("abigail_secure_fs_long_path"));
        std::fs::create_dir(&root_path).unwrap(); // Exclusive fixture creation.
        let canonical_path = std::fs::canonicalize(&root_path).unwrap();
        assert_eq!(canonical_path.parent(), Some(canonical_parent.as_path()));
        let root = TestDirectory {
            created_at: std::fs::symlink_metadata(&root_path)
                .unwrap()
                .creation_time(),
            path: root_path,
            canonical_path,
            canonical_parent,
        };
        let mut parent = root.path.join("Unicode_\u{5bb6}\u{5ead}_\u{1f3e1}");
        while parent.as_os_str().encode_wide().count() < 330 {
            parent = parent.join("nested_directory_for_windows_atomic_replacement");
        }
        let path = parent.join("birth_certificate.json");
        assert!(path.as_os_str().encode_wide().count() > 260);
        assert!(!path.exists());

        write_string_atomic(&path, "first certificate").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first certificate");
        write_string_atomic(&path, "replacement certificate").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "replacement certificate"
        );
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 1);

        // Exercise an already extended-length input as well as the normal
        // drive path above. No registry or application manifest is changed.
        let extended = std::fs::canonicalize(&path).unwrap();
        assert!(extended
            .as_os_str()
            .encode_wide()
            .take(4)
            .eq(r"\\?\".encode_utf16()));
        write_string_atomic(&extended, "extended-path replacement").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "extended-path replacement"
        );
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_paths_preserve_relative_drive_and_unc_forms() {
        for path in [
            r"relative\certificate.json",
            r".\relative\certificate.json",
            r"C:\directory\certificate.json",
            r"\\server\share\directory\certificate.json",
            r"\\?\C:\directory\certificate.json",
            r"\\?\UNC\server\share\directory\certificate.json",
        ] {
            validate_windows_file_path(Path::new(path)).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_rejects_ambiguous_windows_paths_before_writing() {
        for path in [
            r"C:relative\certificate.json",
            r"\root_relative\certificate.json",
            r"\\.\C:\directory\certificate.json",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\certificate.json",
        ] {
            assert!(matches!(
                write_bytes_atomic(Path::new(path), b"must not be written"),
                Err(CoreError::Io(error))
                    if error.kind() == std::io::ErrorKind::InvalidInput
            ));
        }
    }
}
