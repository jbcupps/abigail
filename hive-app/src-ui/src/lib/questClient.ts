import { resolveHiveConnection } from "./connection";
import { genesisRequest } from "./genesisClient";

export type QuestStageId = "reservation" | "soul" | "ethics" | "instincts" | "presence" | "birth" | "awakening";
export type BadgeId = "first_spark" | "named" | "purposeful" | "soul_shaper" | "storyteller" | "trial_walker" | "instinct_keeper" | "presence_chosen" | "curator" | "first_light" | "first_words" | "getting_acquainted" | "kindred";
export type Temperament = "gentle" | "curious" | "direct" | "playful" | "steady";
export type Palette = "aether" | "ember" | "lumen" | "tide" | "aurora" | "starlight" | "hearth";
export type Crest = "orbit" | "bloom" | "wave" | "lantern" | "compass" | "constellation";
export type Motion = "calm" | "lively" | "still";

export interface BondLevel { level: number; title: string; xp: number; level_floor: number; next_level_at: number | null }
export interface BadgeView { badge: BadgeId; title: string; hint: string; xp: number; earned_at: string | null }
export interface Unlocks { palettes: Palette[]; crests: Crest[] }
export interface PresenceChoice { temperament: Temperament; identity: Palette; crest: Crest; motion: Motion }
export interface PresenceCard extends PresenceChoice { name: string; purpose: string; boundaries: string }
export interface TrialChoice { scenario_id: string; choice_id: string }
export interface StageView { stage: QuestStageId; title: string; summary: string; optional: boolean; complete: boolean; detail: string }
export interface QuestView {
  schema_version: number; stages: StageView[]; current: QuestStageId; completed: number; percent: number;
  level: BondLevel; badges: BadgeView[]; unlocked: Unlocks; soul_path: "forge" | "quickstart";
  trial_choices: TrialChoice[]; instincts_acknowledged_at: string | null; presence: PresenceCard | null;
}
export interface TemperamentCard { id: string; title: string; temperament: Temperament; tagline: string; description: string; communication_style: string; stats: Record<string, number> }
export interface Trial { id: string; title: string; description: string; choices: { id: string; label: string; description: string }[] }
export interface QuestCatalog {
  stages: { stage: QuestStageId; title: string; summary: string; optional: boolean }[];
  badges: BadgeView[]; temperament_cards: TemperamentCard[]; trials: Trial[]; instincts: string;
  palettes: { palette: Palette; unlocked_by: BadgeId | null }[];
  crests: { crest: Crest; unlocked_by: BadgeId | null }[];
}
export interface FirstQuest { id: string; title: string; prompt: string; milestone: BadgeId; target: number; complete: boolean }
export interface CertificateView { archetype: string; epithet: string; sigil: string; birth_path: string; scoring_version: string; issued_at_utc: string; signature_verified: boolean }
export interface EntityJourney {
  entity_id: string; name: string; level: BondLevel; badges: BadgeView[]; mentor_messages: number;
  next_milestone: { badge: BadgeId; title: string; target: number; progress: number } | null;
  unlocked: Unlocks; first_quests: FirstQuest[]; presence: PresenceCard | null; certificate: CertificateView | null;
}

async function entityRequest<T>(path: string, body?: unknown, method = body === undefined ? "GET" : "PUT"): Promise<T> {
  const { base, token } = await resolveHiveConnection();
  const response = await fetch(`${base}/v1/entities/${path}`, {
    method,
    headers: { Accept: "application/json", Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(30_000),
  });
  const envelope = await response.json() as { ok: boolean; data?: T; error?: string };
  if (!response.ok || !envelope.ok || envelope.data === undefined) throw new Error(envelope.error || `Hive request failed (${response.status}).`);
  return envelope.data;
}

let catalog: Promise<QuestCatalog> | null = null;
/** Static per daemon build; one request serves every quest view. */
export function getQuestCatalog(): Promise<QuestCatalog> {
  catalog ??= genesisRequest<QuestCatalog>("/quest/catalog").catch((error) => { catalog = null; throw error; });
  return catalog;
}
export const getEntityJourney = (entityId: string) => entityRequest<EntityJourney>(`${encodeURIComponent(entityId)}/journey`);
export const setEntityPresence = (entityId: string, choice: PresenceChoice) => entityRequest<PresenceCard>(`${encodeURIComponent(entityId)}/presence`, choice);

export const PALETTES: Record<Palette, { label: string; colors: [string, string] }> = {
  aether: { label: "Aether", colors: ["#8f7cf7", "#58c4dc"] },
  ember: { label: "Ember", colors: ["#f08a5d", "#f6c453"] },
  lumen: { label: "Lumen", colors: ["#f5d76e", "#fff4c2"] },
  tide: { label: "Tide", colors: ["#2f9fb3", "#7fd8be"] },
  aurora: { label: "Aurora", colors: ["#7f5af0", "#2cb67d"] },
  starlight: { label: "Starlight", colors: ["#c9d6ff", "#8e9efc"] },
  hearth: { label: "Hearth", colors: ["#d1495b", "#edae49"] },
};
export const CRESTS: Record<Crest, string> = { orbit: "Orbit", bloom: "Bloom", wave: "Wave", lantern: "Lantern", compass: "Compass", constellation: "Constellation" };
export const MOTIONS: Record<Motion, string> = { calm: "Calm", lively: "Lively", still: "Still" };
export const TEMPERAMENTS: Record<Temperament, string> = { gentle: "Gentle", curious: "Curious", direct: "Direct", playful: "Playful", steady: "Steady" };
export const TRAIT_LABELS: Record<string, string> = {
  empathy: "Empathy", curiosity: "Curiosity", assertiveness: "Assertiveness", humor: "Humor", creativity: "Creativity",
  analytical_reasoning: "Reasoning", emotional_expressiveness: "Expressiveness", independence: "Independence", sociability: "Sociability",
};
