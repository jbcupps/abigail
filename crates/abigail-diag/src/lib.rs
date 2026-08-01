//! Startup diagnostics: durable file logging + a panic hook.
//!
//! Shared by `hive-daemon` and `entity-daemon`. Both can be spawned by a GUI
//! shell (`windows_subsystem = "windows"`), which has no console. A child
//! spawned that way inherits invalid stdout/stderr handles, and a process that
//! logs to stdout before it binds can die silently with no console, no log,
//! and no Windows crash event. This module routes all logs to a file under a
//! path that does NOT depend on the `directories` crate or `--data-dir`
//! (either of which may be exactly what is failing), so a first-run failure
//! always leaves a readable trail.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Durable log directory, resolved independently of the `directories` crate and
/// `--data-dir`. Mirrors where the Hive runtime descriptor already lives:
/// `%LOCALAPPDATA%\Abigail\logs`. Falls back to the temp dir if unavailable.
pub fn log_dir() -> PathBuf {
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

/// Initialize file logging and a panic hook for `component` (e.g. "hive-daemon").
/// Call this as the very first line of `main()`, before any fallible work.
pub fn init(component: &str) {
    #[cfg(windows)]
    make_stdio_uninheritable();

    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);

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
        Err(_) => {
            let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        }
    }
}

/// Append a one-line fatal record directly to a file, independent of `tracing`
/// (used when `main` exits with an error, in case the subscriber is unusable).
pub fn record_fatal(component: &str, msg: &str) {
    let path = log_dir().join(format!("{component}.log"));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[fatal] {component} exited with error: {msg}");
        let _ = f.flush();
    }
}
