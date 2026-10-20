//! Explicit, local Windows dictation. No audio file or recording is retained.
#[cfg(windows)]
use std::{
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

#[cfg(windows)]
static RECOGNIZER: Mutex<Option<(u64, Child)>> = Mutex::new(None);
#[cfg(windows)]
static RECOGNIZER_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
pub async fn transcribe_genesis_speech(consent: bool) -> Result<String, String> {
    if !consent {
        return Err("Microphone consent is required before listening.".into());
    }
    #[cfg(not(windows))]
    {
        Err(
            "Native dictation is available on Windows. Use browser speech or type your message."
                .into(),
        )
    }
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(recognize)
            .await
            .map_err(|_| "Dictation stopped unexpectedly".to_string())?
    }
}

#[cfg(windows)]
fn recognize() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$recognizer = $null
try {
    Add-Type -AssemblyName System.Speech
    $recognizer = New-Object System.Speech.Recognition.SpeechRecognitionEngine
    $recognizer.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
    $recognizer.SetInputToDefaultAudioDevice()
    $recognizer.InitialSilenceTimeout = [TimeSpan]::FromSeconds(12)
    $recognizer.EndSilenceTimeout = [TimeSpan]::FromMilliseconds(900)
    $result = $recognizer.Recognize([TimeSpan]::FromSeconds(20))
    if ($null -ne $result) { [Console]::Write($result.Text) }
} catch { exit 3 } finally { if ($null -ne $recognizer) { $recognizer.Dispose() } }
"#;
    let recognition_id = RECOGNIZER_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    {
        let mut guard = RECOGNIZER.lock().map_err(|_| "Dictation is unavailable")?;
        if guard.is_some() {
            return Err("A microphone session is already listening.".into());
        }
        let child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| "Windows speech recognition could not start".to_string())?;
        *guard = Some((recognition_id, child));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let mut guard = RECOGNIZER.lock().map_err(|_| "Dictation is unavailable")?;
        let (active_id, child) = guard.as_mut().ok_or("Listening was cancelled.")?;
        if *active_id != recognition_id {
            return Err("Listening was cancelled.".into());
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                let (_, child) = guard.take().unwrap();
                drop(guard);
                let output = child
                    .wait_with_output()
                    .map_err(|_| "Could not read dictation")?;
                if !output.status.success() {
                    return Err("Windows dictation is unavailable. Check your microphone permission and installed speech language, or type your message.".into());
                }
                let text = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .trim_start_matches('\u{feff}')
                    .to_string();
                return if text.is_empty() {
                    Err("No speech was heard. Try again or type your message.".into())
                } else {
                    Ok(text)
                };
            }
            Err(_) => {
                if let Some((_, mut child)) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err("Dictation could not finish. Try again.".into());
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                if let Some((_, mut child)) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err("Listening timed out. Try again.".into());
            }
            Ok(None) => {}
        }
        drop(guard);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[tauri::command]
pub fn cancel_genesis_speech() -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut guard = RECOGNIZER.lock().map_err(|_| "Dictation is unavailable")?;
        if let Some((_, mut child)) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    Ok(())
}
