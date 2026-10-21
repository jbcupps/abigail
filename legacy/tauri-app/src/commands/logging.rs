use crate::log_capture::{self, LogBuffer, LogEntry};
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize)]
pub struct CapturedLogs {
    pub entries: Vec<LogEntry>,
    pub next_index: usize,
}

#[tauri::command]
pub fn get_log_level() -> String {
    log_capture::current_filter()
}

#[tauri::command]
pub fn set_log_level(level: String) -> Result<(), String> {
    let level = level.trim().to_string();
    if level.is_empty() {
        return Err("Filter directive cannot be empty".to_string());
    }
    log_capture::reload_filter(&level)
}

#[tauri::command]
pub fn get_captured_logs(log_buffer: State<LogBuffer>, since_index: Option<usize>) -> CapturedLogs {
    let idx = since_index.unwrap_or(0);
    let ring = log_buffer.lock().unwrap_or_else(|e| e.into_inner());
    let (a, b, next) = ring.since(idx);
    let mut entries = Vec::with_capacity(a.len() + b.len());
    entries.extend_from_slice(a);
    entries.extend_from_slice(b);
    CapturedLogs {
        entries,
        next_index: next,
    }
}

#[tauri::command]
pub fn clear_captured_logs(log_buffer: State<LogBuffer>) {
    let mut ring = log_buffer.lock().unwrap_or_else(|e| e.into_inner());
    ring.clear();
}

#[tauri::command]
pub fn export_logs(log_buffer: State<LogBuffer>) -> String {
    format_log_entries(&log_buffer)
}

#[tauri::command]
pub fn save_logs_to_file(log_buffer: State<LogBuffer>, path: String) -> Result<(), String> {
    let text = format_log_entries(&log_buffer);
    let path = PathBuf::from(path);
    std::fs::write(&path, text).map_err(|e| format!("Failed to write logs: {}", e))
}

fn format_log_entries(log_buffer: &LogBuffer) -> String {
    let ring = log_buffer.lock().unwrap_or_else(|e| e.into_inner());
    let entries = ring.all();
    let mut out = String::with_capacity(entries.len() * 120);
    for e in &entries {
        out.push_str(&format!(
            "{} {:5} [{}] {}\n",
            e.timestamp, e.level, e.target, e.message
        ));
    }
    out
}
