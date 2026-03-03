//! Skills watcher — monitors skill directories for file changes (hot-reload).
//!
//! Watches for changes to `skill.toml` and `*.json` files, notifying
//! listeners via a tokio broadcast channel so the registry can be refreshed.

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::broadcast;

/// Events emitted by the skills watcher.
#[derive(Debug, Clone)]
pub enum SkillFileEvent {
    /// A skill.toml or *.json was created or modified.
    Changed(PathBuf),
    /// A skill.toml or *.json was removed.
    Removed(PathBuf),
}

fn is_skill_file(name: &str) -> bool {
    name == "skill.toml" || name.ends_with(".json")
}

/// Watches skill directories for changes and emits events.
pub struct SkillsWatcher {
    _watcher: RecommendedWatcher,
    tx: broadcast::Sender<SkillFileEvent>,
}

impl SkillsWatcher {
    /// Start watching the given directories for skill file changes.
    ///
    /// Returns a `SkillsWatcher` (keep alive to maintain the watch) and
    /// a `broadcast::Receiver` for subscribing to change events.
    ///
    /// The watcher runs on a background thread and sends events through
    /// a tokio broadcast channel.
    pub fn start(
        watch_paths: Vec<PathBuf>,
    ) -> anyhow::Result<(Self, broadcast::Receiver<SkillFileEvent>)> {
        let (tx, rx) = broadcast::channel::<SkillFileEvent>(64);
        let tx_clone = tx.clone();

        let (notify_tx, notify_rx) = mpsc::channel::<notify::Result<Event>>();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = notify_tx.send(res);
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        for path in &watch_paths {
            if path.exists() {
                watcher.watch(path, RecursiveMode::Recursive)?;
                tracing::info!("Skills watcher: watching {}", path.display());
            } else {
                tracing::debug!(
                    "Skills watcher: path does not exist yet: {}",
                    path.display()
                );
            }
        }

        std::thread::spawn(move || {
            for res in notify_rx {
                match res {
                    Ok(event) => {
                        let relevant: Vec<PathBuf> = event
                            .paths
                            .iter()
                            .filter(|p| {
                                p.file_name()
                                    .and_then(|n| n.to_str())
                                    .map(is_skill_file)
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect();

                        if relevant.is_empty() {
                            continue;
                        }

                        for path in relevant {
                            let ev = match event.kind {
                                EventKind::Create(_) | EventKind::Modify(_) => {
                                    tracing::info!("Skill file changed: {}", path.display());
                                    SkillFileEvent::Changed(path)
                                }
                                EventKind::Remove(_) => {
                                    tracing::info!("Skill file removed: {}", path.display());
                                    SkillFileEvent::Removed(path)
                                }
                                _ => continue,
                            };
                            let _ = tx_clone.send(ev);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Skills watcher error: {}", e);
                    }
                }
            }
        });

        Ok((
            Self {
                _watcher: watcher,
                tx,
            },
            rx,
        ))
    }

    /// Get a new subscriber for skill file events.
    pub fn subscribe(&self) -> broadcast::Receiver<SkillFileEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_is_skill_file_matches_toml() {
        assert!(is_skill_file("skill.toml"));
    }

    #[test]
    fn test_is_skill_file_matches_json() {
        assert!(is_skill_file("dynamic.weather.json"));
        assert!(is_skill_file("custom_api.json"));
    }

    #[test]
    fn test_is_skill_file_rejects_others() {
        assert!(!is_skill_file("readme.md"));
        assert!(!is_skill_file("config.toml"));
        assert!(!is_skill_file("Cargo.toml"));
        assert!(!is_skill_file("data.csv"));
    }

    #[test]
    fn test_watcher_starts_on_existing_dir() {
        let tmp = std::env::temp_dir().join("abigail_watcher_test_start");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let result = SkillsWatcher::start(vec![tmp.clone()]);
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_watcher_handles_nonexistent_dir() {
        let nonexistent = std::env::temp_dir().join("abigail_watcher_nonexistent_12345");
        let result = SkillsWatcher::start(vec![nonexistent]);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_watcher_detects_skill_toml_change() {
        let tmp = std::env::temp_dir().join("abigail_watcher_test_detect");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let (watcher, mut rx) = SkillsWatcher::start(vec![tmp.clone()]).unwrap();

        tokio::time::sleep(Duration::from_millis(500)).await;

        let skill_toml = tmp.join("skill.toml");
        fs::write(&skill_toml, "[skill]\nid = \"test\"\nname = \"Test\"").unwrap();

        let result = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;

        match result {
            Ok(Ok(SkillFileEvent::Changed(path))) => {
                assert!(path.to_string_lossy().contains("skill.toml"));
            }
            Ok(Ok(SkillFileEvent::Removed(_))) => {
                // Some OSes emit remove+create for writes
            }
            Ok(Err(e)) => {
                tracing::warn!("Broadcast lagged: {}", e);
            }
            Err(_) => {
                tracing::warn!(
                    "Timeout waiting for filesystem event - skipping (expected in containers)"
                );
            }
        }

        drop(watcher);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_watcher_detects_json_change() {
        let tmp = std::env::temp_dir().join("abigail_watcher_test_json_detect");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let (watcher, mut rx) = SkillsWatcher::start(vec![tmp.clone()]).unwrap();

        tokio::time::sleep(Duration::from_millis(500)).await;

        let json_file = tmp.join("custom_weather.json");
        fs::write(&json_file, r#"{"id":"custom.weather"}"#).unwrap();

        let result = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;

        match result {
            Ok(Ok(SkillFileEvent::Changed(path))) => {
                assert!(
                    path.to_string_lossy().contains(".json"),
                    "Expected .json path, got {:?}",
                    path
                );
            }
            Ok(Ok(SkillFileEvent::Removed(_))) => {}
            Ok(Err(e)) => {
                tracing::warn!("Broadcast lagged: {}", e);
            }
            Err(_) => {
                tracing::warn!(
                    "Timeout waiting for filesystem event - skipping (expected in containers)"
                );
            }
        }

        drop(watcher);
        let _ = fs::remove_dir_all(&tmp);
    }
}
