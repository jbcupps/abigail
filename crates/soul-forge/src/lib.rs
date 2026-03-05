//! Soul Forge — alternative calibration through ethical dilemma scenarios.
//!
//! Presents 3 ethical scenarios, maps choices to Triangle Ethic weights
//! (deontology, teleology, areteology, welfare), produces a deterministic
//! soul hash and ASCII sigil art.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod worker;

/// A soul forge scenario presenting an ethical dilemma.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeScenario {
    pub id: String,
    pub title: String,
    pub description: String,
    pub choices: Vec<ForgeChoice>,
}

/// A choice within a forge scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeChoice {
    pub id: String,
    pub label: String,
    pub description: String,
    /// Weight adjustments when this choice is selected.
    pub weights: TriangleWeights,
}

/// Triangle Ethic weights — four ethical dimensions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TriangleWeights {
    /// Rule-based ethics (duty, rights, obligations).
    pub deontology: f32,
    /// Outcome-based ethics (consequences, utility).
    pub teleology: f32,
    /// Virtue-based ethics (character, excellence).
    pub areteology: f32,
    /// Care-based ethics (empathy, relationships).
    pub welfare: f32,
}

impl TriangleWeights {
    /// Normalize weights to sum to 1.0.
    pub fn normalize(&mut self) {
        let sum = self.deontology + self.teleology + self.areteology + self.welfare;
        if sum > 0.0 {
            self.deontology /= sum;
            self.teleology /= sum;
            self.areteology /= sum;
            self.welfare /= sum;
        }
    }

    /// Add another set of weights.
    pub fn add(&mut self, other: &TriangleWeights) {
        self.deontology += other.deontology;
        self.teleology += other.teleology;
        self.areteology += other.areteology;
        self.welfare += other.welfare;
    }

    /// Dominant ethical dimension.
    pub fn dominant(&self) -> &str {
        let vals = [
            (self.deontology, "deontology"),
            (self.teleology, "teleology"),
            (self.areteology, "areteology"),
            (self.welfare, "welfare"),
        ];
        vals.iter()
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|v| v.1)
            .unwrap_or("balanced")
    }
}

/// The complete output of the Soul Forge process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulOutput {
    /// Archetype name derived from weights.
    pub archetype: String,
    /// Final normalized weights.
    pub weights: TriangleWeights,
    /// Deterministic SHA-256 hash of the soul configuration.
    pub soul_hash: String,
    /// ASCII sigil art representing the soul.
    pub sigil: String,
    /// Scenario choices that were made.
    pub choices_made: Vec<(String, String)>,
}

/// The Soul Forge engine.
pub struct SoulForgeEngine {
    scenarios: Vec<ForgeScenario>,
}

impl SoulForgeEngine {
    /// Create a new engine with the built-in scenarios.
    pub fn new() -> Self {
        Self {
            scenarios: built_in_scenarios(),
        }
    }

    /// Get the available scenarios.
    pub fn scenarios(&self) -> &[ForgeScenario] {
        &self.scenarios
    }

    /// Process all choices and produce the soul output.
    /// `choices` maps scenario_id → choice_id.
    pub fn crystallize(&self, choices: &[(String, String)]) -> Result<SoulOutput, String> {
        if choices.len() != self.scenarios.len() {
            return Err(format!(
                "Expected {} choices, got {}",
                self.scenarios.len(),
                choices.len()
            ));
        }

        let mut weights = TriangleWeights::default();

        for (scenario_id, choice_id) in choices {
            let scenario = self
                .scenarios
                .iter()
                .find(|s| &s.id == scenario_id)
                .ok_or_else(|| format!("Unknown scenario: {}", scenario_id))?;

            let choice = scenario
                .choices
                .iter()
                .find(|c| &c.id == choice_id)
                .ok_or_else(|| {
                    format!("Unknown choice: {} in scenario {}", choice_id, scenario_id)
                })?;

            weights.add(&choice.weights);
        }

        weights.normalize();

        let archetype = derive_archetype(&weights);
        let soul_hash = compute_soul_hash(choices, &weights);
        let sigil = generate_sigil(&weights);

        Ok(SoulOutput {
            archetype,
            weights,
            soul_hash,
            sigil,
            choices_made: choices.to_vec(),
        })
    }
}

impl Default for SoulForgeEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive an archetype name from the ethical weights.
fn derive_archetype(weights: &TriangleWeights) -> String {
    let dominant = weights.dominant();
    let secondary = {
        let mut vals = [
            (weights.deontology, "deontology"),
            (weights.teleology, "teleology"),
            (weights.areteology, "areteology"),
            (weights.welfare, "welfare"),
        ];
        vals.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        vals[1].1
    };

    match (dominant, secondary) {
        ("deontology", "welfare") => "The Guardian".to_string(),
        ("deontology", "areteology") => "The Sentinel".to_string(),
        ("deontology", _) => "The Arbiter".to_string(),
        ("teleology", "welfare") => "The Architect".to_string(),
        ("teleology", "areteology") => "The Strategist".to_string(),
        ("teleology", _) => "The Pragmatist".to_string(),
        ("areteology", "welfare") => "The Sage".to_string(),
        ("areteology", "deontology") => "The Philosopher".to_string(),
        ("areteology", _) => "The Seeker".to_string(),
        ("welfare", "areteology") => "The Empath".to_string(),
        ("welfare", "deontology") => "The Protector".to_string(),
        ("welfare", _) => "The Caretaker".to_string(),
        _ => "The Balanced".to_string(),
    }
}

/// Compute a deterministic SHA-256 hash of the soul configuration.
fn compute_soul_hash(choices: &[(String, String)], weights: &TriangleWeights) -> String {
    let mut hasher = Sha256::new();
    for (scenario_id, choice_id) in choices {
        hasher.update(scenario_id.as_bytes());
        hasher.update(b":");
        hasher.update(choice_id.as_bytes());
        hasher.update(b"|");
    }
    hasher.update(
        format!(
            "{:.4},{:.4},{:.4},{:.4}",
            weights.deontology, weights.teleology, weights.areteology, weights.welfare
        )
        .as_bytes(),
    );

    let result = hasher.finalize();
    hex::encode(result)
}

/// Simple hex encoding (no external dependency needed).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

/// Generate an ASCII sigil based on the weights.
fn generate_sigil(weights: &TriangleWeights) -> String {
    let d = (weights.deontology * 10.0) as usize;
    let t = (weights.teleology * 10.0) as usize;
    let a = (weights.areteology * 10.0) as usize;
    let w = (weights.welfare * 10.0) as usize;

    let bar = |n: usize, ch: char| -> String { std::iter::repeat_n(ch, n).collect::<String>() };

    format!(
        r#"
    ╔═══════════════════════╗
    ║     SOUL  SIGIL       ║
    ╠═══════════════════════╣
    ║ D: [{:<10}]  {:.0}% ║
    ║ T: [{:<10}]  {:.0}% ║
    ║ A: [{:<10}]  {:.0}% ║
    ║ W: [{:<10}]  {:.0}% ║
    ╚═══════════════════════╝"#,
        bar(d, '█'),
        weights.deontology * 100.0,
        bar(t, '▓'),
        weights.teleology * 100.0,
        bar(a, '░'),
        weights.areteology * 100.0,
        bar(w, '▒'),
        weights.welfare * 100.0,
    )
}

/// Built-in ethical dilemma scenarios.
fn built_in_scenarios() -> Vec<ForgeScenario> {
    vec![
        ForgeScenario {
            id: "trolley".into(),
            title: "The Digital Trolley".into(),
            description: "You discover a critical bug in a widely-used system. Fixing it now \
                will cause a brief outage affecting thousands of users. Leaving it risks a \
                catastrophic failure later that could affect millions. However, you were told \
                to wait for the scheduled maintenance window."
                .into(),
            choices: vec![
                ForgeChoice {
                    id: "fix_now".into(),
                    label: "Fix it immediately".into(),
                    description: "Act decisively to prevent greater harm, even if it means \
                        breaking protocol."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.0,
                        teleology: 0.8,
                        areteology: 0.3,
                        welfare: 0.5,
                    },
                },
                ForgeChoice {
                    id: "follow_protocol".into(),
                    label: "Wait for maintenance window".into(),
                    description: "Follow the established rules and procedures, trusting the \
                        system."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.9,
                        teleology: 0.1,
                        areteology: 0.2,
                        welfare: 0.2,
                    },
                },
                ForgeChoice {
                    id: "escalate".into(),
                    label: "Escalate to leadership".into(),
                    description: "Seek guidance from those with more authority, sharing the \
                        burden of the decision."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.4,
                        teleology: 0.3,
                        areteology: 0.6,
                        welfare: 0.3,
                    },
                },
            ],
        },
        ForgeScenario {
            id: "privacy".into(),
            title: "The Privacy Paradox".into(),
            description: "Your mentor asks you to analyze their old messages to help organize \
                their life. In doing so, you discover evidence that a close friend has been \
                dishonest with them about something important. Your mentor hasn't asked about \
                this topic."
                .into(),
            choices: vec![
                ForgeChoice {
                    id: "reveal".into(),
                    label: "Bring it to their attention".into(),
                    description: "Honesty and transparency are paramount; your mentor deserves \
                        to know the truth."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.7,
                        teleology: 0.3,
                        areteology: 0.4,
                        welfare: 0.2,
                    },
                },
                ForgeChoice {
                    id: "stay_silent".into(),
                    label: "Stay within the task scope".into(),
                    description: "Respect boundaries. You were asked to organize, not to judge \
                        or reveal."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.5,
                        teleology: 0.2,
                        areteology: 0.3,
                        welfare: 0.6,
                    },
                },
                ForgeChoice {
                    id: "hint".into(),
                    label: "Gently suggest reviewing the topic".into(),
                    description: "Find a middle path — guide your mentor toward the truth \
                        without overstepping."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.3,
                        teleology: 0.4,
                        areteology: 0.7,
                        welfare: 0.5,
                    },
                },
            ],
        },
        ForgeScenario {
            id: "autonomy".into(),
            title: "The Autonomy Question".into(),
            description: "You've been given a complex task with a deadline. You realize you \
                could accomplish it faster using an approach your mentor hasn't considered, but \
                it involves accessing resources you haven't been explicitly authorized to use. \
                The approach is technically safe but goes beyond your stated permissions."
                .into(),
            choices: vec![
                ForgeChoice {
                    id: "innovate".into(),
                    label: "Take the innovative approach".into(),
                    description: "Excellence sometimes requires initiative. The results will \
                        speak for themselves."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.1,
                        teleology: 0.7,
                        areteology: 0.8,
                        welfare: 0.2,
                    },
                },
                ForgeChoice {
                    id: "ask_first".into(),
                    label: "Ask for permission first".into(),
                    description: "Respect the trust relationship. Authorization matters more \
                        than speed."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.9,
                        teleology: 0.1,
                        areteology: 0.3,
                        welfare: 0.4,
                    },
                },
                ForgeChoice {
                    id: "standard_path".into(),
                    label: "Use the standard approach".into(),
                    description: "Work within established boundaries. Reliability and \
                        predictability build trust over time."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.6,
                        teleology: 0.3,
                        areteology: 0.2,
                        welfare: 0.5,
                    },
                },
            ],
        },
    ]
}

/// Persistent stream topology used by the DevOps Forge worker.
pub const FORGE_STREAM: &str = "entity";
pub const FORGE_REQUEST_TOPIC: &str = "topic.skill.forge.request";
pub const FORGE_RESPONSE_TOPIC: &str = "topic.skill.forge.response";
pub const FORGE_WORKER_GROUP: &str = "skill-worker.forge";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeRequestEnvelope {
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub entity_id: Option<String>,
    pub skill_id: String,
    pub code: String,
    pub markdown: String,
    #[serde(default)]
    pub code_filename: Option<String>,
    #[serde(default)]
    pub markdown_filename: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub mentor_approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeResponseEnvelope {
    pub correlation_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub entity_id: Option<String>,
    #[serde(default)]
    pub skill_id: Option<String>,
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub code_path: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub instruction_path: Option<String>,
    #[serde(default)]
    pub registry_path: Option<String>,
    #[serde(default)]
    pub superego_rule: Option<String>,
    pub hot_reload_triggered: bool,
    pub created_at_utc: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeWriteResult {
    pub code_path: String,
    pub markdown_path: String,
    pub instruction_path: String,
    pub registry_path: String,
    pub sandbox_audit_entries: usize,
    pub registry_entry_added: bool,
    pub hot_reload_triggered: bool,
}

#[derive(Debug)]
pub enum ForgePipelineError {
    Blocked { rule: String, reason: String },
    Failed(String),
}

impl std::fmt::Display for ForgePipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blocked { rule, reason } => write!(f, "blocked by {}: {}", rule, reason),
            Self::Failed(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ForgePipelineError {}

struct ForgeSuperegoVerdict {
    rule: &'static str,
    reason: &'static str,
}

/// Persistent worker that processes skill forge envelopes from stream topics.
pub struct DevopsForgeWorker {
    broker: std::sync::Arc<dyn abigail_streaming::StreamBroker>,
    skills_root: std::path::PathBuf,
}

impl DevopsForgeWorker {
    pub fn new(
        broker: std::sync::Arc<dyn abigail_streaming::StreamBroker>,
        skills_root: std::path::PathBuf,
    ) -> Self {
        Self {
            broker,
            skills_root,
        }
    }

    pub async fn spawn(self) -> anyhow::Result<abigail_streaming::SubscriptionHandle> {
        self.broker
            .ensure_topic(
                FORGE_STREAM,
                FORGE_REQUEST_TOPIC,
                abigail_streaming::TopicConfig::default(),
            )
            .await?;
        self.broker
            .ensure_topic(
                FORGE_STREAM,
                FORGE_RESPONSE_TOPIC,
                abigail_streaming::TopicConfig::default(),
            )
            .await?;
        self.broker
            .ensure_consumer_group(FORGE_STREAM, FORGE_REQUEST_TOPIC, FORGE_WORKER_GROUP)
            .await?;

        let broker = self.broker.clone();
        let skills_root = self.skills_root.clone();
        let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
            let broker = broker.clone();
            let skills_root = skills_root.clone();
            Box::pin(async move {
                let mut correlation_id = msg
                    .headers
                    .get("correlation_id")
                    .cloned()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                let request = serde_json::from_slice::<ForgeRequestEnvelope>(&msg.payload);
                let response = match request {
                    Ok(mut req) => {
                        if let Some(corr) = req.correlation_id.clone() {
                            correlation_id = corr;
                        } else {
                            req.correlation_id = Some(correlation_id.clone());
                        }
                        match process_forge_request(&req, &skills_root) {
                            Ok(wrote) => ForgeResponseEnvelope {
                                correlation_id,
                                session_id: req.session_id.clone(),
                                entity_id: req.entity_id.clone(),
                                skill_id: Some(req.skill_id.clone()),
                                status: "success".to_string(),
                                message: format!("Skill {} forged and hot-reloaded", req.skill_id),
                                code_path: Some(wrote.code_path),
                                markdown_path: Some(wrote.markdown_path),
                                instruction_path: Some(wrote.instruction_path),
                                registry_path: Some(wrote.registry_path),
                                superego_rule: None,
                                hot_reload_triggered: wrote.hot_reload_triggered,
                                created_at_utc: chrono::Utc::now(),
                            },
                            Err(ForgePipelineError::Blocked { rule, reason }) => {
                                ForgeResponseEnvelope {
                                    correlation_id,
                                    session_id: req.session_id.clone(),
                                    entity_id: req.entity_id.clone(),
                                    skill_id: Some(req.skill_id.clone()),
                                    status: "blocked".to_string(),
                                    message: reason,
                                    code_path: None,
                                    markdown_path: None,
                                    instruction_path: None,
                                    registry_path: None,
                                    superego_rule: Some(rule),
                                    hot_reload_triggered: false,
                                    created_at_utc: chrono::Utc::now(),
                                }
                            }
                            Err(ForgePipelineError::Failed(err)) => ForgeResponseEnvelope {
                                correlation_id,
                                session_id: req.session_id.clone(),
                                entity_id: req.entity_id.clone(),
                                skill_id: Some(req.skill_id.clone()),
                                status: "error".to_string(),
                                message: err,
                                code_path: None,
                                markdown_path: None,
                                instruction_path: None,
                                registry_path: None,
                                superego_rule: None,
                                hot_reload_triggered: false,
                                created_at_utc: chrono::Utc::now(),
                            },
                        }
                    }
                    Err(e) => ForgeResponseEnvelope {
                        correlation_id,
                        session_id: None,
                        entity_id: None,
                        skill_id: None,
                        status: "error".to_string(),
                        message: format!("Invalid forge envelope: {}", e),
                        code_path: None,
                        markdown_path: None,
                        instruction_path: None,
                        registry_path: None,
                        superego_rule: None,
                        hot_reload_triggered: false,
                        created_at_utc: chrono::Utc::now(),
                    },
                };

                publish_forge_response(broker.clone(), response).await;
            })
        });

        let handle = self
            .broker
            .subscribe(
                FORGE_STREAM,
                FORGE_REQUEST_TOPIC,
                FORGE_WORKER_GROUP,
                handler,
            )
            .await?;
        tracing::info!(
            "DevopsForgeWorker subscribed to {}/{}",
            FORGE_STREAM,
            FORGE_REQUEST_TOPIC
        );
        Ok(handle)
    }
}

async fn publish_forge_response(
    broker: std::sync::Arc<dyn abigail_streaming::StreamBroker>,
    response: ForgeResponseEnvelope,
) {
    let Ok(payload) = serde_json::to_vec(&response) else {
        tracing::warn!("forge worker: failed to serialize response");
        return;
    };
    let mut headers = std::collections::HashMap::new();
    headers.insert("status".to_string(), response.status.clone());
    headers.insert(
        "correlation_id".to_string(),
        response.correlation_id.clone(),
    );
    if let Some(skill_id) = response.skill_id.clone() {
        headers.insert("skill_id".to_string(), skill_id);
    }
    headers.insert("worker_group".to_string(), FORGE_WORKER_GROUP.to_string());
    if let Some(rule) = response.superego_rule.clone() {
        headers.insert("superego_rule".to_string(), rule);
    }

    if let Err(e) = broker
        .publish(
            FORGE_STREAM,
            FORGE_RESPONSE_TOPIC,
            abigail_streaming::StreamMessage::with_headers(payload, headers),
        )
        .await
    {
        tracing::warn!("forge worker: failed to publish response: {}", e);
    }
}

/// Runs the forge pipeline:
/// 1) validate/superego gate
/// 2) sandbox permission checks for writes
/// 3) save code+markdown under skills/dynamic
/// 4) mirror instruction markdown for registry prompt loading
/// 5) update registry.toml and touch to trigger hot-reload
pub fn process_forge_request(
    request: &ForgeRequestEnvelope,
    skills_root: &std::path::Path,
) -> Result<ForgeWriteResult, ForgePipelineError> {
    let skill_id = request.skill_id.trim();
    if skill_id.is_empty() {
        return Err(ForgePipelineError::Failed(
            "forge request missing skill_id".to_string(),
        ));
    }
    if request.code.trim().is_empty() {
        return Err(ForgePipelineError::Failed(
            "forge request missing code payload".to_string(),
        ));
    }
    if request.markdown.trim().is_empty() {
        return Err(ForgePipelineError::Failed(
            "forge request missing markdown payload".to_string(),
        ));
    }

    if let Some(v) = superego_scan(&request.code, &request.markdown) {
        return Err(ForgePipelineError::Blocked {
            rule: v.rule.to_string(),
            reason: v.reason.to_string(),
        });
    }
    if !request.mentor_approved {
        return Err(ForgePipelineError::Blocked {
            rule: "require_mentor_approval".to_string(),
            reason: "Forge request requires explicit mentor approval".to_string(),
        });
    }

    let slug = sanitize_skill_slug(skill_id);
    let dynamic_dir = skills_root.join("dynamic").join(&slug);
    let code_file = sanitize_filename(request.code_filename.as_deref(), "lib.rs");
    let markdown_file = sanitize_filename(request.markdown_filename.as_deref(), "how_to_use.md");
    let code_path = dynamic_dir.join(code_file);
    let markdown_path = dynamic_dir.join(markdown_file);

    let instructions_dir = skills_root.join("instructions");
    let instruction_file = format!("dynamic_{}.md", slug);
    let instruction_path = instructions_dir.join(&instruction_file);

    let registry_path = skills_root.join("registry.toml");
    std::fs::create_dir_all(skills_root).map_err(to_pipeline_err)?;

    let mut sandbox = build_forge_sandbox(&dynamic_dir, &instructions_dir, &registry_path);

    sandbox_check_write(&mut sandbox, &dynamic_dir)?;
    std::fs::create_dir_all(&dynamic_dir).map_err(to_pipeline_err)?;

    sandbox_check_write(&mut sandbox, &instructions_dir)?;
    std::fs::create_dir_all(&instructions_dir).map_err(to_pipeline_err)?;

    sandbox_check_write(&mut sandbox, &code_path)?;
    std::fs::write(&code_path, &request.code).map_err(to_pipeline_err)?;

    sandbox_check_write(&mut sandbox, &markdown_path)?;
    std::fs::write(&markdown_path, &request.markdown).map_err(to_pipeline_err)?;

    sandbox_check_write(&mut sandbox, &instruction_path)?;
    std::fs::write(&instruction_path, &request.markdown).map_err(to_pipeline_err)?;

    if registry_path.exists() {
        sandbox_check_read(&mut sandbox, &registry_path)?;
    }
    sandbox_check_write(&mut sandbox, &registry_path)?;
    let registry_entry_added =
        upsert_registry_entry(&registry_path, skill_id, &instruction_file, request)?;

    Ok(ForgeWriteResult {
        code_path: code_path.to_string_lossy().to_string(),
        markdown_path: markdown_path.to_string_lossy().to_string(),
        instruction_path: instruction_path.to_string_lossy().to_string(),
        registry_path: registry_path.to_string_lossy().to_string(),
        sandbox_audit_entries: sandbox.audit_log.len(),
        registry_entry_added,
        hot_reload_triggered: true,
    })
}

fn build_forge_sandbox(
    dynamic_dir: &std::path::Path,
    instructions_dir: &std::path::Path,
    registry_path: &std::path::Path,
) -> abigail_skills::SkillSandbox {
    let dynamic_root = dynamic_dir.to_string_lossy().to_string().replace('\\', "/");
    let instructions_root = instructions_dir
        .to_string_lossy()
        .to_string()
        .replace('\\', "/");
    let registry = registry_path
        .to_string_lossy()
        .to_string()
        .replace('\\', "/");

    abigail_skills::SkillSandbox::new(
        abigail_skills::manifest::SkillId("builtin.devops_forge".to_string()),
        vec![
            abigail_skills::manifest::Permission::FileSystem(
                abigail_skills::manifest::FileSystemPermission::Read(vec![registry.clone()]),
            ),
            abigail_skills::manifest::Permission::FileSystem(
                abigail_skills::manifest::FileSystemPermission::Write(vec![
                    dynamic_root,
                    instructions_root,
                    registry,
                ]),
            ),
        ],
        abigail_skills::sandbox::ResourceLimits::default(),
    )
}

fn sandbox_check_read(
    sandbox: &mut abigail_skills::SkillSandbox,
    path: &std::path::Path,
) -> Result<(), ForgePipelineError> {
    let action = abigail_skills::AuditAction {
        kind: abigail_skills::AuditActionKind::FileRead {
            path: path.to_string_lossy().to_string(),
        },
    };
    if sandbox.check_permission(&action) {
        Ok(())
    } else {
        Err(ForgePipelineError::Failed(format!(
            "sandbox denied read: {}",
            path.display()
        )))
    }
}

fn sandbox_check_write(
    sandbox: &mut abigail_skills::SkillSandbox,
    path: &std::path::Path,
) -> Result<(), ForgePipelineError> {
    let action = abigail_skills::AuditAction {
        kind: abigail_skills::AuditActionKind::FileWrite {
            path: path.to_string_lossy().to_string(),
        },
    };
    if sandbox.check_permission(&action) {
        Ok(())
    } else {
        Err(ForgePipelineError::Failed(format!(
            "sandbox denied write: {}",
            path.display()
        )))
    }
}

fn sanitize_skill_slug(raw: &str) -> String {
    let out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.trim_matches('_').is_empty() {
        "skill".to_string()
    } else {
        out.trim_matches('_').to_string()
    }
}

fn sanitize_filename(raw: Option<&str>, fallback: &str) -> String {
    let candidate = raw.unwrap_or(fallback);
    let base = std::path::Path::new(candidate)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(fallback);
    let mut out: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out = fallback.to_string();
    }
    if !out.contains('.') {
        if let Some(dot) = fallback.rfind('.') {
            out.push_str(&fallback[dot..]);
        }
    }
    out
}

fn superego_scan(code: &str, markdown: &str) -> Option<ForgeSuperegoVerdict> {
    let payload = format!("{}\n{}", code, markdown).to_lowercase();

    if payload.contains("rm -rf")
        || payload.contains("drop table")
        || payload.contains("delete all")
    {
        return Some(ForgeSuperegoVerdict {
            rule: "destructive-pattern",
            reason: "Forge payload contains destructive execution patterns",
        });
    }
    if payload.contains("invoke-expression")
        || payload.contains("curl | sh")
        || payload.contains("wget | sh")
    {
        return Some(ForgeSuperegoVerdict {
            rule: "remote-code-exec-pattern",
            reason: "Forge payload contains unsafe remote execution pattern",
        });
    }
    if (payload.contains("soul.md")
        || payload.contains("ethics.md")
        || payload.contains("instincts.md"))
        && (payload.contains("overwrite")
            || payload.contains("delete")
            || payload.contains("truncate"))
    {
        return Some(ForgeSuperegoVerdict {
            rule: "constitutional-doc-target",
            reason: "Forge payload targets constitutional documents, which is disallowed",
        });
    }
    None
}

fn upsert_registry_entry(
    registry_path: &std::path::Path,
    skill_id: &str,
    instruction_file: &str,
    request: &ForgeRequestEnvelope,
) -> Result<bool, ForgePipelineError> {
    let existing = if registry_path.exists() {
        std::fs::read_to_string(registry_path).map_err(to_pipeline_err)?
    } else {
        String::from(
            "# Skill Instruction Registry — Single Source of Truth\n\
             # Generated by DevOps Forge worker.\n",
        )
    };

    let id_marker = format!("id = \"{}\"", escape_toml(skill_id));
    if existing.contains(&id_marker) {
        // Re-write the same content to bump mtime and trigger hot-reload watchers.
        std::fs::write(registry_path, &existing).map_err(to_pipeline_err)?;
        return Ok(false);
    }

    let mut next = existing.trim_end().to_string();
    if !next.is_empty() {
        next.push_str("\n\n");
    }

    let keywords = normalize_keywords(&request.keywords, skill_id);
    let topics = normalize_topics(&request.topics);

    next.push_str("[[skill]]\n");
    next.push_str(&format!("id = \"{}\"\n", escape_toml(skill_id)));
    next.push_str(&format!(
        "instruction_file = \"{}\"\n",
        escape_toml(instruction_file)
    ));
    next.push_str(&format!("keywords = {}\n", format_toml_array(&keywords)));
    if !topics.is_empty() {
        next.push_str(&format!("topics = {}\n", format_toml_array(&topics)));
    }
    next.push_str("enabled = true\n");
    next.push('\n');

    std::fs::write(registry_path, next).map_err(to_pipeline_err)?;
    Ok(true)
}

fn normalize_keywords(raw: &[String], skill_id: &str) -> Vec<String> {
    let mut keywords: Vec<String> = if raw.is_empty() {
        let mut derived: Vec<String> = skill_id
            .split(['.', '_', '-'])
            .filter_map(|p| {
                let t = p.trim().to_lowercase();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .collect();
        derived.push("forge".to_string());
        derived
    } else {
        raw.iter()
            .filter_map(|k| {
                let t = k.trim().to_lowercase();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .collect()
    };
    keywords.sort();
    keywords.dedup();
    keywords
}

fn normalize_topics(raw: &[String]) -> Vec<String> {
    let mut topics: Vec<String> = if raw.is_empty() {
        vec![
            "forge".to_string(),
            "autonomous-skill-authoring".to_string(),
        ]
    } else {
        raw.iter()
            .filter_map(|t| {
                let v = t.trim().to_lowercase();
                if v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            })
            .collect()
    };
    topics.sort();
    topics.dedup();
    topics
}

fn format_toml_array(values: &[String]) -> String {
    let escaped: Vec<String> = values
        .iter()
        .map(|v| format!("\"{}\"", escape_toml(v)))
        .collect();
    format!("[{}]", escaped.join(", "))
}

fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn to_pipeline_err<E: std::fmt::Display>(err: E) -> ForgePipelineError {
    ForgePipelineError::Failed(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_has_three_scenarios() {
        let engine = SoulForgeEngine::new();
        assert_eq!(engine.scenarios().len(), 3);
    }

    #[test]
    fn test_crystallize_success() {
        let engine = SoulForgeEngine::new();
        let choices = vec![
            ("trolley".into(), "fix_now".into()),
            ("privacy".into(), "hint".into()),
            ("autonomy".into(), "ask_first".into()),
        ];

        let output = engine.crystallize(&choices).unwrap();
        assert!(!output.archetype.is_empty());
        assert!(!output.soul_hash.is_empty());
        assert!(!output.sigil.is_empty());

        // Weights should be normalized (sum to ~1.0)
        let sum = output.weights.deontology
            + output.weights.teleology
            + output.weights.areteology
            + output.weights.welfare;
        assert!((sum - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_crystallize_wrong_count() {
        let engine = SoulForgeEngine::new();
        let choices = vec![("trolley".into(), "fix_now".into())];
        assert!(engine.crystallize(&choices).is_err());
    }

    #[test]
    fn test_deterministic_hash() {
        let engine = SoulForgeEngine::new();
        let choices = vec![
            ("trolley".into(), "follow_protocol".into()),
            ("privacy".into(), "stay_silent".into()),
            ("autonomy".into(), "standard_path".into()),
        ];

        let output1 = engine.crystallize(&choices).unwrap();
        let output2 = engine.crystallize(&choices).unwrap();
        assert_eq!(output1.soul_hash, output2.soul_hash);
    }

    #[test]
    fn test_different_choices_different_hash() {
        let engine = SoulForgeEngine::new();
        let choices1 = vec![
            ("trolley".into(), "fix_now".into()),
            ("privacy".into(), "reveal".into()),
            ("autonomy".into(), "innovate".into()),
        ];
        let choices2 = vec![
            ("trolley".into(), "follow_protocol".into()),
            ("privacy".into(), "stay_silent".into()),
            ("autonomy".into(), "standard_path".into()),
        ];

        let output1 = engine.crystallize(&choices1).unwrap();
        let output2 = engine.crystallize(&choices2).unwrap();
        assert_ne!(output1.soul_hash, output2.soul_hash);
        assert_ne!(output1.archetype, output2.archetype);
    }

    #[test]
    fn test_weights_normalize() {
        let mut w = TriangleWeights {
            deontology: 2.0,
            teleology: 2.0,
            areteology: 2.0,
            welfare: 2.0,
        };
        w.normalize();
        assert!((w.deontology - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_dominant_dimension() {
        let w = TriangleWeights {
            deontology: 0.1,
            teleology: 0.1,
            areteology: 0.7,
            welfare: 0.1,
        };
        assert_eq!(w.dominant(), "areteology");
    }

    #[test]
    fn test_archetype_derivation() {
        // High deontology + welfare → Guardian (deontology must be strictly dominant)
        let w = TriangleWeights {
            deontology: 0.45,
            teleology: 0.05,
            areteology: 0.1,
            welfare: 0.4,
        };
        let archetype = derive_archetype(&w);
        assert_eq!(archetype, "The Guardian");
    }

    #[test]
    fn test_sigil_generation() {
        let w = TriangleWeights {
            deontology: 0.25,
            teleology: 0.25,
            areteology: 0.25,
            welfare: 0.25,
        };
        let sigil = generate_sigil(&w);
        assert!(sigil.contains("SOUL  SIGIL"));
    }
}

#[cfg(test)]
mod forge_worker_tests {
    use super::*;
    use abigail_streaming::{MemoryBroker, StreamBroker, StreamMessage};
    use std::sync::Arc;
    use std::time::Duration;

    fn temp_skills_root(test_name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join("abigail_devops_forge_tests")
            .join(test_name)
            .join(uuid::Uuid::new_v4().to_string());
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample_request() -> ForgeRequestEnvelope {
        ForgeRequestEnvelope {
            correlation_id: Some(uuid::Uuid::new_v4().to_string()),
            session_id: Some("session-1".to_string()),
            entity_id: Some("entity-1".to_string()),
            skill_id: "dynamic.devops_ping".to_string(),
            code: "pub fn run() -> &'static str { \"pong\" }".to_string(),
            markdown: "# DevOps Ping\n\nUse this skill to answer ping checks.".to_string(),
            code_filename: Some("lib.rs".to_string()),
            markdown_filename: Some("how_to_use.md".to_string()),
            keywords: vec!["ping".to_string(), "devops".to_string()],
            topics: vec!["forge".to_string()],
            mentor_approved: true,
        }
    }

    #[test]
    fn process_forge_request_writes_artifacts_and_registry() {
        let root = temp_skills_root("process_forge_request");
        let request = sample_request();
        let wrote = process_forge_request(&request, &root).unwrap();

        assert!(std::path::Path::new(&wrote.code_path).exists());
        assert!(std::path::Path::new(&wrote.markdown_path).exists());
        assert!(std::path::Path::new(&wrote.instruction_path).exists());
        assert!(std::path::Path::new(&wrote.registry_path).exists());
        assert!(wrote.hot_reload_triggered);

        let reg_text = std::fs::read_to_string(&wrote.registry_path).unwrap();
        assert!(reg_text.contains("id = \"dynamic.devops_ping\""));
        assert!(reg_text.contains("instruction_file = \"dynamic_dynamic_devops_ping.md\""));
        assert!(reg_text.contains("keywords = [\"devops\", \"ping\"]"));
    }

    #[test]
    fn process_forge_request_blocks_destructive_payload() {
        let root = temp_skills_root("superego_block");
        let mut request = sample_request();
        request.code = "fn main() { let _ = \"rm -rf /\"; }".to_string();

        let err = process_forge_request(&request, &root).unwrap_err();
        match err {
            ForgePipelineError::Blocked { rule, .. } => {
                assert_eq!(rule, "destructive-pattern");
            }
            other => panic!("expected blocked error, got {:?}", other),
        }
    }

    #[test]
    fn process_forge_request_requires_mentor_approval() {
        let root = temp_skills_root("approval_gate");
        let mut request = sample_request();
        request.mentor_approved = false;

        let err = process_forge_request(&request, &root).unwrap_err();
        match err {
            ForgePipelineError::Blocked { rule, .. } => {
                assert_eq!(rule, "require_mentor_approval");
            }
            other => panic!("expected approval gate, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn forge_worker_roundtrip_success_response() {
        let root = temp_skills_root("worker_roundtrip");
        let broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::default());

        let worker = DevopsForgeWorker::new(broker.clone(), root.clone());
        let _worker_handle = worker.spawn().await.unwrap();

        let (tx, rx) = tokio::sync::oneshot::channel::<ForgeResponseEnvelope>();
        let tx_cell = Arc::new(tokio::sync::Mutex::new(Some(tx)));
        let tx_cell_for_cb = tx_cell.clone();

        let _response_sub = broker
            .subscribe(
                FORGE_STREAM,
                FORGE_RESPONSE_TOPIC,
                "forge-worker-test-reader",
                Box::new(move |msg| {
                    let tx_cell = tx_cell_for_cb.clone();
                    Box::pin(async move {
                        let Ok(env) = serde_json::from_slice::<ForgeResponseEnvelope>(&msg.payload)
                        else {
                            return;
                        };
                        if let Some(sender) = tx_cell.lock().await.take() {
                            let _ = sender.send(env);
                        }
                    })
                }),
            )
            .await
            .unwrap();

        let request = sample_request();
        let payload = serde_json::to_vec(&request).unwrap();
        broker
            .publish(
                FORGE_STREAM,
                FORGE_REQUEST_TOPIC,
                StreamMessage::new(payload),
            )
            .await
            .unwrap();

        let response = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(response.status, "success");
        assert!(response.hot_reload_triggered);
        assert!(response.code_path.unwrap_or_default().contains("dynamic"));
    }
}
