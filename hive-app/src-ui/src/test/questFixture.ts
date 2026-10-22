import type { QuestStageId, QuestView } from "../lib/questClient";

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
