import { isEntityProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import type { CreationAttempt } from "./presenceCreation";

// One unfinished live creation per origin and tab. This survives a reload in
// that tab; it is not a durable backup across browsers or closed sessions.
const DRAFT_KEY = "abigail.first-signal.creation.v1";
export const CREATION_DRAFT_EVENT = "abigail:creation-draft-changed";

export interface CreationDraft {
  version: 1;
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
      && !url.search && !url.hash && value === value.replace(/\/+$/, "");
  } catch {
    return false;
  }
}

function decode(value: unknown): CreationDraft {
  const invalid = () => new Error("The unfinished creation record is invalid. It has been kept unchanged; resolve it before creating another Entity.");
  if (!isRecord(value) || !hasOnlyKeys(value, ["version", "profile", "attempt", "entityId", "recoveryEntityId"])
    || value.version !== 1 || !isRecord(value.profile)
    || !hasOnlyKeys(value.profile, ["name", "purpose", "temperament", "boundaries", "identity"])
    || !isEntityProfile(value.profile) || !isRecord(value.attempt)) throw invalid();
  const attempt = value.attempt;
  if (!hasOnlyKeys(attempt, ["attemptId", "hiveUrl", "name", "baselineIds"])
    || !isId(attempt.attemptId) || !isHiveUrl(attempt.hiveUrl)
    || typeof attempt.name !== "string" || !attempt.name.trim() || attempt.name !== attempt.name.trim()
    || Array.from(attempt.name).length > 80 || /[\u0000-\u001f\u007f]/.test(attempt.name)
    || !Array.isArray(attempt.baselineIds) || !attempt.baselineIds.every(isId)
    || new Set(attempt.baselineIds).size !== attempt.baselineIds.length
    || (value.entityId !== undefined && !isId(value.entityId))
    || (value.recoveryEntityId !== undefined && !isId(value.recoveryEntityId))) throw invalid();
  // Copy just the validated data; no caller-owned object remains in the record.
  return {
    version: 1,
    profile: { ...value.profile },
    attempt: {
      attemptId: attempt.attemptId,
      hiveUrl: attempt.hiveUrl,
      name: attempt.name,
      baselineIds: [...attempt.baselineIds],
    },
    ...(value.entityId === undefined ? {} : { entityId: value.entityId as string }),
    ...(value.recoveryEntityId === undefined ? {} : { recoveryEntityId: value.recoveryEntityId as string }),
  };
}

export function readCreationDraft(): CreationDraft | null {
  const raw = sessionStorage.getItem(DRAFT_KEY);
  if (raw === null) return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error("The unfinished creation record could not be read. It has been kept unchanged; resolve it before creating another Entity.");
  }
  return decode(value);
}

export function writeCreationDraft(draft: CreationDraft): void {
  const next = decode(draft);
  const current = readCreationDraft();
  if (current) {
    if (current.attempt.attemptId !== next.attempt.attemptId) {
      throw new Error("Another creation is unfinished in this tab. Resolve it before creating another Entity.");
    }
    if (current.attempt.hiveUrl !== next.attempt.hiveUrl || current.attempt.name !== next.attempt.name
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
  }
  // Web Storage writes are atomic: a thrown quota/access error leaves the
  // previous entry intact. Never remove the entry before a successful write.
  sessionStorage.setItem(DRAFT_KEY, JSON.stringify(next));
  window.dispatchEvent(new Event(CREATION_DRAFT_EVENT));
}

export function confirmCreationDraft(attemptId: string, id: string): CreationDraft | null {
  if (!isId(attemptId) || !isId(id)) throw new Error("Creation confirmation requires valid attempt and Entity IDs.");
  const current = readCreationDraft();
  if (!current || current.attempt.attemptId !== attemptId) return null;
  const confirmed = { ...current, entityId: id };
  writeCreationDraft(confirmed);
  return confirmed;
}

export function clearCreationDraft(attemptId: string): void {
  if (!isId(attemptId)) throw new Error("Clearing a creation requires a valid attempt ID.");
  const current = readCreationDraft();
  if (current?.attempt.attemptId === attemptId) {
    sessionStorage.removeItem(DRAFT_KEY);
    window.dispatchEvent(new Event(CREATION_DRAFT_EVENT));
  }
}
