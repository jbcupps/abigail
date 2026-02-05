//! First-run state machine: Darkness -> Awakening -> Cognition -> Life.

use ao_core::{AppConfig, Keyring, ReadOnlyFileVault, Verifier};
use ao_memory::{Memory, MemoryStore};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirthStage {
    Darkness,
    Awakening,
    Cognition,
    Life,
}

impl BirthStage {
    pub fn display_message(&self) -> &'static str {
        match self {
            BirthStage::Darkness => "Verifying integrity...",
            BirthStage::Awakening => "Configure AO's email account.",
            BirthStage::Cognition => "Loading the mind...",
            BirthStage::Life => "I am awake.",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            BirthStage::Darkness => "Darkness",
            BirthStage::Awakening => "Awakening",
            BirthStage::Cognition => "Cognition",
            BirthStage::Life => "Life",
        }
    }

    pub fn next(self) -> Option<BirthStage> {
        match self {
            BirthStage::Darkness => Some(BirthStage::Awakening),
            BirthStage::Awakening => Some(BirthStage::Cognition),
            BirthStage::Cognition => Some(BirthStage::Life),
            BirthStage::Life => None,
        }
    }
}

#[derive(Error, Debug)]
pub enum BirthError {
    #[error("Already born")]
    AlreadyBorn,
    #[error("Verification failed: {0}")]
    Verification(String),
    #[error("Email validation failed: {0}")]
    Email(String),
    #[error("Store error: {0}")]
    Store(#[from] ao_memory::StoreError),
    #[error("Config error: {0}")]
    Config(String),
}

/// Orchestrates the birth sequence. Birth cannot happen twice.
pub struct BirthOrchestrator {
    config: AppConfig,
    store: MemoryStore,
    stage: BirthStage,
}

impl BirthOrchestrator {
    pub fn new(config: AppConfig) -> anyhow::Result<Self> {
        let store = MemoryStore::open_with_config(&config)?;
        if store.has_birth()? {
            return Err(BirthError::AlreadyBorn.into());
        }
        Ok(Self {
            config,
            store,
            stage: BirthStage::Darkness,
        })
    }

    pub fn current_stage(&self) -> BirthStage {
        self.stage
    }

    pub fn display_message(&self) -> &'static str {
        self.stage.display_message()
    }

    /// Darkness: verify crypto (soul.md, ethics.md, instincts.md).
    /// Uses external public key from vault if configured/auto-detected, otherwise skips (dev mode).
    pub fn verify_crypto(&mut self, docs_path: &Path) -> anyhow::Result<()> {
        if self.stage != BirthStage::Darkness {
            return Ok(());
        }

        // Use external vault if configured or auto-detected
        if let Some(ref pubkey_path) = self.config.effective_external_pubkey_path() {
            let vault = ReadOnlyFileVault::new(pubkey_path);
            let mut verifier = Verifier::from_vault(&vault)
                .map_err(|e| BirthError::Verification(e.to_string()))?;
            verifier
                .verify_soul(docs_path)
                .map_err(|e| BirthError::Verification(e.to_string()))?;
        } else {
            // Dev mode: skip verification if no external pubkey configured
            tracing::warn!(
                "No external_pubkey_path configured; signature verification skipped (dev mode)"
            );
        }

        self.stage = BirthStage::Awakening;
        Ok(())
    }

    /// Awakening: store email config (MVP: no IMAP/SMTP validation; senses optional).
    pub fn configure_email(
        &mut self,
        address: &str,
        imap_host: &str,
        imap_port: u16,
        smtp_host: &str,
        smtp_port: u16,
        password: &str,
    ) -> anyhow::Result<()> {
        if self.stage != BirthStage::Awakening {
            return Ok(());
        }
        let password_encrypted = Keyring::encrypt_bytes(password.as_bytes())
            .map_err(|e| BirthError::Config(e.to_string()))?;
        self.config.email = Some(ao_core::EmailConfig {
            address: address.to_string(),
            imap_host: imap_host.to_string(),
            imap_port,
            smtp_host: smtp_host.to_string(),
            smtp_port,
            password_encrypted,
        });
        self.config.save(&self.config.config_path())?;
        self.stage = BirthStage::Cognition;
        Ok(())
    }

    /// Cognition: advance to Life (model download is handled by UI; we just advance when ready).
    pub fn advance_cognition(&mut self) {
        if self.stage == BirthStage::Cognition {
            self.stage = BirthStage::Life;
        }
    }

    /// MVP shortcut: skip email and model download, go directly to Life stage.
    /// Can be called from any stage before Life.
    pub fn skip_to_life_for_mvp(&mut self) {
        if self.stage != BirthStage::Life {
            tracing::info!(
                "MVP shortcut: skipping from {:?} to Life",
                self.stage
            );
            self.stage = BirthStage::Life;
        }
    }

    /// Life: complete birth — write crystallized birth memory. Cannot be called twice.
    pub fn complete_birth(&mut self) -> anyhow::Result<()> {
        if self.store.has_birth()? {
            return Err(BirthError::AlreadyBorn.into());
        }
        let content = format!(
            "I was born. First run completed at {}.",
            chrono::Utc::now().to_rfc3339()
        );
        let memory = Memory::crystallized(content);
        self.store.record_birth(&memory)?;
        self.config.birth_complete = true;
        self.config.save(&self.config.config_path())?;
        Ok(())
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a test AppConfig pointing at a temp directory.
    fn test_config(base: &Path) -> AppConfig {
        let data_dir = base.join("data");
        fs::create_dir_all(&data_dir).unwrap();
        AppConfig {
            data_dir: data_dir.clone(),
            models_dir: data_dir.join("models"),
            docs_dir: data_dir.join("docs"),
            db_path: data_dir.join("test.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: Default::default(),
        }
    }

    #[test]
    fn test_initial_stage_is_darkness() {
        let tmp = std::env::temp_dir().join("ao_birth_initial");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let orch = BirthOrchestrator::new(config).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Darkness);
        assert_eq!(orch.display_message(), "Verifying integrity...");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_verify_crypto_without_pubkey_skips_to_awakening() {
        let tmp = std::env::temp_dir().join("ao_birth_skip_verify");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();
        fs::create_dir_all(&docs_dir).unwrap();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        // No external pubkey configured → dev mode skip → advances to Awakening
        orch.verify_crypto(&docs_dir).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Awakening);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_verify_crypto_with_real_keys() {
        use ao_core::{generate_external_keypair, sign_constitutional_documents, parse_private_key};

        let tmp = std::env::temp_dir().join("ao_birth_verify_real");
        let _ = fs::remove_dir_all(&tmp);
        let data_dir = tmp.join("data");
        fs::create_dir_all(&data_dir).unwrap();
        let docs_dir = data_dir.join("docs");
        fs::create_dir_all(&docs_dir).unwrap();

        // Create constitutional docs
        for doc in ["soul.md", "ethics.md", "instincts.md"] {
            fs::write(docs_dir.join(doc), "test content").unwrap();
        }

        // Generate keypair and sign
        let key_result = generate_external_keypair(&data_dir).unwrap();
        let signing_key = parse_private_key(&key_result.private_key_base64).unwrap();
        sign_constitutional_documents(&signing_key, &docs_dir).unwrap();

        let config = AppConfig {
            data_dir: data_dir.clone(),
            models_dir: data_dir.join("models"),
            docs_dir: docs_dir.clone(),
            db_path: data_dir.join("test.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            external_pubkey_path: None, // auto-detect external_pubkey.bin
            local_llm_base_url: None,
            routing_mode: Default::default(),
        };

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.verify_crypto(&docs_dir).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Awakening);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_skip_to_life_from_darkness() {
        let tmp = std::env::temp_dir().join("ao_birth_skip_dark");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Darkness);
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Life);
        assert_eq!(orch.display_message(), "I am awake.");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_skip_to_life_from_awakening() {
        let tmp = std::env::temp_dir().join("ao_birth_skip_awake");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();
        fs::create_dir_all(&docs_dir).unwrap();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.verify_crypto(&docs_dir).unwrap(); // → Awakening
        assert_eq!(orch.current_stage(), BirthStage::Awakening);
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Life);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_skip_to_life_noop_when_already_life() {
        let tmp = std::env::temp_dir().join("ao_birth_skip_noop");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Life);
        // Calling again should be no-op
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Life);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_complete_birth_writes_to_database() {
        let tmp = std::env::temp_dir().join("ao_birth_complete");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let config_path = config.config_path();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.skip_to_life_for_mvp();
        orch.complete_birth().unwrap();

        // Verify config was saved with birth_complete = true
        assert!(orch.config().birth_complete);
        let loaded = AppConfig::load(&config_path).unwrap();
        assert!(loaded.birth_complete);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_double_birth_prevented() {
        let tmp = std::env::temp_dir().join("ao_birth_double");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config.clone()).unwrap();
        orch.skip_to_life_for_mvp();
        orch.complete_birth().unwrap();

        // Second birth attempt with same database should fail
        let result = BirthOrchestrator::new(config);
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("Already born"), "Expected AlreadyBorn, got: {}", err);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_complete_birth_twice_prevented() {
        let tmp = std::env::temp_dir().join("ao_birth_complete_twice");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.skip_to_life_for_mvp();
        orch.complete_birth().unwrap();

        // Calling complete_birth again should fail
        let result = orch.complete_birth();
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stage_transitions() {
        // Test the BirthStage enum transitions
        assert_eq!(BirthStage::Darkness.next(), Some(BirthStage::Awakening));
        assert_eq!(BirthStage::Awakening.next(), Some(BirthStage::Cognition));
        assert_eq!(BirthStage::Cognition.next(), Some(BirthStage::Life));
        assert_eq!(BirthStage::Life.next(), None);
    }

    #[test]
    fn test_stage_names() {
        assert_eq!(BirthStage::Darkness.name(), "Darkness");
        assert_eq!(BirthStage::Awakening.name(), "Awakening");
        assert_eq!(BirthStage::Cognition.name(), "Cognition");
        assert_eq!(BirthStage::Life.name(), "Life");
    }

    #[test]
    fn test_advance_cognition() {
        let tmp = std::env::temp_dir().join("ao_birth_cognition");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();
        fs::create_dir_all(&docs_dir).unwrap();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        // Darkness → Awakening (skip verify)
        orch.verify_crypto(&docs_dir).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Awakening);

        // Can't advance cognition from Awakening (guard check)
        orch.advance_cognition();
        assert_eq!(orch.current_stage(), BirthStage::Awakening);

        // Skip to Cognition stage manually isn't possible through public API
        // without configure_email, so test skip_to_life instead
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Life);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_full_mvp_lifecycle() {
        let tmp = std::env::temp_dir().join("ao_birth_lifecycle");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();
        fs::create_dir_all(&docs_dir).unwrap();

        let mut orch = BirthOrchestrator::new(config).unwrap();

        // 1. Darkness → verify crypto (dev mode, no pubkey)
        assert_eq!(orch.current_stage(), BirthStage::Darkness);
        orch.verify_crypto(&docs_dir).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Awakening);

        // 2. Skip email/model download for MVP
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Life);

        // 3. Complete birth
        orch.complete_birth().unwrap();
        assert!(orch.config().birth_complete);

        let _ = fs::remove_dir_all(&tmp);
    }
}
