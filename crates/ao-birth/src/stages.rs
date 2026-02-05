//! First-run state machine: Darkness -> Ignition -> Connectivity -> Genesis -> Emergence.

use ao_core::{generate_external_keypair, sign_constitutional_documents, AppConfig};
use ao_memory::{Memory, MemoryStore};
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BirthStage {
    /// Initial state: generate keypair, present private key to user
    Darkness,
    /// User configures local LLM (Ollama/LM Studio)
    Ignition,
    /// Id (local LLM) converses with user to acquire API keys
    Connectivity,
    /// Id converses with user to discover name, purpose, personality
    Genesis,
    /// Sign all docs, finalize birth
    Emergence,
}

impl BirthStage {
    pub fn display_message(&self) -> &'static str {
        match self {
            BirthStage::Darkness => "Generating identity...",
            BirthStage::Ignition => "Configure your local LLM.",
            BirthStage::Connectivity => "Establishing connections...",
            BirthStage::Genesis => "Discovering who I am...",
            BirthStage::Emergence => "I am awake.",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            BirthStage::Darkness => "Darkness",
            BirthStage::Ignition => "Ignition",
            BirthStage::Connectivity => "Connectivity",
            BirthStage::Genesis => "Genesis",
            BirthStage::Emergence => "Emergence",
        }
    }

    pub fn next(self) -> Option<BirthStage> {
        match self {
            BirthStage::Darkness => Some(BirthStage::Ignition),
            BirthStage::Ignition => Some(BirthStage::Connectivity),
            BirthStage::Connectivity => Some(BirthStage::Genesis),
            BirthStage::Genesis => Some(BirthStage::Emergence),
            BirthStage::Emergence => None,
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
    #[error("Stage guard: cannot perform this action in stage {0}")]
    StageGuard(String),
    #[error("No signing key held in memory")]
    NoSigningKey,
}

/// Orchestrates the birth sequence. Birth cannot happen twice.
pub struct BirthOrchestrator {
    config: AppConfig,
    store: MemoryStore,
    stage: BirthStage,
    /// Held in memory from Darkness until Emergence, then dropped
    signing_key: Option<SigningKey>,
    /// Base64-encoded private key, returned to user during Darkness
    private_key_base64: Option<String>,
    /// Conversation history for birth chat (role, content)
    conversation_history: Vec<(String, String)>,
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
            signing_key: None,
            private_key_base64: None,
            conversation_history: Vec::new(),
        })
    }

    pub fn current_stage(&self) -> BirthStage {
        self.stage
    }

    pub fn display_message(&self) -> &'static str {
        self.stage.display_message()
    }

    /// Darkness: generate keypair, save pubkey, hold signing key in memory.
    /// Does NOT sign documents yet (signing is deferred to Emergence).
    pub fn generate_identity(&mut self, docs_path: &Path) -> anyhow::Result<()> {
        if self.stage != BirthStage::Darkness {
            return Ok(());
        }

        let data_dir = self.config.data_dir.clone();
        let pubkey_path = data_dir.join("external_pubkey.bin");

        // Check if already generated (idempotent)
        if pubkey_path.exists() && self.signing_key.is_some() {
            return Ok(());
        }

        // Generate the external keypair
        let keypair_result = generate_external_keypair(&data_dir)?;

        // Parse and hold the signing key
        let private_key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&keypair_result.private_key_base64)
            .map_err(|e| BirthError::Config(format!("Failed to decode private key: {}", e)))?;
        let key_bytes: [u8; 32] = private_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| BirthError::Config("Invalid private key length".to_string()))?;

        self.signing_key = Some(SigningKey::from_bytes(&key_bytes));
        self.private_key_base64 = Some(keypair_result.private_key_base64);

        // Update config to point to the pubkey
        self.config.external_pubkey_path = Some(pubkey_path);
        self.config.save(&self.config.config_path())?;

        // Ensure docs_dir exists
        std::fs::create_dir_all(docs_path)?;

        Ok(())
    }

    /// Get the private key base64 for user presentation. One-time access.
    pub fn get_private_key_base64(&self) -> Option<&str> {
        self.private_key_base64.as_deref()
    }

    /// Advance from Darkness to Ignition (after user has saved key).
    pub fn advance_past_darkness(&mut self) -> anyhow::Result<()> {
        if self.stage == BirthStage::Darkness {
            self.stage = BirthStage::Ignition;
            self.persist_stage()?;
        }
        Ok(())
    }

    /// Advance from Ignition to Connectivity (after local LLM is confirmed working).
    pub fn advance_to_connectivity(&mut self) -> anyhow::Result<()> {
        if self.stage == BirthStage::Ignition {
            self.stage = BirthStage::Connectivity;
            self.persist_stage()?;
        }
        Ok(())
    }

    /// Advance from Connectivity to Genesis (after API keys are stored).
    pub fn advance_to_genesis(&mut self) -> anyhow::Result<()> {
        if self.stage == BirthStage::Connectivity {
            self.stage = BirthStage::Genesis;
            self.persist_stage()?;
        }
        Ok(())
    }

    /// Persist current stage to config for recovery/diagnostics.
    fn persist_stage(&mut self) -> anyhow::Result<()> {
        self.config.set_birth_stage(self.stage.name());
        self.config.save(&self.config.config_path())?;
        Ok(())
    }

    /// Add a message to the birth conversation history.
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.conversation_history
            .push((role.to_string(), content.to_string()));
    }

    /// Get the conversation history.
    pub fn get_conversation(&self) -> &[(String, String)] {
        &self.conversation_history
    }

    /// Clear conversation history (used when transitioning between stages).
    pub fn clear_conversation(&mut self) {
        self.conversation_history.clear();
    }

    /// Genesis: write personalized soul.md and growth.md, advance to Emergence.
    pub fn crystallize_soul(
        &mut self,
        soul_content: &str,
        growth_content: &str,
    ) -> anyhow::Result<()> {
        if self.stage != BirthStage::Genesis {
            return Err(BirthError::StageGuard(self.stage.name().to_string()).into());
        }

        let docs_dir = self.config.docs_dir.clone();
        std::fs::create_dir_all(&docs_dir)?;

        // Overwrite soul.md with personalized content
        std::fs::write(docs_dir.join("soul.md"), soul_content)?;

        // Write growth.md (MentorEditable, not part of constitutional signing set)
        std::fs::write(docs_dir.join("growth.md"), growth_content)?;

        self.stage = BirthStage::Emergence;
        self.persist_stage()?;
        Ok(())
    }

    /// Emergence: sign all constitutional docs with the held signing key,
    /// write birth memory, set birth_complete, drop key.
    pub fn complete_emergence(&mut self) -> anyhow::Result<()> {
        if self.stage != BirthStage::Emergence {
            return Err(BirthError::StageGuard(self.stage.name().to_string()).into());
        }

        let signing_key = self
            .signing_key
            .take()
            .ok_or(BirthError::NoSigningKey)?;

        let docs_dir = self.config.docs_dir.clone();

        // Sign all constitutional documents (soul.md, ethics.md, instincts.md)
        sign_constitutional_documents(&signing_key, &docs_dir)?;

        // Drop the signing key (it's been moved out via take())
        drop(signing_key);

        // Clear the private key base64 from memory
        self.private_key_base64 = None;

        // Write birth memory
        if self.store.has_birth()? {
            return Err(BirthError::AlreadyBorn.into());
        }
        let content = format!(
            "I was born. First run completed at {}.",
            chrono::Utc::now().to_rfc3339()
        );
        let memory = Memory::crystallized(content);
        self.store.record_birth(&memory)?;

        // Save config - mark birth complete and clear stage
        self.config.birth_complete = true;
        self.config.clear_birth_stage();
        self.config.save(&self.config.config_path())?;

        Ok(())
    }

    /// MVP shortcut: skip interactive stages, go directly to Emergence.
    /// Can be called from any stage before Emergence.
    pub fn skip_to_emergence(&mut self) {
        if self.stage != BirthStage::Emergence {
            tracing::info!(
                "Skip shortcut: skipping from {:?} to Emergence",
                self.stage
            );
            self.stage = BirthStage::Emergence;
        }
    }

    /// Legacy compatibility: skip directly to completing birth.
    /// Generates identity if not done, skips to Emergence, and completes.
    pub fn skip_to_life_for_mvp(&mut self) {
        if self.stage != BirthStage::Emergence {
            tracing::info!(
                "MVP shortcut: skipping from {:?} to Emergence",
                self.stage
            );
            self.stage = BirthStage::Emergence;
        }
    }

    /// Complete birth (legacy path — signs with held key if available, or just records birth).
    pub fn complete_birth(&mut self) -> anyhow::Result<()> {
        if self.store.has_birth()? {
            return Err(BirthError::AlreadyBorn.into());
        }

        // If we have a signing key (new flow), sign docs
        if let Some(signing_key) = self.signing_key.take() {
            let docs_dir = self.config.docs_dir.clone();
            // Only sign if the docs exist
            if docs_dir.join("soul.md").exists() {
                let _ = sign_constitutional_documents(&signing_key, &docs_dir);
            }
            drop(signing_key);
        }

        self.private_key_base64 = None;

        let content = format!(
            "I was born. First run completed at {}.",
            chrono::Utc::now().to_rfc3339()
        );
        let memory = Memory::crystallized(content);
        self.store.record_birth(&memory)?;
        self.config.birth_complete = true;
        self.config.clear_birth_stage();
        self.config.save(&self.config.config_path())?;
        Ok(())
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut AppConfig {
        &mut self.config
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
            schema_version: ao_core::CONFIG_SCHEMA_VERSION,
            data_dir: data_dir.clone(),
            models_dir: data_dir.join("models"),
            docs_dir: data_dir.join("docs"),
            db_path: data_dir.join("test.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            birth_stage: None,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: Default::default(),
            trinity: None,
            agent_name: None,
            birth_timestamp: None,
        }
    }

    #[test]
    fn test_initial_stage_is_darkness() {
        let tmp = std::env::temp_dir().join("ao_birth_initial_v2");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let orch = BirthOrchestrator::new(config).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Darkness);
        assert_eq!(orch.display_message(), "Generating identity...");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_generate_identity() {
        let tmp = std::env::temp_dir().join("ao_birth_gen_identity");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.generate_identity(&docs_dir).unwrap();

        // Signing key should be held
        assert!(orch.signing_key.is_some());
        assert!(orch.get_private_key_base64().is_some());

        // Stage should still be Darkness (user needs to save key first)
        assert_eq!(orch.current_stage(), BirthStage::Darkness);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_advance_past_darkness() {
        let tmp = std::env::temp_dir().join("ao_birth_advance_dark");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.generate_identity(&docs_dir).unwrap();
        orch.advance_past_darkness().unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Ignition);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_advance_to_connectivity() {
        let tmp = std::env::temp_dir().join("ao_birth_advance_conn");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.generate_identity(&docs_dir).unwrap();
        orch.advance_past_darkness().unwrap();
        orch.advance_to_connectivity().unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Connectivity);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_advance_to_genesis() {
        let tmp = std::env::temp_dir().join("ao_birth_advance_gen");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.generate_identity(&docs_dir).unwrap();
        orch.advance_past_darkness().unwrap();
        orch.advance_to_connectivity().unwrap();
        orch.advance_to_genesis().unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Genesis);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_crystallize_soul() {
        let tmp = std::env::temp_dir().join("ao_birth_crystallize");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();
        fs::create_dir_all(&docs_dir).unwrap();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.generate_identity(&docs_dir).unwrap();
        orch.advance_past_darkness().unwrap();
        orch.advance_to_connectivity().unwrap();
        orch.advance_to_genesis().unwrap();

        orch.crystallize_soul("# Soul\nI am Test.", "# Growth\nGrowing.")
            .unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Emergence);

        // Check files were written
        let soul = fs::read_to_string(docs_dir.join("soul.md")).unwrap();
        assert!(soul.contains("I am Test."));
        let growth = fs::read_to_string(docs_dir.join("growth.md")).unwrap();
        assert!(growth.contains("Growing."));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_complete_emergence() {
        let tmp = std::env::temp_dir().join("ao_birth_emergence");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let docs_dir = config.docs_dir.clone();
        fs::create_dir_all(&docs_dir).unwrap();

        // Write constitutional docs
        for (name, content) in ao_core::templates::CONSTITUTIONAL_DOCS {
            fs::write(docs_dir.join(name), content).unwrap();
        }

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.generate_identity(&docs_dir).unwrap();
        orch.advance_past_darkness().unwrap();
        orch.advance_to_connectivity().unwrap();
        orch.advance_to_genesis().unwrap();
        orch.crystallize_soul("# Soul\nI am Test.", "# Growth\nGrowing.")
            .unwrap();

        orch.complete_emergence().unwrap();
        assert!(orch.config().birth_complete);

        // Signing key should be dropped
        assert!(orch.signing_key.is_none());
        assert!(orch.private_key_base64.is_none());

        // Signatures should exist
        assert!(docs_dir.join("soul.md.sig").exists());
        assert!(docs_dir.join("ethics.md.sig").exists());
        assert!(docs_dir.join("instincts.md.sig").exists());

        // birth_stage should be cleared
        assert!(orch.config().birth_stage.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_skip_to_life_for_mvp() {
        let tmp = std::env::temp_dir().join("ao_birth_skip_mvp_v2");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        assert_eq!(orch.current_stage(), BirthStage::Darkness);
        orch.skip_to_life_for_mvp();
        assert_eq!(orch.current_stage(), BirthStage::Emergence);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_complete_birth_legacy() {
        let tmp = std::env::temp_dir().join("ao_birth_complete_legacy");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);
        let config_path = config.config_path();

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.skip_to_life_for_mvp();
        orch.complete_birth().unwrap();

        assert!(orch.config().birth_complete);
        let loaded = AppConfig::load(&config_path).unwrap();
        assert!(loaded.birth_complete);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_double_birth_prevented() {
        let tmp = std::env::temp_dir().join("ao_birth_double_v2");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config.clone()).unwrap();
        orch.skip_to_life_for_mvp();
        orch.complete_birth().unwrap();

        let result = BirthOrchestrator::new(config);
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(
            err.contains("Already born"),
            "Expected AlreadyBorn, got: {}",
            err
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_stage_transitions() {
        assert_eq!(BirthStage::Darkness.next(), Some(BirthStage::Ignition));
        assert_eq!(BirthStage::Ignition.next(), Some(BirthStage::Connectivity));
        assert_eq!(
            BirthStage::Connectivity.next(),
            Some(BirthStage::Genesis)
        );
        assert_eq!(BirthStage::Genesis.next(), Some(BirthStage::Emergence));
        assert_eq!(BirthStage::Emergence.next(), None);
    }

    #[test]
    fn test_stage_names() {
        assert_eq!(BirthStage::Darkness.name(), "Darkness");
        assert_eq!(BirthStage::Ignition.name(), "Ignition");
        assert_eq!(BirthStage::Connectivity.name(), "Connectivity");
        assert_eq!(BirthStage::Genesis.name(), "Genesis");
        assert_eq!(BirthStage::Emergence.name(), "Emergence");
    }

    #[test]
    fn test_conversation_history() {
        let tmp = std::env::temp_dir().join("ao_birth_convo");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        orch.add_message("system", "Hello");
        orch.add_message("user", "Hi there");
        assert_eq!(orch.get_conversation().len(), 2);
        orch.clear_conversation();
        assert_eq!(orch.get_conversation().len(), 0);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_crystallize_wrong_stage_fails() {
        let tmp = std::env::temp_dir().join("ao_birth_crystal_guard");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        // In Darkness stage, crystallize should fail
        let result = orch.crystallize_soul("soul", "growth");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_complete_emergence_wrong_stage_fails() {
        let tmp = std::env::temp_dir().join("ao_birth_emerge_guard");
        let _ = fs::remove_dir_all(&tmp);
        let config = test_config(&tmp);

        let mut orch = BirthOrchestrator::new(config).unwrap();
        // In Darkness stage, complete_emergence should fail
        let result = orch.complete_emergence();
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }
}
