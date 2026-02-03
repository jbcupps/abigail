//! First-run state machine: Darkness -> Awakening -> Cognition -> Life.

use abby_core::{AppConfig, Keyring, Verifier};
use abby_memory::{Memory, MemoryStore};
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
            BirthStage::Awakening => "Configure Abby's email account.",
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
    Store(#[from] abby_memory::StoreError),
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
    pub fn verify_crypto(&mut self, docs_path: &Path) -> anyhow::Result<()> {
        if self.stage != BirthStage::Darkness {
            return Ok(());
        }
        let keyring = Keyring::load(self.config.data_dir.clone())
            .map_err(|e| BirthError::Verification(e.to_string()))?;
        let mut verifier = Verifier::new(keyring);
        verifier.verify_soul(docs_path).map_err(|e| BirthError::Verification(e.to_string()))?;
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
        self.config.email = Some(abby_core::EmailConfig {
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
