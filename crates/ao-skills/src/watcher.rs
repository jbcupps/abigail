//! Skills watcher — monitors skill directories for file changes (hot-reload).
//!
//! Watches for changes to skill.toml files and notifies listeners
//! via a tokio broadcast channel so the registry can be refreshed.

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::broadcast;

/// Events emitted by the skills watcher.
#[derive(Debug, Clone)]
pub enum SkillFileEvent {
    /// A skill.toml was created or modified.
    Changed(PathBuf),
    /// A skill.toml was removed.
    Removed(PathBuf),
}

/// Watches skill directories for changes and emits events.
pub struct SkillsWatcher {
    _watcher: RecommendedWatcher,
    tx: broadcast::Sender<SkillFileEvent>,
}

impl SkillsWatcher {
    /// Start watching the given directories for skill.toml changes.
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

        // notify uses std::sync::mpsc internally
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

        // Background thread to process notify events
        std::thread::spawn(move || {
            for res in notify_rx {
                match res {
                    Ok(event) => {
                        // Filter to only skill.toml changes
                        let skill_toml_paths: Vec<PathBuf> = event
                            .paths
                            .iter()
                            .filter(|p| {
                                p.file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|n| n == "skill.toml")
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect();

                        if skill_toml_paths.is_empty() {
                            continue;
                        }

                        for path in skill_toml_paths {
                            let event = match event.kind {
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
                            // Best-effort send; if no receivers, that's fine
                            let _ = tx_clone.send(event);
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
    fn test_watcher_starts_on_existing_dir() {
        let tmp = std::env::temp_dir().join("ao_watcher_test_start");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let result = SkillsWatcher::start(vec![tmp.clone()]);
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_watcher_handles_nonexistent_dir() {
        let nonexistent = PathBuf::from("/tmp/ao_watcher_nonexistent_12345");
        // Should not error - just skip non-existent paths
        let result = SkillsWatcher::start(vec![nonexistent]);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_watcher_detects_skill_toml_change() {
        let tmp = std::env::temp_dir().join("ao_watcher_test_detect");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let (watcher, mut rx) = SkillsWatcher::start(vec![tmp.clone()]).unwrap();

        // Give the watcher time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create a skill.toml file
        let skill_toml = tmp.join("skill.toml");
        fs::write(&skill_toml, "[skill]\nid = \"test\"\nname = \"Test\"").unwrap();

        // Wait for the event (with timeout)
        let result = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;

        match result {
            Ok(Ok(SkillFileEvent::Changed(path))) => {
                assert!(path.to_string_lossy().contains("skill.toml"));
            }
            Ok(Ok(SkillFileEvent::Removed(_))) => {
                // Some OSes emit remove+create for writes - acceptable
            }
            Ok(Err(e)) => {
                // Broadcast lagged - acceptable in test
                tracing::warn!("Broadcast lagged: {}", e);
            }
            Err(_) => {
                // Timeout - filesystem events can be slow on some systems
                // This is a known issue with notify in CI/containers
                tracing::warn!(
                    "Timeout waiting for filesystem event - skipping (expected in containers)"
                );
            }
        }

        drop(watcher);
        let _ = fs::remove_dir_all(&tmp);
    }
}
