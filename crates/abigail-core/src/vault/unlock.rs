//! Hybrid unlock providers for obtaining the root Key Encryption Key (KEK).
//!
//! Priority order: the current profile's session cache, OS credential store,
//! then its Windows DPAPI fallback file. Stable mode also retains the existing
//! raw-key and passphrase environment fallbacks. Preview mode never reads those
//! environment secrets or the stable credential entry.
//!
//! On first run, the provider generates a random 32-byte KEK and stores it in
//! the OS credential store. Subsequent runs retrieve it.

use crate::error::{CoreError, Result};
use crate::secure_fs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const KEK_LEN: usize = super::VAULT_KEK_LEN;
const KEYRING_SERVICE: &str = "com.abigail.vault";
const KEYRING_ACCOUNT: &str = "abigail.hive.master-kek-v1";
const PASSPHRASE_ENV: &str = "ABIGAIL_VAULT_PASSPHRASE";
const RAW_KEK_ENV: &str = "ABIGAIL_VAULT_RAW_KEY";
const PASSPHRASE_SALT: &[u8] = b"abigail-vault-passphrase-salt-v1";
const KDF_METADATA_FILE: &str = "vault.kdf.json";
const PROCESS_VAULT_DATA_DIR_ENV: &str = "ABIGAIL_VAULT_DATA_DIR";
#[cfg(windows)]
const WINDOWS_KEK_FALLBACK_FILE: &str = "vault.kek.dpapi";
const ARGON2_MEMORY_COST_KIB: u32 = 64 * 1024;
const ARGON2_TIME_COST: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;
const ARGON2_SALT_LEN: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PassphraseKdfMetadata {
    version: u8,
    algorithm: String,
    salt_base64: Option<String>,
    memory_cost_kib: Option<u32>,
    time_cost: Option<u32>,
    parallelism: Option<u32>,
}

impl PassphraseKdfMetadata {
    fn fresh_argon2() -> Self {
        let mut salt = [0u8; ARGON2_SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        Self {
            version: 1,
            algorithm: "argon2id".to_string(),
            salt_base64: Some(base64::engine::general_purpose::STANDARD.encode(salt)),
            memory_cost_kib: Some(ARGON2_MEMORY_COST_KIB),
            time_cost: Some(ARGON2_TIME_COST),
            parallelism: Some(ARGON2_PARALLELISM),
        }
    }

    fn legacy_hkdf() -> Self {
        Self {
            version: 1,
            algorithm: "legacy_hkdf_v1".to_string(),
            salt_base64: None,
            memory_cost_kib: None,
            time_cost: None,
            parallelism: None,
        }
    }
}

/// Trait for obtaining the root KEK that protects all vault data.
pub trait UnlockProvider: Send + Sync {
    /// Return the 32-byte root KEK, generating or bootstrapping if needed.
    fn root_kek(&self) -> Result<[u8; KEK_LEN]>;
}

/// Tries OS credential store first, then passphrase env var fallback.
pub struct HybridUnlockProvider;

impl HybridUnlockProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HybridUnlockProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Non-secret evidence that the existing preview sentinel and master key can
/// both be decrypted. This does not verify provider accounts, every encrypted
/// file, or the availability of any external credential omitted from a backup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreCredentialVerification {
    pub profile_id: String,
    pub credential_namespace: String,
    pub master_public_key: [u8; 32],
}

/// Cold-restore credential check. Callers retain the stopped-profile guard
/// while probing so files cannot change between hash and credential checks.
/// Implementations return public evidence, never the root encryption key.
pub trait RestoreCredentialProbe: Send + Sync {
    fn verify_profile(
        &self,
        profile: Arc<crate::profile::ProfileContext>,
    ) -> Result<RestoreCredentialVerification>;
}

/// Read existing preview credentials from the profile's OS entry or its
/// copied same-device fallback. No generation, repair, cache access or write
/// occurs. A missing credential is a recovery error, never a bootstrap request.
#[derive(Default)]
pub struct SystemRestoreCredentialProbe;

impl RestoreCredentialProbe for SystemRestoreCredentialProbe {
    fn verify_profile(
        &self,
        profile: Arc<crate::profile::ProfileContext>,
    ) -> Result<RestoreCredentialVerification> {
        verify_restore_credentials_with_backend(profile, &SystemRootKeyBackend)
    }
}

/// Explicit provider seam for synthetic recovery fixtures. The supplied
/// provider must itself be read-only (for example a fixed fake provider or
/// `PassphraseUnlockProvider`); do not supply `HybridUnlockProvider`, whose
/// normal startup contract allows bootstrap and repair. This probe adds no
/// writes or cache behavior and never selects an ambient provider.
pub struct ProvidedRestoreCredentialProbe {
    unlock: Arc<dyn UnlockProvider>,
}

impl ProvidedRestoreCredentialProbe {
    pub fn new(unlock: Arc<dyn UnlockProvider>) -> Self {
        Self { unlock }
    }
}

impl RestoreCredentialProbe for ProvidedRestoreCredentialProbe {
    fn verify_profile(
        &self,
        profile: Arc<crate::profile::ProfileContext>,
    ) -> Result<RestoreCredentialVerification> {
        let context = CredentialContext::for_profile(profile)?;
        let material = RestoreCredentialMaterial::read(&context)?;
        let kek = self.unlock.root_kek().map_err(|error| {
            CoreError::Vault(format!(
                "Cold restore credential unavailable from the supplied provider: {error}"
            ))
        })?;
        material.verify(&context, &kek)
    }
}

struct RestoreCredentialMaterial {
    sentinel: Vec<u8>,
    master: Vec<u8>,
}

impl RestoreCredentialMaterial {
    fn read(context: &CredentialContext) -> Result<Self> {
        let profile = context
            .profile
            .as_ref()
            .ok_or_else(|| CoreError::Vault("Cold restore requires a preview profile".into()))?;
        let marker_path = context.data_root.join("profile.json");
        context.validate_path(&marker_path)?;
        let marker: crate::profile::ProfileMarker =
            serde_json::from_slice(&std::fs::read(&marker_path)?)?;
        if marker.schema_version != crate::profile::PROFILE_SCHEMA_VERSION
            || marker.kind != "preview"
            || marker.channel != profile.channel()
            || marker.profile_id != profile.profile_id()
        {
            return Err(CoreError::Vault(
                "Cold restore profile marker changed or is incompatible; no credentials were read"
                    .into(),
            ));
        }
        let read = |name: &str| -> Result<Vec<u8>> {
            let path = context.data_root.join(name);
            context.validate_path(&path)?;
            let bytes = std::fs::read(&path).map_err(|error| {
                CoreError::Vault(format!(
                    "Cold restore requires existing {name}; no replacement will be generated: {error}"
                ))
            })?;
            if bytes.len() < 29 || !super::crypto::is_vault_envelope(&bytes) {
                return Err(CoreError::Vault(format!(
                    "Cold restore {name} has an unsupported or malformed encrypted envelope"
                )));
            }
            Ok(bytes)
        };
        Ok(Self {
            sentinel: read(super::SENTINEL_FILE_NAME)?,
            master: read("master.key")?,
        })
    }

    fn verify(
        &self,
        context: &CredentialContext,
        kek: &[u8; KEK_LEN],
    ) -> Result<RestoreCredentialVerification> {
        super::decrypt_sentinel_bytes_with_scope(
            &self.sentinel,
            kek,
            &context.scope_label(super::SENTINEL_SCOPE),
        )
        .map_err(|error| {
            CoreError::Vault(format!(
                "Cold restore cannot decrypt vault.sentinel with the existing profile credential: {error}"
            ))
        })?;
        let master = crate::keyring::decode_master_key_envelope(
            &self.master,
            kek,
            &context.scope_label("keyring:master"),
        )
        .map_err(|error| {
            CoreError::Vault(format!(
                "Cold restore cannot decrypt or decode master.key with the existing profile credential: {error}"
            ))
        })?;
        let profile = context.profile.as_ref().expect("validated preview context");
        Ok(RestoreCredentialVerification {
            profile_id: profile.profile_id().to_string(),
            credential_namespace: profile.credential_namespace(),
            master_public_key: master.verifying_key().to_bytes(),
        })
    }
}

fn verify_restore_credentials_with_backend(
    profile: Arc<crate::profile::ProfileContext>,
    backend: &dyn RootKeyBackend,
) -> Result<RestoreCredentialVerification> {
    let context = CredentialContext::for_profile(profile)?;
    // Validate all selected files before asking the backend for a credential.
    let material = RestoreCredentialMaterial::read(&context)?;
    context.validate_path(&context.fallback_path())?;
    let kek = if let Some(kek) = backend.load_keyring(&context)? {
        kek
    } else if let Some(kek) = backend.load_fallback(&context)? {
        kek
    } else {
        return Err(CoreError::Vault(
            "Cold restore credential unavailable. This snapshot does not contain the OS keyring entry; same-device restoration requires the original user's existing preview credential or a decryptable profile DPAPI fallback. No new key was generated.".into(),
        ));
    };
    material.verify(&context, &kek)
}

pub fn configure_process_vault_data_dir(data_root: &Path) {
    std::env::set_var(PROCESS_VAULT_DATA_DIR_ENV, data_root);
}

fn stable_vault_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os(PROCESS_VAULT_DATA_DIR_ENV) {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    crate::AppConfig::default_paths().data_dir
}

pub fn process_vault_data_dir() -> Result<PathBuf> {
    Ok(CredentialContext::current()?.data_root)
}

/// Resolved before cache lookup or any credential access. A preview never
/// consults the ordinary data-root override or ordinary keyring entry.
pub(crate) struct CredentialContext {
    pub(crate) data_root: PathBuf,
    namespace: Option<String>,
    profile: Option<Arc<crate::profile::ProfileContext>>,
}

impl CredentialContext {
    pub(crate) fn current() -> Result<Self> {
        match current_profile()? {
            Some(profile) => Self::for_profile(profile),
            None => Ok(Self::stable(stable_vault_data_dir())),
        }
    }

    fn stable(data_root: PathBuf) -> Self {
        Self {
            data_root,
            namespace: None,
            profile: None,
        }
    }

    fn for_profile(profile: Arc<crate::profile::ProfileContext>) -> Result<Self> {
        profile
            .validate_path(profile.root())
            .map_err(profile_error)?;
        Ok(Self {
            data_root: profile.root().to_path_buf(),
            namespace: Some(profile.credential_namespace()),
            profile: Some(profile),
        })
    }

    pub(crate) fn cache_key(&self) -> (PathBuf, Option<String>) {
        (self.data_root.clone(), self.namespace.clone())
    }

    fn keyring_service(&self) -> String {
        self.namespace.as_ref().map_or_else(
            || KEYRING_SERVICE.to_string(),
            |ns| format!("{KEYRING_SERVICE}.{ns}"),
        )
    }

    fn keyring_account(&self) -> String {
        self.namespace.as_ref().map_or_else(
            || KEYRING_ACCOUNT.to_string(),
            |ns| format!("{KEYRING_ACCOUNT}.{ns}"),
        )
    }

    pub(crate) fn scope_label(&self, label: &str) -> String {
        self.namespace
            .as_ref()
            .map_or_else(|| label.to_string(), |ns| format!("{ns}:{label}"))
    }

    fn validate_path(&self, path: &Path) -> Result<()> {
        if let Some(profile) = &self.profile {
            profile.validate_path(path).map_err(profile_error)?;
        }
        Ok(())
    }

    fn allows_ambient_credentials(&self) -> bool {
        self.namespace.is_none()
    }

    fn fallback_path(&self) -> PathBuf {
        let name = self.namespace.as_ref().map_or_else(
            || "vault.kek.dpapi".to_string(),
            |ns| format!("vault.{ns}.kek.dpapi"),
        );
        self.data_root.join(name)
    }
}

pub(crate) fn validate_credential_path(path: &Path) -> Result<()> {
    if let Some(profile) = current_profile()? {
        profile.validate_path(path).map_err(profile_error)?;
    }
    Ok(())
}

pub(crate) fn credential_scope_label(label: &str) -> Result<String> {
    Ok(match current_profile()? {
        Some(profile) => format!("{}:{label}", profile.credential_namespace()),
        None => label.to_string(),
    })
}

pub(crate) fn require_legacy_credentials_allowed() -> Result<()> {
    require_legacy_credentials_for(current_profile()?.as_deref())
}

fn require_legacy_credentials_for(profile: Option<&crate::profile::ProfileContext>) -> Result<()> {
    if profile.is_some() {
        return Err(CoreError::Vault(
            "Preview profiles do not import legacy credentials. Configure credentials within this preview profile.".into(),
        ));
    }
    Ok(())
}

fn current_profile() -> Result<Option<Arc<crate::profile::ProfileContext>>> {
    crate::profile::current().map_err(profile_error)
}

fn profile_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::Vault(format!("Credential profile validation failed: {error}"))
}

// The orchestration is shared with fake-only tests. Only this backend can call
// the OS credential store, DPAPI or ambient environment.
trait RootKeyBackend {
    fn load_keyring(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>>;
    fn store_keyring(&self, context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<()>;
    fn load_fallback(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>>;
    fn store_fallback(&self, context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<bool>;
    fn environment(&self, name: &str) -> Option<String>;
}

struct SystemRootKeyBackend;

impl RootKeyBackend for SystemRootKeyBackend {
    fn load_keyring(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
        os_keyring_load_optional(context)
    }
    fn store_keyring(&self, context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<()> {
        os_keyring_store_verified(context, kek)
    }
    fn load_fallback(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
        load_context_fallback(context)
    }
    fn store_fallback(&self, context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<bool> {
        persist_context_fallback(context, kek)
    }
    fn environment(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl UnlockProvider for HybridUnlockProvider {
    fn root_kek(&self) -> Result<[u8; KEK_LEN]> {
        root_kek_with_backend(&CredentialContext::current()?, &SystemRootKeyBackend)
    }
}

fn root_kek_with_backend(
    context: &CredentialContext,
    backend: &dyn RootKeyBackend,
) -> Result<[u8; KEK_LEN]> {
    context.validate_path(&context.data_root)?;
    context.validate_path(&super::sentinel_path(&context.data_root))?;
    context.validate_path(&context.fallback_path())?;
    let cache_key = context.cache_key();
    if let Some(kek) = super::cached_session_root_kek(&cache_key) {
        return Ok(kek);
    }

    let data_root = &context.data_root;
    std::fs::create_dir_all(&data_root)?;
    let sentinel_path = super::sentinel_path(&data_root);
    let has_sentinel = sentinel_path.exists();

    // paper Sections 22-27 runtime verification:
    // stable KEK + sentinel check must succeed before runtime unlock is accepted.
    if let Some(kek) = backend.load_keyring(context)? {
        let sentinel_value = verify_context_sentinel(context, &kek)?;
        if let Err(error) = backend.store_fallback(context, &kek) {
            tracing::warn!(
                "Stable KEK loaded from OS keyring but fallback persistence failed: {}",
                error
            );
        }
        super::cache_session_root_kek(cache_key, kek, sentinel_value);
        return Ok(kek);
    }

    if let Some(kek) = backend.load_fallback(context)? {
        tracing::warn!(
            "Stable KEK recovered from DPAPI fallback file because OS keyring lookup failed"
        );
        if let Err(error) = backend.store_keyring(context, &kek) {
            tracing::warn!("Failed to repair OS keyring from fallback KEK: {}", error);
        }
        let sentinel_value = verify_context_sentinel(context, &kek)?;
        super::cache_session_root_kek(cache_key, kek, sentinel_value);
        return Ok(kek);
    }

    if context.allows_ambient_credentials() {
        if let Some(raw_secret) = backend.environment(RAW_KEK_ENV) {
            if !raw_secret.trim().is_empty() {
                let kek = decode_raw_kek(&raw_secret)?;
                let sentinel_value = verify_context_sentinel(context, &kek)?;
                super::cache_session_root_kek(cache_key, kek, sentinel_value);
                return Ok(kek);
            }
        }

        if let Some(passphrase) = backend.environment(PASSPHRASE_ENV) {
            if !passphrase.trim().is_empty() {
                let kek = derive_passphrase_kek(&data_root, &passphrase, has_sentinel)?;
                let sentinel_value = verify_context_sentinel(context, &kek)?;
                super::cache_session_root_kek(cache_key, kek, sentinel_value);
                return Ok(kek);
            }
        }
    }

    if has_sentinel {
        return Err(CoreError::Vault(
            "Recovery Mode: vault sentinel exists but stable KEK could not be loaded. \
                 Refusing to auto-create a new key to avoid wrong-key AES-GCM corruption."
                .to_string(),
        ));
    }

    // First boot only (no sentinel exists): create stable KEK once.
    tracing::info!("No stable vault KEK found; bootstrapping initial root key");
    let kek = generate_random_kek();
    let keyring_persisted = match backend.store_keyring(context, &kek) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!("Stable KEK could not be verified in OS keyring: {}", error);
            false
        }
    };
    let fallback_persisted = match backend.store_fallback(context, &kek) {
        Ok(persisted) => persisted,
        Err(error) => {
            tracing::warn!(
                "Stable KEK could not be persisted to DPAPI fallback file: {}",
                error
            );
            false
        }
    };
    if !keyring_persisted && !fallback_persisted {
        return Err(CoreError::Vault(format!(
                "Recovery Mode: failed to persist stable KEK '{}': OS keyring verification failed and no secure fallback could be written.",
                context.keyring_account()
            )));
    }
    let sentinel_value = verify_context_sentinel(context, &kek)?;
    super::cache_session_root_kek(cache_key, kek, sentinel_value);
    Ok(kek)
}

/// Provider that always derives the KEK from a fixed passphrase.
/// Used in tests and headless/daemon environments.
pub struct PassphraseUnlockProvider {
    passphrase: String,
}

impl PassphraseUnlockProvider {
    pub fn new(passphrase: impl Into<String>) -> Self {
        Self {
            passphrase: passphrase.into(),
        }
    }
}

impl UnlockProvider for PassphraseUnlockProvider {
    fn root_kek(&self) -> Result<[u8; KEK_LEN]> {
        Ok(super::crypto::derive_key_from_passphrase(
            &self.passphrase,
            PASSPHRASE_SALT,
        ))
    }
}

fn generate_random_kek() -> [u8; KEK_LEN] {
    let mut kek = [0u8; KEK_LEN];
    rand::rngs::OsRng.fill_bytes(&mut kek);
    kek
}

fn derive_passphrase_kek(
    data_root: &Path,
    passphrase: &str,
    has_sentinel: bool,
) -> Result<[u8; KEK_LEN]> {
    if let Some(metadata) = load_kdf_metadata(data_root)? {
        return derive_with_kdf_metadata(passphrase, &metadata);
    }

    if has_sentinel {
        let legacy = PassphraseKdfMetadata::legacy_hkdf();
        let kek = derive_with_kdf_metadata(passphrase, &legacy)?;
        super::decrypt_sentinel(data_root, &kek).map_err(|e| {
            CoreError::Vault(format!(
                "Recovery Mode: passphrase metadata is missing and legacy HKDF compatibility could not decrypt {}: {}",
                super::sentinel_path(data_root).display(),
                e
            ))
        })?;
        save_kdf_metadata(data_root, &legacy)?;
        return Ok(kek);
    }

    let metadata = PassphraseKdfMetadata::fresh_argon2();
    save_kdf_metadata(data_root, &metadata)?;
    derive_with_kdf_metadata(passphrase, &metadata)
}

fn derive_with_kdf_metadata(
    passphrase: &str,
    metadata: &PassphraseKdfMetadata,
) -> Result<[u8; KEK_LEN]> {
    match metadata.algorithm.as_str() {
        "argon2id" => {
            let salt = decode_metadata_salt(metadata)?;
            super::crypto::derive_key_from_passphrase_argon2(
                passphrase,
                &salt,
                metadata.memory_cost_kib.unwrap_or(ARGON2_MEMORY_COST_KIB),
                metadata.time_cost.unwrap_or(ARGON2_TIME_COST),
                metadata.parallelism.unwrap_or(ARGON2_PARALLELISM),
            )
        }
        "legacy_hkdf_v1" => Ok(super::crypto::derive_key_from_passphrase(
            passphrase,
            PASSPHRASE_SALT,
        )),
        other => Err(CoreError::Vault(format!(
            "Unsupported vault KDF algorithm '{}' in {}",
            other, KDF_METADATA_FILE
        ))),
    }
}

fn decode_metadata_salt(metadata: &PassphraseKdfMetadata) -> Result<Vec<u8>> {
    let encoded = metadata.salt_base64.as_ref().ok_or_else(|| {
        CoreError::Vault(format!(
            "Vault KDF metadata '{}' is missing a salt for Argon2id",
            KDF_METADATA_FILE
        ))
    })?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| CoreError::Vault(format!("Invalid KDF metadata salt: {}", e)))
}

fn kdf_metadata_path(data_root: &Path) -> PathBuf {
    data_root.join(KDF_METADATA_FILE)
}

fn load_kdf_metadata(data_root: &Path) -> Result<Option<PassphraseKdfMetadata>> {
    let path = kdf_metadata_path(data_root);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let metadata = serde_json::from_str(&content).map_err(|e| {
        CoreError::Vault(format!("Invalid KDF metadata at {}: {}", path.display(), e))
    })?;
    Ok(Some(metadata))
}

fn save_kdf_metadata(data_root: &Path, metadata: &PassphraseKdfMetadata) -> Result<()> {
    let path = kdf_metadata_path(data_root);
    let content = serde_json::to_string_pretty(metadata)?;
    secure_fs::write_string_atomic(&path, &content)
}

fn decode_raw_kek(raw_secret: &str) -> Result<[u8; KEK_LEN]> {
    let trimmed = raw_secret.trim();
    let bytes = if trimmed.len() == KEK_LEN * 2 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = Vec::with_capacity(KEK_LEN);
        for idx in (0..trimmed.len()).step_by(2) {
            let byte = u8::from_str_radix(&trimmed[idx..idx + 2], 16).map_err(|e| {
                CoreError::Vault(format!("Invalid hex raw KEK in {}: {}", RAW_KEK_ENV, e))
            })?;
            out.push(byte);
        }
        out
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .map_err(|e| {
                CoreError::Vault(format!("Invalid base64 raw KEK in {}: {}", RAW_KEK_ENV, e))
            })?
    };

    let kek: [u8; KEK_LEN] = bytes.try_into().map_err(|_| {
        CoreError::Vault(format!(
            "{} must decode to exactly {} bytes",
            RAW_KEK_ENV, KEK_LEN
        ))
    })?;
    Ok(kek)
}

// ---------------------------------------------------------------------------
// OS credential store helpers
// ---------------------------------------------------------------------------

fn os_keyring_load(context: &CredentialContext) -> Result<[u8; KEK_LEN]> {
    let entry = keyring::Entry::new(&context.keyring_service(), &context.keyring_account())
        .map_err(|e| CoreError::Keyring(format!("keyring entry creation failed: {}", e)))?;
    let password = entry
        .get_password()
        .map_err(|e| CoreError::Keyring(format!("keyring get failed: {}", e)))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&password)
        .map_err(|e| CoreError::Keyring(format!("keyring value decode failed: {}", e)))?;
    let kek: [u8; KEK_LEN] = bytes
        .try_into()
        .map_err(|_| CoreError::Keyring("keyring value wrong length".into()))?;
    Ok(kek)
}

fn os_keyring_load_optional(context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
    match os_keyring_load(context) {
        Ok(kek) => Ok(Some(kek)),
        Err(e) => {
            let msg = e.to_string().to_ascii_lowercase();
            if msg.contains("no entry")
                || msg.contains("no matching entry")
                || msg.contains("not found")
                || msg.contains("no password")
                || msg.contains("platform secure storage")
            {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

fn os_keyring_store(context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<()> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(kek);
    let entry = keyring::Entry::new(&context.keyring_service(), &context.keyring_account())
        .map_err(|e| CoreError::Keyring(format!("keyring entry creation failed: {}", e)))?;
    entry
        .set_password(&encoded)
        .map_err(|e| CoreError::Keyring(format!("keyring store failed: {}", e)))?;
    Ok(())
}

fn os_keyring_store_verified(context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<()> {
    os_keyring_store(context, kek)?;
    let reloaded = os_keyring_load(context)?;
    if &reloaded != kek {
        return Err(CoreError::Keyring(
            "keyring verification failed: stored KEK did not round-trip".to_string(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Serialize, Deserialize)]
struct ProfileFallbackKey {
    namespace: String,
    key: [u8; KEK_LEN],
}

#[cfg(windows)]
fn encode_profile_fallback(namespace: &str, kek: &[u8; KEK_LEN]) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&ProfileFallbackKey {
        namespace: namespace.to_string(),
        key: *kek,
    })?)
}

#[cfg(windows)]
fn decode_profile_fallback(namespace: &str, plaintext: &[u8]) -> Result<[u8; KEK_LEN]> {
    let payload: ProfileFallbackKey = serde_json::from_slice(plaintext)?;
    if payload.namespace != namespace {
        return Err(CoreError::Vault(
            "Preview fallback key belongs to another profile".into(),
        ));
    }
    Ok(payload.key)
}

fn persist_context_fallback(context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<bool> {
    context.validate_path(&context.fallback_path())?;
    #[cfg(windows)]
    if let Some(namespace) = &context.namespace {
        let payload = encode_profile_fallback(namespace, kek)?;
        let encrypted = crate::dpapi::dpapi_encrypt(&payload)?;
        secure_fs::write_bytes_atomic(&context.fallback_path(), &encrypted)?;
        return Ok(true);
    }
    persist_windows_kek_fallback(&context.data_root, kek)
}

fn load_context_fallback(context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
    context.validate_path(&context.fallback_path())?;
    #[cfg(windows)]
    if let Some(namespace) = &context.namespace {
        let path = context.fallback_path();
        if !path.exists() {
            return Ok(None);
        }
        let encrypted = std::fs::read(path)?;
        let plaintext = crate::dpapi::dpapi_decrypt(&encrypted)?;
        return Ok(Some(decode_profile_fallback(namespace, &plaintext)?));
    }
    load_windows_kek_fallback_optional(&context.data_root)
}

#[cfg(windows)]
fn windows_kek_fallback_path(data_root: &Path) -> PathBuf {
    data_root.join(WINDOWS_KEK_FALLBACK_FILE)
}

#[cfg(windows)]
fn persist_windows_kek_fallback(data_root: &Path, kek: &[u8; KEK_LEN]) -> Result<bool> {
    let encrypted = crate::dpapi::dpapi_encrypt(kek)?;
    crate::secure_fs::write_bytes_atomic(&windows_kek_fallback_path(data_root), &encrypted)?;
    Ok(true)
}

#[cfg(not(windows))]
fn persist_windows_kek_fallback(_data_root: &Path, _kek: &[u8; KEK_LEN]) -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn load_windows_kek_fallback_optional(data_root: &Path) -> Result<Option<[u8; KEK_LEN]>> {
    let path = windows_kek_fallback_path(data_root);
    if !path.exists() {
        return Ok(None);
    }
    let encrypted = std::fs::read(&path)?;
    let decrypted = crate::dpapi::dpapi_decrypt(&encrypted)?;
    let kek: [u8; KEK_LEN] = decrypted
        .try_into()
        .map_err(|_| CoreError::Vault("DPAPI fallback KEK wrong length".to_string()))?;
    Ok(Some(kek))
}

#[cfg(not(windows))]
fn load_windows_kek_fallback_optional(_data_root: &Path) -> Result<Option<[u8; KEK_LEN]>> {
    Ok(None)
}

fn verify_context_sentinel(context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<String> {
    let data_root = &context.data_root;
    let sentinel_path = super::sentinel_path(data_root);
    context.validate_path(&sentinel_path)?;
    let scope = context.scope_label(super::SENTINEL_SCOPE);
    if sentinel_path.exists() {
        super::decrypt_sentinel_bytes_with_scope(&std::fs::read(&sentinel_path)?, kek, &scope)
            .map_err(|e| {
                CoreError::Vault(format!(
                    "Recovery Mode: failed to decrypt {} with stable KEK '{}': {}",
                    sentinel_path.display(),
                    context.keyring_account(),
                    e
                ))
            })
    } else {
        super::write_encrypted_sentinel_with_scope(data_root, kek, &scope)
    }
}

use base64::Engine as _;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRootKeyBackend {
        keys: Mutex<HashMap<(String, String), [u8; KEK_LEN]>>,
        fallbacks: Mutex<HashMap<PathBuf, [u8; KEK_LEN]>>,
        calls: Mutex<Vec<String>>,
        refuse_persistence: bool,
    }

    impl RootKeyBackend for FakeRootKeyBackend {
        fn load_keyring(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
            let identity = (context.keyring_service(), context.keyring_account());
            self.calls
                .lock()
                .unwrap()
                .push(format!("load:{}", identity.1));
            Ok(self.keys.lock().unwrap().get(&identity).copied())
        }
        fn store_keyring(&self, context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("store:{}", context.keyring_account()));
            if self.refuse_persistence {
                return Err(CoreError::Vault("synthetic store unavailable".into()));
            }
            self.keys
                .lock()
                .unwrap()
                .insert((context.keyring_service(), context.keyring_account()), *kek);
            Ok(())
        }
        fn load_fallback(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
            Ok(self
                .fallbacks
                .lock()
                .unwrap()
                .get(&context.fallback_path())
                .copied())
        }
        fn store_fallback(&self, context: &CredentialContext, kek: &[u8; KEK_LEN]) -> Result<bool> {
            if self.refuse_persistence {
                return Ok(false);
            }
            self.fallbacks
                .lock()
                .unwrap()
                .insert(context.fallback_path(), *kek);
            Ok(true)
        }
        fn environment(&self, _name: &str) -> Option<String> {
            panic!("preview must never request an ambient unlock secret")
        }
    }

    struct SyntheticRoot(PathBuf);
    impl SyntheticRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "abigail_preview_credentials_{}_{}",
                std::process::id(),
                rand::random::<u64>(),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn preview(&self, name: &str) -> CredentialContext {
            let profile =
                crate::profile::ProfileContext::preflight(&self.0.join(name), &[]).unwrap();
            CredentialContext::for_profile(Arc::new(profile)).unwrap()
        }
    }
    impl Drop for SyntheticRoot {
        fn drop(&mut self) {
            // Only the uniquely created synthetic fixture directory is removed.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct RestoreFakeBackend {
        keyring: Option<[u8; KEK_LEN]>,
        fallback: Option<[u8; KEK_LEN]>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl RootKeyBackend for RestoreFakeBackend {
        fn load_keyring(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
            assert!(context.namespace.as_ref().unwrap().starts_with("preview."));
            self.calls.lock().unwrap().push("keyring");
            Ok(self.keyring)
        }
        fn load_fallback(&self, context: &CredentialContext) -> Result<Option<[u8; KEK_LEN]>> {
            assert!(context.fallback_path().starts_with(&context.data_root));
            self.calls.lock().unwrap().push("fallback");
            Ok(self.fallback)
        }
        fn store_keyring(&self, _: &CredentialContext, _: &[u8; KEK_LEN]) -> Result<()> {
            panic!("a restore probe must not create or repair an OS credential")
        }
        fn store_fallback(&self, _: &CredentialContext, _: &[u8; KEK_LEN]) -> Result<bool> {
            panic!("a restore probe must not create or replace a fallback")
        }
        fn environment(&self, _: &str) -> Option<String> {
            panic!("a restore probe must not read ambient credentials")
        }
    }

    fn restore_material(context: &CredentialContext, kek: &[u8; KEK_LEN]) -> [u8; 32] {
        super::super::write_encrypted_sentinel_with_scope(
            &context.data_root,
            kek,
            &context.scope_label(super::super::SENTINEL_SCOPE),
        )
        .unwrap();
        let master = ed25519_dalek::SigningKey::from_bytes(&[19; 32]);
        let payload = serde_json::to_vec(&serde_json::json!({
            "secret": master.to_bytes().to_vec(),
        }))
        .unwrap();
        let dek =
            super::super::crypto::derive_scope_key(kek, &context.scope_label("keyring:master"));
        std::fs::write(
            context.data_root.join("master.key"),
            super::super::crypto::seal(&dek, &payload).unwrap(),
        )
        .unwrap();
        master.verifying_key().to_bytes()
    }

    fn restore_file_bytes(context: &CredentialContext) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<_> = std::fs::read_dir(&context.data_root)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().to_string(),
                    std::fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    #[test]
    fn cold_restore_credentials_use_existing_key_without_reading_or_mutating_cache() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("existing");
        let public_key = restore_material(&context, &[21; KEK_LEN]);
        let before = restore_file_bytes(&context);
        // A stale cached key must neither satisfy nor poison this disk probe.
        super::super::cache_session_root_kek(
            context.cache_key(),
            [99; KEK_LEN],
            "synthetic cache marker".into(),
        );
        let backend = RestoreFakeBackend {
            keyring: Some([21; KEK_LEN]),
            ..Default::default()
        };
        let verified =
            verify_restore_credentials_with_backend(context.profile.clone().unwrap(), &backend)
                .unwrap();
        assert_eq!(verified.master_public_key, public_key);
        assert_eq!(
            verified.credential_namespace,
            context.namespace.clone().unwrap()
        );
        assert_eq!(
            super::super::cached_session_root_kek(&context.cache_key()),
            Some([99; KEK_LEN])
        );
        assert_eq!(*backend.calls.lock().unwrap(), vec!["keyring"]);
        assert_eq!(restore_file_bytes(&context), before);
    }

    #[test]
    fn cold_restore_credentials_accept_existing_fallback_without_repairing_keyring() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("existing");
        restore_material(&context, &[22; KEK_LEN]);
        let before = restore_file_bytes(&context);
        let backend = RestoreFakeBackend {
            fallback: Some([22; KEK_LEN]),
            ..Default::default()
        };
        verify_restore_credentials_with_backend(context.profile.clone().unwrap(), &backend)
            .unwrap();
        assert_eq!(*backend.calls.lock().unwrap(), vec!["keyring", "fallback"]);
        assert!(super::super::cached_session_root_kek(&context.cache_key()).is_none());
        assert_eq!(restore_file_bytes(&context), before);
    }

    #[test]
    fn cold_restore_credentials_missing_key_never_bootstraps_or_uses_cached_key() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("existing");
        restore_material(&context, &[23; KEK_LEN]);
        let before = restore_file_bytes(&context);
        super::super::cache_session_root_kek(
            context.cache_key(),
            [23; KEK_LEN],
            "synthetic cached credential".into(),
        );
        let backend = RestoreFakeBackend::default();
        let error =
            verify_restore_credentials_with_backend(context.profile.clone().unwrap(), &backend)
                .unwrap_err()
                .to_string();
        assert!(error.contains("snapshot does not contain the OS keyring entry"));
        assert_eq!(*backend.calls.lock().unwrap(), vec!["keyring", "fallback"]);
        assert_eq!(restore_file_bytes(&context), before);
    }

    #[test]
    fn cold_restore_credentials_reject_missing_or_invalid_files_before_backend_access() {
        for name in ["vault.sentinel", "master.key"] {
            for missing in [true, false] {
                let temp = SyntheticRoot::new();
                let context = temp.preview("existing");
                restore_material(&context, &[24; KEK_LEN]);
                let target = context.data_root.join(name);
                if missing {
                    std::fs::remove_file(target).unwrap();
                } else {
                    std::fs::write(target, [2; 48]).unwrap();
                }
                let before = restore_file_bytes(&context);
                let backend = RestoreFakeBackend::default();
                let error = verify_restore_credentials_with_backend(
                    context.profile.clone().unwrap(),
                    &backend,
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains(name));
                assert!(backend.calls.lock().unwrap().is_empty());
                assert_eq!(restore_file_bytes(&context), before);
            }
        }
    }

    #[test]
    fn cold_restore_credentials_reject_wrong_key_and_damaged_master_without_writes() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("existing");
        restore_material(&context, &[25; KEK_LEN]);
        let backend = RestoreFakeBackend {
            keyring: Some([26; KEK_LEN]),
            ..Default::default()
        };
        let before = restore_file_bytes(&context);
        let error =
            verify_restore_credentials_with_backend(context.profile.clone().unwrap(), &backend)
                .unwrap_err()
                .to_string();
        assert!(error.contains("vault.sentinel"));
        assert_eq!(restore_file_bytes(&context), before);

        // Keep a valid sentinel but replace master with a validly encrypted
        // payload that is not a signing key. A sentinel-only check would pass.
        let dek = super::super::crypto::derive_scope_key(
            &[25; KEK_LEN],
            &context.scope_label("keyring:master"),
        );
        std::fs::write(
            context.data_root.join("master.key"),
            super::super::crypto::seal(&dek, br#"{"secret":[1,2]}"#).unwrap(),
        )
        .unwrap();
        let backend = RestoreFakeBackend {
            keyring: Some([25; KEK_LEN]),
            ..Default::default()
        };
        let before = restore_file_bytes(&context);
        let error =
            verify_restore_credentials_with_backend(context.profile.clone().unwrap(), &backend)
                .unwrap_err()
                .to_string();
        assert!(error.contains("master.key"));
        assert_eq!(restore_file_bytes(&context), before);
    }

    #[test]
    fn cold_restore_credentials_reject_changed_marker_before_backend_access() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("existing");
        restore_material(&context, &[27; KEK_LEN]);
        let marker_path = context.data_root.join("profile.json");
        let mut marker: crate::profile::ProfileMarker =
            serde_json::from_slice(&std::fs::read(&marker_path).unwrap()).unwrap();
        marker.profile_id = crate::profile::new_instance_id();
        std::fs::write(&marker_path, serde_json::to_vec(&marker).unwrap()).unwrap();
        let before = restore_file_bytes(&context);
        let backend = RestoreFakeBackend::default();
        assert!(verify_restore_credentials_with_backend(
            context.profile.clone().unwrap(),
            &backend
        )
        .is_err());
        assert!(backend.calls.lock().unwrap().is_empty());
        assert_eq!(restore_file_bytes(&context), before);
    }

    #[test]
    fn cold_restore_credentials_injected_provider_reopens_same_uuid_at_fresh_root() {
        let temp = SyntheticRoot::new();
        let source = temp.preview("source");
        let provider = Arc::new(PassphraseUnlockProvider::new("synthetic-cold-restore"));
        let expected = restore_material(&source, &provider.root_kek().unwrap());
        let destination = temp.0.join("destination");
        std::fs::create_dir(&destination).unwrap();
        for (name, bytes) in restore_file_bytes(&source) {
            std::fs::write(destination.join(name), bytes).unwrap();
        }
        let restored =
            Arc::new(crate::profile::ProfileContext::preflight(&destination, &[]).unwrap());
        let restored_context = CredentialContext::for_profile(restored.clone()).unwrap();
        let before = restore_file_bytes(&restored_context);
        let probe = ProvidedRestoreCredentialProbe::new(provider);
        let result = probe.verify_profile(restored.clone()).unwrap();
        assert_eq!(
            result.profile_id,
            source.profile.as_ref().unwrap().profile_id()
        );
        assert_eq!(result.master_public_key, expected);
        assert_eq!(restore_file_bytes(&restored_context), before);
        assert!(super::super::cached_session_root_kek(&restored_context.cache_key()).is_none());
    }

    #[test]
    fn preview_credentials_isolate_cache_keyring_fallback_and_reopen() {
        let temp = SyntheticRoot::new();
        let ongoing = temp.preview("ongoing");
        let disposable = temp.preview("disposable");
        let backend = FakeRootKeyBackend::default();
        let stable = CredentialContext::stable(ongoing.data_root.clone());
        backend.keys.lock().unwrap().insert(
            (KEYRING_SERVICE.into(), KEYRING_ACCOUNT.into()),
            [7; KEK_LEN],
        );
        backend
            .fallbacks
            .lock()
            .unwrap()
            .insert(stable.fallback_path(), [7; KEK_LEN]);
        super::super::cache_session_root_kek(stable.cache_key(), [7; KEK_LEN], "synthetic".into());

        let ongoing_key = root_kek_with_backend(&ongoing, &backend).unwrap();
        let disposable_key = root_kek_with_backend(&disposable, &backend).unwrap();
        assert_ne!(ongoing_key, [7; KEK_LEN]);
        assert_ne!(disposable_key, ongoing_key);
        assert_ne!(ongoing.keyring_service(), KEYRING_SERVICE);
        assert_ne!(ongoing.keyring_account(), KEYRING_ACCOUNT);
        assert_ne!(ongoing.keyring_account(), disposable.keyring_account());
        assert_ne!(ongoing.fallback_path(), stable.fallback_path());
        assert!(ongoing.fallback_path().starts_with(&ongoing.data_root));
        assert!(backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|call| !call.ends_with(KEYRING_ACCOUNT)));
        assert_eq!(
            backend
                .keys
                .lock()
                .unwrap()
                .get(&(KEYRING_SERVICE.into(), KEYRING_ACCOUNT.into())),
            Some(&[7; KEK_LEN])
        );
        assert_eq!(
            backend
                .fallbacks
                .lock()
                .unwrap()
                .get(&stable.fallback_path()),
            Some(&[7; KEK_LEN])
        );

        // Remove only this session's cache entry to exercise keyring retrieval.
        super::super::SESSION_KEYS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .remove(&ongoing.cache_key());
        let reopened = CredentialContext::for_profile(Arc::new(
            crate::profile::ProfileContext::preflight(&ongoing.data_root, &[]).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            root_kek_with_backend(&reopened, &backend).unwrap(),
            ongoing_key
        );

        // Root remains part of the cache identity even if a profile marker was copied.
        let mut moved = CredentialContext::stable(temp.0.join("other-root"));
        moved.namespace = ongoing.namespace.clone();
        assert!(super::super::cached_session_root_kek(&moved.cache_key()).is_none());
    }

    #[test]
    fn preview_credentials_fallback_repairs_only_preview_entry() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("ongoing");
        let backend = FakeRootKeyBackend::default();
        backend
            .fallbacks
            .lock()
            .unwrap()
            .insert(context.fallback_path(), [9; KEK_LEN]);
        assert_eq!(
            root_kek_with_backend(&context, &backend).unwrap(),
            [9; KEK_LEN]
        );
        let keys = backend.keys.lock().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys.get(&(context.keyring_service(), context.keyring_account())),
            Some(&[9; KEK_LEN])
        );
        assert!(!keys.contains_key(&(KEYRING_SERVICE.into(), KEYRING_ACCOUNT.into())));
    }

    #[test]
    fn preview_credentials_refuse_missing_key_and_failed_persistence() {
        let temp = SyntheticRoot::new();
        let existing = temp.preview("existing");
        verify_context_sentinel(&existing, &[3; KEK_LEN]).unwrap();
        let sentinel = std::fs::read(super::super::sentinel_path(&existing.data_root)).unwrap();
        let backend = FakeRootKeyBackend::default();
        assert!(root_kek_with_backend(&existing, &backend).is_err());
        assert!(backend.keys.lock().unwrap().is_empty());
        assert_eq!(
            std::fs::read(super::super::sentinel_path(&existing.data_root)).unwrap(),
            sentinel
        );

        let fresh = temp.preview("fresh");
        let unavailable = FakeRootKeyBackend {
            refuse_persistence: true,
            ..Default::default()
        };
        assert!(root_kek_with_backend(&fresh, &unavailable).is_err());
        assert!(!super::super::sentinel_path(&fresh.data_root).exists());
        assert!(super::super::cached_session_root_kek(&fresh.cache_key()).is_none());
    }

    #[test]
    fn preview_credentials_reject_legacy_and_cross_profile_ciphertext() {
        let temp = SyntheticRoot::new();
        let context = temp.preview("ongoing");
        let other = temp.preview("disposable");
        assert!(require_legacy_credentials_for(context.profile.as_deref()).is_err());
        assert!(require_legacy_credentials_for(None).is_ok());
        assert!(context.validate_path(&temp.0.join("outside")).is_err());
        let stable = CredentialContext::stable(temp.0.join("stable"));
        for label in [
            super::super::SENTINEL_SCOPE,
            "secrets:secrets",
            "keyring:identity",
            "keyring:master",
            "keyring:encrypt",
            "encrypted-storage:general",
            "hive",
        ] {
            assert_eq!(stable.scope_label(label), label);
            let key =
                super::super::crypto::derive_scope_key(&[11; KEK_LEN], &context.scope_label(label));
            let envelope = super::super::crypto::seal(&key, b"synthetic value").unwrap();
            for incompatible in [&stable, &other] {
                let wrong = super::super::crypto::derive_scope_key(
                    &[11; KEK_LEN],
                    &incompatible.scope_label(label),
                );
                assert!(super::super::crypto::open(&wrong, &envelope).is_err());
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn preview_credentials_fallback_payload_rejects_ordinary_and_other_profile_keys() {
        // Test the plaintext codec only; DPAPI and the OS keyring are never called.
        let payload = encode_profile_fallback("synthetic-preview-a", &[13; KEK_LEN]).unwrap();
        assert_eq!(
            decode_profile_fallback("synthetic-preview-a", &payload).unwrap(),
            [13; KEK_LEN]
        );
        assert!(decode_profile_fallback("synthetic-preview-b", &payload).is_err());
        assert!(decode_profile_fallback("synthetic-preview-a", &[13; KEK_LEN]).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_dpapi_fallback_never_claims_key_persistence() {
        let root = std::env::temp_dir().join(format!(
            "abigail_no_dpapi_fallback_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&root).unwrap();
        assert!(!persist_windows_kek_fallback(&root, &[7; KEK_LEN]).unwrap());
        assert!(load_windows_kek_fallback_optional(&root).unwrap().is_none());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn passphrase_provider_deterministic() {
        let p = PassphraseUnlockProvider::new("test-phrase");
        let k1 = p.root_kek().unwrap();
        let k2 = p.root_kek().unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_passphrases_different_keks() {
        let a = PassphraseUnlockProvider::new("alpha");
        let b = PassphraseUnlockProvider::new("beta");
        assert_ne!(a.root_kek().unwrap(), b.root_kek().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn windows_kek_fallback_roundtrips() {
        let dir = std::env::temp_dir().join("abigail_windows_kek_fallback_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut kek = [0u8; KEK_LEN];
        kek.copy_from_slice(&[7u8; KEK_LEN]);

        assert!(persist_windows_kek_fallback(&dir, &kek).unwrap());
        let loaded = load_windows_kek_fallback_optional(&dir).unwrap().unwrap();
        assert_eq!(loaded, kek);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn windows_kek_fallback_is_disabled_off_windows() {
        let dir = std::env::temp_dir().join("abigail_windows_kek_fallback_disabled");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut kek = [0u8; KEK_LEN];
        kek.copy_from_slice(&[7u8; KEK_LEN]);

        persist_windows_kek_fallback(&dir, &kek).unwrap();
        assert!(load_windows_kek_fallback_optional(&dir).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fresh_passphrase_bootstrap_creates_argon2_metadata() {
        let dir = std::env::temp_dir().join("abigail_unlock_argon2_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let kek = derive_passphrase_kek(&dir, "strong-passphrase", false).unwrap();
        assert_ne!(kek, [0u8; KEK_LEN]);

        let metadata = load_kdf_metadata(&dir).unwrap().unwrap();
        assert_eq!(metadata.algorithm, "argon2id");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_passphrase_without_metadata_remains_readable() {
        let dir = std::env::temp_dir().join("abigail_unlock_legacy_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let legacy_kek =
            super::super::crypto::derive_key_from_passphrase("legacy-pass", PASSPHRASE_SALT);
        super::super::write_encrypted_sentinel(&dir, &legacy_kek).unwrap();

        let derived = derive_passphrase_kek(&dir, "legacy-pass", true).unwrap();
        assert_eq!(derived, legacy_kek);

        let metadata = load_kdf_metadata(&dir).unwrap().unwrap();
        assert_eq!(metadata.algorithm, "legacy_hkdf_v1");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
