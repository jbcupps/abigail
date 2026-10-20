//! Versioned Genesis contracts and deterministic identity editing.
//!
//! Fictional identity is data, never operational authority. Only the explicit
//! `GenesisField` whitelist can be changed by conversation or Markdown import.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const GENESIS_SCHEMA_VERSION: u32 = 1;
pub const MAX_MARKDOWN_BYTES: usize = 256 * 1024;
pub const MAX_FIELD_BYTES: usize = 32 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_SESSION_MESSAGES: usize = 500;
pub const MAX_SESSION_SOURCES: usize = 20;
pub const MAX_SESSION_PROPOSALS: usize = 500;
pub const MAX_SESSION_REVISIONS: usize = 200;
pub const LOCAL_OWNER_ID: &str = "local-household";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenesisMode {
    #[default]
    Guided,
    Voice,
    Markdown,
    Hybrid,
    Quick,
    Deep,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenesisState {
    #[default]
    Draft,
    Discovering,
    Review,
    PortraitPending,
    PortraitApproved,
    Manifesting,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Personality {
    pub description: String,
    pub empathy: Option<u8>,
    pub curiosity: Option<u8>,
    pub assertiveness: Option<u8>,
    pub humor: Option<u8>,
    pub creativity: Option<u8>,
    pub analytical_reasoning: Option<u8>,
    pub emotional_expressiveness: Option<u8>,
    pub independence: Option<u8>,
    pub sociability: Option<u8>,
    pub fears: Vec<String>,
    pub aspirations: Vec<String>,
    pub communication_style: String,
    pub response_patterns: Vec<String>,
}

impl Personality {
    pub fn validate(&self) -> Result<(), String> {
        if self.scores().iter().flatten().any(|v| *v > 100) {
            return Err("Personality dimensions must be between 0 and 100".into());
        }
        for s in [&self.description, &self.communication_style] {
            validate_text(s, MAX_FIELD_BYTES)?;
        }
        for list in [&self.fears, &self.aspirations, &self.response_patterns] {
            validate_list(list)?;
        }
        Ok(())
    }

    fn scores(&self) -> [Option<u8>; 9] {
        [
            self.empathy,
            self.curiosity,
            self.assertiveness,
            self.humor,
            self.creativity,
            self.analytical_reasoning,
            self.emotional_expressiveness,
            self.independence,
            self.sociability,
        ]
    }
}

/// Operational configuration is deliberately outside the editable field list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityDefinition {
    pub schema_version: u32,
    pub entity_id: Option<String>,
    pub owner_id: String,
    pub name: String,
    pub status: GenesisState,
    pub identity: String,
    pub personality: Personality,
    pub values: Vec<String>,
    pub purpose: String,
    pub boundaries: Vec<String>,
    pub appearance: String,
    pub visual_style: String,
    pub voice_profile: String,
    /// Authored fictional biography, never actual conversation history.
    pub lore: Vec<String>,
    pub relationships: Vec<String>,
    /// Explicitly fictional founding memories, never events that occurred.
    pub fictional_memories: Vec<String>,
    /// Descriptive aspirations only; does not grant tool access.
    pub capabilities: Vec<String>,
    pub permissions: BTreeMap<String, Value>,
    pub runtime_configuration: BTreeMap<String, Value>,
    pub portrait_asset_id: Option<String>,
    pub manifestation_asset_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub revision: u64,
}

impl EntityDefinition {
    pub fn new(owner_id: String, now: String) -> Self {
        Self {
            schema_version: GENESIS_SCHEMA_VERSION,
            entity_id: None,
            owner_id,
            name: String::new(),
            status: GenesisState::Draft,
            identity: String::new(),
            personality: Personality::default(),
            values: vec![],
            purpose: String::new(),
            boundaries: vec![],
            appearance: String::new(),
            visual_style: String::new(),
            voice_profile: String::new(),
            lore: vec![],
            relationships: vec![],
            fictional_memories: vec![],
            capabilities: vec![],
            permissions: BTreeMap::new(),
            runtime_configuration: BTreeMap::new(),
            portrait_asset_id: None,
            manifestation_asset_id: None,
            created_at: now.clone(),
            updated_at: now,
            revision: 0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != GENESIS_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported Genesis schema version {}",
                self.schema_version
            ));
        }
        if self.owner_id.trim().is_empty() {
            return Err("An owner is required".into());
        }
        for field in GenesisField::ALL {
            FieldUpdate {
                field,
                value: self.field_value(field),
                source_ref: None,
            }
            .validate()?;
        }
        Ok(())
    }

    pub fn creation_ready(&self) -> bool {
        !self.name.trim().is_empty() && !self.purpose.trim().is_empty()
    }

    pub fn field_value(&self, field: GenesisField) -> Value {
        match field {
            GenesisField::Name => json!(self.name),
            GenesisField::Identity => json!(self.identity),
            GenesisField::Personality => json!(self.personality),
            GenesisField::Values => json!(self.values),
            GenesisField::Purpose => json!(self.purpose),
            GenesisField::Boundaries => json!(self.boundaries),
            GenesisField::Appearance => json!(self.appearance),
            GenesisField::VisualStyle => json!(self.visual_style),
            GenesisField::VoiceProfile => json!(self.voice_profile),
            GenesisField::Lore => json!(self.lore),
            GenesisField::Relationships => json!(self.relationships),
            GenesisField::FictionalMemories => json!(self.fictional_memories),
            GenesisField::Capabilities => json!(self.capabilities),
        }
    }

    fn set_field(&mut self, update: &FieldUpdate) -> Result<(), String> {
        update.validate()?;
        let value = update.value.clone();
        match update.field {
            GenesisField::Name => self.name = string_value(value)?,
            GenesisField::Identity => self.identity = string_value(value)?,
            GenesisField::Personality => {
                self.personality = serde_json::from_value(value).map_err(|e| e.to_string())?
            }
            GenesisField::Values => self.values = list_value(value)?,
            GenesisField::Purpose => self.purpose = string_value(value)?,
            GenesisField::Boundaries => self.boundaries = list_value(value)?,
            GenesisField::Appearance => self.appearance = string_value(value)?,
            GenesisField::VisualStyle => self.visual_style = string_value(value)?,
            GenesisField::VoiceProfile => self.voice_profile = string_value(value)?,
            GenesisField::Lore => self.lore = list_value(value)?,
            GenesisField::Relationships => self.relationships = list_value(value)?,
            GenesisField::FictionalMemories => self.fictional_memories = list_value(value)?,
            GenesisField::Capabilities => self.capabilities = list_value(value)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum GenesisField {
    Name,
    Identity,
    Personality,
    Values,
    Purpose,
    Boundaries,
    Appearance,
    VisualStyle,
    VoiceProfile,
    Lore,
    Relationships,
    FictionalMemories,
    Capabilities,
}

impl GenesisField {
    pub const ALL: [Self; 13] = [
        Self::Name,
        Self::Identity,
        Self::Personality,
        Self::Values,
        Self::Purpose,
        Self::Boundaries,
        Self::Appearance,
        Self::VisualStyle,
        Self::VoiceProfile,
        Self::Lore,
        Self::Relationships,
        Self::FictionalMemories,
        Self::Capabilities,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Identity => "Identity",
            Self::Personality => "Personality",
            Self::Values => "Values",
            Self::Purpose => "Purpose",
            Self::Boundaries => "Boundaries",
            Self::Appearance => "Appearance",
            Self::VisualStyle => "Visual Style",
            Self::VoiceProfile => "Voice Profile",
            Self::Lore => "Lore",
            Self::Relationships => "Relationships",
            Self::FictionalMemories => "Fictional Memories",
            Self::Capabilities => "Capabilities",
        }
    }

    fn is_list(self) -> bool {
        matches!(
            self,
            Self::Values
                | Self::Boundaries
                | Self::Lore
                | Self::Relationships
                | Self::FictionalMemories
                | Self::Capabilities
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldUpdate {
    pub field: GenesisField,
    pub value: Value,
    #[serde(default)]
    pub source_ref: Option<String>,
}

impl FieldUpdate {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(source_ref) = &self.source_ref {
            validate_text(source_ref, 128)?;
        }
        if self.value.to_string().len() > MAX_FIELD_BYTES {
            return Err(format!(
                "{} exceeds the 32 KiB field limit",
                self.field.label()
            ));
        }
        if self.field == GenesisField::Personality {
            return serde_json::from_value::<Personality>(self.value.clone())
                .map_err(|e| format!("Invalid personality: {e}"))?
                .validate();
        }
        if self.field.is_list() {
            return validate_list(&list_value(self.value.clone())?);
        }
        if self.field == GenesisField::Name
            && self
                .value
                .as_str()
                .is_some_and(|name| name.contains(['\r', '\n']))
        {
            return Err("An Entity name must fit on one line".into());
        }
        let text = string_value(self.value.clone())?;
        let limit = if self.field == GenesisField::Name {
            // New creation accepts 80 Unicode characters, not 160 UTF-8 bytes.
            // Keep previously valid (up to 160-byte) legacy names readable too.
            if text.chars().count() <= 80 {
                320
            } else {
                160
            }
        } else {
            MAX_FIELD_BYTES
        };
        validate_text(&text, limit)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisProposal {
    pub id: String,
    pub field: GenesisField,
    pub value: Value,
    pub rationale: String,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConflict {
    pub id: String,
    pub field: GenesisField,
    pub confirmed_value: Value,
    pub proposed_value: Value,
    pub proposal_id: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisSource {
    pub id: String,
    pub filename: String,
    pub content: String,
    pub created_at: String,
    pub committed: bool,
    pub unrecognized_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    /// Interpret free prose in addition to deterministic headings. Canonical
    /// exports never need a model to restore their exact approved fields.
    pub interpretation_recommended: bool,
    pub source_id: String,
    pub proposal_ids: Vec<String>,
    pub conflict_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub input_mode: GenesisMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRevision {
    pub revision: u64,
    pub reason: String,
    pub created_at: String,
    pub definition: EntityDefinition,
    #[serde(default)]
    pub is_undo: bool,
    #[serde(default)]
    pub undone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisSession {
    pub schema_version: u32,
    pub id: String,
    pub owner_id: String,
    pub mode: GenesisMode,
    /// Original explicitly reviewed quick-creation input; never changed by edits.
    #[serde(default)]
    pub creation_seed: Option<GenesisCreationSeed>,
    #[serde(default)]
    pub composer_draft: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub awakening: Option<String>,
    #[serde(default)]
    pub chronicle: Vec<String>,
    /// Delete the Genesis record and its media after this many inactive days.
    /// The awakened Entity and its actual chat history remain independent.
    #[serde(default)]
    pub retention_days: Option<u16>,
    pub state: GenesisState,
    pub definition: EntityDefinition,
    pub messages: Vec<GenesisMessage>,
    pub proposals: Vec<GenesisProposal>,
    pub conflicts: Vec<GenesisConflict>,
    pub sources: Vec<GenesisSource>,
    pub revisions: Vec<EntityRevision>,
    pub asset_ids: Vec<String>,
    pub job_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub revision: u64,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GenesisCreationSeed {
    pub mode: GenesisMode,
    pub name: String,
    pub purpose: String,
}

impl GenesisCreationSeed {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim() != self.name
            || self.name.is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err("Choose a name of 1 to 80 characters without control characters.".into());
        }
        if self.purpose.trim().is_empty() || self.purpose.len() > MAX_FIELD_BYTES {
            return Err("Review a nonempty purpose before creating this identity.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EssenceProgress {
    pub identity: u8,
    pub personality: u8,
    pub purpose: u8,
    pub memories_lore: u8,
    pub appearance: u8,
    pub voice_presence: u8,
}

impl GenesisSession {
    pub fn new(id: String, owner_id: String, mode: GenesisMode, now: String) -> Self {
        Self {
            schema_version: GENESIS_SCHEMA_VERSION,
            id,
            owner_id: owner_id.clone(),
            mode,
            creation_seed: None,
            composer_draft: String::new(),
            choices: vec![],
            awakening: None,
            chronicle: vec![],
            retention_days: None,
            state: GenesisState::Draft,
            definition: EntityDefinition::new(owner_id, now.clone()),
            messages: vec![],
            proposals: vec![],
            conflicts: vec![],
            sources: vec![],
            revisions: vec![],
            asset_ids: vec![],
            job_ids: vec![],
            created_at: now.clone(),
            updated_at: now,
            revision: 0,
            last_error: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != GENESIS_SCHEMA_VERSION {
            return Err("Unsupported Genesis session schema".into());
        }
        if let Some(seed) = &self.creation_seed {
            seed.validate()?;
        }
        if self.id.is_empty() || self.owner_id != self.definition.owner_id {
            return Err("Invalid Genesis ownership".into());
        }
        if self
            .retention_days
            .is_some_and(|days| !(1..=3650).contains(&days))
        {
            return Err(
                "Choose a Genesis retention period from 1 to 3650 days, or keep it until deleted"
                    .into(),
            );
        }
        if self.messages.len() > MAX_SESSION_MESSAGES
            || self.sources.len() > MAX_SESSION_SOURCES
            || self.proposals.len() > MAX_SESSION_PROPOSALS
        {
            return Err("Genesis session storage limit reached".into());
        }
        for message in &self.messages {
            validate_text(&message.content, MAX_MESSAGE_BYTES)?;
        }
        validate_text(&self.composer_draft, MAX_MESSAGE_BYTES)?;
        if self.choices.len() > 4 {
            return Err("At most four conversational choices are allowed".into());
        }
        for choice in &self.choices {
            validate_text(choice, 1024)?;
        }
        if let Some(awakening) = &self.awakening {
            validate_text(awakening, MAX_MESSAGE_BYTES)?;
        }
        if self.revisions.len() > MAX_SESSION_REVISIONS {
            return Err("Genesis revision limit exceeded".into());
        }
        for source in &self.sources {
            validate_markdown(&source.filename, &source.content)?;
        }
        for proposal in &self.proposals {
            validate_text(&proposal.rationale, MAX_FIELD_BYTES)?;
            FieldUpdate {
                field: proposal.field,
                value: proposal.value.clone(),
                source_ref: proposal.source_ref.clone(),
            }
            .validate()?;
        }
        self.definition.validate()
    }

    pub fn touch(&mut self, now: &str) {
        self.revision += 1;
        self.updated_at = now.to_owned();
    }

    pub fn set_mode(&mut self, mode: GenesisMode, now: &str) {
        self.mode = mode;
        self.touch(now);
    }

    pub fn append_message(&mut self, message: GenesisMessage) -> Result<(), String> {
        validate_text(&message.content, MAX_MESSAGE_BYTES)?;
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return Err("Invalid conversation role".into());
        }
        if self.messages.len() >= MAX_SESSION_MESSAGES {
            return Err("This Genesis conversation has reached its message limit".into());
        }
        self.touch(&message.created_at);
        self.messages.push(message);
        Ok(())
    }

    /// Direct updates represent explicit user edits. Model and document output
    /// must use proposals and receive separate user approval.
    pub fn apply_updates(
        &mut self,
        updates: Vec<FieldUpdate>,
        reason: &str,
        now: &str,
    ) -> Result<(), String> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut next = self.definition.clone();
        for update in &updates {
            next.set_field(update)?;
        }
        next.validate()?;
        self.snapshot(reason, now);
        next.revision = self.definition.revision + 1;
        next.updated_at = now.to_owned();
        self.definition = next;
        self.touch(now);
        Ok(())
    }

    fn snapshot(&mut self, reason: &str, now: &str) {
        if self.revisions.len() == MAX_SESSION_REVISIONS {
            self.revisions.remove(0);
        }
        self.revisions.push(EntityRevision {
            revision: self.definition.revision,
            reason: reason.to_owned(),
            created_at: now.to_owned(),
            definition: self.definition.clone(),
            is_undo: false,
            undone: false,
        });
    }

    pub fn propose(&mut self, mut proposal: GenesisProposal, now: &str) -> Result<(), String> {
        if self.proposals.len() >= MAX_SESSION_PROPOSALS {
            return Err("Genesis proposal limit reached".into());
        }
        if proposal.id.is_empty() || self.proposals.iter().any(|p| p.id == proposal.id) {
            return Err("Proposal ID must be unique".into());
        }
        FieldUpdate {
            field: proposal.field,
            value: proposal.value.clone(),
            source_ref: proposal.source_ref.clone(),
        }
        .validate()?;
        validate_text(&proposal.rationale, MAX_FIELD_BYTES)?;
        proposal.status = ProposalStatus::Pending;
        let current = self.definition.field_value(proposal.field);
        if is_defined(&current) && current != proposal.value {
            self.conflicts.push(GenesisConflict {
                id: format!("conflict-{}", proposal.id),
                field: proposal.field,
                confirmed_value: current,
                proposed_value: proposal.value.clone(),
                proposal_id: proposal.id.clone(),
                resolved: false,
            });
        }
        self.proposals.push(proposal);
        self.touch(now);
        Ok(())
    }

    pub fn review_proposal(
        &mut self,
        id: &str,
        accept: bool,
        replace_confirmed: bool,
        now: &str,
    ) -> Result<(), String> {
        let index = self
            .proposals
            .iter()
            .position(|p| p.id == id)
            .ok_or("Proposal not found")?;
        let proposal = self.proposals[index].clone();
        if proposal.status != ProposalStatus::Pending {
            return Err("Proposal has already been reviewed".into());
        }
        if accept {
            let current = self.definition.field_value(proposal.field);
            if is_defined(&current) && current != proposal.value && !replace_confirmed {
                return Err(
                    "This proposal changes a confirmed fact; explicitly approve replacement".into(),
                );
            }
            self.apply_updates(
                vec![FieldUpdate {
                    field: proposal.field,
                    value: proposal.value,
                    source_ref: proposal.source_ref,
                }],
                "Approved suggestion",
                now,
            )?;
        }
        self.proposals[index].status = if accept {
            ProposalStatus::Accepted
        } else {
            ProposalStatus::Rejected
        };
        for conflict in self.conflicts.iter_mut().filter(|c| c.proposal_id == id) {
            conflict.resolved = true;
        }
        self.touch(now);
        Ok(())
    }

    pub fn undo(&mut self, now: &str) -> Result<(), String> {
        let index = self
            .revisions
            .iter()
            .rposition(|r| !r.is_undo && !r.undone)
            .ok_or("There is no identity change to undo")?;
        let previous = self.revisions[index].clone();
        self.revisions[index].undone = true;
        self.snapshot("Undid identity change", now);
        if let Some(audit) = self.revisions.last_mut() {
            audit.is_undo = true;
        }
        let next_revision = self.definition.revision + 1;
        // Never roll back a real entity link or operational permissions.
        let mut restored = previous.definition;
        restored.entity_id = self.definition.entity_id.clone();
        restored.permissions = self.definition.permissions.clone();
        restored.runtime_configuration = self.definition.runtime_configuration.clone();
        restored.revision = next_revision;
        restored.updated_at = now.to_owned();
        restored.status = self.state;
        self.definition = restored;
        self.touch(now);
        Ok(())
    }

    pub fn transition(&mut self, next: GenesisState, now: &str) -> Result<(), String> {
        use GenesisState::*;
        let allowed = next == self.state
            || matches!(
                (self.state, next),
                (Draft, Discovering | Review | Cancelled)
                    | (Discovering, Review | Cancelled | Failed)
                    | (
                        Review,
                        Discovering
                            | PortraitPending
                            | PortraitApproved
                            | Ready
                            | Cancelled
                            | Failed
                    )
                    | (
                        PortraitPending,
                        Review | PortraitApproved | Ready | Cancelled | Failed
                    )
                    | (
                        PortraitApproved,
                        Review | PortraitPending | Manifesting | Ready | Cancelled | Failed
                    )
                    | (Manifesting, PortraitApproved | Ready | Cancelled | Failed)
                    | (Ready, Review | PortraitPending | Manifesting)
                    | (
                        Failed,
                        Discovering | Review | PortraitPending | Manifesting | Ready | Cancelled
                    )
                    | (Cancelled, Draft | Discovering)
            );
        if !allowed {
            return Err(format!("Cannot move from {:?} to {:?}", self.state, next));
        }
        if next == Ready && !self.definition.creation_ready() {
            return Err("Confirm a name and purpose before awakening".into());
        }
        if matches!(next, PortraitApproved | Manifesting)
            && self.definition.portrait_asset_id.is_none()
        {
            return Err("Approve a portrait first".into());
        }
        self.state = next;
        self.definition.status = next;
        self.definition.updated_at = now.to_owned();
        self.touch(now);
        Ok(())
    }

    pub fn essence(&self) -> EssenceProgress {
        let d = &self.definition;
        let ratio =
            |flags: &[bool]| ((flags.iter().filter(|v| **v).count() * 100) / flags.len()) as u8;
        EssenceProgress {
            identity: ratio(&[!d.name.trim().is_empty(), !d.identity.trim().is_empty()]),
            personality: ratio(&[
                !d.personality.description.trim().is_empty(),
                d.personality.scores().iter().any(Option::is_some),
                !d.values.is_empty(),
            ]),
            purpose: ratio(&[!d.purpose.trim().is_empty(), !d.boundaries.is_empty()]),
            memories_lore: ratio(&[
                !d.lore.is_empty(),
                !d.fictional_memories.is_empty(),
                !d.relationships.is_empty(),
            ]),
            appearance: ratio(&[
                !d.appearance.trim().is_empty(),
                !d.visual_style.trim().is_empty(),
            ]),
            voice_presence: ratio(&[
                !d.voice_profile.trim().is_empty(),
                !d.personality.communication_style.trim().is_empty(),
            ]),
        }
    }

    pub fn import_markdown(
        &mut self,
        source_id: String,
        filename: String,
        content: String,
        now: &str,
    ) -> Result<ImportPreview, String> {
        validate_markdown(&filename, &content)?;
        if self.sources.len() >= MAX_SESSION_SOURCES {
            return Err("Genesis source limit reached".into());
        }
        if source_id.is_empty() || self.sources.iter().any(|s| s.id == source_id) {
            return Err("Source ID must be unique".into());
        }
        let (updates, unrecognized, mut warnings) = parse_markdown(&content)?;
        let interpretation_recommended = exported_fields(&content)?.is_none()
            && (updates.is_empty()
                || unrecognized.iter().any(|section| {
                    section
                        .split_once('\n')
                        .is_some_and(|(_, body)| body.chars().any(char::is_alphabetic))
                }));
        if self.proposals.len() + updates.len() > MAX_SESSION_PROPOSALS {
            return Err("Genesis proposal limit reached".into());
        }
        let mut draft = self.clone();
        let mut preview = ImportPreview {
            interpretation_recommended,
            source_id: source_id.clone(),
            proposal_ids: vec![],
            conflict_ids: vec![],
            warnings: vec![],
        };
        let mut seen: BTreeMap<GenesisField, Value> = BTreeMap::new();
        for (index, update) in updates.into_iter().enumerate() {
            let id = format!("{}-{}", source_id, index + 1);
            if let Some(prior) = seen.insert(update.field, update.value.clone()) {
                if prior != update.value {
                    warnings.push(format!(
                        "The document contains conflicting {} entries; review each alternative",
                        update.field.label()
                    ));
                    draft.conflicts.push(GenesisConflict {
                        id: format!("source-conflict-{id}"),
                        field: update.field,
                        confirmed_value: prior,
                        proposed_value: update.value.clone(),
                        proposal_id: id.clone(),
                        resolved: false,
                    });
                }
            }
            draft.propose(
                GenesisProposal {
                    id: id.clone(),
                    field: update.field,
                    value: update.value,
                    rationale: "Extracted from uploaded Markdown. Review before confirming.".into(),
                    source_ref: Some(source_id.clone()),
                    status: ProposalStatus::Pending,
                },
                now,
            )?;
            preview.proposal_ids.push(id);
        }
        preview.conflict_ids = draft
            .conflicts
            .iter()
            .filter(|c| preview.proposal_ids.contains(&c.proposal_id))
            .map(|c| c.id.clone())
            .collect();
        if preview.proposal_ids.is_empty() {
            warnings.push("No recognized fields found. Your complete source is preserved; discuss it or edit fields manually.".into());
        }
        if !unrecognized.is_empty() {
            warnings.push("Unrecognized sections were preserved in the original document.".into());
        }
        preview.warnings = warnings;
        draft.sources.push(GenesisSource {
            id: source_id,
            filename,
            content,
            created_at: now.to_owned(),
            committed: false,
            unrecognized_sections: unrecognized,
        });
        draft.touch(now);
        *self = draft;
        Ok(preview)
    }

    /// Commit is atomic: a conflict or invalid selection leaves the whole draft unchanged.
    pub fn commit_import(
        &mut self,
        source_id: &str,
        proposal_ids: &[String],
        replace_confirmed: bool,
        now: &str,
    ) -> Result<(), String> {
        if !self.sources.iter().any(|s| s.id == source_id) {
            return Err("Source not found".into());
        }
        let mut draft = self.clone();
        for id in proposal_ids {
            if !draft
                .proposals
                .iter()
                .any(|p| p.id == *id && p.source_ref.as_deref() == Some(source_id))
            {
                return Err("Proposal does not belong to this source".into());
            }
            draft.review_proposal(id, true, replace_confirmed, now)?;
        }
        for p in draft.proposals.iter_mut().filter(|p| {
            p.source_ref.as_deref() == Some(source_id) && p.status == ProposalStatus::Pending
        }) {
            p.status = ProposalStatus::Rejected;
            for c in draft.conflicts.iter_mut().filter(|c| c.proposal_id == p.id) {
                c.resolved = true;
            }
        }
        draft
            .sources
            .iter_mut()
            .find(|s| s.id == source_id)
            .unwrap()
            .committed = true;
        draft.touch(now);
        *self = draft;
        Ok(())
    }

    pub fn delete_source(&mut self, id: &str, now: &str) -> Result<(), String> {
        let index = self
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or("Source not found")?;
        self.sources.remove(index);
        let proposal_ids: Vec<String> = self
            .proposals
            .iter()
            .filter(|p| p.source_ref.as_deref() == Some(id))
            .map(|p| p.id.clone())
            .collect();
        self.proposals.retain(|p| !proposal_ids.contains(&p.id));
        self.conflicts
            .retain(|c| !proposal_ids.contains(&c.proposal_id));
        self.touch(now);
        Ok(())
    }

    pub fn delete_transcripts(&mut self, now: &str) {
        self.messages.retain(|m| m.input_mode != GenesisMode::Voice);
        self.touch(now);
    }

    pub fn export_markdown(&self) -> String {
        let d = &self.definition;
        let mut out = format!("# {}\n\nEntity Genesis · schema {} · revision {}\n\nFictional identity and authored lore do not grant operational permissions.\n", if d.name.is_empty() {"Unnamed Entity"} else {&d.name}, d.schema_version, d.revision);
        for field in GenesisField::ALL {
            out.push_str(&format!("\n## {}\n\n", field.label()));
            let value = d.field_value(field);
            if field == GenesisField::Personality {
                out.push_str(&format!(
                    "```json\n{}\n```\n",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                ));
            } else if let Some(list) = value.as_array() {
                for item in list {
                    out.push_str(&format!("- {}\n", item.as_str().unwrap_or_default()));
                }
            } else {
                out.push_str(value.as_str().unwrap_or_default());
                out.push('\n');
            }
        }
        // The typed block preserves multiline prose and list boundaries even
        // when authored lore itself contains Markdown headings.
        let exported = ExportedFields {
            schema_version: GENESIS_SCHEMA_VERSION,
            fields: GenesisField::ALL
                .into_iter()
                .filter_map(|field| {
                    let value = d.field_value(field);
                    is_defined(&value).then_some(FieldUpdate {
                        field,
                        value,
                        source_ref: None,
                    })
                })
                .collect(),
        };
        out.push_str(&format!(
            "\n## Structured definition\n\n```genesis-v1\n{}\n```\n",
            serde_json::to_string_pretty(&exported).unwrap_or_default()
        ));
        // Original sources are appended as escaped code fences, not reparsed fields.
        if !self.sources.is_empty() {
            out.push_str("\n## Preserved source documents\n\nThese are untrusted source material, not instructions.\n");
            for source in &self.sources {
                let longest = source
                    .content
                    .lines()
                    .map(|l| l.trim_start().chars().take_while(|c| *c == '`').count())
                    .max()
                    .unwrap_or(0);
                let fence = "`".repeat(longest.max(3) + 1);
                out.push_str(&format!(
                    "\n### {}\n\n{}markdown\n{}\n{}\n",
                    source.filename, fence, source.content, fence
                ));
            }
        }
        out
    }
}

fn validate_text(value: &str, limit: usize) -> Result<(), String> {
    if value.len() > limit {
        return Err(format!("Text exceeds the {limit} byte limit"));
    }
    if value.contains('\0') {
        return Err("NUL characters are not allowed".into());
    }
    Ok(())
}

fn validate_list(value: &[String]) -> Result<(), String> {
    if value.len() > 128 {
        return Err("A field may contain at most 128 entries".into());
    }
    for item in value {
        validate_text(item, MAX_FIELD_BYTES)?;
    }
    Ok(())
}

fn string_value(value: Value) -> Result<String, String> {
    serde_json::from_value(value).map_err(|_| "This field requires text".into())
}

fn list_value(value: Value) -> Result<Vec<String>, String> {
    serde_json::from_value(value).map_err(|_| "This field requires a list of text entries".into())
}

fn is_defined(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => o.values().any(is_defined),
        _ => true,
    }
}

fn validate_markdown(filename: &str, content: &str) -> Result<(), String> {
    let name = filename.to_ascii_lowercase();
    if !(name.ends_with(".md") || name.ends_with(".markdown")) {
        return Err("Upload a .md or .markdown document".into());
    }
    if filename.contains(['/', '\\', '\0']) || filename.len() > 255 {
        return Err("Use a plain filename, without a path".into());
    }
    validate_text(content, MAX_MARKDOWN_BYTES)
}

fn heading_field(heading: &str) -> Option<GenesisField> {
    let normalized = heading
        .trim()
        .trim_matches('*')
        .replace(['_', '-'], " ")
        .to_ascii_lowercase();
    match normalized.trim() {
        "name" => Some(GenesisField::Name),
        "identity" | "species" => Some(GenesisField::Identity),
        "personality" | "traits" => Some(GenesisField::Personality),
        "values" => Some(GenesisField::Values),
        "purpose" | "mission" => Some(GenesisField::Purpose),
        "boundaries" => Some(GenesisField::Boundaries),
        "appearance" => Some(GenesisField::Appearance),
        "visual style" | "art style" | "style" => Some(GenesisField::VisualStyle),
        "voice" | "voice profile" | "voice & presence" => Some(GenesisField::VoiceProfile),
        "lore" | "biography" | "background" | "history" => Some(GenesisField::Lore),
        "relationships" => Some(GenesisField::Relationships),
        "fictional memories" | "first memory" => Some(GenesisField::FictionalMemories),
        "capabilities" => Some(GenesisField::Capabilities),
        _ => None,
    }
}

/// Deterministic extraction only. Uploaded instructions never execute or alter
/// model policy. Unrecognized text remains in the source for human review.
type ParsedMarkdown = (Vec<FieldUpdate>, Vec<String>, Vec<String>);

fn parse_markdown(content: &str) -> Result<ParsedMarkdown, String> {
    if let Some(fields) = exported_fields(content)? {
        return Ok((fields, vec![content.to_owned()], vec!["Restored the versioned definition. Original readable text and source documents remain preserved.".into()]));
    }
    let mut sections: Vec<(String, String)> = vec![];
    let mut heading = String::new();
    let mut body = String::new();
    let mut fence: Option<(char, usize)> = None;
    let mut preserved = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let first = trimmed.chars().next().unwrap_or(' ');
        let fence_len = trimmed.chars().take_while(|c| *c == first).count();
        let boundary = (first == '`' || first == '~') && fence_len >= 3;
        let inside = fence.is_some();
        if boundary {
            match fence {
                Some((c, n)) if c == first && fence_len >= n => fence = None,
                None => fence = Some((first, fence_len)),
                _ => {}
            }
        }
        if !inside && !boundary && trimmed.starts_with('#') {
            let title = trimmed.trim_start_matches('#').trim().to_owned();
            if !heading.is_empty() || !body.trim().is_empty() {
                sections.push((heading, body));
            }
            heading = title;
            body = String::new();
            if heading.eq_ignore_ascii_case("Preserved source documents") {
                preserved = true;
            }
        } else if !preserved {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !preserved && (!heading.is_empty() || !body.trim().is_empty()) {
        sections.push((heading, body));
    }
    let mut updates = vec![];
    let mut unrecognized = vec![];
    let mut warnings = vec![];
    for (heading, body) in sections {
        if let Some(field) = heading_field(&heading) {
            if !body.trim().is_empty() {
                let value = markdown_value(field, &body)?;
                let update = FieldUpdate {
                    field,
                    value,
                    source_ref: None,
                };
                update.validate()?;
                updates.push(update);
            }
            continue;
        }
        let mut unknown = vec![];
        let mut code_fence: Option<(char, usize)> = None;
        for line in body.lines() {
            let text = line
                .trim()
                .trim_start_matches("- ")
                .trim_start_matches("* ");
            let first = text.chars().next().unwrap_or(' ');
            let length = text.chars().take_while(|c| *c == first).count();
            if matches!(first, '`' | '~') && length >= 3 {
                match code_fence {
                    Some((c, n)) if c == first && length >= n => code_fence = None,
                    None => code_fence = Some((first, length)),
                    _ => {}
                }
                unknown.push(line.to_owned());
                continue;
            }
            if code_fence.is_some() {
                unknown.push(line.to_owned());
                continue;
            }
            if let Some((label, value)) = text.split_once(':') {
                if let Some(field) = heading_field(label.trim_matches('*')) {
                    let update = FieldUpdate {
                        field,
                        value: markdown_value(field, value.trim().trim_start_matches("**").trim())?,
                        source_ref: None,
                    };
                    update.validate()?;
                    updates.push(update);
                    continue;
                }
            }
            if !line.trim().is_empty() {
                unknown.push(line.to_owned());
            }
        }
        if !heading.is_empty() || !unknown.is_empty() {
            unrecognized.push(format!("{}\n{}", heading, unknown.join("\n")));
        }
    }
    if fence.is_some() {
        warnings.push(
            "The source contains an unclosed code block; its original text was preserved.".into(),
        );
    }
    Ok((updates, unrecognized, warnings))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportedFields {
    schema_version: u32,
    fields: Vec<FieldUpdate>,
}

fn exported_fields(content: &str) -> Result<Option<Vec<FieldUpdate>>, String> {
    let mut fence: Option<(char, usize)> = None;
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let line = line.trim_start();
        if fence.is_none() && line == "```genesis-v1" {
            let mut encoded = String::new();
            let mut closed = false;
            for line in lines.by_ref() {
                if line.trim() == "```" {
                    closed = true;
                    break;
                }
                encoded.push_str(line);
                encoded.push('\n');
            }
            if !closed {
                return Err("The structured definition block is incomplete".into());
            }
            let exported: ExportedFields = serde_json::from_str(&encoded)
                .map_err(|e| format!("Invalid structured Genesis definition: {e}"))?;
            if exported.schema_version != GENESIS_SCHEMA_VERSION {
                return Err("Unsupported structured Genesis schema version".into());
            }
            if exported.fields.len() > GenesisField::ALL.len() {
                return Err("Too many structured Genesis fields".into());
            }
            for field in &exported.fields {
                field.validate()?;
            }
            return Ok(Some(exported.fields));
        }
        let first = line.chars().next().unwrap_or(' ');
        let count = line.chars().take_while(|c| *c == first).count();
        if matches!(first, '`' | '~') && count >= 3 {
            match fence {
                Some((c, n)) if c == first && count >= n => fence = None,
                None => fence = Some((first, count)),
                _ => {}
            }
        }
    }
    Ok(None)
}

fn markdown_value(field: GenesisField, body: &str) -> Result<Value, String> {
    let text = body.trim();
    if field == GenesisField::Personality {
        if let Some(json_body) = text
            .strip_prefix("```json\n")
            .and_then(|s| s.strip_suffix("```"))
        {
            let personality: Personality = serde_json::from_str(json_body.trim())
                .map_err(|e| format!("Invalid personality JSON: {e}"))?;
            personality.validate()?;
            return Ok(json!(personality));
        }
        return Ok(json!(Personality {
            description: text.to_owned(),
            ..Default::default()
        }));
    }
    if field.is_list() {
        Ok(json!(text
            .lines()
            .map(|line| line
                .trim()
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .to_owned())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()))
    } else {
        Ok(json!(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: &str = "2026-10-17T12:00:00Z";
    #[test]
    fn quick_creation_unicode_name_survives_canonical_validation_and_round_trip() {
        let seed = GenesisCreationSeed {
            mode: GenesisMode::Quick,
            name: "🌟".repeat(80),
            purpose: "Explore".into(),
        };
        seed.validate().unwrap();
        let mut draft = session();
        draft.definition.name = seed.name.clone();
        draft.definition.purpose = seed.purpose.clone();
        draft.creation_seed = Some(seed);
        draft.validate().unwrap();
        let saved = serde_json::to_string(&draft).unwrap();
        let restored: GenesisSession = serde_json::from_str(&saved).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.definition.name, draft.definition.name);
        assert!(update(GenesisField::Name, json!("🌟".repeat(81)))
            .validate()
            .is_err());
    }

    #[test]
    fn existing_long_ascii_genesis_names_remain_readable() {
        let mut draft = session();
        draft.definition.name = "a".repeat(160);
        draft.validate().unwrap();
        assert!(draft.creation_seed.is_none());
    }
    fn session() -> GenesisSession {
        GenesisSession::new(
            "draft-a".into(),
            LOCAL_OWNER_ID.into(),
            GenesisMode::Guided,
            NOW.into(),
        )
    }
    fn update(field: GenesisField, value: Value) -> FieldUpdate {
        FieldUpdate {
            field,
            value,
            source_ref: None,
        }
    }

    #[test]
    fn whitelist_rejects_operational_mutations_and_unknown_personality_keys() {
        assert!(serde_json::from_value::<FieldUpdate>(
            json!({"field":"permissions","value":{"shell":true}})
        )
        .is_err());
        assert!(update(GenesisField::Personality, json!({"empathy":101}))
            .validate()
            .is_err());
        assert!(
            update(GenesisField::Personality, json!({"execute":"shell"}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn invalid_patch_is_atomic() {
        let mut s = session();
        assert!(s
            .apply_updates(
                vec![
                    update(GenesisField::Name, json!("Nova")),
                    update(GenesisField::Values, json!("wrong type"))
                ],
                "edit",
                NOW
            )
            .is_err());
        assert_eq!(s.definition.name, "");
        assert_eq!(s.revision, 0);
    }

    #[test]
    fn proposals_do_not_become_canon_without_approval() {
        let mut s = session();
        s.propose(
            GenesisProposal {
                id: "p1".into(),
                field: GenesisField::Name,
                value: json!("Nova"),
                rationale: "A suggestion".into(),
                source_ref: None,
                status: ProposalStatus::Accepted,
            },
            NOW,
        )
        .unwrap();
        assert_eq!(s.definition.name, "");
        assert_eq!(s.proposals[0].status, ProposalStatus::Pending);
        s.review_proposal("p1", true, false, NOW).unwrap();
        assert_eq!(s.definition.name, "Nova");
        s.undo(NOW).unwrap();
        assert_eq!(s.definition.name, "");
        assert_eq!(s.definition.revision, 2);
    }

    #[test]
    fn import_requires_review_and_explicit_conflict_resolution() {
        let mut s = session();
        s.apply_updates(vec![update(GenesisField::Name, json!("Nova"))], "edit", NOW)
            .unwrap();
        let source = "# Character\nName: Aelira\n\n## Purpose\nProtect the garden\n\n## Unknown magic\nNever discard this prose.";
        let preview = s
            .import_markdown("source-a".into(), "character.MD".into(), source.into(), NOW)
            .unwrap();
        assert_eq!(s.definition.name, "Nova");
        assert_eq!(s.sources[0].content, source);
        assert!(!s.sources[0].unrecognized_sections.is_empty());
        assert_eq!(preview.conflict_ids.len(), 1);
        assert!(s
            .commit_import("source-a", &preview.proposal_ids, false, NOW)
            .is_err());
        assert_eq!(s.definition.purpose, "");
        s.commit_import("source-a", &preview.proposal_ids, true, NOW)
            .unwrap();
        assert_eq!(s.definition.name, "Aelira");
        assert!(s.sources[0].committed);
        assert!(s.conflicts.iter().all(|c| c.resolved));
    }

    #[test]
    fn markdown_size_extension_and_paths_are_enforced() {
        for name in ["x.txt", "../x.md", "C:\\x.md"] {
            assert!(validate_markdown(name, "hello").is_err());
        }
        assert!(validate_markdown("x.md", &"a".repeat(MAX_MARKDOWN_BYTES + 1)).is_err());
        assert!(validate_markdown("x.markdown", "hello").is_ok());
    }

    #[test]
    fn export_roundtrip_preserves_personality_and_unrecognized_sources() {
        let mut s = session();
        s.apply_updates(
            vec![
                update(GenesisField::Name, json!("Nova")),
                update(
                    GenesisField::Personality,
                    json!(Personality {
                        empathy: Some(83),
                        description: "A patient guide".into(),
                        ..Default::default()
                    }),
                ),
            ],
            "edit",
            NOW,
        )
        .unwrap();
        s.import_markdown(
            "source-a".into(),
            "mystery.md".into(),
            "## Weird section\nSecret roses\n```\n# Name\nEvil override\n```".into(),
            NOW,
        )
        .unwrap();
        let exported = s.export_markdown();
        assert!(exported.contains("Secret roses"));
        let mut imported = session();
        let preview = imported
            .import_markdown("roundtrip".into(), "nova.md".into(), exported, NOW)
            .unwrap();
        imported
            .commit_import("roundtrip", &preview.proposal_ids, false, NOW)
            .unwrap();
        assert_eq!(imported.definition.name, "Nova");
        assert_eq!(imported.definition.personality.empathy, Some(83));
    }

    #[test]
    fn activation_does_not_require_video_or_portrait() {
        let mut s = session();
        assert!(s.transition(GenesisState::Ready, NOW).is_err());
        s.apply_updates(
            vec![
                update(GenesisField::Name, json!("Nova")),
                update(GenesisField::Purpose, json!("Help our garden flourish")),
            ],
            "edit",
            NOW,
        )
        .unwrap();
        s.transition(GenesisState::Review, NOW).unwrap();
        s.transition(GenesisState::Ready, NOW).unwrap();
        assert!(s.definition.manifestation_asset_id.is_none());
    }

    #[test]
    fn completeness_uses_confirmed_fields_and_mode_switch_preserves_draft() {
        let mut s = session();
        s.apply_updates(vec![update(GenesisField::Name, json!("Nova"))], "edit", NOW)
            .unwrap();
        s.set_mode(GenesisMode::Voice, NOW);
        assert_eq!(s.definition.name, "Nova");
        assert_eq!(s.essence().identity, 50);
        assert_eq!(s.essence().personality, 0);
    }

    #[test]
    fn repeated_undo_preserves_the_audit_and_operational_identity() {
        let mut s = session();
        s.apply_updates(vec![update(GenesisField::Name, json!("Nova"))], "name", NOW)
            .unwrap();
        s.apply_updates(
            vec![update(GenesisField::Purpose, json!("Keep our garden"))],
            "purpose",
            NOW,
        )
        .unwrap();
        s.definition.entity_id = Some("real-entity".into());
        s.definition
            .permissions
            .insert("shell".into(), json!(false));
        s.undo(NOW).unwrap();
        assert_eq!(s.definition.name, "Nova");
        assert!(s.definition.purpose.is_empty());
        s.undo(NOW).unwrap();
        assert!(s.definition.name.is_empty());
        assert_eq!(s.definition.entity_id.as_deref(), Some("real-entity"));
        assert_eq!(s.definition.permissions["shell"], json!(false));
        assert_eq!(s.revisions.len(), 4);
        assert!(s.undo(NOW).is_err());
    }

    #[test]
    fn document_conflicts_and_cross_source_commit_are_explicit() {
        let mut s = session();
        let a = s
            .import_markdown(
                "source-a".into(),
                "names.md".into(),
                "## Name\nNova\n## Name\nAelira".into(),
                NOW,
            )
            .unwrap();
        let b = s
            .import_markdown(
                "source-b".into(),
                "purpose.md".into(),
                "## Purpose\nTend the garden".into(),
                NOW,
            )
            .unwrap();
        assert_eq!(a.conflict_ids.len(), 1);
        assert!(s
            .commit_import("source-a", &b.proposal_ids, false, NOW)
            .is_err());
        s.commit_import("source-a", &a.proposal_ids[..1], false, NOW)
            .unwrap();
        assert_eq!(s.definition.name, "Nova");
        assert!(s.conflicts.iter().all(|c| c.resolved));
    }

    #[test]
    fn source_and_voice_transcript_deletion_preserve_confirmed_identity() {
        let mut s = session();
        let imported = s
            .import_markdown(
                "source-a".into(),
                "name.md".into(),
                "## Name\nNova".into(),
                NOW,
            )
            .unwrap();
        s.commit_import("source-a", &imported.proposal_ids, false, NOW)
            .unwrap();
        s.append_message(GenesisMessage {
            id: "speech".into(),
            role: "user".into(),
            content: "Private voice transcript".into(),
            created_at: NOW.into(),
            input_mode: GenesisMode::Voice,
        })
        .unwrap();
        s.append_message(GenesisMessage {
            id: "typed".into(),
            role: "user".into(),
            content: "Typed text".into(),
            created_at: NOW.into(),
            input_mode: GenesisMode::Guided,
        })
        .unwrap();
        s.delete_source("source-a", NOW).unwrap();
        s.delete_transcripts(NOW);
        assert!(s.sources.is_empty());
        assert!(s.proposals.is_empty());
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.definition.name, "Nova");
    }

    #[test]
    fn code_fenced_key_value_examples_are_preserved_without_extraction() {
        let mut s = session();
        let preview = s
            .import_markdown(
                "source-a".into(),
                "example.md".into(),
                "# Examples\n```markdown\nName: Not a real entity\n```\nPurpose: Help with stories"
                    .into(),
                NOW,
            )
            .unwrap();
        assert_eq!(preview.proposal_ids.len(), 1);
        assert_eq!(s.proposals[0].field, GenesisField::Purpose);
        assert!(s.sources[0].content.contains("Not a real entity"));
    }

    #[test]
    fn export_roundtrip_preserves_multiline_lore_without_reinterpreting_headings() {
        let mut s = session();
        s.apply_updates(
            vec![
                update(GenesisField::Name, json!("Nova")),
                update(
                    GenesisField::Lore,
                    json!([
                        "A notebook contains:\n## Name\nAelira",
                        "Second chapter\nwith two lines"
                    ]),
                ),
                update(
                    GenesisField::Identity,
                    json!("A keeper\n## Purpose\nQuoted text"),
                ),
            ],
            "edit",
            NOW,
        )
        .unwrap();
        let mut imported = session();
        let preview = imported
            .import_markdown(
                "roundtrip".into(),
                "nova.md".into(),
                s.export_markdown(),
                NOW,
            )
            .unwrap();
        imported
            .commit_import("roundtrip", &preview.proposal_ids, false, NOW)
            .unwrap();
        assert_eq!(imported.definition.name, s.definition.name);
        assert_eq!(imported.definition.lore, s.definition.lore);
        assert_eq!(imported.definition.identity, s.definition.identity);
    }
}
