import { createOrResumeGenesis, type GenesisCreationSeed, type GenesisSession } from "./genesisClient";

export const GENESIS_LAUNCH_DRAFT_KEY = "abigail.first-signal.genesis.v1";
const LOCK_NAME = `${GENESIS_LAUNCH_DRAFT_KEY}.lock`;

export interface GenesisLaunchDraft {
  version: 1;
  creation_key: string;
  seed: GenesisCreationSeed;
}

type Dispatch = (seed: GenesisCreationSeed, creationKey: string) => Promise<GenesisSession>;
export interface DispatchGenesisLaunchOptions {
  expectedDraft?: GenesisLaunchDraft;
  dispatch?: Dispatch;
}
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
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
}

function charLength(value: string): number {
  return [...value].length;
}

function isName(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value === value.trim()
    && charLength(value) <= 80
    && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function isPurpose(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value === value.trim()
    && charLength(value) <= 2000
    && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(value);
}

function isSeed(value: unknown): value is GenesisCreationSeed {
  if (!isRecord(value) || !hasExactKeys(value, ["mode", "name", "purpose", "presence"])) return false;
  const presence = value.presence;
  return typeof value.mode === "string"
    && ["guided", "voice", "markdown", "hybrid", "quick", "deep"].includes(value.mode)
    && isName(value.name)
    && isPurpose(value.purpose)
    && isRecord(presence)
    && hasExactKeys(presence, ["temperament", "identity", "crest", "motion"])
    && typeof presence.temperament === "string"
    && ["gentle", "curious", "direct", "playful", "steady"].includes(presence.temperament)
    && typeof presence.identity === "string"
    && ["aether", "ember", "lumen", "tide", "aurora", "starlight", "hearth"].includes(presence.identity)
    && typeof presence.crest === "string"
    && ["orbit", "bloom", "wave", "lantern", "compass", "constellation"].includes(presence.crest)
    && typeof presence.motion === "string"
    && ["calm", "lively", "still"].includes(presence.motion);
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

function sameDraft(left: GenesisLaunchDraft, right: GenesisLaunchDraft): boolean {
  return left.version === right.version
    && left.creation_key === right.creation_key
    && sameSeed(left.seed, right.seed);
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
  optionsOrDispatch: DispatchGenesisLaunchOptions | Dispatch = {},
): Promise<GenesisSession> {
  return withLaunchLock(async () => {
    if (!isCreationKey(creationKey) || !isSeed(seed)) {
      throw new Error("First Signal requires a canonical creation key and reviewed name, purpose, and Presence.");
    }
    const options = typeof optionsOrDispatch === "function"
      ? { dispatch: optionsOrDispatch }
      : optionsOrDispatch;
    const dispatch = options.dispatch ?? createOrResumeGenesis;
    const existing = readGenesisLaunchDraft();
    if (options.expectedDraft && (!existing || !sameDraft(existing, options.expectedDraft))) {
      throw new Error("The saved creation changed since this First Signal was shown. Reload before retrying.");
    }
    if (existing && existing.creation_key !== creationKey) {
      throw new Error("The saved First Signal creation key is immutable. Resume that launch before beginning another.");
    }
    if (existing && !sameSeed(existing.seed, seed)) {
      throw new Error("The reviewed input differs from the saved First Signal. Resume the saved launch without changing it.");
    }

    const draft: GenesisLaunchDraft = options.expectedDraft ?? existing ?? { version: 1, creation_key: creationKey, seed };
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

export type GenesisLaunchService = (seed: GenesisCreationSeed) => Promise<GenesisSession>;

/**
 * Captures the journal state shown by one mounted workspace. Callers submit
 * only reviewed seeds; keys and stale-view checks never cross the component
 * boundary.
 */
export function createGenesisLaunchService(
  dispatch: Dispatch = createOrResumeGenesis,
): GenesisLaunchService {
  let expectedDraft: GenesisLaunchDraft | undefined;
  let journalError: unknown;
  try {
    expectedDraft = readGenesisLaunchDraft() ?? undefined;
  } catch (error) {
    journalError = error;
  }
  const creationKey = expectedDraft?.creation_key ?? crypto.randomUUID();

  return async (seed) => {
    if (journalError) throw journalError;
    try {
      return await dispatchGenesisLaunch(seed, creationKey, { expectedDraft, dispatch });
    } catch (error) {
      if (!expectedDraft) {
        try {
          const retained = readGenesisLaunchDraft();
          if (retained?.creation_key === creationKey && sameSeed(retained.seed, seed)) {
            expectedDraft = retained;
          }
        } catch {
          // Preserve the original launch failure and the journal bytes.
        }
      }
      throw error;
    }
  };
}
