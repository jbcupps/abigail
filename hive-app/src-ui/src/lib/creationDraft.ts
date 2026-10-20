import { isEntityProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { holdsCreationLock, isCanonicalUuid, withCreationLock, type CreationAttempt, type CreationLease } from "./presenceCreation";

// One unfinished creation per browser origin. This survives closing the tab;
// clearing site data still removes it. Web Locks serialize creation dispatch;
// every mutation holds that lock. Revisions then reject stale snapshots while
// the lock is held; they are not a substitute for an atomic cross-tab lock.
const LEGACY_KEY = "abigail.first-signal.creation.v1";
const LEGACY_RESOLVED_PREFIX = "abigail.first-signal.creation.resolved.v1:";
export const CREATION_DRAFT_KEY = "abigail.first-signal.creation.v2";
export const CREATION_DRAFT_EVENT = "abigail:creation-draft-changed";

export interface CreationDraft {
  version: 1 | 2;
  revision?: number;
  profile: EntityProfile;
  attempt: CreationAttempt;
  entityId?: string;
  // An explicitly selected recovery target is not proof of POST creation.
  recoveryEntityId?: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, allowed: string[]): boolean {
  return Object.keys(value).every(key => allowed.includes(key));
}

function isId(value: unknown): value is string {
  return typeof value === "string" && !!value && value === value.trim()
    && !/[\u0000-\u001f\u007f]/.test(value);
}

function isHiveUrl(value: unknown): value is string {
  if (!isId(value)) return false;
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:")
      && !url.username && !url.password && !url.search && !url.hash && value === value.replace(/\/+$/, "");
  } catch {
    return false;
  }
}

function decode(value: unknown): CreationDraft {
  const invalid = () => new Error("The unfinished creation record is invalid. It has been kept unchanged; resolve it before creating another Entity.");
  if (!isRecord(value) || !hasOnlyKeys(value, ["version", "revision", "profile", "attempt", "entityId", "recoveryEntityId"])
    || ![1, 2].includes(Number(value.version)) || (value.version !== 1 && value.version !== 2)
    || (value.revision !== undefined && (!Number.isSafeInteger(value.revision) || Number(value.revision) < 0)) || !isRecord(value.profile)
    || !hasOnlyKeys(value.profile, ["name", "purpose", "temperament", "boundaries", "identity"])
    || !isEntityProfile(value.profile) || !isRecord(value.attempt)) throw invalid();
  const attempt = value.attempt;
  if (!hasOnlyKeys(attempt, ["attemptId", "hiveUrl", "name", "baselineIds", "creationKey"])
    || !isId(attempt.attemptId) || !isHiveUrl(attempt.hiveUrl)
    || typeof attempt.name !== "string" || !attempt.name.trim() || attempt.name !== attempt.name.trim()
    || Array.from(attempt.name).length > 80 || /[\u0000-\u001f\u007f-\u009f]/.test(attempt.name)
    || !Array.isArray(attempt.baselineIds) || !attempt.baselineIds.every(isId)
    || new Set(attempt.baselineIds).size !== attempt.baselineIds.length
    || (value.entityId !== undefined && !isId(value.entityId))
    || (value.recoveryEntityId !== undefined && !isId(value.recoveryEntityId))
    || (value.version === 1 && attempt.creationKey !== undefined)
    || (value.version === 2 && (!isCanonicalUuid(attempt.creationKey) || attempt.attemptId !== attempt.creationKey
      || attempt.baselineIds.length !== 0 || value.recoveryEntityId !== undefined
      || (value.entityId !== undefined && !isCanonicalUuid(value.entityId))))) throw invalid();
  // Copy just the validated data; no caller-owned object remains in the record.
  return {
    version: value.version,
    revision: Number(value.revision ?? 0),
    profile: { ...value.profile },
    attempt: {
      attemptId: attempt.attemptId,
      hiveUrl: attempt.hiveUrl,
      name: attempt.name,
      baselineIds: [...attempt.baselineIds],
      ...(attempt.creationKey === undefined ? {} : { creationKey: attempt.creationKey as string }),
    },
    ...(value.entityId === undefined ? {} : { entityId: value.entityId as string }),
    ...(value.recoveryEntityId === undefined ? {} : { recoveryEntityId: value.recoveryEntityId as string }),
  };
}

function parse(raw: string): CreationDraft {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error("The unfinished creation record could not be read. It has been kept unchanged; resolve it before creating another Entity.");
  }
  return decode(value);
}

function legacyResolved(draft: CreationDraft): boolean {
  return draft.version === 1 && localStorage.getItem(LEGACY_RESOLVED_PREFIX + encodeURIComponent(draft.attempt.attemptId)) === JSON.stringify(draft.attempt);
}

export function readCreationDraft(): CreationDraft | null {
  const raw = localStorage.getItem(CREATION_DRAFT_KEY);
  const legacyRaw = sessionStorage.getItem(LEGACY_KEY);
  const current = raw === null ? null : parse(raw);
  if (legacyRaw === null) return current;
  const legacy = parse(legacyRaw);
  if (legacy.version !== 1) throw new Error("The old session record has an unsupported format; it has been retained.");
  // Duplicated older tabs keep independent sessionStorage. Clearing the durable
  // record must not allow one of those stale session copies to resurrect it.
  if (legacyResolved(legacy)) return current;
  if (current && (JSON.stringify(current.attempt) !== JSON.stringify(legacy.attempt) || current.version !== 1)) {
    throw new Error("A different creation exists in this browser and in this older tab. Both records are retained. Resolve them before creating another Entity.");
  }
  // Reads never mutate storage. Legacy migration uses the same Web Lock as
  // creation/save, so two tabs cannot overwrite each other's recovery record.
  return current ?? legacy;
}

function removeLegacy(attemptId: string) {
  const raw = sessionStorage.getItem(LEGACY_KEY);
  if (raw !== null && parse(raw).attempt.attemptId === attemptId) sessionStorage.removeItem(LEGACY_KEY);
}

function writeUnlocked(draft: CreationDraft): CreationDraft {
  const next = decode(draft);
  const current = readCreationDraft();
  if (legacyResolved(next) && current?.attempt.attemptId !== next.attempt.attemptId) {
    throw new Error("This older creation was already resolved. A stale session cannot recreate it.");
  }
  if (current) {
    if (current.attempt.attemptId !== next.attempt.attemptId) {
      throw new Error("Another creation is unfinished in this browser. Resume it before creating another Entity.");
    }
    if (current.attempt.hiveUrl !== next.attempt.hiveUrl || current.attempt.name !== next.attempt.name
      || current.version !== next.version || current.attempt.creationKey !== next.attempt.creationKey
      || JSON.stringify(current.attempt.baselineIds) !== JSON.stringify(next.attempt.baselineIds)) {
      throw new Error("The dispatched creation details cannot be changed.");
    }
    if (current.entityId !== undefined) {
      if (next.entityId !== undefined && next.entityId !== current.entityId) {
        throw new Error("This creation already has a confirmed Entity ID.");
      }
      // A render captured before confirmation must not erase that confirmation.
      next.entityId = current.entityId;
    }
    // Omitting a selection in a stale render cannot erase it. Supplying a new
    // selection is the caller's explicit recovery-target change.
    if (next.recoveryEntityId === undefined && current.recoveryEntityId !== undefined) {
      next.recoveryEntityId = current.recoveryEntityId;
    }
    if (current.revision !== next.revision) throw new Error("This creation changed in another tab. Reload its latest brief before saving these edits.");
  } else if (next.revision !== 0) {
    throw new Error("This creation record was already resolved or removed. A stale operation cannot recreate it.");
  }
  // Web Storage writes are atomic: a thrown quota/access error leaves the
  // previous entry intact. Never remove the entry before a successful write.
  next.revision = (next.revision ?? 0) + 1;
  localStorage.setItem(CREATION_DRAFT_KEY, JSON.stringify(next));
  removeLegacy(next.attempt.attemptId);
  window.dispatchEvent(new Event(CREATION_DRAFT_EVENT));
  return next;
}

function underLock<T>(lease: CreationLease | undefined, work: () => T): Promise<T> {
  return holdsCreationLock(lease) ? Promise.resolve().then(work) : withCreationLock(work);
}

export function migrateCreationDraft(lease?: CreationLease): Promise<CreationDraft | null> {
  return underLock(lease, () => {
    const current = readCreationDraft();
    if (!current || sessionStorage.getItem(LEGACY_KEY) === null) return current;
    // Never re-key an older POST. Write first, remove the exact old record only
    // after success; storage failures preserve at least one copy.
    return writeUnlocked(current);
  });
}

export function writeCreationDraft(draft: CreationDraft, lease?: CreationLease): Promise<CreationDraft> {
  return underLock(lease, () => writeUnlocked(draft));
}

export function confirmCreationDraft(attemptId: string, id: string, lease?: CreationLease): Promise<CreationDraft | null> {
  return underLock(lease, () => {
  if (!isId(attemptId) || !isId(id)) throw new Error("Creation confirmation requires valid attempt and Entity IDs.");
  const current = readCreationDraft();
  if (!current || current.attempt.attemptId !== attemptId) return null;
  if (current.entityId === id) return current;
  const confirmed = { ...current, entityId: id };
  return writeUnlocked(confirmed);
  });
}

export function clearCreationDraft(attemptId: string, expectedRevision?: number, lease?: CreationLease): Promise<void> {
  return underLock(lease, () => {
  if (!isId(attemptId)) throw new Error("Clearing a creation requires a valid attempt ID.");
  const current = readCreationDraft();
  if (current?.attempt.attemptId === attemptId) {
    if (expectedRevision !== undefined && current.revision !== expectedRevision) throw new Error("The creation changed in another tab. Its newer record has been retained.");
    if (current.version === 1) {
      // Record exact legacy identity before removal. A failed marker write keeps
      // the journal intact; this stores only the immutable attempt identity.
      localStorage.setItem(LEGACY_RESOLVED_PREFIX + encodeURIComponent(attemptId), JSON.stringify(current.attempt));
    }
    removeLegacy(attemptId);
    localStorage.removeItem(CREATION_DRAFT_KEY);
    window.dispatchEvent(new Event(CREATION_DRAFT_EVENT));
  }
  });
}
