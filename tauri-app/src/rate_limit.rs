//! Simple cooldown-based rate limiter for Tauri commands.
//!
//! Designed for a single-user desktop app — no external crate needed.

use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Guards a command endpoint with a minimum interval between calls.
pub struct CooldownGuard {
    min_interval: Duration,
    last_call: Mutex<Option<Instant>>,
}

impl CooldownGuard {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_call: Mutex::new(None),
        }
    }

    /// Returns `Ok(())` if enough time has elapsed since the last call,
    /// or `Err(remaining)` with the Duration the caller must wait.
    pub async fn check(&self) -> Result<(), Duration> {
        let mut last = self.last_call.lock().await;
        let now = Instant::now();
        if let Some(prev) = *last {
            let elapsed = now.duration_since(prev);
            if elapsed < self.min_interval {
                return Err(self.min_interval - elapsed);
            }
        }
        *last = Some(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_first_call() {
        let guard = CooldownGuard::new(Duration::from_millis(500));
        assert!(guard.check().await.is_ok());
    }

    #[tokio::test]
    async fn blocks_rapid_second_call() {
        let guard = CooldownGuard::new(Duration::from_millis(500));
        guard.check().await.unwrap();
        let result = guard.check().await;
        assert!(result.is_err());
        let remaining = result.unwrap_err();
        assert!(remaining.as_millis() > 0);
    }

    #[tokio::test]
    async fn allows_after_interval() {
        let guard = CooldownGuard::new(Duration::from_millis(50));
        guard.check().await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(guard.check().await.is_ok());
    }
}
