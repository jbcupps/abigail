//! Cold, same-device preview recovery. No daemon, keyring bootstrap or import.
//!
//! The bundle is an uncompressed directory, not an authenticated or encrypted
//! archive. Every retained file is inventoried; transient state and backup
//! history are explicitly excluded. Atomic publication is currently Windows-only.

mod state;
#[cfg(test)]
mod tests;

use crate::profile::{canonical_without_links, new_instance_id, same_path, ProfileContext};
use crate::profile_lease::ColdProfileLease;
use crate::vault::unlock::{RestoreCredentialProbe, RestoreCredentialVerification};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const EXCLUDED: [&str; 4] = ["backups", "logs", "runtime", "tmp"];
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const CREDENTIAL_POLICY: &str =
    "same-device-existing-credentials-required; external-keyring-not-included";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEntry {
    pub path: String,
    pub kind: EntryKind,
    pub length: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub profile_id: String,
    pub channel: String,
    pub source_root: PathBuf,
    pub created_at_utc: String,
    pub credential_policy: String,
    pub excluded_top_level_directories: Vec<String>,
    pub entity_ids: Vec<String>,
    pub creation_receipt_count: usize,
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Serialize)]
pub struct RestoreReceipt {
    pub profile_id: String,
    pub destination: PathBuf,
    pub entity_ids: Vec<String>,
    pub creation_receipt_count: usize,
    pub credential_verification: RestoreCredentialVerification,
    pub runtime_descriptors_restored: bool,
    pub original_source_present_and_locked: bool,
}

/// Snapshot a stopped existing preview. Does not access external credentials.
/// `bundle` must not exist, and must be outside the source profile.
pub fn snapshot(
    source: &Path,
    bundle: &Path,
    forbidden_roots: &[PathBuf],
) -> Result<SnapshotManifest> {
    require_publication_platform()?;
    let profile = ProfileContext::inspect_existing(source, forbidden_roots)?;
    crate::profile::reject_cold_staging_root(profile.root())?;
    let _cold = ColdProfileLease::acquire(&profile)
        .context("Close Hive, every Entity and both app windows before making a cold snapshot")?;
    let profile = ProfileContext::inspect_existing(source, forbidden_roots)?;
    let target = fresh_target(bundle, forbidden_roots)?;
    disjoint(profile.root(), &target)?;
    let summary = state::inspect(profile.root(), profile.root(), profile.profile_id())?;
    let entries = inventory(profile.root(), true)?;
    // Deny write/delete sharing on every retained Windows file throughout the
    // copy. An independently surviving WebView writer must cause refusal.
    let _files = ReadGuards::acquire(profile.root(), &entries)?;
    ensure!(
        inventory(profile.root(), true)? == entries,
        "Profile changed before cold copy"
    );
    let manifest = SnapshotManifest {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        profile_id: profile.profile_id().to_string(),
        channel: profile.channel().to_string(),
        source_root: profile.root().to_path_buf(),
        created_at_utc: chrono::Utc::now().to_rfc3339(),
        credential_policy: CREDENTIAL_POLICY.into(),
        excluded_top_level_directories: EXCLUDED.iter().map(|s| s.to_string()).collect(),
        entity_ids: summary.entity_ids,
        creation_receipt_count: summary.creation_receipt_count,
        entries,
    };
    let staging = Staging::create(&target)?;
    let payload = staging.path.join("profile");
    fs::create_dir(&payload)?;
    copy_entries(profile.root(), &payload, &manifest.entries)?;
    ensure!(
        inventory(profile.root(), true)? == manifest.entries,
        "Profile changed while copying; no snapshot was published"
    );
    ensure!(
        inventory(&payload, false)? == manifest.entries,
        "Snapshot copy failed its file/hash verification"
    );
    write_new(
        &staging.path.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    verify_snapshot_inner(&staging.path)?;
    staging.publish(&target)?;
    Ok(manifest)
}

/// Verify the complete bundle without credentials, migration or store opening.
pub fn verify_snapshot(bundle: &Path) -> Result<SnapshotManifest> {
    let bundle = canonical_without_links(bundle)?;
    crate::profile::reject_cold_staging_root(&bundle)?;
    verify_snapshot_inner(&bundle)
}

fn verify_snapshot_inner(bundle: &Path) -> Result<SnapshotManifest> {
    let bundle = canonical_without_links(bundle)?;
    ensure!(bundle.is_dir(), "Snapshot bundle must be a directory");
    let mut names = fs::read_dir(&bundle)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    ensure!(
        names == ["manifest.json", "profile"].map(std::ffi::OsString::from),
        "Snapshot bundle must contain exactly manifest.json and profile"
    );
    let manifest_path = canonical_without_links(&bundle.join("manifest.json"))?;
    let metadata = fs::metadata(&manifest_path)?;
    ensure!(
        metadata.is_file() && metadata.len() <= MAX_MANIFEST,
        "Snapshot manifest is not a bounded regular file"
    );
    let manifest: SnapshotManifest = serde_json::from_slice(&fs::read(manifest_path)?)
        .context("Malformed or unsupported snapshot manifest")?;
    ensure!(
        manifest.schema_version == SNAPSHOT_SCHEMA_VERSION,
        "Unsupported snapshot schema {}; supported schema is {}",
        manifest.schema_version,
        SNAPSHOT_SCHEMA_VERSION
    );
    crate::profile::validate_uuid(&manifest.profile_id)?;
    ensure!(
        manifest.channel == "beta",
        "Snapshot is not a beta preview profile"
    );
    ensure!(
        manifest.source_root.is_absolute()
            && !manifest
                .source_root
                .components()
                .any(|p| matches!(p, Component::ParentDir | Component::CurDir)),
        "Snapshot source root must be an absolute path without traversal"
    );
    ensure!(
        manifest.credential_policy == CREDENTIAL_POLICY,
        "Unsupported snapshot credential policy"
    );
    ensure!(
        manifest.excluded_top_level_directories == EXCLUDED,
        "Unsupported snapshot exclusions"
    );
    chrono::DateTime::parse_from_rfc3339(&manifest.created_at_utc)
        .context("Invalid snapshot creation timestamp")?;
    validate_entries(&manifest.entries)?;
    let payload = canonical_without_links(&bundle.join("profile"))?;
    ensure!(payload.is_dir(), "Snapshot profile payload is missing");
    ensure!(
        inventory(&payload, false)? == manifest.entries,
        "Snapshot inventory/hash mismatch: missing, extra or changed profile files"
    );
    let summary = state::inspect(&payload, &manifest.source_root, &manifest.profile_id)?;
    ensure!(
        summary.entity_ids == manifest.entity_ids
            && summary.creation_receipt_count == manifest.creation_receipt_count,
        "Snapshot registry summary disagrees with its payload"
    );
    Ok(manifest)
}

/// Restore into a nonexistent destination. All checks and credential probing
/// occur in private staging; failure leaves no destination and preserves bundle.
/// The caller must supply a read-existing-only credential probe.
pub fn restore(
    bundle: &Path,
    destination: &Path,
    forbidden_roots: &[PathBuf],
    credentials: &dyn RestoreCredentialProbe,
) -> Result<RestoreReceipt> {
    require_publication_platform()?;
    let bundle = canonical_without_links(bundle)?;
    let manifest = verify_snapshot(&bundle)?;
    let original_owner = if manifest.source_root.try_exists()? {
        let original = ProfileContext::inspect_for_cold_ownership(
            &manifest.source_root,
            forbidden_roots,
            &manifest.profile_id,
        )?;
        Some(ColdProfileLease::acquire(&original).context(
            "The original preview must be stopped before recovering its identity into another root",
        )?)
    } else {
        None
    };
    let _files = ReadGuards::acquire(&bundle.join("profile"), &manifest.entries)?;
    let _manifest_file = read_guard(&bundle.join("manifest.json"))?;
    let destination = fresh_target(destination, forbidden_roots)?;
    disjoint(&bundle, &destination)?;
    // Do not replace the original profile, even if a user deleted its directory.
    disjoint(&manifest.source_root, &destination)?;
    let staging = Staging::create(&destination)?;
    copy_entries(&bundle.join("profile"), &staging.path, &manifest.entries)?;
    ensure!(
        inventory(&staging.path, false)? == manifest.entries,
        "Restore copy failed its file/hash verification"
    );
    // Validate the input again after copying; an incomplete concurrently edited
    // bundle cannot become a completed restore.
    ensure!(
        verify_snapshot(&bundle)? == manifest,
        "Snapshot changed during restore"
    );
    state::rebase(&staging.path, &manifest.source_root, &staging.path)?;
    let staged_profile = Arc::new(ProfileContext::inspect_existing(
        &staging.path,
        forbidden_roots,
    )?);
    let credential_verification = credentials
        .verify_profile(staged_profile)
        .context("Restore requires the existing credentials for this preview profile; no replacement key was created")?;
    // Only active declared config paths change; user text, opaque WebView data
    // and exact creation keys retain their original bytes.
    state::rebase(&staging.path, &staging.path, &destination)?;
    let final_state = state::inspect(&staging.path, &destination, &manifest.profile_id)?;
    ensure!(
        final_state.entity_ids == manifest.entity_ids
            && final_state.creation_receipt_count == manifest.creation_receipt_count,
        "Restored identity or receipt summary changed"
    );
    staging.publish(&destination)?;
    Ok(RestoreReceipt {
        profile_id: manifest.profile_id,
        destination,
        entity_ids: manifest.entity_ids,
        creation_receipt_count: manifest.creation_receipt_count,
        credential_verification,
        runtime_descriptors_restored: false,
        original_source_present_and_locked: original_owner.is_some(),
    })
}

fn fresh_target(target: &Path, forbidden_roots: &[PathBuf]) -> Result<PathBuf> {
    let target = canonical_without_links(target)?;
    crate::profile::reject_cold_staging_root(&target)?;
    ensure!(
        !target.try_exists()?,
        "Destination already exists; choose a fresh directory"
    );
    ensure!(
        target.parent().is_some_and(Path::is_dir),
        "Destination parent must already exist"
    );
    for forbidden in forbidden_roots {
        disjoint(&target, forbidden).context("Destination overlaps a production-owned location")?;
    }
    Ok(target)
}

fn within(path: &Path, root: &Path) -> bool {
    path.ancestors().any(|p| same_path(p, root))
}

fn disjoint(left: &Path, right: &Path) -> Result<()> {
    ensure!(
        !within(left, right) && !within(right, left),
        "Source, snapshot and recovery destination must not overlap"
    );
    Ok(())
}

fn relative_path(value: &str) -> Result<PathBuf> {
    ensure!(
        !value.is_empty() && !value.contains('\\'),
        "Invalid inventory path"
    );
    let mut output = PathBuf::new();
    for part in value.split('/') {
        ensure!(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(':')
                && !part.contains(['<', '>', '"', '|', '?', '*'])
                && !part.ends_with(['.', ' '])
                && !part.chars().any(char::is_control),
            "Unsafe inventory path"
        );
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        ensure!(
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9')),
            "Inventory path uses a reserved Windows device name"
        );
        let component = Path::new(part);
        ensure!(
            component.components().count() == 1
                && matches!(component.components().next(), Some(Component::Normal(_))),
            "Unsafe inventory component"
        );
        output.push(part);
    }
    Ok(output)
}

fn validate_entries(entries: &[SnapshotEntry]) -> Result<()> {
    ensure!(
        entries.len() <= MAX_ENTRIES,
        "Snapshot has too many entries"
    );
    let mut seen = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut previous = "";
    for entry in entries {
        let relative = relative_path(&entry.path)?;
        ensure!(
            entry.path.as_str() > previous,
            "Snapshot entries must be unique and sorted"
        );
        previous = &entry.path;
        ensure!(
            seen.insert(entry.path.to_lowercase()),
            "Case-colliding snapshot paths"
        );
        let top = entry.path.split('/').next().unwrap_or("");
        ensure!(
            !excluded(top),
            "Snapshot includes excluded runtime/temporary state"
        );
        if let Some(parent) = relative.parent().filter(|p| !p.as_os_str().is_empty()) {
            ensure!(
                directories.contains(parent),
                "Snapshot parent directory is not declared"
            );
        }
        match entry.kind {
            EntryKind::Directory => {
                ensure!(
                    entry.length == 0 && entry.sha256.is_none(),
                    "Malformed directory entry"
                );
                directories.insert(relative);
            }
            EntryKind::File => {
                ensure!(
                    entry.sha256.as_ref().is_some_and(|hash| hash.len() == 64
                        && hash
                            .bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))),
                    "Malformed file SHA-256"
                );
            }
        }
    }
    Ok(())
}

fn inventory(root: &Path, exclude: bool) -> Result<Vec<SnapshotEntry>> {
    let root = canonical_without_links(root)?;
    let mut pending = vec![root.clone()];
    let mut entries = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            canonical_without_links(&path)?; // refuses symlinks and reparse points
            let metadata = fs::symlink_metadata(&path)?;
            let relative = path
                .strip_prefix(&root)?
                .to_str()
                .context("Snapshot paths must be valid Unicode")?
                .replace('\\', "/");
            relative_path(&relative)?;
            if exclude && excluded(&relative) {
                ensure!(metadata.is_dir(), "Excluded state path is not a directory");
                continue;
            }
            let (kind, length, sha256) = if metadata.is_dir() {
                pending.push(path);
                (EntryKind::Directory, 0, None)
            } else {
                ensure!(metadata.is_file(), "Snapshot contains a non-regular file");
                let (length, hash) = hash_file(&path)?;
                (EntryKind::File, length, Some(hash))
            };
            entries.push(SnapshotEntry {
                path: relative,
                kind,
                length,
                sha256,
            });
            ensure!(
                entries.len() <= MAX_ENTRIES,
                "Profile has too many snapshot entries"
            );
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    validate_entries(&entries)?;
    Ok(entries)
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut length = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        length += count as u64;
    }
    let hash = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((length, hash))
}

fn excluded(name: &str) -> bool {
    EXCLUDED
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(name))
}

struct ReadGuards {
    _files: Vec<File>,
}
impl ReadGuards {
    fn acquire(root: &Path, entries: &[SnapshotEntry]) -> Result<Self> {
        let files = entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| {
                let path = canonical_without_links(&root.join(relative_path(&entry.path)?))?;
                read_guard(&path).with_context(|| {
                    format!(
                        "Cannot hold {} cold; close every app or worker writing this profile",
                        entry.path
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { _files: files })
    }
}

fn read_guard(path: &Path) -> Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // FILE_SHARE_READ, deliberately no WRITE or DELETE.
    }
    Ok(options.open(path)?)
}

fn copy_entries(source: &Path, target: &Path, entries: &[SnapshotEntry]) -> Result<()> {
    for entry in entries {
        let relative = relative_path(&entry.path)?;
        let from = canonical_without_links(&source.join(&relative))?;
        let to = canonical_without_links(&target.join(relative))?;
        match entry.kind {
            EntryKind::Directory => fs::create_dir(&to)?,
            EntryKind::File => {
                ensure!(fs::metadata(&from)?.is_file(), "Snapshot file changed type");
                let mut input = File::open(&from)?;
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&to)?;
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
            }
        }
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

struct Staging {
    path: PathBuf,
}
impl Staging {
    fn create(target: &Path) -> Result<Self> {
        ensure!(
            same_path(&canonical_without_links(target)?, target),
            "Destination path changed before staging"
        );
        let parent = target.parent().context("Missing destination parent")?;
        let path = parent.join(format!(".abigail-cold-{}.partial", new_instance_id()));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
    fn publish(self, target: &Path) -> Result<()> {
        ensure!(
            same_path(&canonical_without_links(&self.path)?, &self.path)
                && same_path(&canonical_without_links(target)?, target),
            "Recovery path changed before publication"
        );
        ensure!(
            !target.try_exists()?,
            "Destination appeared during recovery; nothing overwritten"
        );
        publish_directory(&self.path, target).with_context(|| {
            format!(
                "Could not publish cold recovery directory {} to {}; destination was not intentionally overwritten",
                self.path.display(), target.display()
            )
        })?;
        Ok(())
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        // This uniquely created sibling is the only directory this operation
        // owns. A crash may leave it for manual inspection; it is not a bundle.
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn require_publication_platform() -> Result<()> {
    ensure!(
        cfg!(windows),
        "Cold profile publication currently supports Windows only"
    );
    Ok(())
}

#[cfg(windows)]
fn publish_directory(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // Windows scanners may briefly deny a move after the last file was closed.
    // Retry only bounded sharing/access failures when publication is known not
    // to have occurred. Never replace a target, change permissions or retry an
    // uncertain outcome. Same-parent staging keeps the move on one filesystem.
    for attempt in 0..4 {
        let result = unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                let retryable =
                    matches!(error.code().0 as u32, 0x80070005 | 0x80070020 | 0x80070021);
                if attempt == 3 || !retryable || target.try_exists()? || !source.is_dir() {
                    return Err(error.into());
                }
                ensure!(
                    same_path(&canonical_without_links(source)?, source)
                        && same_path(&canonical_without_links(target)?, target),
                    "Recovery path changed after refused publication"
                );
                std::thread::sleep(std::time::Duration::from_millis([25, 75, 150][attempt]));
            }
        }
    }
    unreachable!("bounded publication loop always returns")
}
#[cfg(not(windows))]
fn publish_directory(_source: &Path, _target: &Path) -> Result<()> {
    anyhow::bail!("Cold profile publication currently supports Windows only")
}
