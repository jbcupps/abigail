import { resolveHiveUrl } from "./connection";
import { hiveRequestHeaders, isNativeConnection } from "../../../../entity-runtime-app/src-ui/src/lib/nativeConnection";

const CREATION_TIMEOUT_MS = 30_000;
const CANDIDATE_TIMEOUT_MS = 5_000;

export interface CreationAttempt {
  readonly attemptId: string;
  readonly hiveUrl: string;
  readonly name: string;
  readonly baselineIds: readonly string[];
  /** Present only for the replay-safe protocol. Legacy requests cannot acquire a key later. */
  readonly creationKey?: string;
}

export type CreationLookup =
  | { creation_key: string; state: "absent" }
  | { creation_key: string; state: "reserved"; name: string }
  | { creation_key: string; state: "complete"; name: string; id: string; directory: string }
  | { creation_key: string; state: "unavailable"; name: string; id: string };
export const CREATION_LOCK = "abigail:first-signal:creation";
export interface CreationLease { readonly token: symbol }
const leases = new WeakSet<CreationLease>();
export function holdsCreationLock(lease?: CreationLease): lease is CreationLease { return !!lease && leases.has(lease); }
export async function withCreationLock<T>(work: (lease: CreationLease) => Promise<T> | T): Promise<T> {
  if (!navigator.locks?.request) throw new Error("This browser cannot safely coordinate creation across tabs. Use the current Abigail app or a browser with Web Locks, then check the saved creation again.");
  return navigator.locks.request(CREATION_LOCK, { ifAvailable: true }, async lock => {
    if (!lock) throw new Error("Another tab is editing, checking, or saving an Entity. Stop waiting there, or return here when it finishes.");
    const lease = Object.freeze({ token: Symbol("creation-lock") });
    leases.add(lease);
    try { return await work(lease); } finally { leases.delete(lease); }
  });
}
export const isCanonicalUuid = (value: unknown): value is string => typeof value === "string"
  && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);

export interface CreationCandidate { id: string; name: string }

export class CreationError extends Error {
  constructor(message: string, public readonly outcome: "not-sent" | "unknown", public readonly attempt?: CreationAttempt) {
    super(message);
    this.name = "CreationError";
  }
}

export interface CreationOptions {
  signal?: AbortSignal;
  attempt?: CreationAttempt;
  lease?: CreationLease;
  onDispatched?: (attempt: CreationAttempt) => void | Promise<void>;
  onConfirmed?: (id: string, attempt?: CreationAttempt) => void | Promise<void>;
}

function validId(value: unknown): value is string {
  return typeof value === "string" && !!value.trim() && value === value.trim() && !/[\u0000-\u001f\u007f]/.test(value);
}

function validName(value: unknown): value is string {
  return typeof value === "string" && !!value.trim() && value === value.trim()
    && Array.from(value).length <= 80 && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
}

function pinnedHiveUrl(value: string): string {
  const url = new URL(value);
  if (!/^https?:$/.test(url.protocol) || url.username || url.password || url.search || url.hash || value !== value.trim()) {
    throw new Error("Hive did not provide a usable address.");
  }
  return value.replace(/\/+$/, "");
}

function checkActive(signal: AbortSignal) {
  if (signal.aborted) throw signal.reason;
}

async function bounded<T>(work: (signal: AbortSignal) => Promise<T>, timeoutMs: number, signal?: AbortSignal): Promise<T> {
  if (signal?.aborted) throw new DOMException("Creation request was cancelled.", "AbortError");
  const controller = new AbortController();
  const cancel = () => controller.abort(new DOMException("Creation request was cancelled.", "AbortError"));
  signal?.addEventListener("abort", cancel, { once: true });
  const timeout = setTimeout(() => controller.abort(new DOMException("Creation request timed out.", "TimeoutError")), timeoutMs);
  const interrupted = new Promise<never>((_, reject) => {
    controller.signal.addEventListener("abort", () => reject(controller.signal.reason), { once: true });
  });
  try {
    return await Promise.race([work(controller.signal), interrupted]);
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", cancel);
  }
}

async function readEnvelope(response: Response): Promise<unknown> {
  const value: unknown = await response.json();
  const envelope = value && typeof value === "object" ? value as Record<string, unknown> : null;
  if (!response.ok || envelope?.ok !== true || envelope.data === undefined) {
    throw new Error(typeof envelope?.error === "string" ? envelope.error : `Hive request failed (${response.status}).`);
  }
  return envelope.data;
}

async function readEntities(hiveUrl: string, signal: AbortSignal): Promise<(CreationCandidate & { is_hive: boolean })[]> {
  const headers = await hiveRequestHeaders(hiveUrl, { Accept: "application/json" }, signal);
  checkActive(signal);
  const response = await fetch(`${hiveUrl}/v1/entities`, { headers, signal, redirect: isNativeConnection() ? "error" : undefined });
  const data = await readEnvelope(response);
  if (!Array.isArray(data) || data.some(item => !item || !validId(item.id) || typeof item.name !== "string" || typeof item.is_hive !== "boolean")) {
    throw new Error("Hive returned an invalid Entity list. Creation could not be checked.");
  }
  if (new Set(data.map(item => item.id)).size !== data.length) throw new Error("Hive returned duplicate Entity IDs. Creation could not be checked.");
  return data;
}

async function confirm(options: CreationOptions, id: string, attempt?: CreationAttempt) {
  // Confirmation is transport evidence only. A late observer must not launch a
  // save or reveal, and an observer exception must not reject an already known ID.
  try { await options.onConfirmed?.(id, attempt); } catch { /* Caller owns its journal/UI error reporting. */ }
}

async function lookup(attempt: CreationAttempt, signal: AbortSignal): Promise<CreationLookup> {
  if (!isCanonicalUuid(attempt.creationKey) || attempt.attemptId !== attempt.creationKey || !validName(attempt.name)) {
    throw new Error("This older creation has no replay-safe key. Review it without repeating creation.");
  }
  const base = pinnedHiveUrl(attempt.hiveUrl);
  const headers = await hiveRequestHeaders(base, { Accept: "application/json" }, signal);
  checkActive(signal);
  const response = await fetch(`${base}/v1/entity-creations/${attempt.creationKey}`, {
    headers, signal, redirect: isNativeConnection() ? "error" : undefined,
  });
  if (response.status === 404 || response.status === 405) throw new Error("This Hive does not support exact creation recovery yet. Update Hive before creating an Entity; no unkeyed request will be sent.");
  const raw = await readEnvelope(response);
  const value = raw && typeof raw === "object" && !Array.isArray(raw) ? raw as Record<string, unknown> : null;
  const allowed = value?.state === "absent" ? ["creation_key", "state"]
    : value?.state === "reserved" ? ["creation_key", "state", "name"]
    : value?.state === "complete" ? ["creation_key", "state", "name", "id", "directory"]
    : value?.state === "unavailable" ? ["creation_key", "state", "name", "id"] : [];
  if (!value || value.creation_key !== attempt.creationKey || !allowed.length
    || Object.keys(value).some(key => !allowed.includes(key))
    || allowed.some(key => !(key in value))
    || (value.state !== "absent" && value.name !== attempt.name)
    || (["complete", "unavailable"].includes(String(value.state)) && !isCanonicalUuid(value.id))
    || (value.state === "complete" && !validId(value.directory))) {
    throw new Error("Hive did not confirm the exact creation key and original name. Creation is paused; no unkeyed request will be sent.");
  }
  checkActive(signal);
  return value as unknown as CreationLookup;
}

export function lookupCreationAttempt(attempt: CreationAttempt, signal?: AbortSignal): Promise<CreationLookup> {
  return bounded(requestSignal => lookup(attempt, requestSignal), CANDIDATE_TIMEOUT_MS, signal);
}

export async function createPresenceEntity(name: string, preview: boolean, options: CreationOptions = {}): Promise<string> {
  let dispatched = false;
  let attempt: CreationAttempt | undefined;
  try {
    if (!validName(name)) throw new Error("Choose an Entity name of up to 80 characters without control characters or surrounding spaces.");
    if (options.signal?.aborted) throw new DOMException("Creation request was cancelled.", "AbortError");
    if (preview) {
      const id = crypto.randomUUID();
      await confirm(options, id);
      return id;
    }
    const run = () => bounded(async signal => {
      const hiveUrl = pinnedHiveUrl(options.attempt?.hiveUrl ?? await resolveHiveUrl());
      checkActive(signal);
      const creationKey = options.attempt?.creationKey ?? crypto.randomUUID();
      if (options.attempt && !options.attempt.creationKey) throw new Error("An older unkeyed creation cannot be safely retried. Check its existing results instead.");
      attempt = Object.freeze({ attemptId: options.attempt?.attemptId ?? creationKey, creationKey, hiveUrl,
        name: options.attempt?.name ?? name, baselineIds: Object.freeze([...(options.attempt?.baselineIds ?? [])]) });
      // Prove protocol support before POST: older daemons may silently ignore a
      // creation_key on POST. A list or a matching name is never proof.
      const state = await lookup(attempt, signal);
      checkActive(signal);
      if (!options.attempt && state.state !== "absent") throw new Error("This creation key already exists. No new request was sent.");
      if (state.state === "unavailable") throw new Error(`The Entity for this creation is unavailable (${state.id}). Its key is retained; creating a replacement requires a separate decision.`);
      if (state.state === "complete") {
        await confirm(options, state.id, attempt);
        return state.id;
      }
      // Persist the attempt before sending. If that fails or causes cancellation,
      // POST is never invoked and the caller can safely report "not sent".
      const headers = await hiveRequestHeaders(hiveUrl, { "Content-Type": "application/json", Accept: "application/json" }, signal);
      checkActive(signal);
      await options.onDispatched?.(attempt);
      checkActive(signal);
      dispatched = true;
      const response = await fetch(`${hiveUrl}/v1/entities`, {
        method: "POST", headers, redirect: isNativeConnection() ? "error" : undefined,
        body: JSON.stringify({ name: attempt.name, creation_key: attempt.creationKey }), signal,
      });
      // Deliberately inspect late responses after timeout/cancellation. A valid ID
      // can help the caller recover this exact attempt, without repeating POST.
      const data = await readEnvelope(response);
      const value = data && typeof data === "object" && !Array.isArray(data) ? data as Record<string, unknown> : null;
      const id = value?.id;
      if (!value || !isCanonicalUuid(id) || value.creation_key !== attempt.creationKey || !validId(value.directory)
        || Object.keys(value).some(key => !["id", "directory", "creation_key"].includes(key))) {
        throw new Error("Hive did not confirm the submitted creation key and Entity ID. Check this exact attempt before retrying.");
      }
      await confirm(options, id, attempt);
      return id;
      }, CREATION_TIMEOUT_MS, options.signal);
    return await (holdsCreationLock(options.lease) ? run() : withCreationLock(run));
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new CreationError(detail, dispatched ? "unknown" : "not-sent", dispatched ? attempt : undefined);
  }
}

export async function listCreationCandidates(attempt: CreationAttempt, signal?: AbortSignal): Promise<CreationCandidate[]> {
  if (attempt.creationKey) throw new Error("Use the exact creation key lookup for this attempt, not name candidates.");
  return bounded(async requestSignal => {
    const hiveUrl = pinnedHiveUrl(attempt.hiveUrl);
    const entities = await readEntities(hiveUrl, requestSignal);
    const baseline = new Set(attempt.baselineIds);
    // A matching new ID is only a candidate, never proof that this POST made it.
    return entities.filter(item => !item.is_hive && item.name === attempt.name && !baseline.has(item.id))
      .map(({ id, name }) => ({ id, name }));
  }, CANDIDATE_TIMEOUT_MS, signal);
}
