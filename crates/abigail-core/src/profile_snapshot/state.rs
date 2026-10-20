//! Semantic validation of cold profile files, independent of the running profile.
//!
//! `declared_root` is a recorded location, not a path to inspect. Only payload
//! files are read. Restore rewrites the few declared application path fields;
//! opaque documents, receipt keys and other JSON fields are never retargeted.

use crate::global_config::AgentCreationState;
use crate::profile::{same_path, validate_uuid, ProfileMarker, PROFILE_SCHEMA_VERSION};
use crate::{AppConfig, GlobalConfig};
use anyhow::{bail, ensure, Context, Result};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct StateSummary {
    pub entity_ids: Vec<String>,
    pub creation_receipt_count: usize,
}

struct Inspected {
    summary: StateSummary,
    registry: Value,
    configs: Vec<(PathBuf, Value)>,
}

pub(super) fn inspect(
    payload_root: &Path,
    declared_root: &Path,
    expected_profile_id: &str,
) -> Result<StateSummary> {
    Ok(inspect_state(payload_root, declared_root, expected_profile_id)?.summary)
}

fn inspect_state(
    payload_root: &Path,
    declared_root: &Path,
    expected_profile_id: &str,
) -> Result<Inspected> {
    validate_absolute(declared_root).context("Invalid declared snapshot root")?;
    validate_absolute(payload_root).context("Invalid snapshot payload root")?;
    validate_uuid(expected_profile_id).context("Invalid expected snapshot profile ID")?;
    require_kind(payload_root, true)?;

    let marker_value = read_json(&payload_root.join("profile.json"))?;
    let marker: ProfileMarker =
        serde_json::from_value(marker_value).context("Invalid snapshot profile marker")?;
    ensure!(
        marker.schema_version == PROFILE_SCHEMA_VERSION,
        "Unsupported snapshot profile schema {}; expected {}",
        marker.schema_version,
        PROFILE_SCHEMA_VERSION
    );
    ensure!(
        marker.kind == "preview" && marker.channel == "beta",
        "Snapshot must contain a preview/beta profile"
    );
    ensure!(
        marker.profile_id == expected_profile_id,
        "Snapshot profile ID does not match the expected profile"
    );
    validate_uuid(&marker.profile_id)?;

    require_kind(&payload_root.join("master.key"), false)
        .context("Snapshot is missing its original master.key")?;
    require_kind(&payload_root.join("vault.sentinel"), false)
        .context("Snapshot is missing its original vault.sentinel")?;
    require_kind(&payload_root.join("memory.db"), true)
        .context("Snapshot requires the complete memory.db directory")?;

    let registry = read_json(&payload_root.join("global_settings.json"))?;
    validate_registry_schema(&registry)?;
    let global: GlobalConfig =
        serde_json::from_value(registry.clone()).context("Invalid snapshot Entity registry")?;
    require_exact(
        &global.master_key_path,
        &declared_root.join("master.key"),
        "registry master_key_path",
    )?;
    if let Some(path) = &global.hive_secrets_path {
        let relative =
            relative_to(path, declared_root).context("Invalid registry hive_secrets_path")?;
        if let Some(Component::Normal(top)) = relative.components().next() {
            let top = top.to_str().context("Invalid registry hive_secrets_path")?;
            ensure!(
                !super::EXCLUDED
                    .iter()
                    .any(|excluded| excluded.eq_ignore_ascii_case(top)),
                "Registry hive_secrets_path points into excluded snapshot state"
            );
        }
    }

    let mut registered = BTreeMap::new();
    for agent in &global.agents {
        validate_uuid(&agent.id).context("Invalid registered Entity ID")?;
        ensure!(
            registered.insert(agent.id.clone(), agent).is_none(),
            "Duplicate registered Entity ID"
        );
        validate_relative(&agent.directory)?;
        ensure!(
            agent.directory == PathBuf::from("identities").join(&agent.id),
            "Registry directory does not match its Entity UUID"
        );
    }
    let mut receipt_ids = BTreeSet::new();
    for (key, receipt) in &global.creation_attempts {
        validate_uuid(key).context("Invalid creation receipt key")?;
        validate_uuid(&receipt.entity_id).context("Invalid creation receipt Entity ID")?;
        ensure!(
            receipt_ids.insert(receipt.entity_id.clone()),
            "Conflicting creation receipts assign one Entity to multiple keys"
        );
        ensure!(
            !receipt.name.is_empty()
                && receipt.name.trim() == receipt.name
                && receipt.name.chars().count() <= 80
                && !receipt.name.chars().any(char::is_control),
            "Invalid creation receipt name"
        );
        ensure!(
            receipt.state != AgentCreationState::Reserved
                || !registered.contains_key(&receipt.entity_id),
            "Reserved creation receipt already has a registered Entity"
        );
        // Complete receipts intentionally survive deletion. Their unavailable
        // Entity is a tombstone, not a reason to invent or regenerate an ID.
    }

    let mut configs = Vec::new();
    let root_config = payload_root.join("config.json");
    if exists(&root_config)? {
        let value = inspect_config(&root_config, declared_root, declared_root)?;
        configs.push((PathBuf::from("config.json"), value));
    }

    let identities = payload_root.join("identities");
    let mut config_ids = BTreeSet::new();
    if exists(&identities)? {
        require_kind(&identities, true)?;
        for entry in fs::read_dir(&identities).context("Cannot read snapshot identities")? {
            let entry = entry?;
            require_kind(&entry.path(), true)?;
            let id = entry
                .file_name()
                .to_str()
                .context("Entity directory is not valid UTF-8")?
                .to_owned();
            validate_uuid(&id).context("Invalid Entity directory name")?;
            ensure!(
                registered.contains_key(&id) || receipt_ids.contains(&id),
                "Snapshot contains an identity with no registry or creation receipt"
            );
            let config_path = entry.path().join("config.json");
            if exists(&config_path)? {
                let entity_root = declared_root.join("identities").join(&id);
                let value = inspect_config(&config_path, declared_root, &entity_root)?;
                if let Some(agent) = registered.get(&id) {
                    let is_hive = value
                        .get("is_hive")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    ensure!(
                        is_hive == agent.is_hive,
                        "Entity config and registry disagree about Hive identity"
                    );
                }
                config_ids.insert(id.clone());
                configs.push((
                    PathBuf::from("identities").join(id).join("config.json"),
                    value,
                ));
            }
        }
    }
    for id in registered.keys() {
        ensure!(
            config_ids.contains(id),
            "Registered Entity {id} is missing its config.json"
        );
    }
    configs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Inspected {
        summary: StateSummary {
            entity_ids: registered.into_keys().collect(),
            creation_receipt_count: global.creation_attempts.len(),
        },
        registry,
        configs,
    })
}

/// Rebase validated payload files in an unpublished restore staging directory.
/// The outer transaction owns publication and disposal after an interrupted
/// write. Build all replacement JSON before performing the first write.
pub(super) fn rebase(payload_root: &Path, old_root: &Path, new_root: &Path) -> Result<()> {
    validate_absolute(new_root).context("Invalid restore destination root")?;
    let marker: ProfileMarker =
        serde_json::from_value(read_json(&payload_root.join("profile.json"))?)?;
    let mut state = inspect_state(payload_root, old_root, &marker.profile_id)?;
    let mut writes = Vec::new();
    for (relative, value) in &mut state.configs {
        let object = value.as_object_mut().context("Config must be an object")?;
        for field in ["data_dir", "docs_dir", "models_dir", "db_path"] {
            rebase_field(object, field, old_root, new_root)?;
        }
        if object
            .get("external_pubkey_path")
            .is_some_and(|v| !v.is_null())
        {
            rebase_field(object, "external_pubkey_path", old_root, new_root)?;
        }
        writes.push((
            payload_root.join(relative),
            serde_json::to_vec_pretty(value)?,
        ));
    }
    let registry = state
        .registry
        .as_object_mut()
        .context("Registry must be an object")?;
    rebase_field(registry, "master_key_path", old_root, new_root)?;
    if registry
        .get("hive_secrets_path")
        .is_some_and(|v| !v.is_null())
    {
        rebase_field(registry, "hive_secrets_path", old_root, new_root)?;
    }
    writes.push((
        payload_root.join("global_settings.json"),
        serde_json::to_vec_pretty(&state.registry)?,
    ));
    for (path, bytes) in writes {
        crate::secure_fs::write_bytes_atomic(&path, &bytes)
            .with_context(|| format!("Cannot rebase snapshot file {}", path.display()))?;
    }
    let rebased = inspect(payload_root, new_root, &marker.profile_id)?;
    ensure!(
        rebased == state.summary,
        "Restore path rebasing changed the Entity or receipt summary"
    );
    Ok(())
}

fn inspect_config(path: &Path, declared_root: &Path, expected_data: &Path) -> Result<Value> {
    let value = read_json(path)?;
    let config = AppConfig::from_json_compatible(&serde_json::to_string(&value)?)
        .with_context(|| format!("Invalid snapshot config {}", path.display()))?;
    for (actual, expected, field) in [
        (&config.data_dir, expected_data.to_path_buf(), "data_dir"),
        (&config.docs_dir, expected_data.join("docs"), "docs_dir"),
        (
            &config.models_dir,
            expected_data.join("models"),
            "models_dir",
        ),
        (&config.db_path, declared_root.join("memory.db"), "db_path"),
    ] {
        require_exact(actual, &expected, field)?;
    }
    if let Some(path) = &config.external_pubkey_path {
        require_exact(
            path,
            &expected_data.join("external_pubkey.bin"),
            "external_pubkey_path",
        )?;
    }
    Ok(value)
}

fn validate_registry_schema(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .context("Registry must be a JSON object")?;
    // The existing registry is unversioned. Accept only the initial explicit
    // version if a producer supplies one; never discard a newer declaration.
    for field in ["schema_version", "schemaVersion"] {
        if let Some(version) = object.get(field) {
            ensure!(
                version.as_u64() == Some(1),
                "Unsupported or malformed global registry {field}; expected 1 or absent"
            );
        }
    }
    Ok(())
}

fn rebase_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    old_root: &Path,
    new_root: &Path,
) -> Result<()> {
    let value = object
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing or invalid path field {field}"))?;
    let relative = relative_to(Path::new(value), old_root)?;
    object.insert(
        field.to_owned(),
        serde_json::to_value(new_root.join(relative))?,
    );
    Ok(())
}

fn require_exact(actual: &Path, expected: &Path, field: &str) -> Result<()> {
    validate_absolute(actual).with_context(|| format!("Invalid {field}"))?;
    ensure!(
        same_path(actual, expected),
        "Snapshot {field} does not match its declared profile/Entity path"
    );
    Ok(())
}

/// Compare recorded roots lexically. Never canonicalize or inspect the old
/// filesystem: its files may no longer exist, or that path may now be unrelated.
fn relative_to(path: &Path, root: &Path) -> Result<PathBuf> {
    validate_absolute(path)?;
    validate_absolute(root)?;
    let root_len = root.components().count();
    let parts: Vec<_> = path.components().collect();
    ensure!(
        parts.len() >= root_len,
        "Recorded path is outside its profile"
    );
    let prefix: PathBuf = parts[..root_len].iter().map(|p| p.as_os_str()).collect();
    ensure!(
        same_path(&prefix, root),
        "Recorded path is outside its profile"
    );
    let relative: PathBuf = parts[root_len..].iter().map(|p| p.as_os_str()).collect();
    if !relative.as_os_str().is_empty() {
        validate_relative(&relative)?;
    }
    Ok(relative)
}

fn validate_absolute(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "Snapshot paths must be absolute local paths"
    );
    let text = path.to_str().context("Snapshot path must be valid UTF-8")?;
    ensure!(
        !text
            .split(['/', '\\'])
            .any(|part| part == "." || part == ".."),
        "Snapshot paths cannot contain traversal or dot components"
    );
    for component in path.components() {
        match component {
            Component::ParentDir | Component::CurDir => bail!("Invalid snapshot path component"),
            Component::Normal(name) => validate_component(name)?,
            Component::Prefix(prefix) => {
                ensure!(
                    matches!(
                        prefix.kind(),
                        std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                    ),
                    "Snapshot paths cannot use device or network namespaces"
                );
            }
            Component::RootDir => {}
        }
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty() && !path.is_absolute(),
        "Expected a nonempty relative snapshot path"
    );
    let text = path.to_str().context("Snapshot path must be valid UTF-8")?;
    ensure!(
        !text
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == ".."),
        "Relative snapshot path must contain only normal components"
    );
    for component in path.components() {
        match component {
            Component::Normal(name) => validate_component(name)?,
            _ => bail!("Relative snapshot path must contain only normal components"),
        }
    }
    Ok(())
}

fn validate_component(name: &std::ffi::OsStr) -> Result<()> {
    let name = name.to_str().context("Snapshot path must be valid UTF-8")?;
    ensure!(
        !name.is_empty()
            && !name.ends_with(['.', ' '])
            && !name
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*\\/".contains(c)),
        "Snapshot path contains an unsafe or ambiguous component"
    );
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        || stem
            .strip_prefix("LPT")
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'));
    ensure!(!reserved, "Snapshot path contains a reserved device name");
    Ok(())
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            reject_link(&metadata)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("Cannot inspect snapshot payload"),
    }
}

fn require_kind(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Missing snapshot path {}", path.display()))?;
    reject_link(&metadata)?;
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "Snapshot path has an unexpected file type: {}",
        path.display()
    );
    Ok(())
}

fn reject_link(metadata: &fs::Metadata) -> Result<()> {
    ensure!(
        !metadata.file_type().is_symlink(),
        "Snapshot cannot contain symbolic links"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "Snapshot cannot contain reparse points"
        );
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    require_kind(path, false)?;
    let bytes = fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?;
    let unique: UniqueValue = serde_json::from_slice(&bytes)
        .with_context(|| format!("Invalid JSON in {}", path.display()))?;
    ensure!(unique.0.is_object(), "Snapshot JSON must be an object");
    Ok(unique.0)
}

/// Reject duplicate keys before preserving JSON as Value; Value alone silently
/// keeps the final duplicate, which is unsafe for schema and receipt checks.
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                value: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(value)))
            }
            fn visit_i64<E: serde::de::Error>(
                self,
                value: i64,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(
                self,
                value: u64,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(
                self,
                value: f64,
            ) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("Invalid JSON number"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = access.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = access.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("Duplicate JSON object key"));
                    }
                    let value = access.next_value::<UniqueValue>()?;
                    values.insert(key, value.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::new_instance_id;
    use serde_json::json;

    struct Fixture {
        container: PathBuf,
        payload: PathBuf,
        declared: PathBuf,
        profile_id: String,
        entity_id: String,
        creation_key: String,
    }

    impl Fixture {
        fn new() -> Self {
            let container =
                std::env::temp_dir().join(format!("snapshot-state-{}", new_instance_id()));
            let payload = container.join("payload");
            let declared = container.join("absent-original-root");
            let profile_id = new_instance_id();
            let entity_id = new_instance_id();
            let creation_key = new_instance_id();
            fs::create_dir_all(payload.join("identities").join(&entity_id)).unwrap();
            fs::create_dir(payload.join("memory.db")).unwrap();
            fs::write(
                payload.join("memory.db/synthetic-store"),
                b"all entity scopes",
            )
            .unwrap();
            fs::write(payload.join("master.key"), b"synthetic encrypted master").unwrap();
            fs::write(
                payload.join("vault.sentinel"),
                b"synthetic encrypted sentinel",
            )
            .unwrap();
            let fixture = Self {
                container,
                payload,
                declared,
                profile_id,
                entity_id,
                creation_key,
            };
            fixture.write(
                "profile.json",
                json!({
                    "schema_version": 1, "kind": "preview", "channel": "beta",
                    "profile_id": fixture.profile_id
                }),
            );
            fixture.write(
                "global_settings.json",
                json!({
                    "master_key_path": fixture.declared.join("master.key"),
                    "hive_secrets_path": fixture.declared.join("hive_secrets/secrets.vault"),
                    "agents": [{
                        "id": fixture.entity_id, "name": "Signal", "is_hive": false,
                        "directory": format!("identities/{}", fixture.entity_id)
                    }],
                    "creation_attempts": {
                        fixture.creation_key.clone(): {
                            "name": "Signal", "entity_id": fixture.entity_id, "state": "complete"
                        }
                    },
                    "opaque_registry_extension": {"text": fixture.declared}
                }),
            );
            fixture.write(
                &fixture.config_relative(),
                fixture.config_value(&fixture.entity_id),
            );
            fixture
        }

        fn config_relative(&self) -> String {
            format!("identities/{}/config.json", self.entity_id)
        }

        fn config_value(&self, id: &str) -> Value {
            let data = self.declared.join("identities").join(id);
            json!({
                "schema_version": crate::CONFIG_SCHEMA_VERSION,
                "data_dir": data, "docs_dir": data.join("docs"),
                "models_dir": data.join("models"), "db_path": self.declared.join("memory.db"),
                "birth_complete": false, "external_pubkey_path": data.join("external_pubkey.bin"),
                "opaque_extension": {"literal": self.declared, "nested": [1, true, null]},
                "agent_name": "Signal"
            })
        }

        fn write(&self, relative: &str, value: Value) {
            let path = self.payload.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        }

        fn read(&self, relative: &str) -> Value {
            read_json(&self.payload.join(relative)).unwrap()
        }

        fn inspect(&self) -> Result<StateSummary> {
            inspect(&self.payload, &self.declared, &self.profile_id)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.container);
        }
    }

    #[test]
    fn snapshot_state_accepts_absent_source_and_retains_identity_and_receipts() {
        let f = Fixture::new();
        assert!(!f.declared.exists());
        let summary = f.inspect().unwrap();
        assert_eq!(summary.entity_ids, vec![f.entity_id.clone()]);
        assert_eq!(summary.creation_receipt_count, 1);
        assert!(!f.declared.exists());
    }

    #[test]
    fn snapshot_state_rebase_preserves_unknown_json_and_exact_receipts() {
        let f = Fixture::new();
        let registry_before = f.read("global_settings.json");
        let config_before = f.read(&f.config_relative());
        let marker_before = fs::read(f.payload.join("profile.json")).unwrap();
        let destination = f.container.join("fresh-destination");
        rebase(&f.payload, &f.declared, &destination).unwrap();
        let registry = f.read("global_settings.json");
        let config = f.read(&f.config_relative());
        assert_eq!(
            registry["creation_attempts"],
            registry_before["creation_attempts"]
        );
        assert_eq!(registry["agents"], registry_before["agents"]);
        assert_eq!(
            registry["opaque_registry_extension"],
            registry_before["opaque_registry_extension"]
        );
        assert_eq!(
            config["opaque_extension"],
            config_before["opaque_extension"]
        );
        assert_eq!(config["schema_version"], config_before["schema_version"]);
        assert_eq!(
            config["data_dir"],
            json!(destination.join("identities").join(&f.entity_id))
        );
        assert_eq!(config["db_path"], json!(destination.join("memory.db")));
        assert_eq!(
            config["external_pubkey_path"],
            json!(destination
                .join("identities")
                .join(&f.entity_id)
                .join("external_pubkey.bin"))
        );
        assert_eq!(
            fs::read(f.payload.join("profile.json")).unwrap(),
            marker_before
        );
        assert!(!destination.exists());
        assert_eq!(
            inspect(&f.payload, &destination, &f.profile_id)
                .unwrap()
                .entity_ids,
            vec![f.entity_id.clone()]
        );
    }

    #[test]
    fn snapshot_state_accepts_complete_tombstone_and_reserved_partial_config() {
        let f = Fixture::new();
        let deleted_id = new_instance_id();
        let reserved_id = new_instance_id();
        let mut registry = f.read("global_settings.json");
        registry["creation_attempts"][new_instance_id()] = json!({
            "name": "Deleted", "entity_id": deleted_id, "state": "complete"
        });
        registry["creation_attempts"][new_instance_id()] = json!({
            "name": "Pending", "entity_id": reserved_id, "state": "reserved"
        });
        f.write("global_settings.json", registry);
        f.write(
            &format!("identities/{reserved_id}/config.json"),
            f.config_value(&reserved_id),
        );
        assert_eq!(f.inspect().unwrap().creation_receipt_count, 3);
        assert_eq!(f.inspect().unwrap().entity_ids, vec![f.entity_id.clone()]);
    }

    #[test]
    fn snapshot_state_rejects_missing_registered_config() {
        let f = Fixture::new();
        fs::remove_file(f.payload.join(f.config_relative())).unwrap();
        assert!(f
            .inspect()
            .unwrap_err()
            .to_string()
            .contains("missing its config"));
    }

    #[test]
    fn snapshot_state_rejects_declared_secrets_in_excluded_subtrees_before_rebase() {
        for directory in ["backups", "Logs", "RUNTIME", "tmp"] {
            let f = Fixture::new();
            let mut registry = f.read("global_settings.json");
            registry["hive_secrets_path"] = json!(f.declared.join(directory).join("secrets.vault"));
            f.write("global_settings.json", registry);
            let before = fs::read(f.payload.join("global_settings.json")).unwrap();
            assert!(f
                .inspect()
                .unwrap_err()
                .to_string()
                .contains("excluded snapshot state"));
            assert!(rebase(&f.payload, &f.declared, &f.container.join("destination")).is_err());
            assert_eq!(
                fs::read(f.payload.join("global_settings.json")).unwrap(),
                before
            );
        }

        let f = Fixture::new();
        let mut registry = f.read("global_settings.json");
        registry["hive_secrets_path"] = json!(f.declared.join("hive_secrets.bin"));
        f.write("global_settings.json", registry);
        assert!(f.inspect().is_ok(), "An absent default vault remains valid");
    }

    #[test]
    fn snapshot_state_rejects_entity_path_redirection_before_rebase_writes() {
        let f = Fixture::new();
        let mut config = f.read(&f.config_relative());
        config["docs_dir"] = json!(f
            .declared
            .join("identities")
            .join(new_instance_id())
            .join("docs"));
        f.write(&f.config_relative(), config);
        let registry = fs::read(f.payload.join("global_settings.json")).unwrap();
        let config = fs::read(f.payload.join(f.config_relative())).unwrap();
        assert!(rebase(&f.payload, &f.declared, &f.container.join("destination")).is_err());
        assert_eq!(
            fs::read(f.payload.join("global_settings.json")).unwrap(),
            registry
        );
        assert_eq!(
            fs::read(f.payload.join(f.config_relative())).unwrap(),
            config
        );
    }

    #[test]
    fn snapshot_state_rejects_newer_and_malformed_schemas_and_marker() {
        for version in [
            json!(crate::CONFIG_SCHEMA_VERSION + 1),
            json!("27"),
            json!(-1),
        ] {
            let f = Fixture::new();
            let mut config = f.read(&f.config_relative());
            config["schema_version"] = version;
            f.write(&f.config_relative(), config);
            assert!(f.inspect().is_err());
        }
        for version in [json!(2), json!("1"), json!(null), json!(-1)] {
            let f = Fixture::new();
            let mut registry = f.read("global_settings.json");
            registry["schema_version"] = version;
            f.write("global_settings.json", registry);
            assert!(f.inspect().is_err());
        }
        let f = Fixture::new();
        let mut marker = f.read("profile.json");
        marker["unexpected"] = json!(true);
        f.write("profile.json", marker);
        assert!(f.inspect().is_err());
    }

    #[test]
    fn snapshot_state_rejects_duplicate_registry_ids_and_mismatched_directory() {
        let f = Fixture::new();
        let mut registry = f.read("global_settings.json");
        let duplicate = registry["agents"][0].clone();
        registry["agents"].as_array_mut().unwrap().push(duplicate);
        f.write("global_settings.json", registry);
        assert!(f.inspect().unwrap_err().to_string().contains("Duplicate"));

        let f = Fixture::new();
        let mut registry = f.read("global_settings.json");
        registry["agents"][0]["directory"] = json!(format!("identities/{}", new_instance_id()));
        f.write("global_settings.json", registry);
        assert!(f
            .inspect()
            .unwrap_err()
            .to_string()
            .contains("does not match"));
    }

    #[test]
    fn snapshot_state_rejects_malformed_conflicting_or_reserved_active_receipts() {
        for mode in ["bad-key", "duplicate-id", "reserved-active"] {
            let f = Fixture::new();
            let mut registry = f.read("global_settings.json");
            let receipt = registry["creation_attempts"][&f.creation_key].clone();
            match mode {
                "bad-key" => registry["creation_attempts"]["not-a-uuid"] = receipt,
                "duplicate-id" => registry["creation_attempts"][new_instance_id()] = receipt,
                _ => registry["creation_attempts"][&f.creation_key]["state"] = json!("reserved"),
            }
            f.write("global_settings.json", registry);
            assert!(f.inspect().is_err(), "{mode}");
        }
    }

    #[test]
    fn snapshot_state_rejects_missing_identity_material_and_file_shaped_store() {
        for file in ["master.key", "vault.sentinel"] {
            let f = Fixture::new();
            fs::remove_file(f.payload.join(file)).unwrap();
            assert!(f.inspect().is_err());
        }
        let f = Fixture::new();
        fs::remove_dir_all(f.payload.join("memory.db")).unwrap();
        fs::write(f.payload.join("memory.db"), b"not the whole store").unwrap();
        assert!(f.inspect().is_err());
    }

    #[test]
    fn snapshot_state_rejects_duplicate_json_keys() {
        let f = Fixture::new();
        let text = fs::read_to_string(f.payload.join("global_settings.json")).unwrap();
        let duplicate = text.replacen('{', "{\"schema_version\":2,\"schema_version\":1,", 1);
        fs::write(f.payload.join("global_settings.json"), duplicate).unwrap();
        assert!(format!("{:#}", f.inspect().unwrap_err()).contains("Duplicate JSON"));
    }

    #[test]
    fn snapshot_state_lexical_paths_reject_traversal_prefix_collision_and_devices() {
        let f = Fixture::new();
        assert!(relative_to(&f.declared.join("../elsewhere"), &f.declared).is_err());
        assert!(relative_to(
            &f.container.join("absent-original-root-other/master.key"),
            &f.declared
        )
        .is_err());
        for value in [
            "identities/../escape",
            "identities/./escape",
            "identities/NUL.txt",
            "identities/id:stream",
            "/rooted",
        ] {
            assert!(validate_relative(Path::new(value)).is_err(), "{value}");
        }
    }

    #[test]
    fn snapshot_state_rejects_wrong_profile_and_newer_global_camel_schema() {
        let f = Fixture::new();
        assert!(inspect(&f.payload, &f.declared, &new_instance_id()).is_err());
        let mut registry = f.read("global_settings.json");
        registry["schemaVersion"] = json!(2);
        f.write("global_settings.json", registry);
        assert!(f.inspect().is_err());
    }
}
