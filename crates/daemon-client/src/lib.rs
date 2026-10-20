//! HTTP clients for hive-daemon and entity-daemon.
//!
//! Used by the Tauri desktop app in Daemon runtime mode and by the
//! daemon test harness.

mod entity;
mod hive;

pub use entity::{ChatStreamEvent, EntityClient};
pub use hive::HiveDaemonClient;

fn preview_client(
    base_url: &str,
    identity: &abigail_core::profile::ProcessIdentity,
) -> anyhow::Result<reqwest::Client> {
    let url = reqwest::Url::parse(base_url)?;
    let port = url
        .port()
        .filter(|port| *port != 0 && !(43141..=43150).contains(port))
        .ok_or_else(|| anyhow::anyhow!("Preview daemon requires an explicit local port"))?;
    anyhow::ensure!(
        base_url == format!("http://127.0.0.1:{port}"),
        "Preview daemon requires its exact canonical loopback endpoint"
    );
    Ok(reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(preview_headers(identity)?)
        .build()?)
}

fn preview_headers(
    identity: &abigail_core::profile::ProcessIdentity,
) -> anyhow::Result<reqwest::header::HeaderMap> {
    for value in [
        &identity.profile_id,
        &identity.instance_id,
        &identity.hive_instance_id,
    ] {
        abigail_core::profile::validate_uuid(value)?;
    }
    anyhow::ensure!(
        identity.channel == "beta" && identity.canonical_data_root.is_absolute(),
        "Invalid preview process identity"
    );
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-abigail-profile-id", identity.profile_id.parse()?);
    headers.insert(
        "x-abigail-process-instance-id",
        identity.instance_id.parse()?,
    );
    Ok(headers)
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn identity() -> abigail_core::profile::ProcessIdentity {
        abigail_core::profile::ProcessIdentity {
            channel: "beta".into(),
            profile_id: abigail_core::profile::new_instance_id(),
            canonical_data_root: std::env::temp_dir().join("synthetic-preview"),
            instance_id: abigail_core::profile::new_instance_id(),
            hive_instance_id: abigail_core::profile::new_instance_id(),
        }
    }

    #[test]
    fn preview_clients_pin_exact_request_identity() {
        let expected = identity();
        let headers = preview_headers(&expected).unwrap();
        assert_eq!(headers["x-abigail-profile-id"], expected.profile_id);
        assert_eq!(
            headers["x-abigail-process-instance-id"],
            expected.instance_id
        );
        assert_ne!(
            headers["x-abigail-process-instance-id"],
            expected.hive_instance_id
        );
    }

    #[test]
    fn preview_clients_reject_endpoint_ambiguity_and_invalid_identity() {
        let expected = identity();
        for url in [
            "http://localhost:32123",
            "http://127.0.0.1:0",
            "http://127.0.0.1:32123/",
            "http://127.0.0.1:32123?x=1",
            "https://127.0.0.1:32123",
        ] {
            assert!(preview_client(url, &expected).is_err(), "{url}");
        }
        let mut invalid = expected;
        invalid.instance_id = "not-an-instance".into();
        assert!(preview_client("http://127.0.0.1:32123", &invalid).is_err());
    }
}
