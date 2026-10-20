//! Preview process ownership checks. These identifiers are not authentication
//! secrets; they prevent a stale URL or descriptor from selecting another owner.

use abigail_core::profile::{ProcessIdentity, ProfileContext};
use anyhow::Context;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub const PROFILE_HEADER: &str = "x-abigail-profile-id";
pub const INSTANCE_HEADER: &str = "x-abigail-process-instance-id";

pub fn preview_port_allowed(port: u16) -> bool {
    !(43141..=43150).contains(&port)
}

pub fn validate_launch_arguments(
    profile: Option<&str>,
    root: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        profile == Some("preview") && root.is_some_and(std::path::Path::is_absolute),
        "Preview Hive requires explicit --profile preview --data-dir absolute-root arguments"
    );
    Ok(())
}

/// The open OS lock, rather than a PID or the lock file's presence, owns the
/// preview store. Closing this handle releases ownership even after a crash.
pub struct ProfileOwner {
    _file: std::fs::File,
}

impl ProfileOwner {
    pub fn acquire(profile: &ProfileContext) -> anyhow::Result<Self> {
        let directory = profile.runtime_dir();
        profile.validate_path(&directory)?;
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("hive-owner.lock");
        profile.validate_path(&path)?;
        Self::acquire_file(&path)
    }

    fn acquire_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .context("Cannot open preview Hive ownership lock")?;
        file.try_lock()
            .context("This preview profile already has a Hive owner")?;
        Ok(Self { _file: file })
    }
}

pub fn launch_identity(
    profile: &ProfileContext,
    profile_id: Option<&str>,
    process_instance: Option<&str>,
    hive_instance: Option<&str>,
) -> anyhow::Result<ProcessIdentity> {
    anyhow::ensure!(
        profile_id == Some(profile.profile_id()),
        "Preview child requires the validated ABIGAIL_PROFILE_ID"
    );
    let process_instance = process_instance
        .ok_or_else(|| anyhow::anyhow!("Preview Hive requires ABIGAIL_PROCESS_INSTANCE_ID"))?;
    let hive_instance = hive_instance
        .ok_or_else(|| anyhow::anyhow!("Preview Hive requires ABIGAIL_HIVE_INSTANCE_ID"))?;
    anyhow::ensure!(
        process_instance == hive_instance,
        "Preview Hive process and Hive instance identifiers must match"
    );
    profile.process_identity(process_instance, hive_instance)
}

pub fn validate_request_identity(expected: Option<&ProcessIdentity>, headers: &HeaderMap) -> bool {
    expected.map_or_else(
        || !headers.contains_key(PROFILE_HEADER) && !headers.contains_key(INSTANCE_HEADER),
        |identity| {
            headers
                .get(PROFILE_HEADER)
                .and_then(|value| value.to_str().ok())
                == Some(identity.profile_id.as_str())
                && headers
                    .get(INSTANCE_HEADER)
                    .and_then(|value| value.to_str().ok())
                    == Some(identity.instance_id.as_str())
        },
    )
}

pub async fn require_identity(
    State(identity): State<Option<ProcessIdentity>>,
    request: Request,
    next: Next,
) -> Response {
    // Handshakes expose only status; CORS preflight cannot execute a command.
    let handshake =
        request.method() == Method::GET && matches!(request.uri().path(), "/health" | "/v1/status");
    let stable_rejects_preview =
        identity.is_none() && !validate_request_identity(None, request.headers());
    if !stable_rejects_preview
        && (request.method() == Method::OPTIONS
            || handshake
            || validate_request_identity(identity.as_ref(), request.headers()))
    {
        return next.run(request).await;
    }
    (
        StatusCode::CONFLICT,
        axum::Json(hive_core::ApiEnvelope::<()>::error(
            "Preview process identity no longer matches this request",
        )),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            channel: "beta".into(),
            profile_id: uuid::Uuid::new_v4().to_string(),
            canonical_data_root: std::env::temp_dir().join("synthetic-preview"),
            instance_id: uuid::Uuid::new_v4().to_string(),
            hive_instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    #[test]
    fn preview_requests_require_exact_profile_and_live_instance() {
        let expected = identity();
        let mut headers = HeaderMap::new();
        assert!(!validate_request_identity(Some(&expected), &headers));
        headers.insert(PROFILE_HEADER, expected.profile_id.parse().unwrap());
        assert!(!validate_request_identity(Some(&expected), &headers));
        headers.insert(INSTANCE_HEADER, expected.instance_id.parse().unwrap());
        assert!(validate_request_identity(Some(&expected), &headers));
        headers.insert(
            INSTANCE_HEADER,
            uuid::Uuid::new_v4().to_string().parse().unwrap(),
        );
        assert!(!validate_request_identity(Some(&expected), &headers));
        headers.insert(INSTANCE_HEADER, expected.instance_id.parse().unwrap());
        headers.insert(
            PROFILE_HEADER,
            uuid::Uuid::new_v4().to_string().parse().unwrap(),
        );
        assert!(!validate_request_identity(Some(&expected), &headers));
    }

    #[test]
    fn stable_requests_keep_existing_contract() {
        assert!(validate_request_identity(None, &HeaderMap::new()));
        for key in [PROFILE_HEADER, INSTANCE_HEADER] {
            let mut headers = HeaderMap::new();
            headers.insert(key, uuid::Uuid::new_v4().to_string().parse().unwrap());
            assert!(!validate_request_identity(None, &headers));
        }
    }

    #[test]
    fn preview_ports_exclude_the_normal_daemon_range() {
        for port in 43141..=43150 {
            assert!(!preview_port_allowed(port));
        }
        for port in [0, 32123, 43140, 43151, 65535] {
            assert!(preview_port_allowed(port));
        }
    }

    #[test]
    fn preview_requires_explicit_cli_profile_and_absolute_root() {
        let root = std::env::temp_dir().join("synthetic-preview");
        assert!(validate_launch_arguments(Some("preview"), Some(&root)).is_ok());
        assert!(validate_launch_arguments(None, Some(&root)).is_err());
        assert!(validate_launch_arguments(Some("stable"), Some(&root)).is_err());
        assert!(validate_launch_arguments(Some("preview"), None).is_err());
        assert!(
            validate_launch_arguments(Some("preview"), Some(std::path::Path::new("relative")))
                .is_err()
        );
    }

    #[test]
    fn ownership_uses_live_os_lock_and_preserves_stale_file_contents() {
        let directory =
            std::env::temp_dir().join(format!("abigail-preview-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("hive-owner.lock");
        std::fs::write(&path, b"stale-pid-is-not-authority").unwrap();
        let first = ProfileOwner::acquire_file(&path).unwrap();
        assert!(ProfileOwner::acquire_file(&path).is_err());
        drop(first);
        assert_eq!(std::fs::read(&path).unwrap(), b"stale-pid-is-not-authority");
        let replacement = ProfileOwner::acquire_file(&path).unwrap();
        drop(replacement);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
