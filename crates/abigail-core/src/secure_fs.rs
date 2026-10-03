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
    // A reader or another replacement can briefly hold the destination on
    // Windows. Keep the same synced temporary file and retry only those
    // sharing/access failures; permanent failures still reach the caller.
    retry_windows_atomic_replace(|| unsafe {
        MoveFileExW(
            PCWSTR(src_wide.as_ptr()),
            PCWSTR(dest_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    })
    .map_err(|e| {
        CoreError::Io(std::io::Error::other(format!(
            "Atomic replace failed for '{}' -> '{}': {}",
            src.display(),
            dest.display(),
            e
        )))
    })?;
    Ok(())
}

#[cfg(windows)]
fn retry_windows_atomic_replace(
    mut replace: impl FnMut() -> windows::core::Result<()>,
) -> windows::core::Result<()> {
    use std::time::{Duration, Instant};
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut delay = Duration::from_millis(10);
    loop {
        match replace() {
            Ok(()) => return Ok(()),
            Err(error) => {
                let code = error.code();
                if code != HRESULT::from_win32(ERROR_ACCESS_DENIED.0)
                    && code != HRESULT::from_win32(ERROR_SHARING_VIOLATION.0)
                {
                    return Err(error);
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(error);
                }
                std::thread::sleep(delay.min(remaining));
                if Instant::now() >= deadline {
                    return Err(error);
                }
                delay = delay.saturating_mul(2).min(Duration::from_millis(100));
            }
        }
    }
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

    #[cfg(windows)]
    struct TestDirectory {
        path: PathBuf,
        canonical_path: PathBuf,
        canonical_parent: PathBuf,
        created_at: u64,
    }

    #[cfg(windows)]
    impl TestDirectory {
        fn new(name: &str) -> Self {
            use std::os::windows::fs::MetadataExt;

            let temp_parent = std::env::temp_dir();
            let canonical_parent = std::fs::canonicalize(&temp_parent).unwrap();
            let path = unique_temp_path(&temp_parent.join(name));
            std::fs::create_dir(&path).unwrap(); // Exclusive fixture creation.
            let canonical_path = std::fs::canonicalize(&path).unwrap();
            assert_eq!(canonical_path.parent(), Some(canonical_parent.as_path()));
            Self {
                created_at: std::fs::symlink_metadata(&path).unwrap().creation_time(),
                path,
                canonical_path,
                canonical_parent,
            }
        }
    }

    #[cfg(windows)]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            use std::os::windows::fs::MetadataExt;
            use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

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

        // Own a unique directory so concurrent test processes cannot delete
        // each other's files. Revalidate its resolved identity before cleanup.
        let root = TestDirectory::new("abigail_secure_fs_long_path");
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
    fn windows_atomic_replace_retries_until_sharing_handle_is_released() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::sync::mpsc;
        use std::time::Duration;
        use windows::core::{HRESULT, PCWSTR};
        use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, FILE_SHARE_READ, FILE_SHARE_WRITE, MOVEFILE_REPLACE_EXISTING,
            MOVEFILE_WRITE_THROUGH,
        };

        let root = TestDirectory::new("abigail_secure_fs_sharing_release");
        let path = root.path.join("value.bin");
        let temp = root.path.join("replacement.tmp");
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&temp, b"replacement").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0) // Deliberately no DELETE share.
            .open(&path)
            .unwrap();
        let source = path_to_wide(&std::fs::canonicalize(&temp).unwrap());
        let destination = path_to_wide(&std::fs::canonicalize(&path).unwrap());

        // Release only after the real MoveFileExW has failed with the held
        // handle. This proves a retry occurred without depending on sleeps.
        let (release, release_requested) = mpsc::sync_channel(1);
        let releaser = std::thread::spawn(move || {
            release_requested
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            drop(held);
        });
        let mut release = Some(release);
        let mut attempts = 0;
        let result = retry_windows_atomic_replace(|| {
            attempts += 1;
            let result = unsafe {
                MoveFileExW(
                    PCWSTR(source.as_ptr()),
                    PCWSTR(destination.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if let Some(release) = release.take() {
                let error = result
                    .as_ref()
                    .expect_err("held handle must block replacement");
                assert!(
                    error.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0)
                        || error.code() == HRESULT::from_win32(ERROR_SHARING_VIOLATION.0)
                );
                assert_eq!(std::fs::read(&path).unwrap(), b"original");
                assert_eq!(std::fs::read(&temp).unwrap(), b"replacement");
                release.send(()).unwrap();
            }
            result
        });
        releaser.join().unwrap();
        result.unwrap();
        assert!(attempts >= 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        assert!(!temp.exists());
        assert_eq!(std::fs::read_dir(&root.path).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_permanent_windows_lock_fails_bounded_without_losing_bytes() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::time::{Duration, Instant};
        use windows::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let root = TestDirectory::new("abigail_secure_fs_permanent_lock");
        let path = root.path.join("value.bin");
        write_bytes_atomic(&path, b"original").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .open(&path)
            .unwrap();
        let started = Instant::now();
        let error = write_bytes_atomic(&path, b"replacement").unwrap_err();
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(error.to_string().contains("Atomic replace failed"));
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&root.path).unwrap().count(), 1);

        drop(held);
        write_bytes_atomic(&path, b"replacement").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        assert_eq!(std::fs::read_dir(&root.path).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn atomic_windows_writers_preserve_complete_payloads_on_the_same_target() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let root = TestDirectory::new("abigail_secure_fs_concurrent_writers");
        let path = root.path.join("secrets.vault");
        let payloads = Arc::new(
            (0..5)
                .map(|index| vec![b'A' + index; 64 * 1024])
                .collect::<Vec<_>>(),
        );
        write_bytes_atomic(&path, &payloads[0]).unwrap();
        let remaining = Arc::new(AtomicUsize::new(4));
        let start = Arc::new(Barrier::new(6)); // Four writers, reader, parent.
        let mut writers = Vec::new();
        for index in 1..5 {
            let path = path.clone();
            let payloads = payloads.clone();
            let remaining = remaining.clone();
            let start = start.clone();
            writers.push(std::thread::spawn(
                move || -> std::result::Result<(), String> {
                    start.wait();
                    let result = (0..16).try_for_each(|_| {
                        write_bytes_atomic(&path, &payloads[index])
                            .map_err(|error| error.to_string())
                    });
                    remaining.fetch_sub(1, Ordering::SeqCst);
                    result
                },
            ));
        }
        let reader = {
            let path = path.clone();
            let payloads = payloads.clone();
            let remaining = remaining.clone();
            let start = start.clone();
            std::thread::spawn(move || -> std::result::Result<(), String> {
                start.wait();
                let mut reads = 0;
                while remaining.load(Ordering::SeqCst) > 0 || reads < 150 {
                    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                    if !payloads.iter().any(|payload| payload == &bytes) {
                        return Err("Reader observed a partial or mixed payload".to_string());
                    }
                    reads += 1;
                }
                Ok(())
            })
        };
        start.wait();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        reader.join().unwrap().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(payloads[1..].iter().any(|payload| payload == &bytes));
        assert_eq!(std::fs::read_dir(&root.path).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn windows_atomic_retry_does_not_retry_other_win32_errors() {
        use windows::core::{Error, HRESULT};
        use windows::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_PATH_NOT_FOUND, ERROR_SHARING_VIOLATION,
        };

        let mut attempts = 0;
        let error = retry_windows_atomic_replace(|| {
            let code = [
                ERROR_ACCESS_DENIED,
                ERROR_SHARING_VIOLATION,
                ERROR_PATH_NOT_FOUND,
            ][attempts];
            attempts += 1;
            Err(Error::from(HRESULT::from_win32(code.0)))
        })
        .unwrap_err();
        assert_eq!(attempts, 3);
        assert_eq!(error.code(), HRESULT::from_win32(ERROR_PATH_NOT_FOUND.0));
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
