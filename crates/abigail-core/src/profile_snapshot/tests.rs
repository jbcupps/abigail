//! Synthetic filesystem transaction tests. Actual shared-store fresh-process
//! recovery is exercised separately in the persistence crate.
#![cfg(windows)]

use super::*;
use crate::profile::new_instance_id;
use crate::profile_lease::ProfileUseLease;
use crate::vault::crypto;
use crate::vault::unlock::{ProvidedRestoreCredentialProbe, UnlockProvider};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const FIXTURE_KEK: [u8; 32] = [0x39; 32];
const STORE_BYTES: &[u8] = b"abc";
const STORE_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct FakeUnlock(Option<[u8; 32]>);

impl UnlockProvider for FakeUnlock {
    fn root_kek(&self) -> crate::Result<[u8; 32]> {
        self.0.ok_or_else(|| {
            crate::CoreError::Vault("Synthetic external credential is unavailable".into())
        })
    }
}

fn probe(key: Option<[u8; 32]>) -> ProvidedRestoreCredentialProbe {
    ProvidedRestoreCredentialProbe::new(Arc::new(FakeUnlock(key)))
}

struct Fixture {
    container: PathBuf,
    source: PathBuf,
    bundle: PathBuf,
    destination: PathBuf,
    profile_id: String,
    entity_id: String,
    creation_key: String,
    master_public_key: [u8; 32],
}

impl Fixture {
    fn new() -> Self {
        let container =
            std::env::temp_dir().join(format!("cold-snapshot-io-{}", new_instance_id()));
        fs::create_dir(&container).unwrap();
        let profile = ProfileContext::preflight(&container.join("source"), &[]).unwrap();
        let source = profile.root().to_path_buf();
        let entity_id = new_instance_id();
        let creation_key = new_instance_id();
        let master = ed25519_dalek::SigningKey::from_bytes(&[0x41; 32]);
        let fixture = Self {
            bundle: container.join("snapshot"),
            destination: container.join("restored"),
            profile_id: profile.profile_id().to_string(),
            master_public_key: master.verifying_key().to_bytes(),
            container,
            source,
            entity_id,
            creation_key,
        };
        let namespace = format!("preview.{}", fixture.profile_id);
        fixture.write(
            "vault.sentinel",
            &crypto::seal(
                &crypto::derive_scope_key(&FIXTURE_KEK, &format!("{namespace}:vault:sentinel:v1")),
                b"ABIGAIL_VAULT_v1_2026-10-20T00:00:00+00:00",
            )
            .unwrap(),
        );
        fixture.write(
            "master.key",
            &crypto::seal(
                &crypto::derive_scope_key(&FIXTURE_KEK, &format!("{namespace}:keyring:master")),
                &serde_json::to_vec(&json!({"secret": master.to_bytes().to_vec()})).unwrap(),
            )
            .unwrap(),
        );
        fixture.write_json(
            "global_settings.json",
            json!({
                "master_key_path": fixture.source.join("master.key"),
                "hive_secrets_path": fixture.source.join("hive_secrets/secrets.vault"),
                "agents": [{
                    "id": fixture.entity_id, "name": "Signal", "is_hive": false,
                    "directory": format!("identities/{}", fixture.entity_id),
                }],
                "creation_attempts": {
                    fixture.creation_key.clone(): {
                        "name": "Signal", "entity_id": fixture.entity_id, "state": "complete",
                    },
                },
                "opaque_extension": {"source_text": fixture.source},
            }),
        );
        let data = fixture.source.join("identities").join(&fixture.entity_id);
        fixture.write_json(
            &fixture.config_relative(),
            json!({
                "schema_version": crate::CONFIG_SCHEMA_VERSION,
                "data_dir": data, "docs_dir": data.join("docs"),
                "models_dir": data.join("models"), "db_path": fixture.source.join("memory.db"),
                "birth_complete": false, "is_hive": false, "agent_name": "Signal",
                "external_pubkey_path": data.join("external_pubkey.bin"),
                "opaque_extension": {"literal_path": fixture.source},
            }),
        );
        fixture.write("memory.db/segment", STORE_BYTES);
        fixture.write(
            &format!("identities/{}/docs/soul.md", fixture.entity_id),
            b"# Synthetic Signal\nConstitution remains unchanged.\n",
        );
        fixture.write_json(
            &format!("identities/{}/presence.json", fixture.entity_id),
            json!({"appearance": "aether", "motion": "reduced"}),
        );
        fixture.write(
            &format!("Documents/{}/mentor-note.md", fixture.entity_id),
            b"Synthetic user document.\n",
        );
        fixture.write(
            "webview/hive/Default/Local Storage/creation-journal.bin",
            b"\0synthetic exact-key journal\xff",
        );
        fixture.write(
            "docs/logs/keep.md",
            b"Nested user logs are persistent documents.",
        );
        fixture.write("future-payload.dat", b"Opaque persistent bytes.");
        for excluded in ["runtime", "Logs", "TMP", "Backups"] {
            fixture.write(
                &format!("{excluded}/excluded.txt"),
                b"Transient or prior-backup bytes must stay out.",
            );
        }
        ProfileContext::inspect_existing(&fixture.source, &[]).unwrap();
        fixture
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.source.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn write_json(&self, relative: &str, value: Value) {
        self.write(relative, &serde_json::to_vec_pretty(&value).unwrap());
    }

    fn config_relative(&self) -> String {
        format!("identities/{}/config.json", self.entity_id)
    }

    fn snapshot(&self) -> SnapshotManifest {
        snapshot(&self.source, &self.bundle, &[]).unwrap()
    }

    fn restore(&self) -> Result<RestoreReceipt> {
        restore(
            &self.bundle,
            &self.destination,
            &[],
            &probe(Some(FIXTURE_KEK)),
        )
    }

    fn manifest(&self) -> Value {
        read_json(&self.bundle.join("manifest.json"))
    }

    fn update_manifest(&self, update: impl FnOnce(&mut Value)) {
        let mut manifest = self.manifest();
        update(&mut manifest);
        fs::write(
            self.bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn assert_no_partial(&self) {
        assert!(
            fs::read_dir(&self.container).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".partial")
            }),
            "an unsuccessful transaction left its own staging directory"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this uniquely created synthetic fixture is eligible for cleanup.
        let _ = fs::remove_dir_all(&self.container);
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn tree_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeMap::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

#[test]
fn cold_snapshot_preserves_genesis_capability_assets_and_signed_narrative_as_opaque_bytes() {
    let f = Fixture::new();
    let narrative = format!("identities/{}/docs/genesis.json", f.entity_id);
    for (path, bytes) in [
        (
            "genesis.token",
            b"public-synthetic-capability-fixture".as_slice(),
        ),
        (
            "genesis_assets/fixture/portrait.png",
            b"synthetic-not-an-image".as_slice(),
        ),
        (
            narrative.as_str(),
            br#"{"name":"Vesper","purpose":"Explore","revision":3}"#.as_slice(),
        ),
    ] {
        f.write(path, bytes);
    }
    let manifest = f.snapshot();
    f.restore().unwrap();
    for path in [
        "genesis.token",
        "genesis_assets/fixture/portrait.png",
        narrative.as_str(),
    ] {
        assert!(manifest.entries.iter().any(|entry| entry.path == path));
        assert_eq!(
            fs::read(f.source.join(path)).unwrap(),
            fs::read(f.destination.join(path)).unwrap()
        );
    }
    assert_eq!(
        read_json(&f.source.join("global_settings.json"))["creation_attempts"],
        read_json(&f.destination.join("global_settings.json"))["creation_attempts"]
    );
}

#[test]
fn cold_snapshot_io_preserves_uuid_receipts_documents_webview_and_rebases_only_paths() {
    let f = Fixture::new();
    let marker = fs::read(f.source.join("profile.json")).unwrap();
    let registry = read_json(&f.source.join("global_settings.json"));
    let config = read_json(&f.source.join(f.config_relative()));
    let source_files = tree_files(&f.source);
    let manifest = f.snapshot();
    assert_eq!(manifest.profile_id, f.profile_id);
    assert_eq!(manifest.entity_ids, vec![f.entity_id.clone()]);
    assert_eq!(manifest.creation_receipt_count, 1);
    let segment = manifest
        .entries
        .iter()
        .find(|e| e.path == "memory.db/segment")
        .unwrap();
    assert_eq!(segment.length, 3);
    assert_eq!(segment.sha256.as_deref(), Some(STORE_SHA256));
    assert!(manifest
        .credential_policy
        .contains("external-keyring-not-included"));
    assert!(manifest.entries.iter().all(|e| {
        !["runtime", "logs", "tmp", "backups"].contains(
            &e.path
                .split('/')
                .next()
                .unwrap()
                .to_ascii_lowercase()
                .as_str(),
        )
    }));
    assert!(manifest
        .entries
        .iter()
        .any(|e| e.path == "docs/logs/keep.md"));
    assert_eq!(verify_snapshot(&f.bundle).unwrap(), manifest);
    let bundle_before = tree_files(&f.bundle);

    let receipt = f.restore().unwrap();
    assert_eq!(receipt.profile_id, f.profile_id);
    assert_eq!(receipt.entity_ids, vec![f.entity_id.clone()]);
    assert_eq!(receipt.creation_receipt_count, 1);
    assert_eq!(
        receipt.credential_verification.master_public_key,
        f.master_public_key
    );
    assert!(!receipt.runtime_descriptors_restored);
    assert!(receipt.original_source_present_and_locked);
    assert_eq!(
        fs::read(f.destination.join("profile.json")).unwrap(),
        marker
    );
    let canonical_destination = canonical_without_links(&f.destination).unwrap();
    let restored_config = read_json(&f.destination.join(f.config_relative()));
    let restored_registry = read_json(&f.destination.join("global_settings.json"));
    let data = canonical_destination.join("identities").join(&f.entity_id);
    assert_eq!(restored_config["data_dir"], json!(data));
    assert_eq!(restored_config["docs_dir"], json!(data.join("docs")));
    assert_eq!(restored_config["models_dir"], json!(data.join("models")));
    assert_eq!(
        restored_config["db_path"],
        json!(canonical_destination.join("memory.db"))
    );
    assert_eq!(
        restored_config["external_pubkey_path"],
        json!(data.join("external_pubkey.bin"))
    );
    assert_eq!(
        restored_config["opaque_extension"],
        config["opaque_extension"]
    );
    assert_eq!(restored_registry["agents"], registry["agents"]);
    assert_eq!(
        restored_registry["creation_attempts"],
        registry["creation_attempts"]
    );
    assert_eq!(
        restored_registry["opaque_extension"],
        registry["opaque_extension"]
    );
    assert_eq!(
        restored_registry["master_key_path"],
        json!(canonical_destination.join("master.key"))
    );
    assert_eq!(
        restored_registry["hive_secrets_path"],
        json!(canonical_destination.join("hive_secrets/secrets.vault"))
    );
    for relative in [
        PathBuf::from("memory.db/segment"),
        PathBuf::from(format!("Documents/{}/mentor-note.md", f.entity_id)),
        PathBuf::from(format!("identities/{}/presence.json", f.entity_id)),
        PathBuf::from(format!("identities/{}/docs/soul.md", f.entity_id)),
        PathBuf::from("webview/hive/Default/Local Storage/creation-journal.bin"),
        PathBuf::from("docs/logs/keep.md"),
        PathBuf::from("future-payload.dat"),
        PathBuf::from("master.key"),
        PathBuf::from("vault.sentinel"),
    ] {
        assert_eq!(
            fs::read(f.destination.join(&relative)).unwrap(),
            source_files[&relative],
            "{} changed",
            relative.display()
        );
    }
    for excluded in ["runtime", "logs", "tmp", "backups"] {
        assert!(!f.destination.join(excluded).exists());
    }
    let reopened = ProfileContext::inspect_existing(&f.destination, &[]).unwrap();
    assert_eq!(reopened.profile_id(), f.profile_id);
    assert_eq!(tree_files(&f.bundle), bundle_before);
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_restore_does_not_require_original_root_to_exist() {
    let f = Fixture::new();
    f.snapshot();
    fs::rename(&f.source, f.container.join("retired-source")).unwrap();
    assert!(!f.source.exists());
    assert_eq!(verify_snapshot(&f.bundle).unwrap().profile_id, f.profile_id);
    let receipt = f.restore().unwrap();
    assert_eq!(receipt.profile_id, f.profile_id);
    assert!(!receipt.original_source_present_and_locked);
    assert_eq!(
        fs::read(f.destination.join("memory.db/segment")).unwrap(),
        STORE_BYTES
    );
    assert!(!f.source.exists());
}

#[test]
fn cold_snapshot_io_refuses_live_profile_lease_then_succeeds_after_release() {
    let f = Fixture::new();
    let profile = ProfileContext::inspect_existing(&f.source, &[]).unwrap();
    let lease = ProfileUseLease::acquire(&profile).unwrap();
    let marker = fs::read(f.source.join("profile.json")).unwrap();
    let error = snapshot(&f.source, &f.bundle, &[]).unwrap_err();
    assert!(format!("{error:#}").contains("in use"));
    assert!(!f.bundle.exists());
    assert_eq!(fs::read(f.source.join("profile.json")).unwrap(), marker);
    drop(lease);
    assert_eq!(f.snapshot().profile_id, f.profile_id);
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_restore_refuses_a_live_original_owner_without_publishing_clone() {
    let f = Fixture::new();
    f.snapshot();
    let before = tree_files(&f.bundle);
    let profile = ProfileContext::inspect_existing(&f.source, &[]).unwrap();
    let lease = ProfileUseLease::acquire(&profile).unwrap();
    let error = f.restore().unwrap_err();
    assert!(format!("{error:#}").contains("original preview must be stopped"));
    assert!(!f.destination.exists());
    assert_eq!(tree_files(&f.bundle), before);
    drop(lease);
    assert!(f.restore().unwrap().original_source_present_and_locked);
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_valid_bundle_recovers_without_repairing_damaged_original_config() {
    let f = Fixture::new();
    f.snapshot();
    fs::write(
        f.source.join(f.config_relative()),
        b"{damaged original config",
    )
    .unwrap();
    let original_before = tree_files(&f.source);
    assert!(ProfileContext::inspect_existing(&f.source, &[]).is_err());
    let receipt = f.restore().unwrap();
    assert!(receipt.original_source_present_and_locked);
    assert_eq!(receipt.entity_ids, vec![f.entity_id.clone()]);
    assert_eq!(tree_files(&f.source), original_before);
    let restored = ProfileContext::inspect_existing(&f.destination, &[]).unwrap();
    assert_eq!(restored.profile_id(), f.profile_id);
    assert_eq!(
        read_json(&f.destination.join(f.config_relative()))["agent_name"],
        json!("Signal")
    );
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_wrong_or_missing_credentials_leave_no_destination_and_preserve_bundle() {
    for key in [None, Some([0x40; 32])] {
        let f = Fixture::new();
        f.snapshot();
        let before = tree_files(&f.bundle);
        let error = restore(&f.bundle, &f.destination, &[], &probe(key)).unwrap_err();
        assert!(format!("{error:#}").contains("credential"));
        assert!(!f.destination.exists());
        assert_eq!(tree_files(&f.bundle), before);
        f.assert_no_partial();
    }
}

#[test]
fn cold_snapshot_io_existing_destinations_are_never_overwritten() {
    let f = Fixture::new();
    f.snapshot();
    let bundle_before = tree_files(&f.bundle);
    fs::create_dir(&f.destination).unwrap();
    fs::write(f.destination.join("user-owned.txt"), b"Keep these bytes.").unwrap();
    let before = tree_files(&f.destination);
    assert!(f
        .restore()
        .unwrap_err()
        .to_string()
        .contains("already exists"));
    assert_eq!(tree_files(&f.destination), before);
    assert_eq!(tree_files(&f.bundle), bundle_before);
    assert!(snapshot(&f.source, &f.bundle, &[]).is_err());
    assert_eq!(tree_files(&f.bundle), bundle_before);
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_rejects_malformed_newer_duplicate_or_unknown_manifest_fields() {
    for mode in [
        "malformed",
        "newer",
        "string-version",
        "unknown",
        "duplicate",
    ] {
        let f = Fixture::new();
        f.snapshot();
        match mode {
            "malformed" => fs::write(f.bundle.join("manifest.json"), b"{").unwrap(),
            "newer" => f.update_manifest(|v| v["schema_version"] = json!(2)),
            "string-version" => f.update_manifest(|v| v["schema_version"] = json!("1")),
            "unknown" => f.update_manifest(|v| v["ignore_checks"] = json!(true)),
            _ => {
                let text = fs::read_to_string(f.bundle.join("manifest.json")).unwrap();
                fs::write(
                    f.bundle.join("manifest.json"),
                    text.replacen('{', "{\"schema_version\":2,", 1),
                )
                .unwrap();
            }
        }
        let before = tree_files(&f.bundle);
        assert!(verify_snapshot(&f.bundle).is_err(), "{mode}");
        assert!(f.restore().is_err(), "{mode}");
        assert!(!f.destination.exists(), "{mode}");
        assert_eq!(tree_files(&f.bundle), before, "{mode}");
        f.assert_no_partial();
    }
}

#[test]
fn cold_snapshot_io_rejects_changed_extra_missing_files_and_extra_bundle_content() {
    for mode in ["changed", "extra", "missing", "extra-bundle"] {
        let f = Fixture::new();
        f.snapshot();
        let segment = f.bundle.join("profile/memory.db/segment");
        match mode {
            "changed" => fs::write(segment, b"abd").unwrap(),
            "extra" => fs::write(f.bundle.join("profile/unlisted"), b"extra").unwrap(),
            "missing" => fs::remove_file(segment).unwrap(),
            _ => fs::write(f.bundle.join("extra"), b"extra").unwrap(),
        }
        let before = tree_files(&f.bundle);
        assert!(verify_snapshot(&f.bundle).is_err(), "{mode}");
        assert!(f.restore().is_err(), "{mode}");
        assert!(!f.destination.exists(), "{mode}");
        assert_eq!(tree_files(&f.bundle), before);
    }
}

#[test]
fn cold_snapshot_io_rejects_traversal_absolute_and_case_colliding_inventory_paths() {
    for path in [
        "../outside",
        "/absolute",
        "C:/outside",
        "profile.json/../outside",
    ] {
        let f = Fixture::new();
        f.snapshot();
        f.update_manifest(|v| {
            let mut entry = v["entries"][0].clone();
            entry["path"] = json!(path);
            let entries = v["entries"].as_array_mut().unwrap();
            entries.push(entry);
            entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        });
        assert!(verify_snapshot(&f.bundle).is_err(), "{path}");
        assert!(f.restore().is_err(), "{path}");
        assert!(!f.destination.exists());
        assert!(!f.container.join("outside").exists());
    }
    let f = Fixture::new();
    f.snapshot();
    f.update_manifest(|v| {
        let entries = v["entries"].as_array_mut().unwrap();
        let mut duplicate = entries
            .iter()
            .find(|e| e["path"] == "profile.json")
            .unwrap()
            .clone();
        duplicate["path"] = json!("PROFILE.JSON");
        entries.push(duplicate);
        entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    });
    let error = verify_snapshot(&f.bundle).unwrap_err();
    assert!(format!("{error:#}").contains("Case-colliding"));
    assert!(f.restore().is_err());
    assert!(!f.destination.exists());
}

#[test]
fn cold_snapshot_io_rejects_declared_root_or_identity_summary_mismatch() {
    for mode in ["root", "profile", "entity", "receipts"] {
        let f = Fixture::new();
        f.snapshot();
        f.update_manifest(|v| match mode {
            "root" => v["source_root"] = json!(f.container.join("different-source")),
            "profile" => v["profile_id"] = json!(new_instance_id()),
            "entity" => v["entity_ids"] = json!([new_instance_id()]),
            _ => v["creation_receipt_count"] = json!(2),
        });
        assert!(verify_snapshot(&f.bundle).is_err(), "{mode}");
        assert!(f.restore().is_err(), "{mode}");
        assert!(!f.destination.exists(), "{mode}");
        assert!(!f.container.join("different-source").exists());
    }
}

#[test]
fn cold_snapshot_io_stale_partial_is_not_a_completed_bundle_or_active_profile() {
    let f = Fixture::new();
    let stale = f
        .container
        .join(format!(".abigail-cold-{}.partial", new_instance_id()));
    fs::create_dir(&stale).unwrap();
    fs::copy(f.source.join("profile.json"), stale.join("profile.json")).unwrap();
    fs::write(stale.join("unfinished"), b"Interrupted operation.").unwrap();
    let before = tree_files(&stale);
    assert!(verify_snapshot(&stale).is_err());
    assert!(crate::profile::reject_cold_staging_root(&stale).is_err());
    assert!(restore(&stale, &f.destination, &[], &probe(Some(FIXTURE_KEK))).is_err());
    assert!(!f.destination.exists());
    assert_eq!(tree_files(&stale), before);
    f.snapshot();
    f.restore().unwrap();
    assert_eq!(tree_files(&stale), before);
}

struct DestinationRaceProbe {
    destination: PathBuf,
}

impl RestoreCredentialProbe for DestinationRaceProbe {
    fn verify_profile(
        &self,
        profile: Arc<ProfileContext>,
    ) -> crate::Result<RestoreCredentialVerification> {
        let result = probe(Some(FIXTURE_KEK)).verify_profile(profile)?;
        fs::create_dir(&self.destination)?;
        fs::write(
            self.destination.join("user-owned.txt"),
            b"Created during restore.",
        )?;
        Ok(result)
    }
}

#[test]
fn cold_snapshot_io_publication_race_preserves_the_new_destination() {
    let f = Fixture::new();
    f.snapshot();
    let before = tree_files(&f.bundle);
    let error = restore(
        &f.bundle,
        &f.destination,
        &[],
        &DestinationRaceProbe {
            destination: f.destination.clone(),
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("Destination appeared"));
    assert_eq!(
        tree_files(&f.destination),
        BTreeMap::from([(
            PathBuf::from("user-owned.txt"),
            b"Created during restore.".to_vec()
        )])
    );
    assert_eq!(tree_files(&f.bundle), before);
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_existing_writer_handle_prevents_snapshot_publication() {
    use std::os::windows::fs::OpenOptionsExt;

    let f = Fixture::new();
    let segment = f.source.join("memory.db/segment");
    let writer = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0x1 | 0x2 | 0x4)
        .open(&segment)
        .unwrap();
    assert!(
        snapshot(&f.source, &f.bundle, &[]).is_err(),
        "an existing writer must prevent a cold snapshot from claiming a stopped store"
    );
    assert!(!f.bundle.exists());
    assert_eq!(fs::read(&segment).unwrap(), STORE_BYTES);
    drop(writer);
    assert_eq!(f.snapshot().profile_id, f.profile_id);
    f.assert_no_partial();
}

#[test]
fn cold_snapshot_io_refuses_overlapping_source_bundle_or_restore_roots() {
    let f = Fixture::new();
    assert!(snapshot(&f.source, &f.source.join("snapshot"), &[]).is_err());
    assert!(!f.source.join("snapshot").exists());
    f.snapshot();
    assert!(restore(
        &f.bundle,
        &f.bundle.join("restore"),
        &[],
        &probe(Some(FIXTURE_KEK)),
    )
    .is_err());
    assert!(!f.bundle.join("restore").exists());
    assert!(restore(
        &f.bundle,
        &f.source.join("restore"),
        &[],
        &probe(Some(FIXTURE_KEK)),
    )
    .is_err());
    assert!(!f.source.join("restore").exists());
}

#[test]
fn cold_snapshot_io_forbidden_destination_preserves_source_and_bundle() {
    let f = Fixture::new();
    f.snapshot();
    let before = tree_files(&f.bundle);
    let forbidden = f.container.join("protected");
    fs::create_dir(&forbidden).unwrap();
    fs::write(forbidden.join("user-owned.txt"), b"Protected data.").unwrap();
    assert!(restore(
        &f.bundle,
        &forbidden.join("restore"),
        std::slice::from_ref(&forbidden),
        &probe(Some(FIXTURE_KEK)),
    )
    .is_err());
    assert_eq!(
        fs::read(forbidden.join("user-owned.txt")).unwrap(),
        b"Protected data."
    );
    assert!(!forbidden.join("restore").exists());
    assert_eq!(tree_files(&f.bundle), before);
}
