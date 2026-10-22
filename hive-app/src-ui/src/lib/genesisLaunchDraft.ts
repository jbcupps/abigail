import { createOrResumeGenesis, type GenesisCreationSeed, type GenesisSession } from "./genesisClient";

export const GENESIS_LAUNCH_DRAFT_KEY = "abigail.first-signal.genesis.v1";
const LOCK_NAME = `${GENESIS_LAUNCH_DRAFT_KEY}.lock`;

export interface GenesisLaunchDraft {
  version: 1;
  creation_key: string;
  seed: GenesisCreationSeed;
}

type Dispatch = (seed: GenesisCreationSeed, creationKey: string) => Promise<GenesisSession>;
type LockManagerLike = {
  request<T>(name: string, callback: () => Promise<T>): Promise<T>;
};

let fallbackLock: Promise<void> = Promise.resolve();

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
}

function isCreationKey(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value);
}

function isSeed(value: unknown): value is GenesisCreationSeed {
  if (!isRecord(value) || !hasExactKeys(value, ["mode", "name", "purpose", "presence"])) return false;
  const presence = value.presence;
  return ["guided", "voice", "markdown", "hybrid", "quick", "deep"].includes(String(value.mode))
    && typeof value.name === "string"
    && typeof value.purpose === "string"
    && isRecord(presence)
    && hasExactKeys(presence, ["temperament", "identity", "crest", "motion"])
    && ["gentle", "curious", "direct", "playful", "steady"].includes(String(presence.temperament))
    && ["aether", "ember", "lumen", "tide", "aurora", "starlight", "hearth"].includes(String(presence.identity))
    && ["orbit", "bloom", "wave", "lantern", "compass", "constellation"].includes(String(presence.crest))
    && ["calm", "lively", "still"].includes(String(presence.motion));
}

function sameSeed(left: GenesisCreationSeed, right: GenesisCreationSeed): boolean {
  return left.mode === right.mode
    && left.name === right.name
    && left.purpose === right.purpose
    && left.presence.temperament === right.presence.temperament
    && left.presence.identity === right.presence.identity
    && left.presence.crest === right.presence.crest
    && left.presence.motion === right.presence.motion;
}

export function readGenesisLaunchDraft(): GenesisLaunchDraft | null {
  const raw = localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY);
  if (raw === null) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("The saved First Signal recovery journal is malformed. It has been retained for recovery.");
  }
  if (!isRecord(parsed)
    || !hasExactKeys(parsed, ["version", "creation_key", "seed"])
    || parsed.version !== 1
    || !isCreationKey(parsed.creation_key)
    || !isSeed(parsed.seed)) {
    throw new Error("The saved First Signal recovery journal has an unknown or invalid format. It has been retained for recovery.");
  }
  return parsed as unknown as GenesisLaunchDraft;
}

async function withLaunchLock<T>(work: () => Promise<T>): Promise<T> {
  const locks = typeof navigator === "undefined"
    ? undefined
    : (navigator as Navigator & { locks?: LockManagerLike }).locks;
  if (locks?.request) return locks.request(LOCK_NAME, work);

  const result = fallbackLock.then(work, work);
  fallbackLock = result.then(() => undefined, () => undefined);
  return result;
}

export function dispatchGenesisLaunch(
  seed: GenesisCreationSeed,
  creationKey: string,
  dispatch: Dispatch = createOrResumeGenesis,
): Promise<GenesisSession> {
  return withLaunchLock(async () => {
    const existing = readGenesisLaunchDraft();
    if (existing && existing.creation_key !== creationKey) {
      throw new Error("The saved First Signal creation key is immutable. Resume that launch before beginning another.");
    }
    if (existing && !sameSeed(existing.seed, seed)) {
      throw new Error("The reviewed input differs from the saved First Signal. Resume the saved launch without changing it.");
    }

    const draft: GenesisLaunchDraft = existing ?? { version: 1, creation_key: creationKey, seed };
    if (!existing) localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(draft));

    const created = await dispatch(draft.seed, draft.creation_key);
    if (created.id !== draft.creation_key) {
      throw new Error("Hive returned a different session for this First Signal. The recovery journal was retained.");
    }

    const current = readGenesisLaunchDraft();
    if (current?.creation_key === draft.creation_key && sameSeed(current.seed, draft.seed)) {
      localStorage.removeItem(GENESIS_LAUNCH_DRAFT_KEY);
    }
    return created;
  });
}
