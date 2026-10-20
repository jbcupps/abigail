//! Opt-in expiration of Genesis records. Identity and real chat are independent.
use crate::{genesis::GenesisControl, genesis_auth::OWNER, state::HiveDaemonState};
use chrono::{DateTime, Duration, Utc};
use hive_core::genesis::GenesisSession;

/// Invalid legacy timestamps and disabled policies fail closed: keep the record.
fn expired(session: &GenesisSession, now: DateTime<Utc>) -> bool {
    let Some(days) = session
        .retention_days
        .filter(|days| (1..=3650).contains(days))
    else {
        return false;
    };
    if session.owner_id != OWNER {
        return false;
    }
    DateTime::parse_from_rfc3339(&session.updated_at)
        .ok()
        .and_then(|updated| updated.checked_add_signed(Duration::days(i64::from(days))))
        .is_some_and(|deadline| deadline <= now)
}

/// Reload after obtaining the same exclusive lock used by interactive actions.
/// A list snapshot alone must never authorize deleting a subsequently edited record.
fn try_expire(
    control: &GenesisControl,
    id: &str,
    now: DateTime<Utc>,
    load: impl FnOnce() -> Result<Option<GenesisSession>, String>,
    delete: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    let Ok(_guard) = control.lock(id) else {
        // Busy or unavailable records remain intact and are retried next minute.
        return Ok(false);
    };
    let Some(session) = load()? else {
        return Ok(false);
    };
    if session.id != id || !expired(&session, now) {
        return Ok(false);
    }
    delete()
}

fn sweep(state: &HiveDaemonState, now: DateTime<Utc>) -> Result<(usize, usize), String> {
    let sessions = state
        .genesis
        .list(OWNER)
        .map_err(|_| "Genesis retention list unavailable".to_owned())?;
    let mut deleted = 0;
    let mut failed = 0;
    for session in sessions.into_iter().filter(|session| expired(session, now)) {
        let id = session.id;
        let result = try_expire(
            &state.genesis_control,
            &id,
            now,
            || {
                state
                    .genesis
                    .load(OWNER, &id)
                    .map_err(|_| "Genesis retention record unavailable".to_owned())
            },
            || crate::genesis::delete_genesis_record_unlocked(state, &id),
        );
        match result {
            Ok(true) => deleted += 1,
            Ok(false) => (),
            Err(_) => failed += 1,
        }
    }
    Ok((deleted, failed))
}

pub fn start_genesis_retention(state: HiveDaemonState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let state = state.clone();
            // All per-record locks are acquired and dropped in synchronous work.
            match tokio::task::spawn_blocking(move || sweep(&state, Utc::now())).await {
                Ok(Ok((deleted, failed))) => {
                    if deleted > 0 {
                        tracing::info!(deleted, "Expired Genesis records removed");
                    }
                    if failed > 0 {
                        tracing::warn!(failed, "Genesis retention will retry incomplete deletions");
                    }
                }
                _ => tracing::warn!("Genesis retention unavailable; saved records retained"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::genesis::GenesisMode;
    use std::cell::Cell;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
    fn record() -> GenesisSession {
        let mut session = GenesisSession::new(
            "record".into(),
            OWNER.into(),
            GenesisMode::Guided,
            "2026-10-16T12:00:00Z".into(),
        );
        session.retention_days = Some(1);
        session
    }

    #[test]
    fn cutoff_is_exact_and_retention_is_opt_in_for_legacy_records() {
        let session = record();
        assert!(expired(&session, now()));
        assert!(!expired(&session, now() - Duration::seconds(1)));
        let mut encoded = serde_json::to_value(session).unwrap();
        encoded.as_object_mut().unwrap().remove("retention_days");
        let legacy: GenesisSession = serde_json::from_value(encoded).unwrap();
        assert_eq!(legacy.retention_days, None);
        assert!(!expired(&legacy, now()));
    }

    #[test]
    fn invalid_policy_time_or_ownership_never_authorizes_deletion() {
        for days in [0, 3651, u16::MAX] {
            let mut session = record();
            session.retention_days = Some(days);
            assert!(session.validate().is_err());
            assert!(!expired(&session, now()));
        }
        let mut session = record();
        session.updated_at = "invalid".into();
        assert!(!expired(&session, now()));
        session.updated_at = "2026-10-18T12:00:00Z".into();
        assert!(!expired(&session, now()));
        session = record();
        session.owner_id = "another-household".into();
        assert!(!expired(&session, now()));
    }

    #[test]
    fn busy_records_are_not_read_or_deleted() {
        let control = GenesisControl::default();
        let _guard = control.lock("record").unwrap();
        assert!(!try_expire(
            &control,
            "record",
            now(),
            || panic!("busy record must not be read"),
            || panic!("busy record must not be deleted")
        )
        .unwrap());
    }

    #[test]
    fn refreshed_or_disabled_records_survive_stale_list_candidates() {
        let control = GenesisControl::default();
        let mut updated = record();
        updated.updated_at = now().to_rfc3339();
        assert!(!try_expire(
            &control,
            "record",
            now(),
            || Ok(Some(updated)),
            || panic!("fresh record must survive")
        )
        .unwrap());
        let mut disabled = record();
        disabled.retention_days = None;
        assert!(!try_expire(
            &control,
            "record",
            now(),
            || Ok(Some(disabled)),
            || panic!("disabled policy must survive")
        )
        .unwrap());
    }

    #[test]
    fn deletion_runs_under_lock_and_failure_is_retryable() {
        let control = GenesisControl::default();
        let calls = Cell::new(0);
        assert!(try_expire(
            &control,
            "record",
            now(),
            || Ok(Some(record())),
            || {
                assert!(control.lock("record").is_err());
                calls.set(calls.get() + 1);
                Ok(true)
            }
        )
        .unwrap());
        assert_eq!(calls.get(), 1);
        assert!(try_expire(
            &control,
            "record",
            now(),
            || Ok(Some(record())),
            || Err("retry".into())
        )
        .is_err());
        assert!(control.lock("record").is_ok());
    }
}
