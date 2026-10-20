//! Synthetic Windows cold-profile proof. Each phase is a new process so the
//! persistence engine cache cannot make a copied store look successfully reopened.
//! No phase uses HybridUnlockProvider, an OS keyring, a daemon or a real profile.
#![cfg(windows)]

use abigail_core::global_config::{AgentCreationRecord, AgentCreationState};
use abigail_core::profile::{reject_cold_staging_root, same_path, ProfileContext};
use abigail_core::profile_lease::ProfileUseLease;
use abigail_core::profile_snapshot::{restore, snapshot};
use abigail_core::vault::crypto;
use abigail_core::vault::unlock::{
    ProvidedRestoreCredentialProbe, RestoreCredentialProbe, RestoreCredentialVerification,
};
use abigail_core::{AgentEntry, AppConfig, CoreError, GlobalConfig, UnlockProvider};
use abigail_persistence::{EntityScope, PersistenceHandle};
use anyhow::{Context, Result};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

const ENTITY_A: &str = "11111111-1111-4111-8111-111111111111";
const ENTITY_B: &str = "22222222-2222-4222-8222-222222222222";
const RESERVED_ENTITY: &str = "33333333-3333-4333-8333-333333333333";
const DELETED_ENTITY: &str = "44444444-4444-4444-8444-444444444444";
const COMPLETE_KEY: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const RESERVED_KEY: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const DELETED_KEY: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const ROOT_KEY: [u8; 32] = [0x52; 32];
const MASTER_SECRET: [u8; 32] = [0x41; 32];
const PHASE_ENV: &str = "ABIGAIL_COLD_FIXTURE_PHASE";
const BASE_ENV: &str = "ABIGAIL_COLD_FIXTURE_BASE";

struct FakeCredentials(Option<[u8; 32]>);

impl UnlockProvider for FakeCredentials {
    fn root_kek(&self) -> abigail_core::Result<[u8; 32]> {
        self.0.ok_or_else(|| {
            CoreError::Vault("Synthetic credential unavailable; no replacement generated".into())
        })
    }
}

fn probe(key: Option<[u8; 32]>) -> ProvidedRestoreCredentialProbe {
    ProvidedRestoreCredentialProbe::new(Arc::new(FakeCredentials(key)))
}

struct InterruptBeforePublication {
    evidence_path: PathBuf,
}

impl RestoreCredentialProbe for InterruptBeforePublication {
    fn verify_profile(
        &self,
        profile: Arc<ProfileContext>,
    ) -> abigail_core::Result<RestoreCredentialVerification> {
        // The actual restore has copied and validated files into this stage.
        // Exit without unwinding so Staging::drop cannot clean it up.
        assert!(reject_cold_staging_root(profile.root()).is_err());
        fs::write(
            &self.evidence_path,
            serde_json::to_vec(&json!({
                "phase":"interrupted-restore",
                "pid":std::process::id(),
                "staging_root":profile.root()
            }))?,
        )?;
        std::process::exit(73)
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "abigail-cold-store-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("child-temp")).unwrap();
        fs::write(
            root.join("fixture-gate"),
            b"synthetic-cold-profile-store-v1",
        )
        .unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test's unique, previously created temporary directory.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_phase(base: &Path, phase: &str) -> u32 {
    run_phase_with_exit(base, phase, 0)
}

fn run_phase_with_exit(base: &Path, phase: &str, expected_exit: i32) -> u32 {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW for this owned test child.
        .args([
            "--exact",
            "cold_profile_child_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(PHASE_ENV, phase)
        .env(BASE_ENV, base)
        .env("TEMP", base.join("child-temp"))
        .env("TMP", base.join("child-temp"));
    // Windows loader/OS variables only. In particular, no provider, credential,
    // profile, daemon URL, CI-memory-mode or transport environment is inherited.
    for name in ["SystemRoot", "WINDIR", "SystemDrive"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let output = command.output().unwrap();
    assert!(
        output.status.code() == Some(expected_exit),
        "fresh process phase {phase} expected exit {expected_exit}, got {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence: Value =
        serde_json::from_slice(&fs::read(base.join(format!("{phase}.json"))).unwrap()).unwrap();
    assert_eq!(evidence["phase"], phase);
    println!("Cold-profile fresh-process evidence: {evidence}");
    evidence["pid"].as_u64().unwrap().try_into().unwrap()
}

#[test]
fn cold_profile_full_store_and_identity_survive_fresh_process_restore() {
    let fixture = Fixture::new();
    let pids = ["seed", "snapshot", "restore", "reopen"]
        .map(|phase| run_phase(&fixture.0, phase))
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(pids.len(), 4, "each operation must run in a fresh process");
    let reopened: Value =
        serde_json::from_slice(&fs::read(fixture.0.join("reopen.json")).unwrap()).unwrap();
    assert_eq!(reopened["verified_entity_ids"], json!([ENTITY_A, ENTITY_B]));
    assert_eq!(reopened["verified_creation_receipts"], 3);
    assert_eq!(reopened["verified_store_scopes"], 3);
    assert_eq!(reopened["verified_conversation_turns"], 4);
    assert_eq!(reopened["verified_entity_signatures"], 2);
}

#[test]
fn cold_profile_missing_or_wrong_credentials_preserve_bundle_and_destination() {
    let fixture = Fixture::new();
    run_phase(&fixture.0, "seed");
    run_phase(&fixture.0, "snapshot");
    let bundle_before = tree_bytes(&fixture.0.join("snapshot")).unwrap();
    run_phase(&fixture.0, "missing-credential");
    run_phase(&fixture.0, "wrong-credential");
    assert_eq!(
        tree_bytes(&fixture.0.join("snapshot")).unwrap(),
        bundle_before,
        "failed restores must leave every snapshot byte unchanged"
    );
    assert!(!fixture.0.join("restored").exists());
    // Failure must not poison subsequent recovery with the original key.
    run_phase(&fixture.0, "restore");
    run_phase(&fixture.0, "reopen");
}

#[test]
fn cold_profile_interrupted_restore_never_publishes_and_can_retry() {
    let fixture = Fixture::new();
    run_phase(&fixture.0, "seed");
    run_phase(&fixture.0, "snapshot");
    let bundle_before = tree_bytes(&fixture.0.join("snapshot")).unwrap();
    run_phase_with_exit(&fixture.0, "interrupted-restore", 73);
    assert!(!fixture.0.join("restored").exists());
    assert_eq!(
        tree_bytes(&fixture.0.join("snapshot")).unwrap(),
        bundle_before
    );
    let interrupted: Value =
        serde_json::from_slice(&fs::read(fixture.0.join("interrupted-restore.json")).unwrap())
            .unwrap();
    let stage = PathBuf::from(interrupted["staging_root"].as_str().unwrap());
    let stages: Vec<_> = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with(".abigail-cold-") && name.ends_with(".partial")
        })
        .collect();
    assert_eq!(
        stages.len(),
        1,
        "the interrupted operation must leave its private stage"
    );
    assert!(same_path(&fs::canonicalize(&stages[0]).unwrap(), &stage));
    assert!(reject_cold_staging_root(&stage).is_err());
    let stage_before_retry = tree_bytes(&stage).unwrap();
    run_phase(&fixture.0, "restore");
    run_phase(&fixture.0, "reopen");
    assert_eq!(tree_bytes(&stage).unwrap(), stage_before_retry);
    assert_eq!(
        tree_bytes(&fixture.0.join("snapshot")).unwrap(),
        bundle_before
    );
}

#[test]
#[ignore = "Invoked only by parent tests with an isolated synthetic phase"]
fn cold_profile_child_fixture() {
    let phase = std::env::var(PHASE_ENV).expect("parent must select a synthetic phase");
    let base = PathBuf::from(std::env::var_os(BASE_ENV).expect("parent must supply fixture root"));
    assert!(base.is_absolute());
    assert!(
        base.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("abigail-cold-store-fixture-"),
        "child fixture must never run against an ordinary profile"
    );
    assert_eq!(
        fs::read(base.join("fixture-gate")).unwrap(),
        b"synthetic-cold-profile-store-v1"
    );
    child_phase(&base, &phase).unwrap();
}

fn child_phase(base: &Path, phase: &str) -> Result<()> {
    let source = base.join("source");
    let bundle = base.join("snapshot");
    let restored = base.join("restored");
    let mut evidence = json!({"phase":phase, "pid":std::process::id()});
    match phase {
        "seed" => seed(&source, base)?,
        "snapshot" => {
            let before = persistent_tree_bytes(&source)?;
            snapshot(&source, &bundle, &[])?;
            assert_eq!(persistent_tree_bytes(&source)?, before);
            assert!(source.join("memory.db").is_dir());
        }
        "restore" => {
            let before = tree_bytes(&bundle)?;
            restore(&bundle, &restored, &[], &probe(Some(ROOT_KEY)))?;
            assert_eq!(tree_bytes(&bundle)?, before);
            assert!(!restored.join("runtime").exists());
            assert!(!restored.join("tmp").exists());
        }
        "interrupted-restore" => {
            let interrupt = InterruptBeforePublication {
                evidence_path: base.join("interrupted-restore.json"),
            };
            restore(&bundle, &restored, &[], &interrupt)?;
            anyhow::bail!("Interruption fixture returned instead of exiting before publication");
        }
        "missing-credential" | "wrong-credential" => {
            let before = tree_bytes(&bundle)?;
            let key = (phase == "wrong-credential").then_some([0x99; 32]);
            let failure = restore(&bundle, &restored, &[], &probe(key))
                .err()
                .context("unavailable/wrong credential unexpectedly restored a profile")?;
            assert!(
                !format!("{failure:#}").trim().is_empty(),
                "recovery failure must explain why it stopped"
            );
            assert!(!restored.exists());
            assert_eq!(tree_bytes(&bundle)?, before);
        }
        "reopen" => {
            verify_restored(&restored, base)?;
            evidence["verified_entity_ids"] = json!([ENTITY_A, ENTITY_B]);
            evidence["verified_creation_receipts"] = json!(3);
            evidence["verified_store_scopes"] = json!(3);
            evidence["verified_conversation_turns"] = json!(4);
            evidence["verified_entity_signatures"] = json!(2);
        }
        other => anyhow::bail!("Unknown synthetic phase {other}"),
    }
    fs::write(
        base.join(format!("{phase}.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(())
}

fn seed(source: &Path, base: &Path) -> Result<()> {
    let profile = Arc::new(ProfileContext::preflight(source, &[])?);
    let root = profile.root();
    let _process_lease = ProfileUseLease::acquire(&profile)?;
    let mut global = GlobalConfig::new(root);
    for (id, name, color) in [
        (ENTITY_A, "Ongoing", "aether"),
        (ENTITY_B, "Second", "ember"),
    ] {
        let directory = root.join("identities").join(id);
        fs::create_dir_all(directory.join("docs"))?;
        fs::create_dir_all(directory.join("models"))?;
        let mut config = AppConfig::defaults_at(&directory);
        config.db_path = profile.shared_memory_path();
        config.agent_name = Some(name.into());
        config.birth_complete = true;
        config.external_pubkey_path = Some(directory.join("external_pubkey.bin"));
        let mut config_json = serde_json::to_value(&config)?;
        // This deliberately looks like a source path but is opaque user text;
        // restore must not perform blanket string replacement.
        config_json["synthetic_user_text"] = json!(format!("Keep literal {}", root.display()));
        fs::write(
            directory.join("config.json"),
            serde_json::to_vec_pretty(&config_json)?,
        )?;
        let entity_key = entity_signing_key(id);
        fs::write(
            directory.join("external_pubkey.bin"),
            entity_key.verifying_key().to_bytes(),
        )?;
        fs::write(
            directory.join("signature.sig"),
            abigail_core::sign_agent_key(
                &SigningKey::from_bytes(&MASTER_SECRET),
                &entity_key.verifying_key(),
            ),
        )?;
        fs::write(
            directory.join("presence.json"),
            serde_json::to_vec(&json!({"version":1, "palette":color, "name":name}))?,
        )?;
        fs::write(
            directory.join("docs").join("soul.md"),
            format!("# {name}\nSynthetic identity."),
        )?;
        fs::create_dir_all(root.join("Documents").join(id).join("empty-directory"))?;
        fs::write(
            root.join("Documents").join(id).join("note.md"),
            format!("Notes for {id}"),
        )?;
        global.agents.push(AgentEntry {
            id: id.into(),
            name: name.into(),
            is_hive: false,
            directory: PathBuf::from("identities").join(id),
        });
    }
    for (key, entity_id, name, state) in [
        (
            COMPLETE_KEY,
            ENTITY_A,
            "Ongoing",
            AgentCreationState::Complete,
        ),
        (
            RESERVED_KEY,
            RESERVED_ENTITY,
            "Unfinished",
            AgentCreationState::Reserved,
        ),
        (
            DELETED_KEY,
            DELETED_ENTITY,
            "Deleted",
            AgentCreationState::Complete,
        ),
    ] {
        global.creation_attempts.insert(
            key.into(),
            AgentCreationRecord {
                name: name.into(),
                entity_id: entity_id.into(),
                state,
            },
        );
    }
    fs::create_dir_all(root.join("identities").join(RESERVED_ENTITY))?;
    fs::write(
        root.join("identities")
            .join(RESERVED_ENTITY)
            .join("partial.txt"),
        b"Reserved creation must retain its exact key and partial files",
    )?;
    fs::write(
        root.join("global_settings.json"),
        serde_json::to_vec_pretty(&global)?,
    )?;
    write_encrypted(
        &profile,
        "vault.sentinel",
        "vault:sentinel:v1",
        b"ABIGAIL_VAULT_v1_2026-10-20T00:00:00+00:00",
    )?;
    write_encrypted(
        &profile,
        "master.key",
        "keyring:master",
        &serde_json::to_vec(&json!({"secret":MASTER_SECRET.to_vec()}))?,
    )?;
    write_encrypted(
        &profile,
        "hive_secrets/secrets.vault",
        "secrets:secrets",
        &serde_json::to_vec(&json!({"synthetic":"not-a-real-provider-credential"}))?,
    )?;
    let verification = probe(Some(ROOT_KEY)).verify_profile(profile.clone())?;
    fs::create_dir_all(root.join("webview").join("hive"))?;
    fs::write(
        root.join("webview")
            .join("hive")
            .join("synthetic-local-storage.json"),
        serde_json::to_vec(&json!({
            "abigail.first-signal.creation.v2":{
                "creationKey":RESERVED_KEY,
                "endpoint":"http://127.0.0.1:59091",
                "status":"unknown"
            }
        }))?,
    )?;
    fs::create_dir_all(root.join("runtime"))?;
    fs::write(
        root.join("runtime").join("stale-descriptor.json"),
        b"{\"pid\":42}",
    )?;
    fs::create_dir_all(root.join("tmp").join(ENTITY_A))?;
    fs::write(
        root.join("tmp").join(ENTITY_A).join("scratch.txt"),
        b"temporary",
    )?;
    fs::create_dir_all(root.join("archives").join(ENTITY_A))?;
    fs::write(
        root.join("archives").join(ENTITY_A).join("retained.txt"),
        b"archive bytes",
    )?;
    for scope in all_scopes() {
        let handle = PersistenceHandle::open(profile.shared_memory_path(), scope.clone())?;
        handle.create("job_record", "same-job", &job_for(&scope))?;
        match &scope {
            EntityScope::Hive => {
                handle.create(
                    "hive_meta",
                    "cold-proof",
                    &json!({"value":"shared-root-metadata"}),
                )?;
            }
            EntityScope::Entity(id) => {
                for number in 1..=2 {
                    handle.create(
                        "conversation_turn",
                        &format!("same-turn-{number}"),
                        &turn_for(id, number),
                    )?;
                }
                handle.create(
                    "memory_entry",
                    "same-memory",
                    &json!({"content":format!("Memory {id}")}),
                )?;
            }
        }
    }
    fs::write(
        base.join("expected.json"),
        serde_json::to_vec_pretty(&json!({
            "profile_id":profile.profile_id(),
            "master_public_key":verification.master_public_key.to_vec(),
            "literal_user_text":format!("Keep literal {}",root.display()),
            "creation_attempts":global.creation_attempts
        }))?,
    )?;
    // All seeded store handles remain cached until this process exits. A source
    // with a live participating owner must already be refused before copying.
    assert!(snapshot(root, &base.join("must-not-snapshot-live"), &[]).is_err());
    assert!(!base.join("must-not-snapshot-live").exists());
    Ok(())
}

fn verify_restored(root: &Path, base: &Path) -> Result<()> {
    let expected: Value = serde_json::from_slice(&fs::read(base.join("expected.json"))?)?;
    let profile = Arc::new(ProfileContext::inspect_existing(root, &[])?);
    assert_eq!(
        profile.profile_id(),
        expected["profile_id"].as_str().unwrap()
    );
    let _process_lease = ProfileUseLease::acquire(&profile)?;
    let credential = probe(Some(ROOT_KEY)).verify_profile(profile.clone())?;
    assert_eq!(
        json!(credential.master_public_key.to_vec()),
        expected["master_public_key"]
    );
    let master: Value =
        serde_json::from_slice(&read_encrypted(&profile, "master.key", "keyring:master")?)?;
    assert_eq!(master, json!({"secret":MASTER_SECRET.to_vec()}));
    let secrets: Value = serde_json::from_slice(&read_encrypted(
        &profile,
        "hive_secrets/secrets.vault",
        "secrets:secrets",
    )?)?;
    assert_eq!(
        secrets,
        json!({"synthetic":"not-a-real-provider-credential"})
    );
    let global: GlobalConfig =
        serde_json::from_slice(&fs::read(profile.root().join("global_settings.json"))?)?;
    assert_eq!(
        global
            .agents
            .iter()
            .map(|entity| entity.id.as_str())
            .collect::<Vec<_>>(),
        vec![ENTITY_A, ENTITY_B]
    );
    assert_eq!(
        serde_json::to_value(&global.creation_attempts)?,
        expected["creation_attempts"]
    );
    assert!(same_path(
        &global.master_key_path,
        &profile.root().join("master.key")
    ));
    for (id, name, color) in [
        (ENTITY_A, "Ongoing", "aether"),
        (ENTITY_B, "Second", "ember"),
    ] {
        let entity_root = profile.root().join("identities").join(id);
        let raw: Value = serde_json::from_slice(&fs::read(entity_root.join("config.json"))?)?;
        let config = AppConfig::from_json_compatible(&serde_json::to_string(&raw)?)?;
        profile.validate_config_at_path(&config, &entity_root.join("config.json"))?;
        assert!(same_path(&config.db_path, &profile.shared_memory_path()));
        assert_eq!(raw["synthetic_user_text"], expected["literal_user_text"]);
        assert_eq!(config.agent_name.as_deref(), Some(name));
        let restored_public: [u8; 32] = fs::read(entity_root.join("external_pubkey.bin"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Restored Entity public key has wrong length"))?;
        assert_eq!(
            restored_public,
            entity_signing_key(id).verifying_key().to_bytes()
        );
        let restored_signature = fs::read(entity_root.join("signature.sig"))?;
        assert!(abigail_core::verify_agent_signature(
            &VerifyingKey::from_bytes(&credential.master_public_key)?,
            &VerifyingKey::from_bytes(&restored_public)?,
            &restored_signature,
        ));
        let presence: Value =
            serde_json::from_slice(&fs::read(entity_root.join("presence.json"))?)?;
        assert_eq!(presence, json!({"version":1, "palette":color, "name":name}));
        assert!(profile
            .root()
            .join("Documents")
            .join(id)
            .join("empty-directory")
            .is_dir());
        assert_eq!(
            fs::read_to_string(profile.root().join("Documents").join(id).join("note.md"))?,
            format!("Notes for {id}")
        );
    }
    let journal: Value = serde_json::from_slice(&fs::read(
        profile
            .root()
            .join("webview/hive/synthetic-local-storage.json"),
    )?)?;
    assert_eq!(
        journal["abigail.first-signal.creation.v2"]["creationKey"],
        RESERVED_KEY
    );
    assert_eq!(
        journal["abigail.first-signal.creation.v2"]["endpoint"],
        "http://127.0.0.1:59091"
    );
    assert!(profile
        .root()
        .join("identities")
        .join(RESERVED_ENTITY)
        .join("partial.txt")
        .is_file());
    assert!(!profile
        .root()
        .join("identities")
        .join(DELETED_ENTITY)
        .exists());
    assert!(!profile
        .root()
        .join("runtime")
        .join("stale-descriptor.json")
        .exists());
    assert_eq!(
        fs::read(
            profile
                .root()
                .join("archives")
                .join(ENTITY_A)
                .join("retained.txt")
        )?,
        b"archive bytes"
    );
    for scope in all_scopes() {
        let handle = PersistenceHandle::open(profile.shared_memory_path(), scope.clone())?;
        let mut job: Value = handle
            .select_record("job_record", "same-job")?
            .context("Missing scoped job")?;
        job.as_object_mut().unwrap().remove("id");
        assert_eq!(job, job_for(&scope), "job scope {} changed", scope.label());
        match &scope {
            EntityScope::Hive => {
                let meta: Value = handle
                    .select_record("hive_meta", "cold-proof")?
                    .context("Missing Hive metadata")?;
                assert_eq!(meta["value"], "shared-root-metadata");
            }
            EntityScope::Entity(id) => {
                for number in 1..=2 {
                    let mut turn: Value = handle
                        .select_record("conversation_turn", &format!("same-turn-{number}"))?
                        .context("Missing restored conversation turn")?;
                    turn.as_object_mut().unwrap().remove("id");
                    assert_eq!(turn, turn_for(id, number));
                }
                let memory: Value = handle
                    .select_record("memory_entry", "same-memory")?
                    .context("Missing scoped memory")?;
                assert_eq!(memory["content"], format!("Memory {id}"));
            }
        }
    }
    Ok(())
}

fn all_scopes() -> [EntityScope; 3] {
    [
        EntityScope::Hive,
        EntityScope::Entity(ENTITY_A.into()),
        EntityScope::Entity(ENTITY_B.into()),
    ]
}

fn entity_signing_key(entity_id: &str) -> SigningKey {
    // Test-only deterministic key, distinct for the two synthetic Entities.
    SigningKey::from_bytes(&[entity_id.as_bytes()[0]; 32])
}

fn job_for(scope: &EntityScope) -> Value {
    json!({"status":"pending", "topic":"synthetic-cold-proof", "owner":scope.label(), "attempts":2})
}

fn turn_for(entity_id: &str, number: u32) -> Value {
    json!({
        "session_id":"same-session",
        "turn_number":number,
        "role":if number == 1 {"user"} else {"assistant"},
        "content":format!("Exact synthetic turn {number} for {entity_id}\nUnicode: \u{03a9}"),
        "created_at":format!("2026-10-20T00:00:0{number}Z"),
        "provider":"synthetic-fixture",
        "model":"none"
    })
}

fn write_encrypted(
    profile: &ProfileContext,
    path: &str,
    scope: &str,
    plaintext: &[u8],
) -> Result<()> {
    let file = profile.root().join(path);
    fs::create_dir_all(file.parent().unwrap())?;
    let key = crypto::derive_scope_key(
        &ROOT_KEY,
        &format!("{}:{scope}", profile.credential_namespace()),
    );
    fs::write(file, crypto::seal(&key, plaintext)?)?;
    Ok(())
}

fn read_encrypted(profile: &ProfileContext, path: &str, scope: &str) -> Result<Vec<u8>> {
    let key = crypto::derive_scope_key(
        &ROOT_KEY,
        &format!("{}:{scope}", profile.credential_namespace()),
    );
    Ok(crypto::open(&key, &fs::read(profile.root().join(path))?)?)
}

fn tree_bytes(root: &Path) -> Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let relative = path.strip_prefix(root)?.to_path_buf();
            if path.is_dir() {
                entries.insert(relative, None);
                pending.push(path);
            } else {
                entries.insert(relative, Some(fs::read(path)?));
            }
        }
    }
    Ok(entries)
}

fn persistent_tree_bytes(root: &Path) -> Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
    Ok(tree_bytes(root)?
        .into_iter()
        .filter(|(path, _)| {
            !["runtime", "tmp", "logs", "backups"]
                .iter()
                .any(|name| path.starts_with(name))
        })
        .collect())
}
