import { resolveHiveConnection } from "./connection";
import type {
  BadgeId,
  PresenceCard,
  PresenceChoice,
} from "./questClient";

const V1_KEY = "abigail.first-signal.creation.v1";
const V2_KEY = "abigail.first-signal.creation.v2";
const RESOLVED_PREFIX = "abigail.first-signal.creation.resolved.v1:";
const CREATION_LOCK = "abigail:first-signal:creation";
const MAX_ATTEMPT_KEY_BYTES = 512;
const MAX_PRESENCE_TEXT_CHARS = 2000;

type LockManagerLike = {
  request<T>(name: string, callback: () => Promise<T>): Promise<T>;
};

let fallbackClearLock: Promise<void> = Promise.resolve();

export interface LegacyProfile {
  name: string;
  purpose: string;
  temperament: "gentle" | "direct" | "curious";
  boundaries: string;
  identity: "aether" | "ember" | "lumen";
}

export interface LegacyAttempt {
  attemptId: string;
  hiveUrl: string;
  name: string;
  baselineIds: string[];
  creationKey?: string;
}

export interface LegacyJournalRecord {
  storage: "local" | "session";
  key: string;
  raw: string;
}

export interface LegacyFirstSignalJournal {
  source: "session-v1" | "local-v2";
  version: 1 | 2;
  revision: number;
  profile: LegacyProfile;
  attempt: LegacyAttempt;
  entityId?: string;
  recoveryEntityId?: string;
  records: LegacyJournalRecord[];
}

export interface LegacyCandidate {
  id: string;
  name: string;
  resolution: "completed_receipt" | "name_candidate";
}

export interface LegacyMigrationRequest {
  attempt_key: string;
  original_name: string;
  purpose: string;
  boundaries: string;
  presence: PresenceChoice;
}

export interface LegacyMigrationResult {
  entity_id: string;
  presence: PresenceCard;
  badges: BadgeId[];
  migrated_at: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, allowed: string[]): boolean {
  return Object.keys(value).every((key) => allowed.includes(key));
}

function charLength(value: string): number {
  return [...value].length;
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function hasControl(value: string): boolean {
  return /[\u0000-\u001f\u007f-\u009f]/u.test(value);
}

function hasDisallowedTextControl(value: string): boolean {
  return /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(value);
}

function isRustWhitespace(character: string): boolean {
  const code = character.codePointAt(0)!;
  return (code >= 0x0009 && code <= 0x000d)
    || code === 0x0020
    || code === 0x0085
    || code === 0x00a0
    || code === 0x1680
    || (code >= 0x2000 && code <= 0x200a)
    || code === 0x2028
    || code === 0x2029
    || code === 0x202f
    || code === 0x205f
    || code === 0x3000;
}

function trimRustWhitespace(value: string): string[] {
  const characters = [...value];
  let start = 0;
  let end = characters.length;
  while (start < end && isRustWhitespace(characters[start])) start += 1;
  while (end > start && isRustWhitespace(characters[end - 1])) end -= 1;
  return characters.slice(start, end);
}

function isId(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value === value.trim()
    && !/[\u0000-\u001f\u007f]/u.test(value);
}

function isAttemptKey(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value === trimRustWhitespace(value).join("")
    && utf8Length(value) <= MAX_ATTEMPT_KEY_BYTES
    && !hasControl(value);
}

function isCanonicalUuid(value: unknown): value is string {
  return typeof value === "string"
    && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(value);
}

function isHiveUrl(value: unknown): value is string {
  if (!isId(value)) return false;
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:")
      && !url.username
      && !url.password
      && !url.search
      && !url.hash
      && value === value.replace(/\/+$/u, "");
  } catch {
    return false;
  }
}

function isLegacyProfile(value: unknown): value is LegacyProfile {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["name", "purpose", "temperament", "boundaries", "identity"])) {
    return false;
  }
  return typeof value.name === "string"
    && value.name.trim().length > 0
    && charLength(value.name) <= 80
    && !/[\u0000-\u001f\u007f]/u.test(value.name)
    && typeof value.purpose === "string"
    && charLength(value.purpose) <= 2000
    && !hasDisallowedTextControl(value.purpose)
    && typeof value.boundaries === "string"
    && charLength(value.boundaries) <= 2000
    && !hasDisallowedTextControl(value.boundaries)
    && ["gentle", "direct", "curious"].includes(String(value.temperament))
    && ["aether", "ember", "lumen"].includes(String(value.identity));
}

function invalidJournal(): Error {
  return new Error(
    "The retained legacy First Signal journal has an unknown or invalid format. Its bytes were retained for recovery.",
  );
}

function invalidAttemptKey(): Error {
  return new Error(
    "The retained legacy First Signal attempt key is invalid. Its bytes were retained for recovery.",
  );
}

function invalidBrief(): Error {
  return new Error(
    "The retained legacy First Signal brief is invalid. Its bytes were retained for recovery.",
  );
}

function decode(
  value: unknown,
  source: LegacyFirstSignalJournal["source"],
  record: LegacyJournalRecord,
): LegacyFirstSignalJournal {
  if (!isRecord(value)
    || !hasOnlyKeys(value, ["version", "revision", "profile", "attempt", "entityId", "recoveryEntityId"])
    || (value.version !== 1 && value.version !== 2)
    || (source === "session-v1" && value.version !== 1)
    || (value.revision !== undefined
      && (!Number.isSafeInteger(value.revision) || Number(value.revision) < 0))
    || !isRecord(value.attempt)) {
    throw invalidJournal();
  }
  if (!isLegacyProfile(value.profile)) throw invalidBrief();
  const attempt = value.attempt;
  if (!hasOnlyKeys(attempt, ["attemptId", "hiveUrl", "name", "baselineIds", "creationKey"])
    || !isHiveUrl(attempt.hiveUrl)
    || typeof attempt.name !== "string"
    || attempt.name.length === 0
    || attempt.name !== attempt.name.trim()
    || charLength(attempt.name) > 80
    || /[\u0000-\u001f\u007f-\u009f]/u.test(attempt.name)
    || !Array.isArray(attempt.baselineIds)
    || !attempt.baselineIds.every(isId)
    || new Set(attempt.baselineIds).size !== attempt.baselineIds.length
    || (value.entityId !== undefined && !isId(value.entityId))
    || (value.recoveryEntityId !== undefined && !isId(value.recoveryEntityId))) {
    throw invalidJournal();
  }
  if (!isAttemptKey(attempt.attemptId)) throw invalidAttemptKey();
  if (value.version === 1) {
    if (attempt.creationKey !== undefined) throw invalidJournal();
  } else if (!isCanonicalUuid(attempt.creationKey)
    || attempt.attemptId !== attempt.creationKey
    || attempt.baselineIds.length !== 0
    || value.recoveryEntityId !== undefined
    || (value.entityId !== undefined && !isCanonicalUuid(value.entityId))) {
    throw invalidJournal();
  }

  return {
    source,
    version: value.version,
    revision: Number(value.revision ?? 0),
    profile: { ...value.profile },
    attempt: {
      attemptId: attempt.attemptId,
      hiveUrl: attempt.hiveUrl,
      name: attempt.name,
      baselineIds: [...attempt.baselineIds],
      ...(attempt.creationKey === undefined ? {} : { creationKey: attempt.creationKey }),
    },
    ...(value.entityId === undefined ? {} : { entityId: value.entityId }),
    ...(value.recoveryEntityId === undefined
      ? {}
      : { recoveryEntityId: value.recoveryEntityId }),
    records: [record],
  };
}

function parse(
  raw: string,
  source: LegacyFirstSignalJournal["source"],
  record: LegacyJournalRecord,
): LegacyFirstSignalJournal {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error(
      "The retained legacy First Signal journal is malformed. Its bytes were retained for recovery.",
    );
  }
  return decode(value, source, record);
}

/** Mirrors `hive_core::genesis_quest::excerpt` for legacy multiline text. */
export function canonicalLegacyPresenceText(value: string): string {
  const clean = trimRustWhitespace(value)
    .filter((character) => !hasControl(character) || character === "\n")
    .join("");
  const characters = [...clean];
  if (characters.length <= MAX_PRESENCE_TEXT_CHARS) return clean;
  return `${characters.slice(0, MAX_PRESENCE_TEXT_CHARS - 1).join("")}…`;
}

function resolutionMarker(journal: LegacyFirstSignalJournal): string {
  return `${RESOLVED_PREFIX}${encodeURIComponent(journal.attempt.attemptId)}`;
}

function resolvedSession(journal: LegacyFirstSignalJournal): boolean {
  return journal.version === 1
    && localStorage.getItem(resolutionMarker(journal)) === JSON.stringify(journal.attempt);
}

function sameAttempt(left: LegacyAttempt, right: LegacyAttempt): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

/**
 * Strictly decodes the two retired browser journals. This function is
 * deliberately read-only: malformed and conflicting bytes remain available
 * for explicit recovery.
 */
export function readLegacyFirstSignalJournal(): LegacyFirstSignalJournal | null {
  const localRaw = localStorage.getItem(V2_KEY);
  const sessionRaw = sessionStorage.getItem(V1_KEY);
  const local = localRaw === null
    ? null
    : parse(localRaw, "local-v2", { storage: "local", key: V2_KEY, raw: localRaw });
  let session = sessionRaw === null
    ? null
    : parse(sessionRaw, "session-v1", { storage: "session", key: V1_KEY, raw: sessionRaw });
  if (session && resolvedSession(session)) session = null;
  if (!session) return local;
  if (!local) return session;
  if (local.version !== 1 || !sameAttempt(local.attempt, session.attempt)) {
    throw new Error(
      "A different legacy First Signal is retained in local and session storage. Both records were retained for recovery.",
    );
  }
  return { ...local, records: [...local.records, ...session.records] };
}

async function readEnvelope(response: Response, path: string): Promise<unknown> {
  const envelope = await response.json() as {
    ok?: unknown;
    data?: unknown;
    error?: unknown;
  };
  if (!response.ok || envelope.ok !== true || envelope.data === undefined) {
    throw new Error(
      typeof envelope.error === "string"
        ? envelope.error
        : `Hive request to ${path} failed (${response.status}).`,
    );
  }
  return envelope.data;
}

function exactKeys(value: Record<string, unknown>, expected: string[]): boolean {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  return actual.length === wanted.length
    && actual.every((key, index) => key === wanted[index]);
}

/**
 * Resolves only existing Entities. No path in legacy recovery sends an Entity
 * creation request.
 */
export async function resolveLegacyCandidates(
  journal: LegacyFirstSignalJournal,
): Promise<LegacyCandidate[]> {
  const { base, token } = await resolveHiveConnection();
  const headers = {
    Accept: "application/json",
    Authorization: `Bearer ${token}`,
  };
  if (journal.attempt.creationKey) {
    const path = `/v1/entity-creations/${encodeURIComponent(journal.attempt.creationKey)}`;
    const response = await fetch(`${base}${path}`, {
      method: "GET",
      headers,
      signal: AbortSignal.timeout(5_000),
    });
    const raw = await readEnvelope(response, path);
    if (!isRecord(raw)
      || raw.creation_key !== journal.attempt.creationKey
      || typeof raw.state !== "string") {
      throw new Error("Hive returned an invalid legacy creation receipt.");
    }
    if (raw.state === "absent") {
      if (!exactKeys(raw, ["creation_key", "state"])) {
        throw new Error("Hive returned an invalid legacy creation receipt.");
      }
      return [];
    }
    if (raw.state === "reserved") {
      if (!exactKeys(raw, ["creation_key", "state", "name"])
        || raw.name !== journal.attempt.name) {
        throw new Error("Hive returned an invalid legacy creation receipt.");
      }
      return [];
    }
    if (raw.state === "unavailable") {
      if (!exactKeys(raw, ["creation_key", "state", "name", "id"])
        || raw.name !== journal.attempt.name
        || !isCanonicalUuid(raw.id)) {
        throw new Error("Hive returned an invalid legacy creation receipt.");
      }
      return [];
    }
    if (raw.state !== "complete"
      || !exactKeys(raw, ["creation_key", "state", "name", "id", "directory"])
      || raw.name !== journal.attempt.name
      || !isCanonicalUuid(raw.id)
      || !isId(raw.directory)) {
      throw new Error("Hive returned an invalid legacy creation receipt.");
    }
    return [{
      id: raw.id,
      name: raw.name,
      resolution: "completed_receipt",
    }];
  }

  const path = "/v1/entities";
  const response = await fetch(`${base}${path}`, {
    method: "GET",
    headers,
    signal: AbortSignal.timeout(5_000),
  });
  const raw = await readEnvelope(response, path);
  if (!Array.isArray(raw)
    || raw.some((item) => !isRecord(item)
      || !isId(item.id)
      || typeof item.name !== "string"
      || typeof item.is_hive !== "boolean")) {
    throw new Error("Hive returned an invalid Entity list for legacy recovery.");
  }
  const ids = raw.map((item) => (item as Record<string, unknown>).id);
  if (new Set(ids).size !== ids.length) {
    throw new Error("Hive returned duplicate Entity IDs for legacy recovery.");
  }
  const baseline = new Set(journal.attempt.baselineIds);
  return raw
    .filter((item) => {
      const candidate = item as Record<string, unknown>;
      return candidate.is_hive === false
        && candidate.name === journal.attempt.name
        && !baseline.has(candidate.id as string);
    })
    .map((item) => ({
      id: (item as Record<string, unknown>).id as string,
      name: (item as Record<string, unknown>).name as string,
      resolution: "name_candidate" as const,
    }));
}

async function withCreationLock<T>(work: () => Promise<T>): Promise<T> {
  const locks = typeof navigator === "undefined"
    ? undefined
    : (navigator as Navigator & { locks?: LockManagerLike }).locks;
  if (locks?.request) return locks.request(CREATION_LOCK, work);

  const result = fallbackClearLock.then(work, work);
  fallbackClearLock = result.then(() => undefined, () => undefined);
  return result;
}

async function clearUnlocked(journal: LegacyFirstSignalJournal): Promise<void> {
  for (const record of journal.records) {
    const area = record.storage === "local" ? localStorage : sessionStorage;
    const current = area.getItem(record.key);
    if (current !== null && current !== record.raw) {
      throw new Error(
        "The retained legacy First Signal changed after it was reviewed. Reload before clearing it.",
      );
    }
  }
  if (journal.version === 1) {
    const marker = resolutionMarker(journal);
    const exact = JSON.stringify(journal.attempt);
    const current = localStorage.getItem(marker);
    if (current !== null && current !== exact) {
      throw new Error(
        "The retained legacy First Signal resolution marker changed. Its records were retained.",
      );
    }
    if (current === null) localStorage.setItem(marker, exact);
  }
  for (const record of journal.records) {
    const area = record.storage === "local" ? localStorage : sessionStorage;
    if (area.getItem(record.key) === record.raw) area.removeItem(record.key);
  }
}

/**
 * Removes only the exact bytes the mentor reviewed. A v1 marker is committed
 * first so a duplicated tab cannot resurrect its independent session copy.
 */
export function clearLegacyFirstSignalJournal(
  journal: LegacyFirstSignalJournal,
): Promise<void> {
  return withCreationLock(() => clearUnlocked(journal));
}
