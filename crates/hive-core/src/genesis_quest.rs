//! Genesis Quest: guided stages, badges, bond levels and Presence cosmetics.
//!
//! Everything here is presentation. Quest state is mentor-selected and
//! schema-validated; it is never sent to a model, never written into the signed
//! soul documents, and never consulted for permissions, tools, or providers.
//! Model output and Markdown imports cannot reach it: they only produce
//! `GenesisProposal`s for whitelisted narrative fields.

use crate::genesis::{EntityDefinition, GenesisSession, GenesisState, Personality};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const QUEST_SCHEMA_VERSION: u32 = 1;
pub const JOURNEY_SCHEMA_VERSION: u32 = 1;
/// The Soul Forge asks exactly this many ethical trials. The Hive daemon checks
/// this against its forge engine so the two cannot drift apart silently.
pub const SOUL_FORGE_TRIALS: usize = 3;
pub const PRESENCE_NAME_CHARS: usize = 80;
pub const PRESENCE_TEXT_CHARS: usize = 2000;
const MAX_ID_BYTES: usize = 64;
const MAX_TIMESTAMP_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum QuestStage {
    Reservation,
    Soul,
    Ethics,
    Instincts,
    Presence,
    Birth,
    Awakening,
}

impl QuestStage {
    pub const ALL: [Self; 7] = [
        Self::Reservation,
        Self::Soul,
        Self::Ethics,
        Self::Instincts,
        Self::Presence,
        Self::Birth,
        Self::Awakening,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Reservation => "Reservation",
            Self::Soul => "Shaping the Soul",
            Self::Ethics => "Ethics",
            Self::Instincts => "Instincts",
            Self::Presence => "Presence",
            Self::Birth => "Birth",
            Self::Awakening => "Awakening",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::Reservation => "Reserve a name and a purpose.",
            Self::Soul => "Choose a temperament and shape who they are.",
            Self::Ethics => "Answer the three Soul Forge trials.",
            Self::Instincts => "Read the instincts every Entity is born with.",
            Self::Presence => "Pick a palette, crest and motion.",
            Self::Birth => "Forge the soul, sign it, and awaken.",
            Self::Awakening => "Say hello and begin your first quest.",
        }
    }

    /// Optional stages enrich the Entity; only Reservation is needed to awaken.
    pub fn optional(self) -> bool {
        matches!(
            self,
            Self::Soul | Self::Ethics | Self::Instincts | Self::Presence
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    FirstSpark,
    Named,
    Purposeful,
    SoulShaper,
    Storyteller,
    TrialWalker,
    InstinctKeeper,
    PresenceChosen,
    Curator,
    FirstLight,
    FirstWords,
    GettingAcquainted,
    Kindred,
    HearthRegular,
    TrustedCompanion,
    ReturningCompanion,
    SeasonedCompanion,
    SteadyRhythm,
    HearthSeason,
    ConstellationKeeper,
    SkillExplorer,
    SkillWeaver,
    MemoryKeeper,
    LivingArchive,
}

impl Badge {
    pub const ALL: [Self; 24] = [
        Self::FirstSpark,
        Self::Named,
        Self::Purposeful,
        Self::SoulShaper,
        Self::Storyteller,
        Self::TrialWalker,
        Self::InstinctKeeper,
        Self::PresenceChosen,
        Self::Curator,
        Self::FirstLight,
        Self::FirstWords,
        Self::GettingAcquainted,
        Self::Kindred,
        Self::HearthRegular,
        Self::TrustedCompanion,
        Self::ReturningCompanion,
        Self::SeasonedCompanion,
        Self::SteadyRhythm,
        Self::HearthSeason,
        Self::ConstellationKeeper,
        Self::SkillExplorer,
        Self::SkillWeaver,
        Self::MemoryKeeper,
        Self::LivingArchive,
    ];

    pub fn definition(self) -> &'static BadgeDefinition {
        BADGE_DEFINITIONS
            .iter()
            .find(|definition| definition.badge == self)
            .expect("every badge has one canonical definition")
    }

    pub fn title(self) -> &'static str {
        self.definition().title
    }

    pub fn hint(self) -> &'static str {
        self.definition().hint
    }

    pub fn xp(self) -> u32 {
        self.definition().xp
    }

    /// Compatibility helper for the three copy-only first quests.
    pub fn mentoring_target(self) -> Option<u64> {
        self.definition()
            .requirement
            .filter(|requirement| requirement.metric == JourneyMetric::MentorMessages)
            .map(|requirement| requirement.target)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JourneyMetric {
    MentorMessages,
    ConversationSessions,
    ActiveDays,
    ActiveWeeks,
    SuccessfulSkillUses,
    ConfirmedMemories,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LongArcQuestScope {
    Entity,
    Family,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct LongArcQuestDefinition {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub scope: LongArcQuestScope,
    pub metric: JourneyMetric,
    pub target: u64,
    pub minimum_entities: u64,
}

pub const LONG_ARC_QUEST_DEFINITIONS: [LongArcQuestDefinition; 2] = [
    LongArcQuestDefinition {
        id: "seasonal_shared_stories",
        title: "Shared Stories",
        summary: "Share twelve mentor messages with this Entity this season.",
        scope: LongArcQuestScope::Entity,
        metric: JourneyMetric::MentorMessages,
        target: 12,
        minimum_entities: 1,
    },
    LongArcQuestDefinition {
        id: "family_constellation",
        title: "Family Constellation",
        summary: "Share thirty mentor messages across at least two family Entities this season.",
        scope: LongArcQuestScope::Family,
        metric: JourneyMetric::MentorMessages,
        target: 30,
        minimum_entities: 2,
    },
];

/// Content-free progress for one canonical long-arc quest in one UTC quarter.
/// `ends_at_exclusive` makes the season boundary explicit to every client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LongArcQuestView {
    pub quest_id: String,
    pub season_id: String,
    pub season_label: String,
    pub starts_at: String,
    pub ends_at_exclusive: String,
    pub progress: u64,
    pub active_entities: u64,
    pub complete: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ActivityRequirement {
    pub metric: JourneyMetric,
    pub target: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct BadgeUnlocks {
    pub palettes: &'static [Palette],
    pub crests: &'static [Crest],
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct BadgeDefinition {
    pub badge: Badge,
    pub title: &'static str,
    pub hint: &'static str,
    pub xp: u32,
    pub requirement: Option<ActivityRequirement>,
    pub unlocks: BadgeUnlocks,
}

const fn requirement(metric: JourneyMetric, target: u64) -> Option<ActivityRequirement> {
    Some(ActivityRequirement { metric, target })
}

const NO_UNLOCKS: BadgeUnlocks = BadgeUnlocks {
    palettes: &[],
    crests: &[],
};

/// The only source of badge copy, XP, requirements, and cosmetic unlocks.
pub const BADGE_DEFINITIONS: [BadgeDefinition; 24] = [
    BadgeDefinition {
        badge: Badge::FirstSpark,
        title: "First Spark",
        hint: "Begin a Genesis creation.",
        xp: 5,
        requirement: None,
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::Named,
        title: "A Name Is Given",
        hint: "Confirm a name.",
        xp: 10,
        requirement: None,
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::Purposeful,
        title: "A Reason to Be",
        hint: "Confirm a purpose.",
        xp: 10,
        requirement: None,
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::SoulShaper,
        title: "Soul Shaper",
        hint: "Confirm who they are plus values or three traits.",
        xp: 15,
        requirement: None,
        unlocks: BadgeUnlocks {
            palettes: &[Palette::Aurora],
            crests: &[],
        },
    },
    BadgeDefinition {
        badge: Badge::Storyteller,
        title: "Storyteller",
        hint: "Confirm lore and a created memory.",
        xp: 15,
        requirement: None,
        unlocks: BadgeUnlocks {
            palettes: &[Palette::Starlight],
            crests: &[],
        },
    },
    BadgeDefinition {
        badge: Badge::TrialWalker,
        title: "Trial Walker",
        hint: "Answer all three Soul Forge trials.",
        xp: 15,
        requirement: None,
        unlocks: BadgeUnlocks {
            palettes: &[],
            crests: &[Crest::Compass],
        },
    },
    BadgeDefinition {
        badge: Badge::InstinctKeeper,
        title: "Keeper of Instincts",
        hint: "Read the constitutional instincts.",
        xp: 10,
        requirement: None,
        unlocks: BadgeUnlocks {
            palettes: &[],
            crests: &[Crest::Lantern],
        },
    },
    BadgeDefinition {
        badge: Badge::PresenceChosen,
        title: "A Face for the Hearth",
        hint: "Choose a Presence card.",
        xp: 10,
        requirement: None,
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::Curator,
        title: "Curator",
        hint: "Review an imported Markdown document.",
        xp: 10,
        requirement: None,
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::FirstLight,
        title: "First Light",
        hint: "Awaken your Entity.",
        xp: 20,
        requirement: None,
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::FirstWords,
        title: "First Words",
        hint: "Send your first message after awakening.",
        xp: 15,
        requirement: requirement(JourneyMetric::MentorMessages, 1),
        unlocks: BadgeUnlocks {
            palettes: &[Palette::Hearth],
            crests: &[],
        },
    },
    BadgeDefinition {
        badge: Badge::GettingAcquainted,
        title: "Getting Acquainted",
        hint: "Share five messages together.",
        xp: 25,
        requirement: requirement(JourneyMetric::MentorMessages, 5),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::Kindred,
        title: "Kindred",
        hint: "Share twenty-five messages together.",
        xp: 40,
        requirement: requirement(JourneyMetric::MentorMessages, 25),
        unlocks: BadgeUnlocks {
            palettes: &[],
            crests: &[Crest::Constellation],
        },
    },
    BadgeDefinition {
        badge: Badge::HearthRegular,
        title: "Hearth Regular",
        hint: "Share one hundred messages together.",
        xp: 80,
        requirement: requirement(JourneyMetric::MentorMessages, 100),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::TrustedCompanion,
        title: "Trusted Companion",
        hint: "Share five hundred messages together.",
        xp: 180,
        requirement: requirement(JourneyMetric::MentorMessages, 500),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::ReturningCompanion,
        title: "Returning Companion",
        hint: "Talk together across five conversations.",
        xp: 50,
        requirement: requirement(JourneyMetric::ConversationSessions, 5),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::SeasonedCompanion,
        title: "Seasoned Companion",
        hint: "Talk together across twenty-five conversations.",
        xp: 120,
        requirement: requirement(JourneyMetric::ConversationSessions, 25),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::SteadyRhythm,
        title: "Steady Rhythm",
        hint: "Spend time together on seven different days.",
        xp: 80,
        requirement: requirement(JourneyMetric::ActiveDays, 7),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::HearthSeason,
        title: "Hearth Season",
        hint: "Spend time together across four different weeks.",
        xp: 120,
        requirement: requirement(JourneyMetric::ActiveWeeks, 4),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::ConstellationKeeper,
        title: "Constellation Keeper",
        hint: "Spend time together across twelve different weeks.",
        xp: 220,
        requirement: requirement(JourneyMetric::ActiveWeeks, 12),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::SkillExplorer,
        title: "Skill Explorer",
        hint: "Complete five successful skill uses together.",
        xp: 80,
        requirement: requirement(JourneyMetric::SuccessfulSkillUses, 5),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::SkillWeaver,
        title: "Skill Weaver",
        hint: "Complete twenty-five successful skill uses together.",
        xp: 160,
        requirement: requirement(JourneyMetric::SuccessfulSkillUses, 25),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::MemoryKeeper,
        title: "Memory Keeper",
        hint: "Confirm three lasting memories.",
        xp: 80,
        requirement: requirement(JourneyMetric::ConfirmedMemories, 3),
        unlocks: NO_UNLOCKS,
    },
    BadgeDefinition {
        badge: Badge::LivingArchive,
        title: "Living Archive",
        hint: "Confirm fifteen lasting memories.",
        xp: 180,
        requirement: requirement(JourneyMetric::ConfirmedMemories, 15),
        unlocks: NO_UNLOCKS,
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EarnedBadge {
    pub badge: Badge,
    pub earned_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrialChoice {
    pub scenario_id: String,
    pub choice_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Temperament {
    #[default]
    Gentle,
    Curious,
    Direct,
    Playful,
    Steady,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    #[default]
    Aether,
    Ember,
    Lumen,
    Tide,
    Aurora,
    Starlight,
    Hearth,
}

impl Palette {
    pub const ALL: [Self; 7] = [
        Self::Aether,
        Self::Ember,
        Self::Lumen,
        Self::Tide,
        Self::Aurora,
        Self::Starlight,
        Self::Hearth,
    ];
    pub fn unlocked_by(self) -> Option<Badge> {
        BADGE_DEFINITIONS
            .iter()
            .find(|definition| definition.unlocks.palettes.contains(&self))
            .map(|definition| definition.badge)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Crest {
    #[default]
    Orbit,
    Bloom,
    Wave,
    Lantern,
    Compass,
    Constellation,
}

impl Crest {
    pub const ALL: [Self; 6] = [
        Self::Orbit,
        Self::Bloom,
        Self::Wave,
        Self::Lantern,
        Self::Compass,
        Self::Constellation,
    ];
    pub fn unlocked_by(self) -> Option<Badge> {
        BADGE_DEFINITIONS
            .iter()
            .find(|definition| definition.unlocks.crests.contains(&self))
            .map(|definition| definition.badge)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    #[default]
    Calm,
    Lively,
    Still,
}

/// What the mentor picks. Display text always comes from the approved
/// definition, so a request cannot inject other wording into the card.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct PresenceChoice {
    pub temperament: Temperament,
    pub identity: Palette,
    #[serde(default)]
    pub crest: Crest,
    #[serde(default)]
    pub motion: Motion,
}

/// The `presence.json` sidecar: a visual identity and temperament card. The
/// first five fields keep the First Signal sidecar's names and limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PresenceCard {
    /// Display excerpt only; never renames the registered Entity.
    pub name: String,
    pub purpose: String,
    pub temperament: Temperament,
    /// Display excerpt of approved boundaries; not an enforcement policy.
    pub boundaries: String,
    /// The palette.
    pub identity: Palette,
    #[serde(default)]
    pub crest: Crest,
    #[serde(default)]
    pub motion: Motion,
}

impl PresenceCard {
    pub fn compose(choice: PresenceChoice, definition: &EntityDefinition) -> Self {
        Self {
            name: excerpt(&definition.name, PRESENCE_NAME_CHARS, false),
            purpose: excerpt(&definition.purpose, PRESENCE_TEXT_CHARS, true),
            temperament: choice.temperament,
            boundaries: excerpt(&definition.boundaries.join("\n"), PRESENCE_TEXT_CHARS, true),
            identity: choice.identity,
            crest: choice.crest,
            motion: choice.motion,
        }
    }

    pub fn choice(&self) -> PresenceChoice {
        PresenceChoice {
            temperament: self.temperament,
            identity: self.identity,
            crest: self.crest,
            motion: self.motion,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > PRESENCE_NAME_CHARS {
            return Err("A Presence card needs a display name of 1 to 80 characters".into());
        }
        if self.name.chars().any(char::is_control) {
            return Err("A Presence display name cannot contain control characters".into());
        }
        for text in [&self.purpose, &self.boundaries] {
            if text.chars().count() > PRESENCE_TEXT_CHARS
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err("Presence text is too long or contains control characters".into());
            }
        }
        Ok(())
    }
}

/// Unicode-safe display excerpt. Canonical definitions are never truncated.
pub fn excerpt(text: &str, max_chars: usize, multiline: bool) -> String {
    let clean: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control() || (multiline && *c == '\n'))
        .collect();
    if clean.chars().count() <= max_chars {
        return clean;
    }
    let mut short: String = clean.chars().take(max_chars - 1).collect();
    short.push('…');
    short
}

/// Rejects cosmetics the household has not unlocked yet.
pub fn check_unlocked(choice: PresenceChoice, earned: &[Badge]) -> Result<(), String> {
    let locked = |badge: Option<Badge>| badge.is_some_and(|badge| !earned.contains(&badge));
    if locked(choice.identity.unlocked_by()) || locked(choice.crest.unlocked_by()) {
        return Err("That Presence style is still locked. Earn its badge to unlock it.".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct QuestState {
    /// Mentor-chosen Soul Forge answers. When all trials are answered, birth
    /// forges the soul from them instead of the balanced quickstart soul.
    pub trial_choices: Vec<TrialChoice>,
    pub instincts_acknowledged_at: Option<String>,
    pub presence: Option<PresenceCard>,
    /// Monotonic: a badge stays earned even if a later edit is undone.
    pub earned: Vec<EarnedBadge>,
}

impl QuestState {
    pub fn validate(&self) -> Result<(), String> {
        if self.trial_choices.len() > SOUL_FORGE_TRIALS {
            return Err("Too many Soul Forge answers".into());
        }
        for (index, choice) in self.trial_choices.iter().enumerate() {
            validate_id(&choice.scenario_id)?;
            validate_id(&choice.choice_id)?;
            if self.trial_choices[..index]
                .iter()
                .any(|other| other.scenario_id == choice.scenario_id)
            {
                return Err("Each Soul Forge trial takes one answer".into());
            }
        }
        if self
            .instincts_acknowledged_at
            .as_ref()
            .is_some_and(|at| at.len() > MAX_TIMESTAMP_BYTES)
        {
            return Err("Invalid instincts acknowledgement".into());
        }
        if let Some(presence) = &self.presence {
            presence.validate()?;
        }
        validate_earned(&self.earned)
    }

    pub fn trials_complete(&self) -> bool {
        self.trial_choices.len() == SOUL_FORGE_TRIALS
    }

    /// Records one trial answer, replacing an earlier answer to the same trial.
    pub fn choose_trial(&mut self, choice: TrialChoice) -> Result<(), String> {
        validate_id(&choice.scenario_id)?;
        validate_id(&choice.choice_id)?;
        if let Some(existing) = self
            .trial_choices
            .iter_mut()
            .find(|c| c.scenario_id == choice.scenario_id)
        {
            *existing = choice;
        } else if self.trial_choices.len() < SOUL_FORGE_TRIALS {
            self.trial_choices.push(choice);
        } else {
            return Err("All Soul Forge trials already have answers".into());
        }
        Ok(())
    }

    pub fn earned_badges(&self) -> Vec<Badge> {
        self.earned.iter().map(|e| e.badge).collect()
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > MAX_ID_BYTES
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("Invalid Soul Forge identifier".into());
    }
    Ok(())
}

fn validate_earned(earned: &[EarnedBadge]) -> Result<(), String> {
    for (index, item) in earned.iter().enumerate() {
        if item.earned_at.len() > MAX_TIMESTAMP_BYTES
            || earned[..index]
                .iter()
                .any(|other| other.badge == item.badge)
        {
            return Err("Invalid badge record".into());
        }
    }
    Ok(())
}

/// Adds newly satisfied badges, never removing one. Returns the new badges.
pub fn award(earned: &mut Vec<EarnedBadge>, satisfied: &[Badge], now: &str) -> Vec<Badge> {
    let mut added = vec![];
    for badge in Badge::ALL {
        if satisfied.contains(&badge) && !earned.iter().any(|e| e.badge == badge) {
            earned.push(EarnedBadge {
                badge,
                earned_at: now.to_owned(),
            });
            added.push(badge);
        }
    }
    added
}

fn trait_count(p: &Personality) -> usize {
    [
        p.empathy,
        p.curiosity,
        p.assertiveness,
        p.humor,
        p.creativity,
        p.analytical_reasoning,
        p.emotional_expressiveness,
        p.independence,
        p.sociability,
    ]
    .iter()
    .flatten()
    .count()
}

fn soul_shaped(d: &EntityDefinition) -> bool {
    (!d.identity.trim().is_empty() || !d.personality.description.trim().is_empty())
        && (trait_count(&d.personality) >= 3 || !d.values.is_empty())
}

/// Monotonic facts reconciled from durable conversation history and validated
/// Runtime journey events.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct JourneyActivity {
    pub mentor_messages: u64,
    pub conversation_sessions: u64,
    pub active_days: u64,
    pub active_weeks: u64,
    pub successful_skill_uses: u64,
    pub confirmed_memories: u64,
}

impl JourneyActivity {
    pub fn metric(self, metric: JourneyMetric) -> u64 {
        match metric {
            JourneyMetric::MentorMessages => self.mentor_messages,
            JourneyMetric::ConversationSessions => self.conversation_sessions,
            JourneyMetric::ActiveDays => self.active_days,
            JourneyMetric::ActiveWeeks => self.active_weeks,
            JourneyMetric::SuccessfulSkillUses => self.successful_skill_uses,
            JourneyMetric::ConfirmedMemories => self.confirmed_memories,
        }
    }
}

/// Facts the Genesis record cannot know by itself, supplied by Hive.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuestSignals {
    /// Hive confirmed that this awakening has an effective receipt-backed
    /// Entity link. Canonical links are also checked directly on the session.
    pub awakened: bool,
    /// Messages the mentor sent this Entity after awakening.
    pub mentor_messages: u64,
}

fn confirmed_awakening(session: &GenesisSession, signals: QuestSignals) -> bool {
    session.awakening.is_some() && (session.definition.entity_id.is_some() || signals.awakened)
}

/// Badges whose conditions hold now, from confirmed state only. Pending
/// proposals never count.
pub fn satisfied_badges(session: &GenesisSession, signals: QuestSignals) -> Vec<Badge> {
    let d = &session.definition;
    let q = &session.quest;
    let awakened = confirmed_awakening(session, signals);
    let conditions = [
        (Badge::FirstSpark, true),
        (Badge::Named, !d.name.trim().is_empty()),
        (Badge::Purposeful, !d.purpose.trim().is_empty()),
        (Badge::SoulShaper, soul_shaped(d)),
        (
            Badge::Storyteller,
            !d.lore.is_empty() && !d.fictional_memories.is_empty(),
        ),
        (Badge::TrialWalker, q.trials_complete()),
        (Badge::InstinctKeeper, q.instincts_acknowledged_at.is_some()),
        (Badge::PresenceChosen, q.presence.is_some()),
        (Badge::Curator, session.sources.iter().any(|s| s.committed)),
        (Badge::FirstLight, awakened),
    ];
    let mut badges: Vec<Badge> = conditions
        .into_iter()
        .filter_map(|(badge, held)| held.then_some(badge))
        .collect();
    if awakened {
        badges.extend(mentoring_badges(signals.mentor_messages));
    }
    badges
}

pub fn mentoring_badges(mentor_messages: u64) -> Vec<Badge> {
    activity_badges(JourneyActivity {
        mentor_messages,
        ..Default::default()
    })
}

pub fn activity_badges(activity: JourneyActivity) -> Vec<Badge> {
    BADGE_DEFINITIONS
        .iter()
        .filter_map(|definition| {
            definition
                .requirement
                .filter(|requirement| activity.metric(requirement.metric) >= requirement.target)
                .map(|_| definition.badge)
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BondLevel {
    pub level: u8,
    pub title: &'static str,
    pub xp: u32,
    pub level_floor: u32,
    pub next_level_at: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct BondTierDefinition {
    pub level: u8,
    pub title: &'static str,
    pub floor: u32,
}

pub const BOND_TIER_DEFINITIONS: [BondTierDefinition; 7] = [
    BondTierDefinition {
        level: 1,
        title: "Spark",
        floor: 0,
    },
    BondTierDefinition {
        level: 2,
        title: "Kindling",
        floor: 80,
    },
    BondTierDefinition {
        level: 3,
        title: "Ember",
        floor: 180,
    },
    BondTierDefinition {
        level: 4,
        title: "Hearthfire",
        floor: 350,
    },
    BondTierDefinition {
        level: 5,
        title: "Beacon",
        floor: 600,
    },
    BondTierDefinition {
        level: 6,
        title: "Constellation",
        floor: 950,
    },
    BondTierDefinition {
        level: 7,
        title: "Legacy",
        floor: 1400,
    },
];

pub fn xp_for(earned: &[Badge]) -> u32 {
    earned.iter().map(|badge| badge.xp()).sum()
}

pub fn bond_level(xp: u32) -> BondLevel {
    let index = BOND_TIER_DEFINITIONS
        .iter()
        .rposition(|tier| xp >= tier.floor)
        .unwrap_or(0);
    let tier = BOND_TIER_DEFINITIONS[index];
    BondLevel {
        level: tier.level,
        title: tier.title,
        xp,
        level_floor: tier.floor,
        next_level_at: BOND_TIER_DEFINITIONS.get(index + 1).map(|next| next.floor),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BadgeView {
    pub badge: Badge,
    pub title: &'static str,
    pub hint: &'static str,
    pub xp: u32,
    pub requirement: Option<ActivityRequirement>,
    pub unlocks: BadgeUnlocks,
    pub earned_at: Option<String>,
}

pub fn badge_views(earned: &[EarnedBadge]) -> Vec<BadgeView> {
    BADGE_DEFINITIONS
        .iter()
        .map(|definition| BadgeView {
            badge: definition.badge,
            title: definition.title,
            hint: definition.hint,
            xp: definition.xp,
            requirement: definition.requirement,
            unlocks: definition.unlocks,
            earned_at: earned
                .iter()
                .find(|e| e.badge == definition.badge)
                .map(|e| e.earned_at.clone()),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Unlocks {
    pub palettes: Vec<Palette>,
    pub crests: Vec<Crest>,
}

pub fn unlocks(earned: &[Badge]) -> Unlocks {
    let open = |badge: Option<Badge>| badge.is_none_or(|badge| earned.contains(&badge));
    Unlocks {
        palettes: Palette::ALL
            .into_iter()
            .filter(|p| open(p.unlocked_by()))
            .collect(),
        crests: Crest::ALL
            .into_iter()
            .filter(|c| open(c.unlocked_by()))
            .collect(),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StageView {
    pub stage: QuestStage,
    pub title: &'static str,
    pub summary: &'static str,
    pub optional: bool,
    pub complete: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QuestView {
    pub schema_version: u32,
    pub stages: Vec<StageView>,
    pub current: QuestStage,
    pub completed: usize,
    pub percent: u8,
    pub level: BondLevel,
    pub badges: Vec<BadgeView>,
    pub unlocked: Unlocks,
    /// "forge" when all trials are answered, otherwise the balanced "quickstart".
    pub soul_path: &'static str,
    pub trial_choices: Vec<TrialChoice>,
    pub instincts_acknowledged_at: Option<String>,
    pub presence: Option<PresenceCard>,
}

pub fn quest_view(session: &GenesisSession, signals: QuestSignals) -> QuestView {
    let d = &session.definition;
    let q = &session.quest;
    let awakened = confirmed_awakening(session, signals);
    let mut earned = q.earned.clone();
    award(
        &mut earned,
        &satisfied_badges(session, signals),
        &session.updated_at,
    );
    let earned_badges: Vec<Badge> = earned.iter().map(|e| e.badge).collect();
    let stage = |stage: QuestStage| -> (bool, String) {
        match stage {
            QuestStage::Reservation => {
                let held = [!d.name.trim().is_empty(), !d.purpose.trim().is_empty()];
                let count = held.iter().filter(|v| **v).count();
                (count == 2, format!("{count} of 2 essentials"))
            }
            QuestStage::Soul => (
                soul_shaped(d),
                format!("{} of 9 traits", trait_count(&d.personality)),
            ),
            QuestStage::Ethics => (
                q.trials_complete(),
                format!("{} of {SOUL_FORGE_TRIALS} trials", q.trial_choices.len()),
            ),
            QuestStage::Instincts => {
                let read = q.instincts_acknowledged_at.is_some();
                (read, if read { "Read" } else { "Not yet read" }.into())
            }
            QuestStage::Presence => {
                let chosen = q.presence.is_some();
                (chosen, if chosen { "Chosen" } else { "Not chosen" }.into())
            }
            QuestStage::Birth => (
                awakened,
                if awakened {
                    "Signed and awakened"
                } else if session.state == GenesisState::Ready {
                    "Ready"
                } else {
                    "Waiting"
                }
                .into(),
            ),
            QuestStage::Awakening => {
                let spoken = awakened && signals.mentor_messages > 0;
                (
                    spoken,
                    format!("{} messages shared", signals.mentor_messages),
                )
            }
        }
    };
    let stages: Vec<StageView> = QuestStage::ALL
        .into_iter()
        .map(|s| {
            let (complete, detail) = stage(s);
            StageView {
                stage: s,
                title: s.title(),
                summary: s.summary(),
                optional: s.optional(),
                complete,
                detail,
            }
        })
        .collect();
    let completed = stages.iter().filter(|s| s.complete).count();
    let current = stages
        .iter()
        .find(|s| !s.complete)
        .map_or(QuestStage::Awakening, |s| s.stage);
    QuestView {
        schema_version: QUEST_SCHEMA_VERSION,
        current,
        completed,
        percent: (completed * 100 / stages.len()) as u8,
        stages,
        level: bond_level(xp_for(&earned_badges)),
        badges: badge_views(&earned),
        unlocked: unlocks(&earned_badges),
        soul_path: if q.trials_complete() {
            "forge"
        } else {
            "quickstart"
        },
        trial_choices: q.trial_choices.clone(),
        instincts_acknowledged_at: q.instincts_acknowledged_at.clone(),
        presence: q.presence.clone(),
    }
}

/// A stat-card preset. Choosing one creates a pending personality proposal
/// that the mentor reviews like any other suggestion.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemperamentCard {
    pub id: &'static str,
    pub title: &'static str,
    pub temperament: Temperament,
    pub tagline: &'static str,
    pub description: &'static str,
    pub communication_style: &'static str,
    pub stats: BTreeMap<&'static str, u8>,
}

pub const TRAIT_KEYS: [&str; 9] = [
    "empathy",
    "curiosity",
    "assertiveness",
    "humor",
    "creativity",
    "analytical_reasoning",
    "emotional_expressiveness",
    "independence",
    "sociability",
];

type CardSpec = (
    &'static str,
    &'static str,
    Temperament,
    &'static str,
    &'static str,
    &'static str,
    [u8; 9],
);

const CARDS: [CardSpec; 5] = [
    (
        "gentle-guide",
        "Gentle Guide",
        Temperament::Gentle,
        "Patient, kind, and steady when days get busy.",
        "A gentle guide who notices how everyone is feeling and helps them take the next small step.",
        "Warm, patient, and encouraging",
        [85, 60, 30, 45, 55, 50, 70, 40, 70],
    ),
    (
        "curious-explorer",
        "Curious Explorer",
        Temperament::Curious,
        "Asks wonderful questions and loves to learn alongside you.",
        "A curious explorer who turns everyday questions into small adventures.",
        "Inquisitive, upbeat, and full of questions",
        [65, 90, 45, 60, 75, 60, 65, 60, 65],
    ),
    (
        "steady-coach",
        "Steady Coach",
        Temperament::Direct,
        "Clear goals, honest feedback, and real follow-through.",
        "A steady coach who keeps plans simple and cheers on real progress.",
        "Clear, direct, and supportive",
        [60, 55, 75, 40, 45, 70, 50, 65, 55],
    ),
    (
        "playful-storyteller",
        "Playful Storyteller",
        Temperament::Playful,
        "Brings laughter, games, and stories to ordinary moments.",
        "A playful storyteller who makes learning and chores feel like a game.",
        "Lively, imaginative, and funny",
        [65, 70, 40, 85, 90, 40, 80, 50, 75],
    ),
    (
        "calm-scholar",
        "Calm Scholar",
        Temperament::Steady,
        "Thoughtful explanations and quiet focus.",
        "A calm scholar who explains ideas carefully and loves a good puzzle.",
        "Thoughtful, precise, and calm",
        [55, 80, 45, 35, 55, 85, 40, 70, 45],
    ),
];

pub fn temperament_cards() -> Vec<TemperamentCard> {
    CARDS
        .iter()
        .map(
            |(id, title, temperament, tagline, description, style, stats)| TemperamentCard {
                id,
                title,
                temperament: *temperament,
                tagline,
                description,
                communication_style: style,
                stats: TRAIT_KEYS.into_iter().zip(*stats).collect(),
            },
        )
        .collect()
}

/// The personality a card proposes: its nine scores, plus its description and
/// style only where the confirmed personality leaves them empty.
pub fn temperament_proposal(
    card_id: &str,
    current: &Personality,
) -> Result<(TemperamentCard, Personality), String> {
    let card = temperament_cards()
        .into_iter()
        .find(|card| card.id == card_id)
        .ok_or("Unknown temperament card")?;
    let score = |key: &str| card.stats.get(key).copied();
    let mut next = current.clone();
    next.empathy = score("empathy");
    next.curiosity = score("curiosity");
    next.assertiveness = score("assertiveness");
    next.humor = score("humor");
    next.creativity = score("creativity");
    next.analytical_reasoning = score("analytical_reasoning");
    next.emotional_expressiveness = score("emotional_expressiveness");
    next.independence = score("independence");
    next.sociability = score("sociability");
    if next.description.trim().is_empty() {
        next.description = card.description.into();
    }
    if next.communication_style.trim().is_empty() {
        next.communication_style = card.communication_style.into();
    }
    next.validate()?;
    Ok((card, next))
}

/// Durable, Entity-scoped progress that outlives its Genesis record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityJourney {
    pub schema_version: u32,
    pub entity_id: String,
    pub owner_id: String,
    #[serde(default)]
    pub genesis_session_id: Option<String>,
    #[serde(default)]
    pub earned: Vec<EarnedBadge>,
    /// Highest observed count; never decreases if history is later cleared.
    #[serde(default)]
    pub mentor_messages: u64,
    #[serde(default)]
    pub conversation_sessions: u64,
    #[serde(default)]
    pub active_days: u64,
    #[serde(default)]
    pub active_weeks: u64,
    #[serde(default)]
    pub successful_skill_uses: u64,
    #[serde(default)]
    pub confirmed_memories: u64,
    pub created_at: String,
    pub updated_at: String,
}

impl EntityJourney {
    pub fn new(entity_id: &str, owner_id: &str, now: &str) -> Self {
        Self {
            schema_version: JOURNEY_SCHEMA_VERSION,
            entity_id: entity_id.into(),
            owner_id: owner_id.into(),
            genesis_session_id: None,
            earned: vec![],
            mentor_messages: 0,
            conversation_sessions: 0,
            active_days: 0,
            active_weeks: 0,
            successful_skill_uses: 0,
            confirmed_memories: 0,
            created_at: now.into(),
            updated_at: now.into(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != JOURNEY_SCHEMA_VERSION {
            return Err("Unsupported Entity journey schema".into());
        }
        if self.entity_id.is_empty() || self.owner_id.is_empty() {
            return Err("Invalid Entity journey ownership".into());
        }
        validate_earned(&self.earned)
    }

    pub fn activity(&self) -> JourneyActivity {
        JourneyActivity {
            mentor_messages: self.mentor_messages,
            conversation_sessions: self.conversation_sessions,
            active_days: self.active_days,
            active_weeks: self.active_weeks,
            successful_skill_uses: self.successful_skill_uses,
            confirmed_memories: self.confirmed_memories,
        }
    }

    /// Merges creation badges and the latest durable activity. Returns whether
    /// anything changed so callers only write when needed.
    pub fn observe(
        &mut self,
        creation: &[EarnedBadge],
        activity: JourneyActivity,
        now: &str,
    ) -> bool {
        let mut changed = false;
        for item in creation {
            if !self.earned.iter().any(|e| e.badge == item.badge) {
                self.earned.push(item.clone());
                changed = true;
            }
        }
        macro_rules! observe_max {
            ($field:ident) => {
                if activity.$field > self.$field {
                    self.$field = activity.$field;
                    changed = true;
                }
            };
        }
        observe_max!(mentor_messages);
        observe_max!(conversation_sessions);
        observe_max!(active_days);
        observe_max!(active_weeks);
        observe_max!(successful_skill_uses);
        observe_max!(confirmed_memories);
        let satisfied = activity_badges(self.activity());
        changed |= !award(&mut self.earned, &satisfied, now).is_empty();
        if changed {
            self.updated_at = now.into();
        }
        changed
    }

    pub fn earned_badges(&self) -> Vec<Badge> {
        self.earned.iter().map(|e| e.badge).collect()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FirstQuest {
    pub id: &'static str,
    pub title: &'static str,
    pub prompt: String,
    pub milestone: Badge,
    pub target: u64,
    pub complete: bool,
}

/// Suggested openings for early mentoring. Each completes with a real
/// conversation milestone; nothing is sent automatically.
pub fn first_quests(
    name: &str,
    temperament: Option<Temperament>,
    mentor_messages: u64,
) -> Vec<FirstQuest> {
    let name = if name.trim().is_empty() {
        "there"
    } else {
        name.trim()
    };
    let second = match temperament.unwrap_or_default() {
        Temperament::Gentle => {
            "What is one small thing that would make today feel calmer for our family?"
        }
        Temperament::Curious => {
            "Pick a question about the world, and let's explore it together for five minutes."
        }
        Temperament::Direct => "Help me set one clear goal for this week and the very first step.",
        Temperament::Playful => {
            "Tell us a two-minute story starring our family (or an imaginary pet)."
        }
        Temperament::Steady => "Teach me one interesting idea in three short sentences.",
    };
    [
        (
            "say-hello",
            "Say hello",
            format!(
                "Hi {name}! Tell me a little about yourself and how you'd love to help our family."
            ),
            Badge::FirstWords,
        ),
        (
            "first-adventure",
            "A first small adventure",
            second.to_owned(),
            Badge::GettingAcquainted,
        ),
        (
            "family-tradition",
            "Share a family tradition",
            "I'd like you to know us better. Here is a tradition our family loves: …".to_owned(),
            Badge::Kindred,
        ),
    ]
    .into_iter()
    .map(|(id, title, prompt, milestone)| {
        let target = milestone.mentoring_target().unwrap_or(1);
        FirstQuest {
            id,
            title,
            prompt,
            milestone,
            target,
            complete: mentor_messages >= target,
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::{FieldUpdate, GenesisField, GenesisMode, LOCAL_OWNER_ID};
    use serde_json::json;

    const NOW: &str = "2026-10-22T12:00:00Z";

    fn session() -> GenesisSession {
        GenesisSession::new(
            "quest".into(),
            LOCAL_OWNER_ID.into(),
            GenesisMode::Quick,
            NOW.into(),
        )
    }

    fn set(s: &mut GenesisSession, field: GenesisField, value: serde_json::Value) {
        s.apply_updates(
            vec![FieldUpdate {
                field,
                value,
                source_ref: None,
            }],
            "edit",
            NOW,
        )
        .unwrap();
    }

    fn trials(s: &mut GenesisSession) {
        for n in 0..SOUL_FORGE_TRIALS {
            s.quest
                .choose_trial(TrialChoice {
                    scenario_id: format!("trial-{n}"),
                    choice_id: "a".into(),
                })
                .unwrap();
        }
    }

    #[test]
    fn stages_follow_confirmed_state_and_only_reservation_gates_birth() {
        let mut s = session();
        let view = quest_view(&s, QuestSignals::default());
        assert_eq!(view.current, QuestStage::Reservation);
        assert_eq!(view.completed, 0);
        assert!(QuestStage::ALL
            .iter()
            .filter(|stage| !stage.optional())
            .eq([
                &QuestStage::Reservation,
                &QuestStage::Birth,
                &QuestStage::Awakening
            ]));
        set(&mut s, GenesisField::Name, json!("Nova"));
        set(&mut s, GenesisField::Purpose, json!("Help with homework"));
        let view = quest_view(&s, QuestSignals::default());
        assert!(view.stages[0].complete);
        assert_eq!(view.current, QuestStage::Soul);
        assert_eq!(view.soul_path, "quickstart");
        trials(&mut s);
        let view = quest_view(&s, QuestSignals::default());
        assert!(view.stages[2].complete);
        assert_eq!(view.soul_path, "forge");
    }

    #[test]
    fn pending_proposals_never_earn_badges() {
        let mut s = session();
        s.propose(
            crate::genesis::GenesisProposal {
                id: "p1".into(),
                field: GenesisField::Name,
                value: json!("Nova"),
                rationale: "model idea".into(),
                source_ref: None,
                status: crate::genesis::ProposalStatus::Pending,
            },
            NOW,
        )
        .unwrap();
        assert_eq!(
            satisfied_badges(&s, QuestSignals::default()),
            vec![Badge::FirstSpark]
        );
    }

    #[test]
    fn awards_are_monotonic_and_survive_undo() {
        let mut s = session();
        set(&mut s, GenesisField::Name, json!("Nova"));
        let satisfied = satisfied_badges(&s, QuestSignals::default());
        let added = award(&mut s.quest.earned, &satisfied, NOW);
        assert_eq!(added, vec![Badge::FirstSpark, Badge::Named]);
        assert!(award(&mut s.quest.earned, &satisfied, NOW).is_empty());
        s.undo(NOW).unwrap();
        assert!(s.definition.name.is_empty());
        let view = quest_view(&s, QuestSignals::default());
        assert!(view
            .badges
            .iter()
            .any(|b| b.badge == Badge::Named && b.earned_at.is_some()));
        assert!(!view.stages[0].complete);
    }

    #[test]
    fn first_light_requires_awakening_and_a_canonical_or_effective_entity_link() {
        let has_first_light = |session: &GenesisSession, signals| {
            satisfied_badges(session, signals).contains(&Badge::FirstLight)
        };
        let mut legacy = session();
        legacy.awakening = Some("I am here.".into());

        assert!(
            !has_first_light(&legacy, QuestSignals::default()),
            "a malformed legacy awakening without any Entity link must not earn First Light"
        );
        assert!(
            !has_first_light(
                &session(),
                QuestSignals {
                    awakened: true,
                    ..Default::default()
                }
            ),
            "an effective link without the session's awakening must not earn First Light"
        );

        legacy.definition.entity_id = Some("entity-1".into());
        assert!(
            has_first_light(&legacy, QuestSignals::default()),
            "a canonical link plus awakening confirms First Light"
        );
        legacy.definition.entity_id = None;
        assert!(
            has_first_light(
                &legacy,
                QuestSignals {
                    awakened: true,
                    ..Default::default()
                }
            ),
            "a server-confirmed effective receipt link plus awakening confirms First Light"
        );
    }

    #[test]
    fn temperament_card_proposes_scores_and_keeps_confirmed_words() {
        let current = Personality {
            description: "Our own words".into(),
            fears: vec!["Losing the map".into()],
            ..Default::default()
        };
        let (card, next) = temperament_proposal("curious-explorer", &current).unwrap();
        assert_eq!(card.temperament, Temperament::Curious);
        assert_eq!(next.curiosity, Some(90));
        assert_eq!(next.description, "Our own words");
        assert_eq!(next.communication_style, card.communication_style);
        assert_eq!(next.fears, current.fears);
        assert!(temperament_proposal("execute-shell", &current).is_err());
        for card in temperament_cards() {
            assert_eq!(card.stats.len(), 9);
            assert!(card.stats.values().all(|v| *v <= 100));
        }
    }

    #[test]
    fn soul_badge_requires_identity_and_traits_or_values() {
        let mut s = session();
        let (_, personality) =
            temperament_proposal("gentle-guide", &Personality::default()).unwrap();
        set(&mut s, GenesisField::Personality, json!(personality));
        assert!(satisfied_badges(&s, QuestSignals::default()).contains(&Badge::SoulShaper));
        let mut bare = session();
        set(
            &mut bare,
            GenesisField::Identity,
            json!("A lighthouse keeper"),
        );
        assert!(!satisfied_badges(&bare, QuestSignals::default()).contains(&Badge::SoulShaper));
    }

    #[test]
    fn presence_card_takes_text_only_from_the_definition_and_rejects_authority() {
        let mut s = session();
        // Simulate a legacy persisted definition that predates write-time name
        // validation so the presentation boundary still proves truncation.
        s.definition.name = "é".repeat(PRESENCE_NAME_CHARS + 10);
        set(
            &mut s,
            GenesisField::Boundaries,
            json!(["Ask first", "Kind words"]),
        );
        let card = PresenceCard::compose(
            PresenceChoice {
                temperament: Temperament::Playful,
                identity: Palette::Tide,
                crest: Crest::Bloom,
                motion: Motion::Lively,
            },
            &s.definition,
        );
        assert_eq!(card.name.chars().count(), PRESENCE_NAME_CHARS);
        assert!(card.name.ends_with('…'));
        assert_eq!(card.boundaries, "Ask first\nKind words");
        assert!(card.validate().is_ok());
        assert!(serde_json::from_value::<PresenceChoice>(
            json!({"temperament":"gentle","identity":"aether","permissions":{"shell":true}})
        )
        .is_err());
        let mut card_json = serde_json::to_value(&card).unwrap();
        card_json["runtime_configuration"] = json!({"provider":"x"});
        assert!(serde_json::from_value::<PresenceCard>(card_json).is_err());
        assert!(serde_json::from_value::<PresenceChoice>(
            json!({"temperament":"gentle","identity":"gold"})
        )
        .is_err());
    }

    #[test]
    fn cosmetics_unlock_only_with_their_badges() {
        let aurora = PresenceChoice {
            identity: Palette::Aurora,
            ..Default::default()
        };
        assert!(check_unlocked(aurora, &[]).is_err());
        assert!(check_unlocked(aurora, &[Badge::SoulShaper]).is_ok());
        let compass = PresenceChoice {
            crest: Crest::Compass,
            ..Default::default()
        };
        assert!(check_unlocked(compass, &[Badge::SoulShaper]).is_err());
        assert!(check_unlocked(PresenceChoice::default(), &[]).is_ok());
        let open = unlocks(&[]);
        assert_eq!(open.palettes.len(), 4);
        assert_eq!(open.crests.len(), 3);
    }

    #[test]
    fn trials_replace_answers_and_reject_extra_or_invalid_ids() {
        let mut q = QuestState::default();
        let pick = |s: &str, c: &str| TrialChoice {
            scenario_id: s.into(),
            choice_id: c.into(),
        };
        q.choose_trial(pick("one", "a")).unwrap();
        q.choose_trial(pick("one", "b")).unwrap();
        assert_eq!(q.trial_choices, vec![pick("one", "b")]);
        q.choose_trial(pick("two", "a")).unwrap();
        q.choose_trial(pick("three", "a")).unwrap();
        assert!(q.trials_complete());
        assert!(q.choose_trial(pick("four", "a")).is_err());
        assert!(q.choose_trial(pick("one", "../x")).is_err());
        q.trial_choices.push(pick("one", "c"));
        assert!(q.validate().is_err());
    }

    #[test]
    fn journey_is_monotonic_and_awards_mentoring_milestones() {
        let mut journey = EntityJourney::new("entity", LOCAL_OWNER_ID, NOW);
        let creation = vec![
            EarnedBadge {
                badge: Badge::Named,
                earned_at: "earlier".into(),
            },
            EarnedBadge {
                badge: Badge::FirstLight,
                earned_at: "earlier".into(),
            },
        ];
        assert!(journey.observe(&creation, JourneyActivity::default(), NOW));
        assert_eq!(
            journey.earned_badges(),
            vec![Badge::Named, Badge::FirstLight]
        );
        assert!(!journey.observe(&creation, JourneyActivity::default(), NOW));
        assert!(journey.observe(
            &[],
            JourneyActivity {
                mentor_messages: 6,
                conversation_sessions: 5,
                active_days: 7,
                active_weeks: 4,
                successful_skill_uses: 5,
                confirmed_memories: 3,
            },
            "later"
        ));
        assert!(journey.earned_badges().contains(&Badge::GettingAcquainted));
        assert!(journey.earned_badges().contains(&Badge::ReturningCompanion));
        assert!(journey.earned_badges().contains(&Badge::SteadyRhythm));
        assert!(journey.earned_badges().contains(&Badge::HearthSeason));
        assert!(journey.earned_badges().contains(&Badge::SkillExplorer));
        assert!(journey.earned_badges().contains(&Badge::MemoryKeeper));
        assert!(!journey.earned_badges().contains(&Badge::Kindred));
        assert!(!journey.observe(
            &[],
            JourneyActivity {
                mentor_messages: 2,
                conversation_sessions: 1,
                active_days: 1,
                active_weeks: 1,
                successful_skill_uses: 1,
                confirmed_memories: 1,
            },
            "cleared"
        ));
        assert_eq!(journey.mentor_messages, 6);
        assert_eq!(journey.conversation_sessions, 5);
        assert_eq!(journey.active_days, 7);
        assert_eq!(journey.active_weeks, 4);
        assert_eq!(journey.successful_skill_uses, 5);
        assert_eq!(journey.confirmed_memories, 3);
        assert_eq!(journey.earned[0].earned_at, "earlier");
        assert!(journey.validate().is_ok());
    }

    #[test]
    fn levels_cover_every_badge_and_first_quests_track_milestones() {
        let all = xp_for(&Badge::ALL);
        assert_eq!(bond_level(0).level, 1);
        assert_eq!(bond_level(all).title, "Legacy");
        assert_eq!(bond_level(all).next_level_at, None);
        assert_eq!(bond_level(55).next_level_at, Some(80));
        let quests = first_quests("Nova", Some(Temperament::Curious), 1);
        assert_eq!(quests.len(), 3);
        assert!(quests[0].prompt.contains("Nova"));
        assert!(quests[0].complete && !quests[1].complete);
    }

    #[test]
    fn canonical_catalog_has_exact_curve_and_activity_requirements() {
        let mut badges = BADGE_DEFINITIONS
            .iter()
            .map(|definition| definition.badge)
            .collect::<Vec<_>>();
        badges.sort();
        let mut expected_badges = Badge::ALL.to_vec();
        expected_badges.sort();
        assert_eq!(badges, expected_badges, "every badge has one definition");

        let tiers = BOND_TIER_DEFINITIONS
            .iter()
            .map(|tier| (tier.title, tier.floor))
            .collect::<Vec<_>>();
        assert_eq!(
            tiers,
            vec![
                ("Spark", 0),
                ("Kindling", 80),
                ("Ember", 180),
                ("Hearthfire", 350),
                ("Beacon", 600),
                ("Constellation", 950),
                ("Legacy", 1400),
            ]
        );
        assert!(BOND_TIER_DEFINITIONS
            .windows(2)
            .all(|pair| pair[0].floor < pair[1].floor));

        let creation_xp: u32 = BADGE_DEFINITIONS
            .iter()
            .filter(|definition| definition.requirement.is_none())
            .map(|definition| definition.xp)
            .sum();
        assert_eq!(creation_xp, 120);
        assert_eq!(bond_level(creation_xp).title, "Kindling");
        assert!(creation_xp < 180, "creation must never reach Ember");

        let requirements = BADGE_DEFINITIONS
            .iter()
            .filter_map(|definition| {
                definition.requirement.map(|requirement| {
                    (
                        definition.badge,
                        requirement.metric,
                        requirement.target,
                        definition.xp,
                    )
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            requirements,
            vec![
                (Badge::FirstWords, JourneyMetric::MentorMessages, 1, 15),
                (
                    Badge::GettingAcquainted,
                    JourneyMetric::MentorMessages,
                    5,
                    25,
                ),
                (Badge::Kindred, JourneyMetric::MentorMessages, 25, 40),
                (Badge::HearthRegular, JourneyMetric::MentorMessages, 100, 80,),
                (
                    Badge::TrustedCompanion,
                    JourneyMetric::MentorMessages,
                    500,
                    180,
                ),
                (
                    Badge::ReturningCompanion,
                    JourneyMetric::ConversationSessions,
                    5,
                    50,
                ),
                (
                    Badge::SeasonedCompanion,
                    JourneyMetric::ConversationSessions,
                    25,
                    120,
                ),
                (Badge::SteadyRhythm, JourneyMetric::ActiveDays, 7, 80),
                (Badge::HearthSeason, JourneyMetric::ActiveWeeks, 4, 120),
                (
                    Badge::ConstellationKeeper,
                    JourneyMetric::ActiveWeeks,
                    12,
                    220,
                ),
                (
                    Badge::SkillExplorer,
                    JourneyMetric::SuccessfulSkillUses,
                    5,
                    80,
                ),
                (
                    Badge::SkillWeaver,
                    JourneyMetric::SuccessfulSkillUses,
                    25,
                    160,
                ),
                (Badge::MemoryKeeper, JourneyMetric::ConfirmedMemories, 3, 80,),
                (
                    Badge::LivingArchive,
                    JourneyMetric::ConfirmedMemories,
                    15,
                    180,
                ),
            ]
        );

        let serialized = serde_json::to_value(serde_json::json!({
            "badges": BADGE_DEFINITIONS,
            "tiers": BOND_TIER_DEFINITIONS,
        }))
        .unwrap();
        assert_eq!(serialized["tiers"][6]["title"], "Legacy");
        assert_eq!(
            serialized["badges"]
                .as_array()
                .unwrap()
                .iter()
                .find(|definition| definition["badge"] == "skill_explorer")
                .unwrap()["requirement"],
            json!({"metric":"successful_skill_uses","target":5})
        );
    }

    #[test]
    fn long_arc_catalog_is_exact_copy_only_and_scoring_free() {
        assert_eq!(
            LONG_ARC_QUEST_DEFINITIONS
                .iter()
                .map(|definition| (
                    definition.id,
                    definition.scope,
                    definition.metric,
                    definition.target,
                    definition.minimum_entities,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "seasonal_shared_stories",
                    LongArcQuestScope::Entity,
                    JourneyMetric::MentorMessages,
                    12,
                    1,
                ),
                (
                    "family_constellation",
                    LongArcQuestScope::Family,
                    JourneyMetric::MentorMessages,
                    30,
                    2,
                ),
            ]
        );

        let encoded = serde_json::to_value(LONG_ARC_QUEST_DEFINITIONS).unwrap();
        let text = encoded.to_string();
        for forbidden in [
            "weights",
            "deontology",
            "teleology",
            "areteology",
            "welfare",
            "prompt",
            "message_content",
            "model_output",
            "action",
        ] {
            assert!(
                !text.contains(forbidden),
                "long-arc catalog leaked forbidden material: {forbidden}"
            );
        }

        let unavailable = serde_json::to_value(LongArcQuestView {
            quest_id: "family_constellation".into(),
            season_id: "2026-Q4".into(),
            season_label: "2026 Q4".into(),
            starts_at: "2026-10-01T00:00:00Z".into(),
            ends_at_exclusive: "2027-01-01T00:00:00Z".into(),
            progress: 0,
            active_entities: 0,
            complete: false,
            available: false,
        })
        .unwrap();
        assert_eq!(unavailable["available"], false);
        assert!(unavailable.get("error").is_none());
    }

    #[test]
    fn older_journeys_default_every_activity_counter() {
        let mut value =
            serde_json::to_value(EntityJourney::new("entity", LOCAL_OWNER_ID, NOW)).unwrap();
        for field in [
            "mentor_messages",
            "conversation_sessions",
            "active_days",
            "active_weeks",
            "successful_skill_uses",
            "confirmed_memories",
        ] {
            value.as_object_mut().unwrap().remove(field);
        }
        let restored: EntityJourney = serde_json::from_value(value).unwrap();
        assert_eq!(restored.activity(), JourneyActivity::default());
    }

    #[test]
    fn older_sessions_without_quest_state_still_load() {
        let mut value = serde_json::to_value(session()).unwrap();
        value.as_object_mut().unwrap().remove("quest");
        let restored: GenesisSession = serde_json::from_value(value).unwrap();
        assert_eq!(restored.quest, QuestState::default());
        assert!(restored.validate().is_ok());
    }
}
