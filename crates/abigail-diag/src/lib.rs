//! Startup diagnostics: durable file logging + a panic hook.
//!
//! Shared by `hive-daemon` and `entity-daemon`. Both can be spawned by a GUI
//! shell (`windows_subsystem = "windows"`), which has no console. A child
//! spawned that way inherits invalid stdout/stderr handles, and a process that
//! logs to stdout before it binds can die silently with no console, no log,
//! and no Windows crash event. This module routes all logs to a file under a
//! stable fallback path independent of the `directories` crate. Preview
//! entrypoints must validate their profile first and supply its log directory;
//! that directory remains fixed for all later diagnostic writes.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

static LOG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

/// The fixed process log directory, or the historical stable default before
/// initialization. Preview entrypoints must initialize their validated path first.
pub fn log_dir() -> PathBuf {
    LOG_DIRECTORY.get().cloned().unwrap_or_else(stable_log_dir)
}

fn stable_log_dir() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            return PathBuf::from(local).join("Abigail").join("logs");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".abigail").join("logs");
        }
    }
    std::env::temp_dir().join("Abigail").join("logs")
}

/// A cloneable, synchronous file writer so `tracing` flushes each line before
/// the process can exit (no background thread that could lose buffered output).
#[derive(Clone)]
struct FileWriter(Arc<Mutex<std::fs::File>>);

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log file mutex poisoned").write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("log file mutex poisoned").flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileWriter {
    type Writer = FileWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// On Windows, std handles inherited from a parent keep their
/// `HANDLE_FLAG_INHERIT` flag, and Rust spawns children with
/// `bInheritHandles=TRUE` (required to wire stdio). So when a GUI shell pipes
/// our stdout to parse the listen line, every child we spawn — even with
/// `Stdio::null()` — leaks a raw copy of that pipe handle and keeps it open
/// after we exit, hanging any reader that waits for EOF. Clearing the inherit
/// flag on our own std handles stops the leak at the source.
#[cfg(windows)]
fn make_stdio_uninheritable() {
    use windows_sys::Win32::Foundation::{
        SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Initialize stable file logging and a panic hook for `component`.
/// Preview entrypoints must finish profile preflight and use `init_with_log_dir`.
pub fn init(component: &str) {
    let dir = LOG_DIRECTORY.get_or_init(stable_log_dir);
    let _ = init_in_dir(component, dir, false);
}

/// Initialize diagnostics only inside an already validated profile log directory.
/// Missing/unwritable preview paths return an error without stable-path fallback.
pub fn init_with_log_dir(component: &str, logs: &Path) -> std::io::Result<()> {
    latch_log_dir(&LOG_DIRECTORY, logs)?;
    init_in_dir(component, logs, true)
}

fn latch_log_dir(slot: &OnceLock<PathBuf>, logs: &Path) -> std::io::Result<()> {
    if !logs.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Explicit diagnostic log directory must be absolute.",
        ));
    }
    let selected = slot.get_or_init(|| logs.to_path_buf());
    if selected != logs {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Diagnostic log directory is already fixed for this process.",
        ));
    }
    Ok(())
}

fn init_in_dir(component: &str, dir: &Path, strict: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    make_stdio_uninheritable();

    let create_result = std::fs::create_dir_all(dir);
    if strict {
        create_result?;
    }

    // Panic hook: capture panics (which would otherwise vanish with no console)
    // to a sibling file, then chain to the default hook.
    let panic_path = dir.join(format!("{component}.panic.log"));
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&panic_path)
        {
            let _ = writeln!(f, "[panic] {info}");
            let _ = writeln!(f, "{}", std::backtrace::Backtrace::force_capture());
            let _ = f.flush();
        }
        default_hook(info);
    }));

    let default_directives = format!(
        "{}=debug,abigail_identity=debug,abigail_hive=debug,abigail_core=debug,hive_daemon=debug,entity_daemon=debug",
        component.replace('-', "_")
    );
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_directives));

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{component}.log")))
    {
        Ok(file) => {
            let writer = FileWriter(Arc::new(Mutex::new(file)));
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(writer)
                .try_init();
        }
        // Last resort: if even the log file can't be opened, fall back to the
        // (possibly invalid) stdout so we at least don't change prior behavior.
        Err(error) => {
            if strict {
                return Err(error);
            }
            let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        }
    }
    Ok(())
}

/// Append a one-line fatal record directly to a file, independent of `tracing`
/// (used when `main` exits with an error, in case the subscriber is unusable).
pub fn record_fatal(component: &str, msg: &str) {
    record_fatal_in_dir(&log_dir(), component, msg);
}

fn record_fatal_in_dir(dir: &Path, component: &str, msg: &str) {
    let path = dir.join(format!("{component}.log"));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[fatal] {component} exited with error: {msg}");
        let _ = f.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_log_directory_is_immutable_without_global_environment_changes() {
        let slot = OnceLock::new();
        let first = std::env::temp_dir().join("preview-diag-first");
        let other = std::env::temp_dir().join("preview-diag-other");
        latch_log_dir(&slot, &first).unwrap();
        latch_log_dir(&slot, &first).unwrap();
        assert!(latch_log_dir(&slot, &other).is_err());
        assert_eq!(slot.get(), Some(&first));
    }

    #[test]
    fn relative_log_directory_is_rejected_without_latching_a_fallback() {
        let slot = OnceLock::new();
        assert!(latch_log_dir(&slot, Path::new("relative/logs")).is_err());
        assert!(slot.get().is_none());
    }

    #[test]
    fn fatal_record_uses_only_the_selected_synthetic_directory() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture =
            std::env::temp_dir().join(format!("preview-diag-{}-{suffix}", std::process::id()));
        let logs = fixture.join("preview/logs");
        let stable = fixture.join("stable-sentinel");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join("keep.txt"), b"synthetic stable sentinel").unwrap();
        record_fatal_in_dir(&logs, "fixture", "synthetic failure");
        assert_eq!(
            std::fs::read_to_string(logs.join("fixture.log")).unwrap(),
            "[fatal] fixture exited with error: synthetic failure\n"
        );
        assert_eq!(
            std::fs::read(stable.join("keep.txt")).unwrap(),
            b"synthetic stable sentinel"
        );
        assert_eq!(std::fs::read_dir(&stable).unwrap().count(), 1);
        std::fs::remove_dir_all(fixture).unwrap();
    }
}
