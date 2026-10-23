export type Temperament = "gentle" | "curious" | "direct" | "playful" | "steady";
export type Palette = "aether" | "ember" | "lumen" | "tide" | "aurora" | "starlight" | "hearth";
export type Crest = "orbit" | "bloom" | "wave" | "lantern" | "compass" | "constellation";
export type Motion = "calm" | "lively" | "still";

/** Mirrors the canonical `hive_core::genesis_quest::PresenceCard`. */
export interface PresenceCard {
  name: string;
  purpose: string;
  temperament: Temperament;
  boundaries: string;
  identity: Palette;
  crest: Crest;
  motion: Motion;
}

export const PALETTES: Record<Palette, { label: string; colors: [string, string] }> = {
  aether: { label: "Aether", colors: ["#8f7cf7", "#58c4dc"] },
  ember: { label: "Ember", colors: ["#f08a5d", "#f6c453"] },
  lumen: { label: "Lumen", colors: ["#f5d76e", "#fff4c2"] },
  tide: { label: "Tide", colors: ["#2f9fb3", "#7fd8be"] },
  aurora: { label: "Aurora", colors: ["#7f5af0", "#2cb67d"] },
  starlight: { label: "Starlight", colors: ["#c9d6ff", "#8e9efc"] },
  hearth: { label: "Hearth", colors: ["#d1495b", "#edae49"] },
};

export const TEMPERAMENTS: Record<Temperament, string> = {
  gentle: "Gentle",
  curious: "Curious",
  direct: "Direct",
  playful: "Playful",
  steady: "Steady",
};

export const CRESTS: Record<Crest, string> = {
  orbit: "Orbit",
  bloom: "Bloom",
  wave: "Wave",
  lantern: "Lantern",
  compass: "Compass",
  constellation: "Constellation",
};

export const MOTIONS: Record<Motion, string> = {
  calm: "Calm",
  lively: "Lively",
  still: "Still",
};

const TEMPERAMENT_VALUES = new Set<string>(Object.keys(TEMPERAMENTS));
const PALETTE_VALUES = new Set<string>(Object.keys(PALETTES));
const CREST_VALUES = new Set<string>(Object.keys(CRESTS));
const MOTION_VALUES = new Set<string>(Object.keys(MOTIONS));

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parsePresence(value: unknown): PresenceCard {
  if (
    !isRecord(value)
    || typeof value.name !== "string"
    || typeof value.purpose !== "string"
    || typeof value.boundaries !== "string"
    || typeof value.temperament !== "string"
    || !TEMPERAMENT_VALUES.has(value.temperament)
    || typeof value.identity !== "string"
    || !PALETTE_VALUES.has(value.identity)
    || typeof value.crest !== "string"
    || !CREST_VALUES.has(value.crest)
    || typeof value.motion !== "string"
    || !MOTION_VALUES.has(value.motion)
  ) {
    throw new Error("The Entity returned an invalid Presence card.");
  }

  // Copy only the closed canonical presentation fields into Runtime UI state.
  return {
    name: value.name,
    purpose: value.purpose,
    temperament: value.temperament as Temperament,
    boundaries: value.boundaries,
    identity: value.identity as Palette,
    crest: value.crest as Crest,
    motion: value.motion as Motion,
  };
}

export async function getRuntimePresence(
  baseUrl: string,
  signal: AbortSignal,
): Promise<PresenceCard | null> {
  const response = await fetch(`${baseUrl.replace(/\/+$/, "")}/v1/presence`, {
    headers: { Accept: "application/json" },
    signal,
  });
  if (!response.ok) throw new Error("Presence is unavailable.");

  const envelope: unknown = await response.json();
  if (!isRecord(envelope) || envelope.ok !== true || !("data" in envelope)) {
    throw new Error("Presence is unavailable.");
  }
  return envelope.data === null ? null : parsePresence(envelope.data);
}
