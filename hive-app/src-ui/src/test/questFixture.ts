import type { EntityJourney, LongArcQuestView, QuestCatalog, QuestStageId, QuestView } from "../lib/questClient";

const STAGES: [QuestStageId, string, boolean][] = [["reservation", "Reservation", false], ["soul", "Shaping the Soul", true], ["ethics", "Ethics", true], ["instincts", "Instincts", true], ["presence", "Presence", true], ["birth", "Birth", false], ["awakening", "Awakening", false]];
export function questFixture(overrides: Partial<QuestView> = {}): QuestView {
  return {
    schema_version: 1, current: "soul", completed: 1, percent: 14, soul_path: "quickstart", trial_choices: [], instincts_acknowledged_at: null, presence: null,
    stages: STAGES.map(([stage, title, optional], index) => ({ stage, title, optional, summary: `${title} summary`, complete: index === 0, detail: index === 0 ? "2 of 2 essentials" : "Waiting" })),
    level: { level: 1, title: "Spark", xp: 30, level_floor: 0, next_level_at: 80 },
    badges: [
      { badge: "first_spark", title: "First Spark", hint: "Begin a Genesis creation.", xp: 5, requirement: null, unlocks: { palettes: [], crests: [] }, earned_at: "2026-10-22T12:00:00Z" },
      { badge: "soul_shaper", title: "Soul Shaper", hint: "Confirm who they are plus values or three traits.", xp: 15, requirement: null, unlocks: { palettes: ["aurora"], crests: [] }, earned_at: null },
    ],
    unlocked: { palettes: ["aether", "ember", "lumen", "tide"], crests: ["orbit", "bloom", "wave"] },
    ...overrides,
  };
}

export function longArcQuestFixture(overrides: Partial<LongArcQuestView> = {}): LongArcQuestView {
  return {
    quest_id: "seasonal_shared_stories",
    season_id: "2026-Q4",
    season_label: "2026 Q4",
    starts_at: "2026-10-01T00:00:00Z",
    ends_at_exclusive: "2027-01-01T00:00:00Z",
    progress: 0,
    active_entities: 0,
    complete: false,
    available: true,
    ...overrides,
  };
}

export function entityJourneyFixture(overrides: Partial<EntityJourney> = {}): EntityJourney {
  return {
    entity_id: "e1",
    name: "Nova",
    level: { level: 2, title: "Kindling", xp: 120, level_floor: 80, next_level_at: 180 },
    badges: questFixture().badges,
    mentor_messages: 3,
    conversation_sessions: 1,
    active_days: 2,
    active_weeks: 1,
    successful_skill_uses: 2,
    confirmed_memories: 1,
    next_milestone: {
      badge: "getting_acquainted",
      title: "Getting Acquainted",
      metric: "mentor_messages",
      target: 5,
      progress: 3,
    },
    unlocked: { palettes: ["aether"], crests: ["orbit"] },
    first_quests: [
      {
        id: "say-hello",
        title: "Say hello",
        prompt: "Hi Nova!",
        milestone: "first_words",
        target: 1,
        complete: true,
      },
    ],
    long_arc_quests: [
      longArcQuestFixture({ progress: 3, active_entities: 1 }),
      longArcQuestFixture({
        quest_id: "family_constellation",
        progress: 8,
        active_entities: 2,
      }),
    ],
    presence: {
      temperament: "gentle",
      identity: "aether",
      crest: "orbit",
      motion: "calm",
      name: "Nova",
      purpose: "Help us explore",
      boundaries: "",
    },
    certificate: null,
    ...overrides,
  };
}

export function questCatalogFixture(overrides: Partial<QuestCatalog> = {}): QuestCatalog {
  return {
    stages: [],
    badges: [
      {
        badge: "first_spark",
        title: "First Spark",
        hint: "Begin a Genesis creation.",
        xp: 5,
        requirement: null,
        unlocks: { palettes: [], crests: [] },
      },
      {
        badge: "first_words",
        title: "First Words",
        hint: "Send the first mentor message.",
        xp: 15,
        requirement: { metric: "mentor_messages", target: 1 },
        unlocks: { palettes: [], crests: [] },
      },
      {
        badge: "getting_acquainted",
        title: "Getting Acquainted",
        hint: "Share five mentor messages.",
        xp: 25,
        requirement: { metric: "mentor_messages", target: 5 },
        unlocks: { palettes: [], crests: [] },
      },
    ],
    tiers: [
      { level: 1, title: "Spark", floor: 0 },
      { level: 2, title: "Kindling", floor: 80 },
      { level: 3, title: "Ember", floor: 180 },
      { level: 4, title: "Hearthfire", floor: 350 },
      { level: 5, title: "Beacon", floor: 600 },
      { level: 6, title: "Constellation", floor: 950 },
      { level: 7, title: "Legacy", floor: 1400 },
    ],
    temperament_cards: [],
    trials: [],
    instincts: "# Instincts\nProtect the family.",
    palettes: [{ palette: "aether", unlocked_by: null }],
    crests: [{ crest: "orbit", unlocked_by: null }],
    long_arc_quests: [
      {
        id: "seasonal_shared_stories",
        title: "Shared Stories",
        summary: "Share twelve messages this season.",
        scope: "entity",
        metric: "mentor_messages",
        target: 12,
        minimum_entities: 1,
      },
      {
        id: "family_constellation",
        title: "Family Constellation",
        summary: "Share thirty messages across the family.",
        scope: "family",
        metric: "mentor_messages",
        target: 30,
        minimum_entities: 2,
      },
    ],
    ...overrides,
  };
}
