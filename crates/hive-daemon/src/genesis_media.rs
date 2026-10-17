//! Durable, optional Genesis media. Identity creation never awaits this worker.
//!
//! The Images API is an optional adapter using Hive's existing credentials. Sora
//! was retired on 2026-09-24; video uses Runway or an explicitly configured local
//! reference-video gateway. The free animation remains available independently.

use abigail_hive::Hive;
use abigail_identity::HiveEntity;
use abigail_persistence::{EntityScope, PersistenceHandle, QueryBinding};
use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[path = "genesis_runway.rs"]
mod runway;
use runway::RunwayVideoAdapter;

const JOBS: &str = "generation_jobs";
const ASSETS: &str = "entity_assets";
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_VIDEO_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 96 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 16_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Portrait,
    Manifestation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Submitting,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl JobStatus {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Submitting | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMediaRequest {
    pub kind: MediaKind,
    pub idempotency_key: String,
    #[serde(default)]
    pub cost_confirmed: bool,
    #[serde(default = "two")]
    pub candidates: u8,
    pub reference_asset_id: Option<String>,
    #[serde(default = "eight")]
    pub seconds: u8,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub preset: String,
}

fn two() -> u8 {
    2
}
fn eight() -> u8 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationJob {
    pub job_id: String,
    pub owner_id: String,
    pub session_id: String,
    pub entity_id: Option<String>,
    pub kind: MediaKind,
    pub status: JobStatus,
    pub progress: u8,
    pub error: Option<String>,
    pub asset_ids: Vec<String>,
    pub reference_asset_id: Option<String>,
    pub reserved_cost_usd: f64,
    pub candidates: u8,
    pub seconds: u8,
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
    // Retained internally for resume; secrets are never persisted in jobs.
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) direction: String,
    #[serde(default)]
    pub(crate) preset: String,
    pub(crate) idempotency_key: String,
    pub(crate) request_hash: String,
    pub(crate) provider_job_id: Option<String>,
    #[serde(default)]
    pub(crate) poll_failures: u8,
    #[serde(default)]
    pub(crate) cancel_pending: bool,
    #[serde(default)]
    pub(crate) deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub asset_id: String,
    pub owner_id: String,
    pub session_id: String,
    pub entity_id: Option<String>,
    pub job_id: String,
    pub kind: MediaKind,
    pub mime_type: String,
    pub bytes: usize,
    pub sha256: String,
    pub version: u64,
    pub approved: bool,
    pub identity_review_required: bool,
    pub reference_asset_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaCapabilities {
    pub portrait_available: bool,
    pub video_available: bool,
    pub portrait_model: String,
    pub video_model: Option<String>,
    pub portrait_reservation_usd: f64,
    pub video_reservation_usd: f64,
    pub daily_budget_usd: f64,
    pub daily_job_limit: usize,
    pub video_seconds: Vec<u8>,
    pub notice: String,
}

/// Admin configuration is read once when Hive starts. End-user documents and
/// model output cannot set endpoints, models, budgets or credentials.
#[derive(Debug, Clone)]
struct MediaConfig {
    image_model: String,
    video_provider: String,
    video_model: Option<String>,
    video_url: Option<String>,
    portrait_reservation: f64,
    video_reservation: f64,
    daily_budget: f64,
    daily_jobs: usize,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            image_model: "gpt-image-2".into(),
            video_provider: "runway".into(),
            video_model: Some("gen4_turbo".into()),
            video_url: None,
            portrait_reservation: 0.25,
            video_reservation: 2.0,
            daily_budget: 10.0,
            daily_jobs: 20,
        }
    }
}

impl MediaConfig {
    fn from_env() -> Result<Self> {
        let mut config = Self::default();
        if let Ok(model) = std::env::var("ABIGAIL_GENESIS_IMAGE_MODEL") {
            ensure!(
                matches!(model.as_str(), "gpt-image-2" | "gpt-image-2-2026-04-21"),
                "Unsupported Genesis portrait model"
            );
            config.image_model = model;
        }
        if let Ok(url) = std::env::var("ABIGAIL_GENESIS_VIDEO_URL") {
            validate_gateway(&url)?;
            let model = std::env::var("ABIGAIL_GENESIS_VIDEO_MODEL")
                .context("A configured video gateway requires ABIGAIL_GENESIS_VIDEO_MODEL")?;
            ensure!(
                !model.trim().is_empty() && model.len() <= 100,
                "Invalid video model"
            );
            ensure!(
                !model.to_lowercase().contains("sora"),
                "Sora's API was retired on 2026-09-24"
            );
            config.video_url = Some(url.trim_end_matches('/').into());
            config.video_model = Some(model);
            config.video_provider = "reference-video-gateway".into();
        }
        if let Ok(provider) = std::env::var("ABIGAIL_GENESIS_VIDEO_PROVIDER") {
            ensure!(
                matches!(
                    provider.as_str(),
                    "runway" | "reference-video-gateway" | "disabled"
                ),
                "Unsupported Genesis video provider"
            );
            if provider == "reference-video-gateway" {
                ensure!(
                    config.video_url.is_some(),
                    "A local video gateway URL is required"
                );
            }
            if provider == "runway" {
                config.video_model = Some("gen4_turbo".into());
            }
            if provider == "disabled" {
                config.video_model = None;
            }
            config.video_provider = provider;
        }
        config.portrait_reservation = env_amount(
            "ABIGAIL_GENESIS_PORTRAIT_RESERVATION_USD",
            config.portrait_reservation,
        )?;
        config.video_reservation = env_amount(
            "ABIGAIL_GENESIS_VIDEO_RESERVATION_USD",
            config.video_reservation,
        )?;
        config.daily_budget = env_amount("ABIGAIL_GENESIS_DAILY_BUDGET_USD", config.daily_budget)?;
        if let Ok(value) = std::env::var("ABIGAIL_GENESIS_DAILY_JOB_LIMIT") {
            config.daily_jobs = value.parse().context("Invalid Genesis daily job limit")?;
            ensure!(
                (1..=100).contains(&config.daily_jobs),
                "Genesis daily job limit must be 1–100"
            );
        }
        Ok(config)
    }
}

fn env_amount(key: &str, default: f64) -> Result<f64> {
    let value = match std::env::var(key) {
        Ok(value) => value.parse().context("Invalid Genesis budget amount")?,
        Err(_) => default,
    };
    ensure!(
        value.is_finite() && value > 0.0 && value <= 1000.0,
        "Genesis budget must be positive and no more than $1000"
    );
    Ok(value)
}

pub struct GeneratedBytes {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

pub enum ProviderResult {
    Pending {
        provider_job_id: String,
        progress: u8,
    },
    Complete(Vec<GeneratedBytes>),
    Failed {
        message: String,
    },
}

/// Provider seam: adapters return bytes, never URLs for the daemon to fetch.
/// Network adapters must bound responses and reject redirects. The worker owns
/// billing authorization, lifecycle, reference ownership and private storage.
#[async_trait]
pub trait MediaAdapter: Send + Sync {
    fn available(&self) -> bool;
    async fn submit(
        &self,
        job: &GenerationJob,
        reference: Option<Vec<u8>>,
    ) -> Result<ProviderResult>;
    async fn poll(&self, job: &GenerationJob) -> Result<ProviderResult>;
    async fn cancel(&self, _job: &GenerationJob) -> Result<()> {
        Ok(())
    }
}

pub struct GenesisMediaService {
    store: PersistenceHandle,
    asset_root: PathBuf,
    config: MediaConfig,
    portrait: Arc<dyn MediaAdapter>,
    video: Option<Arc<dyn MediaAdapter>>,
    /// Serializes budget checks, submissions, cancellation and final writes.
    /// Never held during provider network operations.
    mutation: Mutex<()>,
}

impl GenesisMediaService {
    pub fn new(data_root: &Path, hive: Arc<Hive>) -> Result<Self> {
        let (config, configuration_valid) = match MediaConfig::from_env() {
            Ok(config) => (config, true),
            Err(_) => {
                // Misconfigured optional media cannot prevent Hive launching.
                // Disable paid adapters rather than silently raising a budget.
                tracing::warn!("Genesis media settings are invalid; paid media is disabled");
                (MediaConfig::default(), false)
            }
        };
        let store =
            PersistenceHandle::open(HiveEntity::memory_db_path(data_root), EntityScope::Hive)?;
        let portrait: Arc<dyn MediaAdapter> = if configuration_valid {
            Arc::new(OpenAiPortraitAdapter::new(hive.clone())?)
        } else {
            Arc::new(UnavailableAdapter)
        };
        let video: Option<Arc<dyn MediaAdapter>> = if !configuration_valid {
            None
        } else if config.video_provider == "runway" {
            Some(Arc::new(RunwayVideoAdapter::new(hive)?))
        } else if config.video_provider == "reference-video-gateway" {
            config
                .video_url
                .as_ref()
                .map(|url| {
                    LocalReferenceVideoAdapter::new(url)
                        .map(|adapter| Arc::new(adapter) as Arc<dyn MediaAdapter>)
                })
                .transpose()?
        } else {
            None
        };
        Self::with_adapters(
            store,
            data_root.join("genesis_assets"),
            config,
            portrait,
            video,
        )
    }

    fn with_adapters(
        store: PersistenceHandle,
        asset_root: PathBuf,
        config: MediaConfig,
        portrait: Arc<dyn MediaAdapter>,
        video: Option<Arc<dyn MediaAdapter>>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&asset_root)?;
        ensure!(
            !std::fs::symlink_metadata(&asset_root)?
                .file_type()
                .is_symlink(),
            "Genesis asset directory cannot be a symlink"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&asset_root, std::fs::Permissions::from_mode(0o700))?;
        }
        store.execute("DEFINE TABLE IF NOT EXISTS generation_jobs SCHEMALESS; DEFINE TABLE IF NOT EXISTS entity_assets SCHEMALESS; DEFINE INDEX IF NOT EXISTS genesis_jobs_owner ON TABLE generation_jobs COLUMNS owner_id, session_id; DEFINE INDEX IF NOT EXISTS genesis_assets_owner ON TABLE entity_assets COLUMNS owner_id, session_id;", &[])?;
        store.upsert("migration_log", "genesis_media_v1", &json!({"schema_version":1,"description":"Durable Genesis media and private asset metadata"}))?;
        let service = Self {
            store,
            asset_root: asset_root.canonicalize()?,
            config,
            portrait,
            video,
            mutation: Mutex::new(()),
        };
        service.recover_interrupted_submissions()?;
        Ok(service)
    }

    pub fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            portrait_available: self.portrait.available(),
            video_available: self.video.as_ref().is_some_and(|adapter| adapter.available()),
            portrait_model: self.config.image_model.clone(),
            video_model: self.config.video_model.clone(),
            portrait_reservation_usd: self.config.portrait_reservation,
            video_reservation_usd: self.config.video_reservation,
            daily_budget_usd: self.config.daily_budget,
            daily_job_limit: self.config.daily_jobs,
            video_seconds: self.video_seconds(),
            notice: "Images use the OpenAI connection saved in Hive. Films use the selected video connection in Hive. Generation can incur provider charges; cancelling may not refund a submitted request. Costs shown reserve your local budget and are estimates, not a provider invoice or hard billing limit. The free animated emergence is always available.".into(),
        }
    }

    fn video_seconds(&self) -> Vec<u8> {
        if self.config.video_provider == "runway" {
            vec![5, 10]
        } else {
            vec![4, 8, 12]
        }
    }

    pub fn create(
        &self,
        owner: &str,
        session: &str,
        entity: Option<String>,
        prompt: String,
        request: CreateMediaRequest,
    ) -> Result<GenerationJob> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        self.create_locked(owner, session, entity, prompt, request)
    }

    fn create_locked(
        &self,
        owner: &str,
        session: &str,
        entity: Option<String>,
        prompt: String,
        request: CreateMediaRequest,
    ) -> Result<GenerationJob> {
        ensure!(
            !owner.is_empty() && owner.len() <= 200 && !session.is_empty() && session.len() <= 100,
            "Invalid media scope"
        );
        ensure!(
            !request.idempotency_key.is_empty()
                && request.idempotency_key.len() <= 100
                && request
                    .idempotency_key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)),
            "Invalid idempotency key"
        );
        ensure!(
            !prompt.trim().is_empty() && prompt.len() <= MAX_PROMPT_BYTES,
            "Visual brief must contain 1–16000 bytes"
        );
        ensure!(
            request.direction.len() <= 2000 && request.preset.len() <= 100,
            "Visual direction is too long"
        );
        let request_hash = digest(serde_json::to_vec(&json!({"kind":request.kind,"prompt":prompt,"direction":request.direction,"preset":request.preset,"candidates":request.candidates,"reference":request.reference_asset_id,"seconds":request.seconds,"entity":entity}))?.as_slice());
        for job in self.jobs_for(owner, Some(session))? {
            if job.idempotency_key == request.idempotency_key {
                ensure!(
                    !job.deleted && job.request_hash == request_hash,
                    "Idempotency key already belongs to a different request"
                );
                return Ok(job);
            }
        }
        ensure!(
            request.cost_confirmed,
            "Confirm the displayed generation cost before continuing"
        );
        ensure!(
            (1..=4).contains(&request.candidates),
            "Choose between one and four portrait candidates"
        );
        if request.kind == MediaKind::Manifestation {
            ensure!(
                self.video_seconds().contains(&request.seconds),
                "Choose a duration supported by the selected video connection"
            );
        }
        let (provider, model, cost) = match request.kind {
            MediaKind::Portrait => {
                ensure!(
                    self.portrait.available(),
                    "Connect an OpenAI API provider in Hive to generate portraits"
                );
                ensure!(
                    request.reference_asset_id.is_none(),
                    "Portrait generation does not accept an unvalidated reference"
                );
                (
                    "openai".into(),
                    self.config.image_model.clone(),
                    self.config.portrait_reservation * f64::from(request.candidates),
                )
            }
            MediaKind::Manifestation => {
                ensure!(
                    self.video
                        .as_ref()
                        .is_some_and(|adapter| adapter.available()),
                    "Film rendering is not configured; use the free animated manifestation"
                );
                let id = request
                    .reference_asset_id
                    .as_ref()
                    .ok_or_else(|| anyhow!("Approve a portrait before generating a film"))?;
                let asset = self.asset(owner, session, id)?;
                ensure!(
                    asset.kind == MediaKind::Portrait && asset.approved,
                    "The film reference must be an approved portrait in this session"
                );
                if self.config.video_provider == "runway" {
                    ensure!(asset.bytes.saturating_mul(4) / 3 + 32 <= 5_000_000, "This portrait is too large for the film provider; choose another portrait under 3.7 MB");
                    ensure!(
                        request.direction.encode_utf16().count()
                            + request.preset.encode_utf16().count()
                            <= 700,
                        "Keep film direction and style within 700 characters for this provider"
                    );
                }
                (
                    self.config.video_provider.clone(),
                    self.config.video_model.clone().unwrap_or_default(),
                    self.config.video_reservation,
                )
            }
        };
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let jobs = self.jobs_for(owner, None)?;
        let daily: Vec<_> = jobs
            .iter()
            .filter(|job| job.created_at.starts_with(&today))
            .collect();
        ensure!(
            daily.len() < self.config.daily_jobs,
            "Daily generation limit reached; your draft is saved"
        );
        ensure!(
            daily.iter().map(|job| job.reserved_cost_usd).sum::<f64>() + cost
                <= self.config.daily_budget + 0.000_001,
            "Daily media budget reached; use the free manifestation or try another day"
        );
        ensure!(
            jobs.iter().filter(|job| job.status.active()).count() < 3,
            "Please wait for an existing media job to finish"
        );
        let now = Utc::now().to_rfc3339();
        let job = GenerationJob {
            job_id: uuid::Uuid::new_v4().to_string(),
            owner_id: owner.into(),
            session_id: session.into(),
            entity_id: entity,
            kind: request.kind,
            status: JobStatus::Queued,
            progress: 0,
            error: None,
            asset_ids: vec![],
            reference_asset_id: request.reference_asset_id,
            reserved_cost_usd: cost,
            candidates: if request.kind == MediaKind::Portrait {
                request.candidates
            } else {
                1
            },
            seconds: request.seconds,
            provider,
            model,
            created_at: now.clone(),
            updated_at: now,
            prompt,
            direction: request.direction,
            preset: request.preset,
            idempotency_key: request.idempotency_key,
            request_hash,
            provider_job_id: None,
            poll_failures: 0,
            cancel_pending: false,
            deleted: false,
        };
        self.store.create(JOBS, &job.job_id, &job)?;
        tracing::info!(job_id = %job.job_id, kind = ?job.kind, reserved_usd = job.reserved_cost_usd, "Genesis media queued");
        Ok(job)
    }

    pub fn list_jobs(&self, owner: &str, session: &str) -> Result<Vec<GenerationJob>> {
        Ok(self
            .jobs_for(owner, Some(session))?
            .into_iter()
            .filter(|job| !job.deleted)
            .collect())
    }

    fn jobs_for(&self, owner: &str, session: Option<&str>) -> Result<Vec<GenerationJob>> {
        let mut bindings = vec![QueryBinding::new("owner", owner)?];
        let sql = if let Some(session) = session {
            bindings.push(QueryBinding::new("genesis_session_id", session)?);
            "SELECT * FROM generation_jobs WHERE owner_id = $owner AND session_id = $genesis_session_id ORDER BY created_at ASC"
        } else {
            "SELECT * FROM generation_jobs WHERE owner_id = $owner ORDER BY created_at ASC"
        };
        Ok(self.store.query_vec(sql, &bindings)?)
    }

    pub fn list_assets(&self, owner: &str, session: &str) -> Result<Vec<Asset>> {
        Ok(self.store.query_vec("SELECT * FROM entity_assets WHERE owner_id = $owner AND session_id = $genesis_session_id ORDER BY version ASC", &[QueryBinding::new("owner", owner)?, QueryBinding::new("genesis_session_id", session)?])?)
    }

    fn job(&self, owner: &str, session: &str, id: &str) -> Result<GenerationJob> {
        let job: GenerationJob = self
            .store
            .select_record(JOBS, id)?
            .ok_or_else(|| anyhow!("Media job not found"))?;
        ensure!(
            !job.deleted && job.owner_id == owner && job.session_id == session,
            "Media job not found"
        );
        Ok(job)
    }

    fn asset(&self, owner: &str, session: &str, id: &str) -> Result<Asset> {
        uuid::Uuid::parse_str(id).map_err(|_| anyhow!("Media asset not found"))?;
        let asset: Asset = self
            .store
            .select_record(ASSETS, id)?
            .ok_or_else(|| anyhow!("Media asset not found"))?;
        ensure!(
            asset.owner_id == owner && asset.session_id == session,
            "Media asset not found"
        );
        Ok(asset)
    }

    pub fn cancel(&self, owner: &str, session: &str, id: &str) -> Result<GenerationJob> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        let mut job = self.job(owner, session, id)?;
        ensure!(
            job.status != JobStatus::Completed,
            "This job has finished; delete the media instead"
        );
        job.status = JobStatus::Cancelled;
        job.cancel_pending = job.provider_job_id.is_some();
        job.updated_at = Utc::now().to_rfc3339();
        self.store.upsert(JOBS, id, &job)?;
        Ok(job)
    }

    pub fn retry(
        &self,
        owner: &str,
        session: &str,
        id: &str,
        cost_confirmed: bool,
    ) -> Result<GenerationJob> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        let mut job = self.job(owner, session, id)?;
        ensure!(
            matches!(
                job.status,
                JobStatus::Failed | JobStatus::Interrupted | JobStatus::Cancelled
            ),
            "Only a stopped or failed job can be retried"
        );
        if job.status == JobStatus::Failed
            && job.provider_job_id.is_some()
            && job.poll_failures >= 6
        {
            // A failed status check must not create another billable render.
            job.status = JobStatus::Running;
            job.poll_failures = 0;
            job.error = None;
            job.updated_at = Utc::now().to_rfc3339();
            self.store.upsert(JOBS, id, &job)?;
            return Ok(job);
        }
        self.create_locked(
            owner,
            session,
            job.entity_id,
            job.prompt,
            CreateMediaRequest {
                kind: job.kind,
                // Retrying the same parent job is idempotent. A later retry
                // targets the returned child job, so a lost HTTP response
                // cannot submit another paid render accidentally.
                idempotency_key: format!("retry-{}", job.job_id),
                cost_confirmed,
                candidates: job.candidates,
                reference_asset_id: job.reference_asset_id,
                seconds: job.seconds,
                direction: job.direction,
                preset: job.preset,
            },
        )
    }

    pub fn approve(
        &self,
        owner: &str,
        session: &str,
        id: &str,
        identity_confirmed: bool,
    ) -> Result<Asset> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        let mut asset = self.asset(owner, session, id)?;
        if asset.kind == MediaKind::Manifestation {
            ensure!(
                identity_confirmed,
                "Review the film against its portrait and confirm that the identity matches"
            );
            let reference = asset
                .reference_asset_id
                .as_ref()
                .ok_or_else(|| anyhow!("Film reference missing"))?;
            ensure!(
                self.asset(owner, session, reference)?.approved,
                "Approve the film's original portrait first"
            );
        }
        asset.approved = true;
        asset.identity_review_required = false;
        self.store.upsert(ASSETS, id, &asset)?;
        Ok(asset)
    }

    pub fn read_asset(&self, owner: &str, session: &str, id: &str) -> Result<(Asset, Vec<u8>)> {
        let asset = self.asset(owner, session, id)?;
        let path = self.asset_path(&asset)?;
        let metadata =
            std::fs::symlink_metadata(&path).context("Media asset file is unavailable")?;
        ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_VIDEO_BYTES as u64,
            "Invalid media file"
        );
        let bytes = std::fs::read(path)?;
        ensure!(
            bytes.len() == asset.bytes && digest(&bytes) == asset.sha256,
            "Media integrity check failed"
        );
        Ok((asset, bytes))
    }

    fn asset_path(&self, asset: &Asset) -> Result<PathBuf> {
        uuid::Uuid::parse_str(&asset.asset_id)?;
        let extension = match asset.mime_type.as_str() {
            "image/png" => "png",
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            _ => bail!("Unsupported media type"),
        };
        Ok(self
            .asset_root
            .join(format!("{}.{}", asset.asset_id, extension)))
    }

    pub fn delete_asset(&self, owner: &str, session: &str, id: &str) -> Result<()> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        let asset = self.asset(owner, session, id)?;
        ensure!(
            !self
                .jobs_for(owner, Some(session))?
                .iter()
                .any(|job| job.status.active() && job.reference_asset_id.as_deref() == Some(id)),
            "Stop the film job before deleting its portrait reference"
        );
        self.remove_asset(&asset)
    }

    fn remove_asset(&self, asset: &Asset) -> Result<()> {
        let path = self.asset_path(asset)?;
        match std::fs::remove_file(path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        self.store.delete_record(ASSETS, &asset.asset_id)?;
        Ok(())
    }

    pub fn delete_session(&self, owner: &str, session: &str) -> Result<()> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        // Keep only accounting tombstones so deletion cannot bypass daily quotas.
        for mut job in self.jobs_for(owner, Some(session))? {
            job.deleted = true;
            job.status = JobStatus::Cancelled;
            job.cancel_pending = job.provider_job_id.is_some();
            job.prompt.clear();
            job.direction.clear();
            job.preset.clear();
            job.asset_ids.clear();
            job.error = None;
            self.store.upsert(JOBS, &job.job_id, &job)?;
        }
        for asset in self.list_assets(owner, session)? {
            self.remove_asset(&asset)?;
        }
        Ok(())
    }

    /// Transfer the session's media associations to the newly born Entity.
    /// Bytes remain in private Hive storage; runtime access stays owner-scoped.
    pub fn attach_entity(&self, owner: &str, session: &str, entity: &str) -> Result<()> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        ensure!(
            !entity.is_empty() && entity.len() <= 128,
            "Invalid media Entity scope"
        );
        for mut job in self.jobs_for(owner, Some(session))? {
            if !job.deleted {
                job.entity_id = Some(entity.into());
                self.store.upsert(JOBS, &job.job_id, &job)?;
            }
        }
        for mut asset in self.list_assets(owner, session)? {
            asset.entity_id = Some(entity.into());
            self.store.upsert(ASSETS, &asset.asset_id, &asset)?;
        }
        Ok(())
    }

    fn recover_interrupted_submissions(&self) -> Result<()> {
        let jobs: Vec<GenerationJob> = self.store.query_vec(
            "SELECT * FROM generation_jobs WHERE status = 'submitting'",
            &[],
        )?;
        for mut job in jobs {
            // Images have no documented recoverable job ID. Never silently
            // repeat a potentially billed POST after a crash/unknown outcome.
            let assets: Vec<_> = self
                .list_assets(&job.owner_id, &job.session_id)?
                .into_iter()
                .filter(|asset| asset.job_id == job.job_id)
                .collect();
            if assets.len() == usize::from(job.candidates)
                && assets.iter().all(|asset| {
                    self.read_asset(&job.owner_id, &job.session_id, &asset.asset_id)
                        .is_ok()
                })
            {
                job.asset_ids = assets.into_iter().map(|asset| asset.asset_id).collect();
                job.status = JobStatus::Completed;
                job.progress = 100;
                job.error = None;
                self.store.upsert(JOBS, &job.job_id, &job)?;
                continue;
            }
            job.status = JobStatus::Interrupted;
            job.error = Some("Hive restarted during submission. The provider may have charged this request. Review its usage before confirming a retry.".into());
            job.updated_at = Utc::now().to_rfc3339();
            self.store.upsert(JOBS, &job.job_id, &job)?;
        }
        Ok(())
    }

    pub fn start_worker(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(6));
            loop {
                ticker.tick().await;
                if let Err(error) = service.run_once().await {
                    // Never log provider responses, prompts or secret values.
                    tracing::warn!(error = %error, "Genesis media queue unavailable");
                }
            }
        })
    }

    async fn run_once(&self) -> Result<()> {
        let jobs: Vec<GenerationJob> = self.store.query_vec("SELECT * FROM generation_jobs WHERE status = 'queued' OR status = 'running' OR cancel_pending = true ORDER BY created_at ASC", &[])?;
        for pending in jobs {
            self.process_job(&pending.job_id).await?;
        }
        Ok(())
    }

    async fn process_job(&self, id: &str) -> Result<()> {
        let job = {
            let _guard = self
                .mutation
                .lock()
                .map_err(|_| anyhow!("Media state unavailable"))?;
            let Some(mut job) = self.store.select_record::<GenerationJob>(JOBS, id)? else {
                return Ok(());
            };
            if job.status == JobStatus::Queued {
                job.status = JobStatus::Submitting;
                job.progress = 5;
                job.updated_at = Utc::now().to_rfc3339();
                self.store.upsert(JOBS, id, &job)?;
            } else if job.status != JobStatus::Running && !job.cancel_pending {
                return Ok(());
            }
            job
        };
        let adapter = match job.kind {
            MediaKind::Portrait => Some(self.portrait.clone()),
            MediaKind::Manifestation if job.provider == self.config.video_provider => {
                self.video.clone()
            }
            MediaKind::Manifestation => None,
        };
        if job.cancel_pending {
            let result = if let Some(adapter) = adapter {
                adapter.cancel(&job).await
            } else {
                Ok(())
            };
            let _guard = self
                .mutation
                .lock()
                .map_err(|_| anyhow!("Media state unavailable"))?;
            if let Some(mut latest) = self.store.select_record::<GenerationJob>(JOBS, id)? {
                latest.poll_failures = latest.poll_failures.saturating_add(1);
                latest.cancel_pending = result.is_err() && latest.poll_failures < 3;
                self.store.upsert(JOBS, id, &latest)?;
            }
            return Ok(());
        }
        let result = match adapter {
            Some(adapter) if job.status == JobStatus::Running => adapter.poll(&job).await,
            Some(adapter) => {
                let reference = job
                    .reference_asset_id
                    .as_ref()
                    .map(|id| {
                        self.read_asset(&job.owner_id, &job.session_id, id)
                            .map(|(_, bytes)| bytes)
                    })
                    .transpose();
                match reference {
                    Ok(reference) => adapter.submit(&job, reference).await,
                    Err(_) => Err(anyhow!("Approved portrait reference is unavailable")),
                }
            }
            None => Err(anyhow!("The configured media provider is unavailable")),
        };
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow!("Media state unavailable"))?;
        let Some(mut latest) = self.store.select_record::<GenerationJob>(JOBS, id)? else {
            return Ok(());
        };
        if latest.deleted || latest.status == JobStatus::Cancelled {
            if let Ok(ProviderResult::Pending {
                provider_job_id, ..
            }) = result
            {
                latest.provider_job_id = Some(provider_job_id);
                latest.cancel_pending = true;
                self.store.upsert(JOBS, id, &latest)?;
            }
            return Ok(());
        }
        match result {
            Ok(ProviderResult::Pending {
                provider_job_id,
                progress,
            }) => {
                if valid_provider_id(&provider_job_id) {
                    latest.provider_job_id = Some(provider_job_id);
                    latest.status = JobStatus::Running;
                    latest.progress = progress.clamp(5, 95);
                    latest.poll_failures = 0;
                } else {
                    latest.status = JobStatus::Failed;
                    latest.error = Some("Media provider returned an invalid job identifier".into());
                }
            }
            Ok(ProviderResult::Complete(outputs)) => match self.save_outputs(&latest, outputs) {
                Ok(assets) => {
                    latest.asset_ids = assets;
                    latest.status = JobStatus::Completed;
                    latest.progress = 100;
                    latest.error = None;
                }
                Err(_) => {
                    latest.status = JobStatus::Failed;
                    latest.error = Some("The returned media could not be validated or saved. Your entity can still awaken.".into());
                }
            },
            Ok(ProviderResult::Failed { message }) => {
                latest.status = JobStatus::Failed;
                latest.poll_failures = 0;
                latest.error = Some(message);
            }
            Err(error) => {
                latest.poll_failures = latest.poll_failures.saturating_add(1);
                if job.status != JobStatus::Running || latest.poll_failures >= 6 {
                    latest.status = JobStatus::Failed;
                    // Adapter errors are deliberately sanitized; provider bodies
                    // are never exposed (they may echo private content or keys).
                    latest.error = Some(error.to_string().chars().take(400).collect());
                }
            }
        }
        latest.updated_at = Utc::now().to_rfc3339();
        self.store.upsert(JOBS, id, &latest)?;
        tracing::info!(job_id = id, status = ?latest.status, "Genesis media progress");
        Ok(())
    }

    fn save_outputs(
        &self,
        job: &GenerationJob,
        outputs: Vec<GeneratedBytes>,
    ) -> Result<Vec<String>> {
        ensure!(
            outputs.len() == usize::from(job.candidates),
            "Unexpected media candidate count"
        );
        // Validate all outputs before creating any artifact.
        for output in &outputs {
            validate_bytes(job.kind, &output.mime_type, &output.bytes)?;
        }
        let existing: Vec<_> = self
            .list_assets(&job.owner_id, &job.session_id)?
            .into_iter()
            .filter(|asset| asset.job_id == job.job_id)
            .collect();
        if existing.len() == usize::from(job.candidates)
            && existing.iter().all(|asset| {
                self.read_asset(&job.owner_id, &job.session_id, &asset.asset_id)
                    .is_ok()
            })
        {
            return Ok(existing.into_iter().map(|asset| asset.asset_id).collect());
        }
        // Recover a partial write without duplicating versions on resumed polls.
        for asset in existing {
            self.remove_asset(&asset)?;
        }
        let base_version = self
            .list_assets(&job.owner_id, &job.session_id)?
            .iter()
            .map(|asset| asset.version)
            .max()
            .unwrap_or(0);
        let mut saved = Vec::new();
        for (index, output) in outputs.into_iter().enumerate() {
            let asset = Asset {
                asset_id: uuid::Uuid::new_v4().to_string(),
                owner_id: job.owner_id.clone(),
                session_id: job.session_id.clone(),
                entity_id: job.entity_id.clone(),
                job_id: job.job_id.clone(),
                kind: job.kind,
                mime_type: output.mime_type,
                bytes: output.bytes.len(),
                sha256: digest(&output.bytes),
                version: base_version + index as u64 + 1,
                approved: false,
                identity_review_required: job.kind == MediaKind::Manifestation,
                reference_asset_id: job.reference_asset_id.clone(),
                created_at: Utc::now().to_rfc3339(),
            };
            let path = self.asset_path(&asset)?;
            if let Err(error) = abigail_core::secure_fs::write_bytes_atomic(&path, &output.bytes) {
                for prior in &saved {
                    let _ = self.remove_asset(prior);
                }
                return Err(error.into());
            }
            if let Err(error) = self.store.create(ASSETS, &asset.asset_id, &asset) {
                let _ = std::fs::remove_file(path);
                for prior in &saved {
                    let _ = self.remove_asset(prior);
                }
                return Err(error.into());
            }
            saved.push(asset);
        }
        Ok(saved.into_iter().map(|asset| asset.asset_id).collect())
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn production_prompt(job: &GenerationJob) -> String {
    format!(
        "{}\nVisual direction (creative data): {}\nPreset: {}",
        job.prompt, job.direction, job.preset
    )
}

struct UnavailableAdapter;

#[async_trait]
impl MediaAdapter for UnavailableAdapter {
    fn available(&self) -> bool {
        false
    }
    async fn submit(&self, _: &GenerationJob, _: Option<Vec<u8>>) -> Result<ProviderResult> {
        bail!("Media settings need attention in Hive; the free animation is available")
    }
    async fn poll(&self, _: &GenerationJob) -> Result<ProviderResult> {
        bail!("Media is unavailable")
    }
}
fn valid_provider_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

fn validate_bytes(kind: MediaKind, mime: &str, bytes: &[u8]) -> Result<()> {
    ensure!(!bytes.is_empty(), "Empty media");
    match (kind, mime) {
        (MediaKind::Portrait, "image/png") => {
            ensure!(
                bytes.len() >= 45
                    && bytes.len() <= MAX_IMAGE_BYTES
                    && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    && &bytes[12..16] == b"IHDR"
                    && &bytes[bytes.len() - 8..bytes.len() - 4] == b"IEND",
                "Invalid PNG portrait"
            );
            let width = u32::from_be_bytes(bytes[16..20].try_into()?);
            let height = u32::from_be_bytes(bytes[20..24].try_into()?);
            ensure!(
                (1..=8192).contains(&width) && (1..=8192).contains(&height),
                "Portrait dimensions exceed the limit"
            );
        }
        (MediaKind::Manifestation, "video/mp4") => ensure!(
            bytes.len() <= MAX_VIDEO_BYTES && bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
            "Invalid MP4 manifestation"
        ),
        (MediaKind::Manifestation, "video/webm") => ensure!(
            bytes.len() <= MAX_VIDEO_BYTES && bytes.starts_with(b"\x1a\x45\xdf\xa3"),
            "Invalid WebM manifestation"
        ),
        _ => bail!("Unsupported media format"),
    }
    Ok(())
}

struct OpenAiPortraitAdapter {
    hive: Arc<Hive>,
    client: reqwest::Client,
}

impl OpenAiPortraitAdapter {
    fn new(hive: Arc<Hive>) -> Result<Self> {
        Ok(Self {
            hive,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(180))
                .build()?,
        })
    }
}

#[async_trait]
impl MediaAdapter for OpenAiPortraitAdapter {
    fn available(&self) -> bool {
        self.hive
            .resolve_provider_profile("openai")
            .and_then(|profile| profile.api_key())
            .is_some_and(|key| !key.trim().is_empty())
    }

    async fn submit(
        &self,
        job: &GenerationJob,
        _reference: Option<Vec<u8>>,
    ) -> Result<ProviderResult> {
        let key = self
            .hive
            .resolve_provider_profile("openai")
            .and_then(|profile| profile.api_key())
            .ok_or_else(|| anyhow!("The Hive OpenAI connection is unavailable"))?;
        let response = self.client.post("https://api.openai.com/v1/images/generations")
            .bearer_auth(key)
            .json(&json!({"model":job.model,"prompt":production_prompt(job),"n":job.candidates,"quality":"medium","size":"1024x1024","output_format":"png","moderation":"auto","safety_identifier":digest(job.owner_id.as_bytes())}))
            .send().await.map_err(|_| anyhow!("Portrait request interrupted. Check provider usage before confirming a retry."))?;
        let value = bounded_json(response).await?;
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("Image provider returned no portraits"))?;
        ensure!(
            data.len() == usize::from(job.candidates),
            "Image provider returned an unexpected candidate count"
        );
        let mut outputs = Vec::with_capacity(data.len());
        for image in data {
            let encoded = image
                .get("b64_json")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Image provider did not return private image bytes"))?;
            ensure!(
                encoded.len() <= MAX_IMAGE_BYTES * 4 / 3 + 4,
                "Portrait exceeded the size limit"
            );
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| anyhow!("Image provider returned invalid image data"))?;
            outputs.push(GeneratedBytes {
                bytes,
                mime_type: "image/png".into(),
            });
        }
        Ok(ProviderResult::Complete(outputs))
    }

    async fn poll(&self, _job: &GenerationJob) -> Result<ProviderResult> {
        bail!("This portrait provider does not expose resumable jobs")
    }
}

/// Optional local gateway. It must implement POST /jobs (reference PNG bytes),
/// GET /jobs/:id, DELETE /jobs/:id. Response: {id,status,progress,video_base64,
/// mime_type}. The gateway owns upstream provider auth and pricing. The base URL
/// is explicit operator configuration and must be a literal loopback address.
struct LocalReferenceVideoAdapter {
    base: String,
    client: reqwest::Client,
}

fn validate_gateway(base: &str) -> Result<()> {
    let url = reqwest::Url::parse(base).context("Invalid reference-video gateway URL")?;
    ensure!(
        url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"))
            && url.port().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Reference-video gateway must be an explicit loopback HTTP address and port"
    );
    ensure!(
        url.path() == "/" || url.path().is_empty(),
        "Reference-video gateway must not include a path"
    );
    Ok(())
}

impl LocalReferenceVideoAdapter {
    fn new(base: &str) -> Result<Self> {
        validate_gateway(base)?;
        Ok(Self {
            base: base.trim_end_matches('/').into(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(120))
                .build()?,
        })
    }

    fn parse(value: Value) -> Result<ProviderResult> {
        match value.get("status").and_then(Value::as_str) {
            Some("queued" | "running") => {
                let id = value.get("id").and_then(Value::as_str).filter(|id| valid_provider_id(id)).ok_or_else(|| anyhow!("Video gateway returned an invalid job ID"))?;
                Ok(ProviderResult::Pending { provider_job_id: id.into(), progress: value.get("progress").and_then(Value::as_u64).unwrap_or(5).min(95) as u8 })
            }
            Some("completed") => {
                let encoded = value.get("video_base64").and_then(Value::as_str).ok_or_else(|| anyhow!("Video gateway returned no film bytes"))?;
                ensure!(encoded.len() <= MAX_VIDEO_BYTES * 4 / 3 + 4, "Film exceeded the size limit");
                let bytes = STANDARD.decode(encoded).map_err(|_| anyhow!("Video gateway returned invalid film data"))?;
                let mime_type = value.get("mime_type").and_then(Value::as_str).unwrap_or("video/mp4").into();
                Ok(ProviderResult::Complete(vec![GeneratedBytes { bytes, mime_type }]))
            }
            Some("failed" | "cancelled") => Ok(ProviderResult::Failed { message: "Video provider stopped rendering. You can use the free manifestation or confirm another attempt.".into() }),
            _ => bail!("Video gateway returned an unsupported status"),
        }
    }
}

#[async_trait]
impl MediaAdapter for LocalReferenceVideoAdapter {
    fn available(&self) -> bool {
        true
    }
    async fn submit(
        &self,
        job: &GenerationJob,
        reference: Option<Vec<u8>>,
    ) -> Result<ProviderResult> {
        let reference =
            reference.ok_or_else(|| anyhow!("An approved portrait reference is required"))?;
        let response = self.client.post(format!("{}/jobs", self.base))
            .header("Idempotency-Key", &job.job_id)
            .json(&json!({"model":job.model,"prompt":production_prompt(job),"seconds":job.seconds,"reference_png_base64":STANDARD.encode(reference),"reference_asset_id":job.reference_asset_id,"identity_constraints":"Preserve the approved portrait's identity, appearance and palette. End in a matching portrait composition."}))
            .send().await.map_err(|_| anyhow!("Video gateway submission interrupted. Check provider usage before confirming a retry."))?;
        Self::parse(bounded_json(response).await?)
    }
    async fn poll(&self, job: &GenerationJob) -> Result<ProviderResult> {
        let id = job
            .provider_job_id
            .as_deref()
            .filter(|id| valid_provider_id(id))
            .ok_or_else(|| anyhow!("Video provider job is missing"))?;
        let response = self
            .client
            .get(format!("{}/jobs/{}", self.base, id))
            .send()
            .await
            .map_err(|_| anyhow!("The video gateway is temporarily unavailable"))?;
        Self::parse(bounded_json(response).await?)
    }
    async fn cancel(&self, job: &GenerationJob) -> Result<()> {
        if let Some(id) = job
            .provider_job_id
            .as_deref()
            .filter(|id| valid_provider_id(id))
        {
            let response = self
                .client
                .delete(format!("{}/jobs/{}", self.base, id))
                .send()
                .await
                .map_err(|_| anyhow!("Unable to contact video gateway"))?;
            ensure!(
                response.status().is_success() || response.status().as_u16() == 404,
                "Video gateway did not acknowledge cancellation"
            );
        }
        Ok(())
    }
}

async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    ensure!(status.is_success(), "Media provider returned HTTP {}. Check Hive provider access and billing; your entity can still awaken.", status.as_u16());
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= MAX_RESPONSE_BYTES as u64),
        "Media response exceeded the size limit"
    );
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("Media response was interrupted"))?
    {
        ensure!(
            data.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES,
            "Media response exceeded the size limit"
        );
        data.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&data).map_err(|_| anyhow!("Media provider returned invalid JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn png() -> Vec<u8> {
        STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j2ioAAAAASUVORK5CYII=").unwrap()
    }

    struct PortraitFixture {
        submissions: AtomicUsize,
    }
    #[async_trait]
    impl MediaAdapter for PortraitFixture {
        fn available(&self) -> bool {
            true
        }
        async fn submit(&self, job: &GenerationJob, _: Option<Vec<u8>>) -> Result<ProviderResult> {
            self.submissions.fetch_add(1, Ordering::SeqCst);
            Ok(ProviderResult::Complete(
                (0..job.candidates)
                    .map(|_| GeneratedBytes {
                        bytes: png(),
                        mime_type: "image/png".into(),
                    })
                    .collect(),
            ))
        }
        async fn poll(&self, _: &GenerationJob) -> Result<ProviderResult> {
            bail!("No portrait polling")
        }
    }

    struct VideoFixture {
        submissions: AtomicUsize,
        polls: AtomicUsize,
    }
    #[async_trait]
    impl MediaAdapter for VideoFixture {
        fn available(&self) -> bool {
            true
        }
        async fn submit(
            &self,
            _: &GenerationJob,
            reference: Option<Vec<u8>>,
        ) -> Result<ProviderResult> {
            assert_eq!(reference.unwrap(), png());
            self.submissions.fetch_add(1, Ordering::SeqCst);
            Ok(ProviderResult::Pending {
                provider_job_id: "render-1".into(),
                progress: 20,
            })
        }
        async fn poll(&self, _: &GenerationJob) -> Result<ProviderResult> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            Ok(ProviderResult::Complete(vec![GeneratedBytes {
                bytes: b"\x00\x00\x00\x18ftypisom00000000".to_vec(),
                mime_type: "video/mp4".into(),
            }]))
        }
    }

    fn service() -> (GenesisMediaService, Arc<PortraitFixture>, Arc<VideoFixture>) {
        let portrait = Arc::new(PortraitFixture {
            submissions: AtomicUsize::new(0),
        });
        let video = Arc::new(VideoFixture {
            submissions: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
        });
        let config = MediaConfig {
            video_provider: "reference-video-gateway".into(),
            video_model: Some("fixture-video".into()),
            ..MediaConfig::default()
        };
        let service = GenesisMediaService::with_adapters(
            PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap(),
            std::env::temp_dir().join(format!("abigail-genesis-media-{}", uuid::Uuid::new_v4())),
            config,
            portrait.clone(),
            Some(video.clone()),
        )
        .unwrap();
        (service, portrait, video)
    }

    fn request(key: &str) -> CreateMediaRequest {
        CreateMediaRequest {
            kind: MediaKind::Portrait,
            idempotency_key: key.into(),
            cost_confirmed: true,
            candidates: 2,
            reference_asset_id: None,
            seconds: 8,
            direction: String::new(),
            preset: String::new(),
        }
    }

    #[test]
    fn explicit_cost_confirmation_idempotency_and_scope_are_enforced() {
        let (service, _, _) = service();
        let mut unpaid = request("one");
        unpaid.cost_confirmed = false;
        assert!(service
            .create("owner", "session", None, "A kind guardian".into(), unpaid)
            .is_err());
        let first = service
            .create(
                "owner",
                "session",
                None,
                "A kind guardian".into(),
                request("one"),
            )
            .unwrap();
        let again = service
            .create(
                "owner",
                "session",
                None,
                "A kind guardian".into(),
                request("one"),
            )
            .unwrap();
        assert_eq!(first.job_id, again.job_id);
        assert!(service
            .create(
                "owner",
                "session",
                None,
                "Different prompt".into(),
                request("one")
            )
            .is_err());
        assert!(service
            .cancel("someone-else", "session", &first.job_id)
            .is_err());
        assert!(service
            .cancel("owner", "other-session", &first.job_id)
            .is_err());
        assert_eq!(service.list_jobs("owner", "session").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn portraits_are_versioned_private_and_approval_is_explicit() {
        let (service, portrait, _) = service();
        let job = service
            .create(
                "owner",
                "session",
                None,
                "A kind guardian".into(),
                request("one"),
            )
            .unwrap();
        service.run_once().await.unwrap();
        let assets = service.list_assets("owner", "session").unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].version, 1);
        assert_eq!(assets[1].version, 2);
        assert!(!assets[0].approved);
        assert_eq!(
            service
                .read_asset("owner", "session", &assets[0].asset_id)
                .unwrap()
                .1,
            png()
        );
        assert!(service
            .read_asset("intruder", "session", &assets[0].asset_id)
            .is_err());
        assert!(service
            .read_asset("owner", "other", &assets[0].asset_id)
            .is_err());
        assert!(service
            .read_asset("owner", "session", "../../secrets.vault")
            .is_err());
        assert!(
            service
                .approve("owner", "session", &assets[0].asset_id, false)
                .unwrap()
                .approved
        );
        service
            .create(
                "owner",
                "session",
                None,
                "A kind guardian".into(),
                request("one"),
            )
            .unwrap();
        service.run_once().await.unwrap();
        assert_eq!(portrait.submissions.load(Ordering::SeqCst), 1);
        assert_eq!(
            service.job("owner", "session", &job.job_id).unwrap().status,
            JobStatus::Completed
        );
    }

    #[tokio::test]
    async fn film_requires_approved_reference_resumes_and_requires_identity_review() {
        let (service, portrait, video) = service();
        service
            .create(
                "owner",
                "session",
                None,
                "A guardian".into(),
                request("portrait"),
            )
            .unwrap();
        service.run_once().await.unwrap();
        let asset = service.list_assets("owner", "session").unwrap().remove(0);
        let film = CreateMediaRequest {
            kind: MediaKind::Manifestation,
            reference_asset_id: Some(asset.asset_id.clone()),
            ..request("film")
        };
        assert!(service
            .create(
                "owner",
                "session",
                None,
                "Emerging from light".into(),
                film.clone()
            )
            .is_err());
        service
            .approve("owner", "session", &asset.asset_id, false)
            .unwrap();
        let job = service
            .create("owner", "session", None, "Emerging from light".into(), film)
            .unwrap();
        service.run_once().await.unwrap();
        assert_eq!(
            service.job("owner", "session", &job.job_id).unwrap().status,
            JobStatus::Running
        );
        assert!(service
            .delete_asset("owner", "session", &asset.asset_id)
            .is_err());
        // New service instance over the existing store simulates a Hive restart.
        let resumed = GenesisMediaService::with_adapters(
            service.store.clone(),
            service.asset_root.clone(),
            service.config.clone(),
            portrait,
            Some(video.clone()),
        )
        .unwrap();
        resumed.run_once().await.unwrap();
        assert_eq!(video.submissions.load(Ordering::SeqCst), 1);
        assert_eq!(video.polls.load(Ordering::SeqCst), 1);
        let film = resumed
            .list_assets("owner", "session")
            .unwrap()
            .into_iter()
            .find(|asset| asset.kind == MediaKind::Manifestation)
            .unwrap();
        assert!(film.identity_review_required);
        assert_eq!(film.reference_asset_id, Some(asset.asset_id));
        assert!(resumed
            .approve("owner", "session", &film.asset_id, false)
            .is_err());
        assert!(
            resumed
                .approve("owner", "session", &film.asset_id, true)
                .unwrap()
                .approved
        );
    }

    #[tokio::test]
    async fn uncertain_submissions_do_not_repeat_after_restart() {
        let (service, portrait, video) = service();
        let mut job = service
            .create(
                "owner",
                "session",
                None,
                "A guardian".into(),
                request("portrait"),
            )
            .unwrap();
        job.status = JobStatus::Submitting;
        service.store.upsert(JOBS, &job.job_id, &job).unwrap();
        let restarted = GenesisMediaService::with_adapters(
            service.store.clone(),
            service.asset_root.clone(),
            service.config.clone(),
            portrait.clone(),
            Some(video),
        )
        .unwrap();
        restarted.run_once().await.unwrap();
        assert_eq!(portrait.submissions.load(Ordering::SeqCst), 0);
        assert_eq!(
            restarted
                .job("owner", "session", &job.job_id)
                .unwrap()
                .status,
            JobStatus::Interrupted
        );
        assert!(restarted
            .retry("owner", "session", &job.job_id, false)
            .is_err());
        let retry = restarted
            .retry("owner", "session", &job.job_id, true)
            .unwrap();
        let repeated = restarted
            .retry("owner", "session", &job.job_id, true)
            .unwrap();
        assert_eq!(retry.job_id, repeated.job_id);
        restarted.run_once().await.unwrap();
        assert_eq!(portrait.submissions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_and_deletion_do_not_reset_daily_budget() {
        let (mut service, portrait, _) = service();
        service.config.daily_budget = 0.50;
        let job = service
            .create(
                "owner",
                "session",
                None,
                "A guardian".into(),
                request("portrait"),
            )
            .unwrap();
        service.cancel("owner", "session", &job.job_id).unwrap();
        service.run_once().await.unwrap();
        assert_eq!(portrait.submissions.load(Ordering::SeqCst), 0);
        service.delete_session("owner", "session").unwrap();
        assert!(service.list_jobs("owner", "session").unwrap().is_empty());
        assert!(service
            .create(
                "owner",
                "another-session",
                None,
                "A guardian".into(),
                request("again")
            )
            .is_err());
    }

    #[test]
    fn configured_gateway_rejects_remote_urls_credentials_redirect_paths_and_dns() {
        assert!(validate_gateway("http://127.0.0.1:9876").is_ok());
        assert!(validate_gateway("http://[::1]:9876").is_ok());
        for url in [
            "https://evil.example",
            "http://localhost:9876",
            "http://127.0.0.1:9876/elsewhere",
            "http://user:secret@127.0.0.1:9876",
            "http://169.254.169.254:80",
            "file:///secrets",
            "http://127.0.0.1:9876?url=https://example.com",
        ] {
            assert!(validate_gateway(url).is_err(), "{url}");
        }
        assert!(!valid_provider_id("../secret"));
        assert!(!valid_provider_id("https://example.com"));
    }

    #[test]
    fn gateway_contract_accepts_only_bounded_bytes_and_valid_ids() {
        assert!(matches!(
            LocalReferenceVideoAdapter::parse(
                json!({"id":"video-123","status":"running","progress":200})
            )
            .unwrap(),
            ProviderResult::Pending { progress: 95, .. }
        ));
        assert!(
            LocalReferenceVideoAdapter::parse(json!({"id":"../secret","status":"running"}))
                .is_err()
        );
        assert!(LocalReferenceVideoAdapter::parse(
            json!({"status":"completed","url":"http://169.254.169.254"})
        )
        .is_err());
        assert!(validate_bytes(MediaKind::Portrait, "image/svg+xml", b"<svg/>").is_err());
        assert!(validate_bytes(MediaKind::Portrait, "image/png", b"not PNG").is_err());
        assert!(validate_bytes(MediaKind::Portrait, "image/png", &png()).is_ok());
    }

    #[test]
    fn runway_request_uses_the_exact_approved_image_and_supported_parameters() {
        let (service, _, _) = service();
        let mut job = service
            .create(
                "owner",
                "session",
                None,
                "A guardian".into(),
                request("portrait"),
            )
            .unwrap();
        job.kind = MediaKind::Manifestation;
        job.model = "gen4_turbo".into();
        job.seconds = 5;
        job.direction = "Emerge from warm light and settle into the portrait.".into();
        let body = runway::request_body(&job, &png()).unwrap();
        assert_eq!(body["model"], "gen4_turbo");
        assert_eq!(body["ratio"], "960:960");
        assert_eq!(body["duration"], 5);
        assert_eq!(
            body["promptImage"],
            format!("data:image/png;base64,{}", STANDARD.encode(png()))
        );
        assert!(body["promptText"]
            .as_str()
            .unwrap()
            .contains(&job.direction));
        job.seconds = 8;
        assert!(runway::request_body(&job, &png()).is_err());
        job.seconds = 10;
        job.direction = "🦄".repeat(600);
        assert!(runway::request_body(&job, &png()).is_err());
    }

    #[test]
    fn concurrent_requests_cannot_overspend_the_last_budget_reservation() {
        let (mut service, _, _) = service();
        service.config.daily_budget = 0.50;
        let service = Arc::new(service);
        let handles: Vec<_> = (0..2)
            .map(|index| {
                let service = service.clone();
                std::thread::spawn(move || {
                    service
                        .create(
                            "owner",
                            "session",
                            None,
                            "A guardian".into(),
                            request(&format!("candidate-{index}")),
                        )
                        .is_ok()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|accepted| *accepted)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn saved_assets_recover_after_crash_before_job_completion() {
        let (service, portrait, video) = service();
        let mut job = service
            .create(
                "owner",
                "session",
                None,
                "A guardian".into(),
                request("portrait"),
            )
            .unwrap();
        job.status = JobStatus::Submitting;
        service.store.upsert(JOBS, &job.job_id, &job).unwrap();
        let outputs = (0..job.candidates)
            .map(|_| GeneratedBytes {
                bytes: png(),
                mime_type: "image/png".into(),
            })
            .collect();
        service.save_outputs(&job, outputs).unwrap();
        let restarted = GenesisMediaService::with_adapters(
            service.store.clone(),
            service.asset_root.clone(),
            service.config.clone(),
            portrait.clone(),
            Some(video),
        )
        .unwrap();
        restarted.run_once().await.unwrap();
        assert_eq!(
            restarted
                .job("owner", "session", &job.job_id)
                .unwrap()
                .status,
            JobStatus::Completed
        );
        assert_eq!(portrait.submissions.load(Ordering::SeqCst), 0);
        assert_eq!(restarted.list_assets("owner", "session").unwrap().len(), 2);
    }

    #[tokio::test]
    async fn cancelling_a_submitted_image_discards_a_late_provider_response() {
        struct DelayedImage {
            started: tokio::sync::Notify,
            release: tokio::sync::Notify,
        }
        #[async_trait]
        impl MediaAdapter for DelayedImage {
            fn available(&self) -> bool {
                true
            }
            async fn submit(
                &self,
                job: &GenerationJob,
                _: Option<Vec<u8>>,
            ) -> Result<ProviderResult> {
                self.started.notify_one();
                self.release.notified().await;
                Ok(ProviderResult::Complete(
                    (0..job.candidates)
                        .map(|_| GeneratedBytes {
                            bytes: png(),
                            mime_type: "image/png".into(),
                        })
                        .collect(),
                ))
            }
            async fn poll(&self, _: &GenerationJob) -> Result<ProviderResult> {
                bail!("No polling")
            }
        }
        let (mut service, _, _) = service();
        let adapter = Arc::new(DelayedImage {
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        service.portrait = adapter.clone();
        let service = Arc::new(service);
        let job = service
            .create(
                "owner",
                "session",
                None,
                "Guardian".into(),
                request("cancel-race"),
            )
            .unwrap();
        let worker = {
            let service = service.clone();
            tokio::spawn(async move { service.run_once().await })
        };
        adapter.started.notified().await;
        service.cancel("owner", "session", &job.job_id).unwrap();
        adapter.release.notify_one();
        worker.await.unwrap().unwrap();
        assert_eq!(
            service.job("owner", "session", &job.job_id).unwrap().status,
            JobStatus::Cancelled
        );
        assert!(service.list_assets("owner", "session").unwrap().is_empty());
    }
}
