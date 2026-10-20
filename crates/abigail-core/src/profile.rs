//! Preview profile identity and non-writing startup validation.
//!
//! Only `preflight`'s final step may create a new, empty profile's marker.
//! Existing profiles are inspected without migration or credential access.
//! Stable callers retain their historical defaults when no preview is requested.

use crate::{AppConfig, GlobalConfig};
use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub const PROFILE_SCHEMA_VERSION: u32 = 1;
static CURRENT: OnceLock<Arc<ProfileContext>> = OnceLock::new();
static PROCESS_LEASE: OnceLock<crate::profile_lease::ProfileUseLease> = OnceLock::new();
static INITIALIZATION: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileMarker {
    pub schema_version: u32,
    pub kind: String,
    pub channel: String,
    pub profile_id: String,
}

/// Public, non-secret ownership metadata. Equality to an independently expected
/// live instance is required; an HTTP response claiming a profile is not proof.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub channel: String,
    pub profile_id: String,
    pub canonical_data_root: PathBuf,
    pub instance_id: String,
    pub hive_instance_id: String,
}

#[derive(Debug, Clone)]
pub struct ProfileContext {
    root: PathBuf,
    marker: ProfileMarker,
}

pub fn compiled_preview() -> bool {
    option_env!("ABIGAIL_BUILD_PROFILE") == Some("preview")
}

pub fn new_instance_id() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub fn validate_uuid(value: &str) -> Result<()> {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
        || value == "00000000-0000-0000-0000-000000000000"
    {
        bail!("Preview identity must be a non-nil canonical UUID");
    }
    Ok(())
}

pub fn default_preview_root() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().context("No local application directory")?;
    Ok(base
        .data_local_dir()
        .join("AbigailPreview")
        .join("profiles")
        .join("ongoing"))
}

/// Interrupted cold operations leave reserved staging directories for explicit
/// inspection only. Normal startup must never consume or initialize their data.
/// Check ancestors too: snapshot payloads live beneath a staging directory.
pub fn reject_cold_staging_root(root: &Path) -> Result<()> {
    for component in root.components() {
        if let Component::Normal(name) = component {
            let name = name.to_string_lossy().to_ascii_lowercase();
            if name.starts_with(".abigail-cold-") && name.ends_with(".partial") {
                bail!(
                    "Cold snapshot/restore staging cannot be opened as an active preview profile"
                );
            }
        }
    }
    Ok(())
}

/// Resolve mode independently of I/O, making missing/conflicting launcher
/// arguments testable without changing the environment of parallel tests.
pub fn resolve_preview_request(
    build_profile: Option<&str>,
    env_profile: Option<&str>,
    env_root: Option<&Path>,
    cli_profile: Option<&str>,
    cli_root: Option<&Path>,
) -> Result<Option<PathBuf>> {
    if build_profile.is_some_and(|v| !matches!(v, "preview" | "stable")) {
        bail!("Unsupported compiled application profile");
    }
    for value in [env_profile, cli_profile].into_iter().flatten() {
        if !matches!(value, "preview" | "stable") {
            bail!("Unsupported or blank application profile");
        }
    }
    let preview = build_profile == Some("preview")
        || env_profile == Some("preview")
        || cli_profile == Some("preview");
    if !preview {
        return Ok(None);
    }
    if env_profile != Some("preview") || cli_profile.is_some_and(|v| v != "preview") {
        bail!("Preview startup requires an explicit matching ABIGAIL_PROFILE=preview");
    }
    let root = env_root.context("Preview startup requires ABIGAIL_DATA_DIR")?;
    validate_absolute(root)?;
    if let Some(cli_root) = cli_root {
        validate_absolute(cli_root)?;
        if !same_path(
            &canonical_without_links(root)?,
            &canonical_without_links(cli_root)?,
        ) {
            bail!("Preview argument and environment roots disagree");
        }
    }
    Ok(Some(root.to_path_buf()))
}

/// Entry-point operation, before logging, WebViews, vaults, DBs, or requests.
/// Runtime daemons additionally require explicit CLI arguments at their boundary.
pub fn initialize_process(
    cli_profile: Option<&str>,
    cli_root: Option<&Path>,
) -> Result<Option<Arc<ProfileContext>>> {
    let env_profile = match std::env::var("ABIGAIL_PROFILE") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error).context("Invalid application profile environment"),
    };
    let env_root = std::env::var_os("ABIGAIL_DATA_DIR").map(PathBuf::from);
    let requested = resolve_preview_request(
        option_env!("ABIGAIL_BUILD_PROFILE"),
        env_profile.as_deref(),
        env_root.as_deref(),
        cli_profile,
        cli_root,
    )?;
    let Some(root) = requested else {
        if CURRENT.get().is_some() {
            bail!("A preview process cannot change to the stable profile");
        }
        return Ok(None);
    };
    // Serialize publishing the process context and its lifetime lease. A lease
    // must never be installed for one root while CURRENT names another root.
    let _initialization = INITIALIZATION
        .lock()
        .map_err(|_| anyhow::anyhow!("Preview profile initialization was interrupted"))?;
    // Validate inherited non-secret context before creating even a new marker.
    let expected_id = match std::env::var("ABIGAIL_PROFILE_ID") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error).context("Invalid inherited preview profile identity"),
    };
    if let Some(id) = expected_id.as_deref() {
        validate_uuid(id)?;
    }
    let canonical = canonical_without_links(&root)?;
    reject_cold_staging_root(&canonical)?;
    if let Some(docs) = std::env::var_os("ABIGAIL_DOCUMENTS_DIR") {
        if !same_path(
            &canonical_without_links(Path::new(&docs))?,
            &canonical.join("Documents"),
        ) {
            bail!("Preview Documents override must match the profile Documents directory");
        }
    }
    let context = ProfileContext::inspect(&root, &production_roots(), expected_id.as_deref())?;
    if let Some(existing) = CURRENT.get() {
        if existing.marker != context.marker || !same_path(existing.root(), context.root()) {
            bail!("The process profile is immutable");
        }
        return Ok(Some(existing.clone()));
    }
    context.create_marker_if_missing()?;
    let lease = crate::profile_lease::ProfileUseLease::acquire(&context)?;
    // Inspection precedes the lease so rejected schemas/paths remain a
    // non-writing refusal. Repeat it under the lease before any consumer starts.
    let verified = ProfileContext::inspect(
        context.root(),
        &production_roots(),
        Some(context.profile_id()),
    )?;
    if context.marker != verified.marker || !same_path(context.root(), verified.root()) {
        bail!("Preview profile changed during initialization");
    }
    PROCESS_LEASE
        .set(lease)
        .map_err(|_| anyhow::anyhow!("The process profile lease was initialized concurrently"))?;
    let context = Arc::new(context);
    CURRENT
        .set(context.clone())
        .map_err(|_| anyhow::anyhow!("The process profile was initialized concurrently"))?;
    Ok(Some(context))
}

/// Fallible by design: a preview binary may never fall back to normal paths
/// merely because its entry point forgot to initialize the profile.
pub fn current() -> Result<Option<Arc<ProfileContext>>> {
    if let Some(context) = CURRENT.get() {
        return Ok(Some(context.clone()));
    }
    if compiled_preview() || std::env::var_os("ABIGAIL_PROFILE").is_some_and(|v| v != "stable") {
        bail!("Preview profile preflight has not completed");
    }
    Ok(None)
}

impl ProfileContext {
    /// Explicit dependency-injection seam for synthetic tests and tools. The
    /// caller supplies forbidden roots; no OS profile or keyring is read.
    pub fn preflight(root: &Path, forbidden_roots: &[PathBuf]) -> Result<Self> {
        let context = Self::inspect(root, forbidden_roots, None)?;
        context.create_marker_if_missing()?;
        Ok(context)
    }

    /// Inspect an existing profile without initialization, migration, credential
    /// access or a process lease. Offline tools take a cold lease separately.
    pub fn inspect_existing(root: &Path, forbidden_roots: &[PathBuf]) -> Result<Self> {
        let context = Self::inspect(root, forbidden_roots, None)?;
        let marker: ProfileMarker = serde_json::from_str(
            &fs::read_to_string(context.root.join("profile.json"))
                .context("Cold profile operation requires an existing profile.json marker")?,
        )
        .context("Invalid existing preview profile marker")?;
        if marker != context.marker {
            bail!("Preview profile marker changed during inspection");
        }
        Ok(context)
    }

    /// Validate only an existing root and its expected compatible marker before
    /// acquiring cold ownership. Damaged active configs are deliberately not
    /// read: recovery must be able to exclude a broken original profile.
    /// This does not establish that the profile's data is valid or stopped.
    pub fn inspect_for_cold_ownership(
        root: &Path,
        forbidden_roots: &[PathBuf],
        expected_profile_id: &str,
    ) -> Result<Self> {
        validate_uuid(expected_profile_id)?;
        Self::inspect_marker(root, forbidden_roots, Some(expected_profile_id))
            .context("Cannot identify the existing preview profile for cold ownership")
    }

    fn inspect(
        root: &Path,
        forbidden_roots: &[PathBuf],
        expected_id: Option<&str>,
    ) -> Result<Self> {
        let context = Self::inspect_marker(root, forbidden_roots, expected_id)?;
        context.validate_existing_files()?;
        Ok(context)
    }

    fn inspect_marker(
        root: &Path,
        forbidden_roots: &[PathBuf],
        expected_id: Option<&str>,
    ) -> Result<Self> {
        let root = canonical_without_links(root)?;
        for forbidden in forbidden_roots {
            // Do not inspect a normal profile. Compare its lexical path; the
            // preview path itself is canonical and may not contain links.
            let forbidden = lexical_absolute(forbidden)?;
            if path_within(&root, &forbidden) || path_within(&forbidden, &root) {
                bail!("Preview root overlaps a production-owned location");
            }
        }
        let marker_path = root.join("profile.json");
        canonical_without_links(&marker_path)?;
        let marker = match fs::read_to_string(&marker_path) {
            Ok(json) => {
                let marker: ProfileMarker = serde_json::from_str(&json)
                    .context("Invalid preview profile marker; no files were changed")?;
                if marker.schema_version != PROFILE_SCHEMA_VERSION {
                    bail!(
                        "Unsupported preview profile schema {}; no files were changed",
                        marker.schema_version
                    );
                }
                if marker.kind != "preview" || marker.channel != "beta" {
                    bail!("Profile marker belongs to a different application/channel");
                }
                validate_uuid(&marker.profile_id)?;
                marker
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if root.try_exists()? && fs::read_dir(&root)?.next().transpose()?.is_some() {
                    bail!("A populated unmarked preview root requires explicit import");
                }
                if expected_id.is_some() {
                    bail!("A child cannot initialize a missing parent profile marker");
                }
                ProfileMarker {
                    schema_version: PROFILE_SCHEMA_VERSION,
                    kind: "preview".into(),
                    channel: "beta".into(),
                    profile_id: new_instance_id(),
                }
            }
            Err(error) => return Err(error).context("Cannot read preview profile marker"),
        };
        if expected_id.is_some_and(|id| id != marker.profile_id) {
            bail!("Inherited profile identity does not match the selected root");
        }
        Ok(Self { root, marker })
    }

    fn create_marker_if_missing(&self) -> Result<()> {
        let marker_path = self.root.join("profile.json");
        if marker_path.try_exists()? {
            canonical_without_links(&marker_path)?;
            let observed: ProfileMarker = serde_json::from_str(&fs::read_to_string(&marker_path)?)?;
            if observed != self.marker {
                bail!("Preview marker changed during initialization");
            }
            return Ok(());
        }
        // Revalidate after inspection and before the only initialization write.
        canonical_without_links(&self.root)?;
        fs::create_dir_all(&self.root)?;
        if fs::read_dir(&self.root)?.next().transpose()?.is_some() {
            bail!("Preview root changed during initialization; refusing adoption");
        }
        let temp = self
            .root
            .join(format!(".profile-{}.tmp", new_instance_id()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(&serde_json::to_vec_pretty(&self.marker)?)?;
            file.sync_all()?;
            drop(file);
            // Atomic, no-clobber publication. Failure is closed, never replace a
            // concurrently published marker or adopt its different profile.
            publish_marker(&temp, &marker_path).context("Cannot publish preview profile marker")?;
            Ok(())
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    fn validate_existing_files(&self) -> Result<()> {
        if !self.root.try_exists()? {
            return Ok(());
        }
        let mut pending = vec![self.root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                reject_link(&metadata)?;
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else if entry.file_name() == "config.json" && self.is_active_config(&entry.path())
                {
                    let config =
                        AppConfig::from_json_compatible(&fs::read_to_string(entry.path())?)?;
                    self.validate_config_at_path(&config, &entry.path())?;
                }
            }
        }
        let registry = self.root.join("global_settings.json");
        if registry.try_exists()? {
            let config: GlobalConfig = serde_json::from_str(&fs::read_to_string(registry)?)?;
            self.validate_global_config(&config)?;
        }
        Ok(())
    }

    fn is_active_config(&self, path: &Path) -> bool {
        path.parent().is_some_and(|parent| {
            same_path(parent, &self.root)
                || parent
                    .parent()
                    .is_some_and(|grandparent| same_path(grandparent, &self.identities_dir()))
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn profile_id(&self) -> &str {
        &self.marker.profile_id
    }
    pub fn channel(&self) -> &str {
        &self.marker.channel
    }
    pub fn is_preview(&self) -> bool {
        true
    }
    pub fn credential_namespace(&self) -> String {
        format!("preview.{}", self.profile_id())
    }
    pub fn identities_dir(&self) -> PathBuf {
        self.root.join("identities")
    }
    pub fn shared_memory_path(&self) -> PathBuf {
        self.root.join("memory.db")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn entity_documents(&self, id: &str) -> Result<PathBuf> {
        self.entity_path("Documents", id)
    }
    pub fn entity_tmp(&self, id: &str) -> Result<PathBuf> {
        self.entity_path("tmp", id)
    }
    pub fn entity_archives(&self, id: &str) -> Result<PathBuf> {
        self.entity_path("archives", id)
    }
    fn entity_path(&self, purpose: &str, id: &str) -> Result<PathBuf> {
        validate_uuid(id)?;
        let path = self.root.join(purpose).join(id);
        self.validate_path(&path)?;
        Ok(path)
    }

    pub fn validate_path(&self, path: &Path) -> Result<()> {
        let canonical = canonical_without_links(path)?;
        if !path_within(&canonical, &self.root) {
            bail!("Path is outside the selected preview profile");
        }
        Ok(())
    }

    pub fn validate_config_paths(&self, config: &AppConfig) -> Result<()> {
        for path in [
            &config.data_dir,
            &config.docs_dir,
            &config.models_dir,
            &config.db_path,
        ] {
            self.validate_path(path)?;
        }
        let data = canonical_without_links(&config.data_dir)?;
        if !same_path(&data, &self.root) {
            let identities = self.identities_dir();
            let relative = data
                .strip_prefix(&identities)
                .context("Preview config data directory must identify a registered Entity")?;
            if relative.components().count() != 1 {
                bail!("Preview Entity config directory must contain only its UUID");
            }
            validate_uuid(relative.to_str().context("Invalid Entity UUID directory")?)?;
        }
        for (actual, expected) in [
            (&config.docs_dir, data.join("docs")),
            (&config.models_dir, data.join("models")),
        ] {
            if !same_path(&canonical_without_links(actual)?, &expected) {
                bail!("Preview config cannot redirect its Entity documents or models");
            }
        }
        if !same_path(
            &canonical_without_links(&config.db_path)?,
            &self.shared_memory_path(),
        ) {
            bail!("Preview config must use the profile's shared memory.db");
        }
        if let Some(path) = &config.external_pubkey_path {
            self.validate_path(path)?;
            if !same_path(
                &canonical_without_links(path)?,
                &data.join("external_pubkey.bin"),
            ) {
                bail!("Preview public key must belong to its Entity directory");
            }
        }
        Ok(())
    }

    pub fn validate_config_at_path(&self, config: &AppConfig, path: &Path) -> Result<()> {
        self.validate_path(path)?;
        self.validate_config_paths(config)?;
        let canonical = canonical_without_links(path)?;
        if self.is_active_config(&canonical)
            && !same_path(
                &canonical_without_links(&config.data_dir)?,
                canonical
                    .parent()
                    .context("Config has no parent directory")?,
            )
        {
            bail!("Active preview config belongs to a different Entity directory");
        }
        Ok(())
    }

    pub fn validate_global_config(&self, config: &GlobalConfig) -> Result<()> {
        self.validate_path(&config.master_key_path)?;
        if let Some(path) = &config.hive_secrets_path {
            self.validate_path(path)?;
        }
        for agent in &config.agents {
            validate_uuid(&agent.id)?;
            let expected = PathBuf::from("identities").join(&agent.id);
            let relative = crate::global_config::normalize_agent_directory(&agent.directory)?;
            if relative != expected {
                bail!("Preview registry directory does not match its Entity UUID");
            }
            self.validate_path(&self.root.join(relative))?;
        }
        for attempt in config.creation_attempts.values() {
            validate_uuid(&attempt.entity_id)?;
        }
        Ok(())
    }

    pub fn process_identity(
        &self,
        instance_id: &str,
        hive_instance_id: &str,
    ) -> Result<ProcessIdentity> {
        validate_uuid(instance_id)?;
        validate_uuid(hive_instance_id)?;
        Ok(ProcessIdentity {
            channel: self.channel().into(),
            profile_id: self.profile_id().into(),
            canonical_data_root: self.root.clone(),
            instance_id: instance_id.into(),
            hive_instance_id: hive_instance_id.into(),
        })
    }

    pub fn validate_identity(&self, identity: &ProcessIdentity) -> Result<()> {
        validate_uuid(&identity.instance_id)?;
        validate_uuid(&identity.hive_instance_id)?;
        if identity.channel != self.channel()
            || identity.profile_id != self.profile_id()
            || !same_path(&identity.canonical_data_root, &self.root)
        {
            bail!("Daemon belongs to a different preview profile/channel/root");
        }
        Ok(())
    }
}

/// Lexically derived forbidden roots; this does not inspect their contents.
pub fn production_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dirs) = directories::ProjectDirs::from("com", "abigail", "Abigail") {
        roots.push(dirs.data_local_dir().to_path_buf());
    }
    if let Some(base) = directories::BaseDirs::new() {
        for name in ["Abigail", "com.abigail", "com.abigail.entity-runtime"] {
            roots.push(base.data_local_dir().join(name));
            roots.push(base.data_dir().join(name));
        }
        roots.push(base.home_dir().join(".abigail"));
    }
    if let Some(user) = directories::UserDirs::new() {
        if let Some(docs) = user.document_dir() {
            roots.push(docs.join("Abigail"));
        }
    }
    roots.push(std::env::temp_dir().join("Abigail"));
    roots
}

#[cfg(windows)]
fn publish_marker(temp: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let dest: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // Deliberately omit MOVEFILE_REPLACE_EXISTING: a concurrent marker wins,
    // and this caller fails instead of silently changing profile identity.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(dest.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn publish_marker(temp: &Path, target: &Path) -> Result<()> {
    fs::hard_link(temp, target)?;
    Ok(())
}

fn validate_absolute(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.parent().is_none() {
        bail!("Preview paths must be non-root absolute paths");
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        bail!("Preview paths cannot contain traversal");
    }
    #[cfg(windows)]
    for part in path.components() {
        if let Component::Prefix(prefix) = part {
            if !matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            ) {
                bail!("Preview paths must use a local disk, not a device or network namespace");
            }
        }
        if let Component::Normal(name) = part {
            let name = name.to_string_lossy();
            if name.ends_with(['.', ' ']) || name.contains(':') {
                bail!("Preview path contains an ambiguous Windows component");
            }
        }
    }
    Ok(())
}

fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    validate_absolute(path)?;
    Ok(path.to_path_buf())
}

fn reject_link(metadata: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            bail!("Preview paths cannot contain reparse points");
        }
    }
    if metadata.file_type().is_symlink() {
        bail!("Preview paths cannot contain symbolic links");
    }
    Ok(())
}

/// Resolve a possibly nonexistent path through its existing ancestors without
/// following a symlink/junction. Every existing component is inspected first.
pub fn canonical_without_links(path: &Path) -> Result<PathBuf> {
    validate_absolute(path)?;
    let mut prefix = PathBuf::new();
    let mut existing = PathBuf::new();
    let mut tail = PathBuf::new();
    let mut missing = false;
    for part in path.components() {
        prefix.push(part.as_os_str());
        // A Windows prefix such as C: or \\?\C: is not itself a filesystem
        // root. Wait for RootDir before querying metadata.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        if missing {
            tail.push(part.as_os_str());
            continue;
        }
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) => {
                reject_link(&metadata)?;
                existing = prefix.clone();
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                tail.push(part.as_os_str());
            }
            Err(error) => return Err(error).context("Cannot inspect preview path"),
        }
    }
    if existing.as_os_str().is_empty() {
        bail!("Preview path has no accessible existing ancestor");
    }
    let canonical = fs::canonicalize(existing)?;
    Ok(if tail.as_os_str().is_empty() {
        canonical
    } else {
        canonical.join(tail)
    })
}

fn comparable(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        value
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase()
    }
    #[cfg(not(windows))]
    {
        value.into_owned()
    }
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    comparable(left) == comparable(right)
}

fn path_within(path: &Path, root: &Path) -> bool {
    let path = comparable(path);
    let root = comparable(root);
    path == root || path.starts_with(&(root + std::path::MAIN_SEPARATOR_STR))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("preview-profile-{}", new_instance_id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn root(&self) -> PathBuf {
            self.0.join("ongoing")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut result = std::collections::BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    result.insert(relative, None);
                    pending.push(path);
                } else {
                    result.insert(relative, Some(fs::read(path).unwrap()));
                }
            }
        }
        result
    }

    /// Exercise the real environment/OnceLock entry point in a fresh unit-test
    /// child, never an app or daemon. All fixture inputs and TEMP are explicit.
    fn check_startup_refusal_in_child(case: &str, expected_error: &str) {
        let fixture = Fixture::new();
        let profile = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let entity_dir = profile.identities_dir().join(new_instance_id());
        fs::create_dir_all(&entity_dir).unwrap();
        let mut config = AppConfig::defaults_at(&entity_dir);
        config.db_path = profile.shared_memory_path();
        if case == "newer-schema" {
            config.schema_version = crate::CONFIG_SCHEMA_VERSION + 1;
        }
        fs::write(
            entity_dir.join("config.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        let sibling = fixture.0.join("synthetic-stable");
        fs::create_dir_all(sibling.join("Documents")).unwrap();
        for name in ["master.key", "memory.db", "Documents/keep.txt"] {
            fs::write(sibling.join(name), b"synthetic sentinel: do not modify").unwrap();
        }
        let child_temp = fixture.0.join("child-temp");
        fs::create_dir(&child_temp).unwrap();
        fs::write(
            fixture.0.join("unit-test-fixture"),
            b"initialize-process-refusal-v1",
        )
        .unwrap();
        let before = snapshot(&fixture.0);

        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "profile::tests::startup_refusal_child_fixture",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .env("TEMP", &child_temp)
            .env("TMP", &child_temp)
            .env("TMPDIR", &child_temp)
            .env("ABIGAIL_PROFILE", "preview")
            .env("ABIGAIL_DATA_DIR", profile.root())
            .env(
                "ABIGAIL_PROFILE_ID",
                if case == "wrong-profile-id" {
                    new_instance_id()
                } else {
                    profile.profile_id().to_string()
                },
            )
            .env("ABIGAIL_PROFILE_TEST_GATE", "initialize-process-refusal-v1")
            .env("ABIGAIL_PROFILE_TEST_CONTAINER", &fixture.0)
            .env("ABIGAIL_PROFILE_TEST_CLI_PROFILE", "preview")
            .env("ABIGAIL_PROFILE_TEST_CLI_ROOT", profile.root())
            .env("ABIGAIL_PROFILE_TEST_EXPECTED_ERROR", expected_error);
        // Windows loader support only; no user/provider/credential environment.
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                child.env(name, value);
            }
        }
        if case == "outside-documents" {
            child.env("ABIGAIL_DOCUMENTS_DIR", sibling.join("Documents"));
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "Startup refusal child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(snapshot(&fixture.0), before);
    }

    #[test]
    fn startup_entry_refuses_newer_active_config_without_writes() {
        check_startup_refusal_in_child("newer-schema", "newer than this Abigail version supports");
    }

    #[test]
    fn startup_entry_refuses_inherited_profile_mismatch_without_writes() {
        check_startup_refusal_in_child(
            "wrong-profile-id",
            "Inherited profile identity does not match the selected root",
        );
    }

    #[test]
    fn startup_entry_refuses_outside_documents_override_without_writes() {
        check_startup_refusal_in_child(
            "outside-documents",
            "Preview Documents override must match the profile Documents directory",
        );
    }

    #[test]
    #[ignore = "Only the synthetic startup parent tests may invoke this child fixture"]
    fn startup_refusal_child_fixture() {
        assert_eq!(
            std::env::var("ABIGAIL_PROFILE_TEST_GATE").as_deref(),
            Ok("initialize-process-refusal-v1"),
            "This ignored fixture requires the explicit synthetic parent gate"
        );
        let container = PathBuf::from(std::env::var_os("ABIGAIL_PROFILE_TEST_CONTAINER").unwrap());
        let root = PathBuf::from(std::env::var_os("ABIGAIL_PROFILE_TEST_CLI_ROOT").unwrap());
        let cli_profile = std::env::var("ABIGAIL_PROFILE_TEST_CLI_PROFILE").unwrap();
        let expected_error = std::env::var("ABIGAIL_PROFILE_TEST_EXPECTED_ERROR").unwrap();
        assert!(container.is_absolute());
        assert!(container
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("preview-profile-"));
        assert!(same_path(
            &canonical_without_links(&container.join("ongoing")).unwrap(),
            &root
        ));
        assert_eq!(
            fs::read(container.join("unit-test-fixture")).unwrap(),
            b"initialize-process-refusal-v1"
        );
        assert!(CURRENT.get().is_none());
        let error = initialize_process(Some(&cli_profile), Some(&root)).unwrap_err();
        assert!(
            format!("{error:#}").contains(&expected_error),
            "Unexpected startup refusal: {error:#}"
        );
        assert!(
            CURRENT.get().is_none(),
            "Rejected preflight installed a context"
        );
        assert!(
            current().is_err(),
            "Rejected preflight allowed a stable fallback"
        );
        for name in [
            "logs",
            "memory.db",
            "runtime",
            "webview",
            "vault.sentinel",
            "master.key",
        ] {
            assert!(!root.join(name).exists(), "Startup created {name}");
        }
    }

    #[test]
    fn compiled_preview_never_silently_becomes_stable() {
        let fixture = Fixture::new();
        let root = fixture.root();
        for (mode, path) in [
            (None, None),
            (Some("stable"), Some(root.as_path())),
            (Some("preview"), None),
            (Some(""), Some(root.as_path())),
        ] {
            assert!(resolve_preview_request(Some("preview"), mode, path, None, None).is_err());
        }
        assert!(resolve_preview_request(None, None, None, None, None)
            .unwrap()
            .is_none());
        assert!(resolve_preview_request(
            Some("stable"),
            Some("preview"),
            Some(&root),
            Some("preview"),
            Some(&root)
        )
        .unwrap()
        .is_some());
        assert!(resolve_preview_request(
            Some("preview"),
            Some("preview"),
            Some(&root),
            Some("stable"),
            Some(&root)
        )
        .is_err());
        assert!(resolve_preview_request(
            Some("preview"),
            Some("preview"),
            Some(&root),
            Some("preview"),
            Some(&fixture.0.join("other"))
        )
        .is_err());
        assert!(!root.exists());
    }

    #[test]
    fn profile_marker_reopens_without_changing_identity_or_bytes() {
        let fixture = Fixture::new();
        let first = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let bytes = fs::read(first.root().join("profile.json")).unwrap();
        fs::write(first.root().join("synthetic-note.txt"), b"ongoing").unwrap();
        let reopened = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        assert_eq!(first.profile_id(), reopened.profile_id());
        assert_eq!(bytes, fs::read(first.root().join("profile.json")).unwrap());
        assert_eq!(
            fs::read(first.root().join("synthetic-note.txt")).unwrap(),
            b"ongoing"
        );
    }

    #[test]
    fn rejects_overlap_relative_roots_and_unmarked_populated_roots_without_writes() {
        let fixture = Fixture::new();
        let stable = fixture.0.join("production");
        fs::create_dir_all(&stable).unwrap();
        fs::write(stable.join("sentinel"), b"untouched").unwrap();
        for root in [stable.clone(), stable.join("nested"), fixture.0.clone()] {
            assert!(ProfileContext::preflight(&root, std::slice::from_ref(&stable)).is_err());
            assert!(!root.join("profile.json").exists());
        }
        assert!(ProfileContext::preflight(Path::new("relative"), &[]).is_err());
        fs::create_dir_all(fixture.root()).unwrap();
        fs::write(fixture.root().join("existing"), b"unmarked").unwrap();
        assert!(ProfileContext::preflight(&fixture.root(), &[]).is_err());
        assert!(!fixture.root().join("profile.json").exists());
        assert_eq!(fs::read(stable.join("sentinel")).unwrap(), b"untouched");
    }

    #[test]
    fn rejects_newer_or_wrong_marker_and_preserves_bytes() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        for change in ["schema", "channel", "uuid"] {
            let mut marker = context.marker.clone();
            match change {
                "schema" => marker.schema_version += 1,
                "channel" => marker.channel = "stable".into(),
                _ => marker.profile_id = "../outside".into(),
            }
            let bytes = serde_json::to_vec_pretty(&marker).unwrap();
            fs::write(context.root().join("profile.json"), &bytes).unwrap();
            assert!(ProfileContext::preflight(&fixture.root(), &[]).is_err());
            assert_eq!(
                fs::read(context.root().join("profile.json")).unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn newer_config_refuses_whole_profile_preflight_without_migration() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let mut config = AppConfig::defaults_at(context.root());
        config.schema_version = crate::CONFIG_SCHEMA_VERSION + 1;
        let bytes = serde_json::to_vec_pretty(&config).unwrap();
        fs::write(context.root().join("config.json"), &bytes).unwrap();
        let marker = fs::read(context.root().join("profile.json")).unwrap();
        assert!(ProfileContext::preflight(&fixture.root(), &[]).is_err());
        assert_eq!(fs::read(context.root().join("config.json")).unwrap(), bytes);
        assert_eq!(
            fs::read(context.root().join("profile.json")).unwrap(),
            marker
        );
        assert!(!context.logs_dir().exists());
        assert!(!context.shared_memory_path().exists());
    }

    #[test]
    fn config_and_registry_cannot_redirect_outside_profile() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let outside = fixture.0.join("production");
        let mut config = AppConfig::defaults_at(context.root());
        assert!(context.validate_config_paths(&config).is_ok());
        config.docs_dir = outside.clone();
        assert!(context.validate_config_paths(&config).is_err());
        config = AppConfig::defaults_at(context.root());
        config.db_path = context.root().join("other.db");
        assert!(context.validate_config_paths(&config).is_err());
        let mut registry = GlobalConfig::new(context.root());
        registry.master_key_path = outside.join("master.key");
        assert!(context.validate_global_config(&registry).is_err());
        assert!(!outside.exists());
    }

    #[test]
    fn ownership_rejects_other_channel_profile_root_and_stale_instance() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let instance = new_instance_id();
        let expected = context.process_identity(&instance, &instance).unwrap();
        assert!(context.validate_identity(&expected).is_ok());
        for field in ["channel", "profile", "root", "instance"] {
            let mut wrong = expected.clone();
            match field {
                "channel" => wrong.channel = "stable".into(),
                "profile" => wrong.profile_id = new_instance_id(),
                "root" => wrong.canonical_data_root = fixture.0.join("elsewhere"),
                _ => wrong.instance_id = new_instance_id(),
            }
            assert_ne!(wrong, expected);
            if field != "instance" {
                assert!(context.validate_identity(&wrong).is_err());
            }
        }
    }

    #[test]
    fn entity_paths_are_uuid_scoped_and_reject_traversal() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let one = new_instance_id();
        let two = new_instance_id();
        assert_ne!(
            context.entity_documents(&one).unwrap(),
            context.entity_documents(&two).unwrap()
        );
        for invalid in [
            "../outside",
            "friendly name",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert!(context.entity_documents(invalid).is_err());
            assert!(context.entity_tmp(invalid).is_err());
        }
        assert!(context
            .validate_path(&context.root().join("../outside"))
            .is_err());
    }

    #[test]
    fn marker_publication_cannot_replace_an_existing_owner() {
        let fixture = Fixture::new();
        let source = fixture.0.join("candidate.tmp");
        let target = fixture.0.join("profile.json");
        fs::write(&source, b"new candidate").unwrap();
        fs::write(&target, b"existing owner").unwrap();
        assert!(publish_marker(&source, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"existing owner");
        assert_eq!(fs::read(&source).unwrap(), b"new candidate");
    }

    #[test]
    fn startup_does_not_interpret_webview_skill_or_archived_configs_as_active() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        for folder in [
            "webview/hive/browser",
            "skills/example",
            "backups/older-snapshot",
        ] {
            let dir = context.root().join(folder);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("config.json"),
                b"{\"schema_version\":999,\"foreign\":true}",
            )
            .unwrap();
        }
        let reopened = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        assert_eq!(reopened.profile_id(), context.profile_id());
        assert_eq!(
            fs::read(context.backups_dir().join("older-snapshot/config.json")).unwrap(),
            b"{\"schema_version\":999,\"foreign\":true}"
        );
    }

    #[test]
    fn active_entity_config_cannot_select_another_entitys_paths() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let first = context.identities_dir().join(new_instance_id());
        let other = context.identities_dir().join(new_instance_id());
        fs::create_dir_all(&first).unwrap();
        let mut config = AppConfig::defaults_at(&other);
        config.db_path = context.shared_memory_path();
        let bytes = serde_json::to_vec_pretty(&config).unwrap();
        fs::write(first.join("config.json"), &bytes).unwrap();
        assert!(ProfileContext::preflight(&fixture.root(), &[]).is_err());
        assert_eq!(fs::read(first.join("config.json")).unwrap(), bytes);
        config.data_dir = first.clone();
        assert!(context.validate_config_paths(&config).is_err());
        assert!(!other.exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_links_before_reading_outside_profile() {
        let fixture = Fixture::new();
        let context = ProfileContext::preflight(&fixture.root(), &[]).unwrap();
        let outside = fixture.0.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"unchanged").unwrap();
        std::os::unix::fs::symlink(&outside, context.root().join("Documents")).unwrap();
        assert!(context.entity_documents(&new_instance_id()).is_err());
        assert!(ProfileContext::preflight(&fixture.root(), &[]).is_err());
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unchanged");
    }
}
