import { resolveHiveUrl } from "./connection";

const CREATION_TIMEOUT_MS = 30_000;
const CANDIDATE_TIMEOUT_MS = 5_000;

export interface CreationAttempt {
  readonly attemptId: string;
  readonly hiveUrl: string;
  readonly name: string;
  readonly baselineIds: readonly string[];
}

export interface CreationCandidate { id: string; name: string }

export class CreationError extends Error {
  constructor(message: string, public readonly outcome: "not-sent" | "unknown", public readonly attempt?: CreationAttempt) {
    super(message);
    this.name = "CreationError";
  }
}

export interface CreationOptions {
  signal?: AbortSignal;
  onDispatched?: (attempt: CreationAttempt) => void;
  onConfirmed?: (id: string, attempt?: CreationAttempt) => void;
}

function validId(value: unknown): value is string {
  return typeof value === "string" && !!value.trim() && value === value.trim() && !/[\u0000-\u001f\u007f]/.test(value);
}

function validName(value: unknown): value is string {
  return typeof value === "string" && !!value.trim() && value === value.trim()
    && Array.from(value).length <= 80 && !/[\u0000-\u001f\u007f]/.test(value);
}

function pinnedHiveUrl(value: string): string {
  const url = new URL(value);
  if (!/^https?:$/.test(url.protocol) || url.search || url.hash || value !== value.trim()) {
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
  checkActive(signal);
  const response = await fetch(`${hiveUrl}/v1/entities`, { headers: { Accept: "application/json" }, signal });
  const data = await readEnvelope(response);
  if (!Array.isArray(data) || data.some(item => !item || !validId(item.id) || typeof item.name !== "string" || typeof item.is_hive !== "boolean")) {
    throw new Error("Hive returned an invalid Entity list. Creation could not be checked.");
  }
  if (new Set(data.map(item => item.id)).size !== data.length) throw new Error("Hive returned duplicate Entity IDs. Creation could not be checked.");
  return data;
}

function confirm(options: CreationOptions, id: string, attempt?: CreationAttempt) {
  // Confirmation is transport evidence only. A late observer must not launch a
  // save or reveal, and an observer exception must not reject an already known ID.
  try { options.onConfirmed?.(id, attempt); } catch { /* Caller owns its journal/UI error reporting. */ }
}

export async function createPresenceEntity(name: string, preview: boolean, options: CreationOptions = {}): Promise<string> {
  let dispatched = false;
  let attempt: CreationAttempt | undefined;
  try {
    if (!validName(name)) throw new Error("Choose an Entity name of up to 80 characters without control characters or surrounding spaces.");
    if (options.signal?.aborted) throw new DOMException("Creation request was cancelled.", "AbortError");
    if (preview) {
      const id = crypto.randomUUID();
      confirm(options, id);
      return id;
    }
    return await bounded(async signal => {
      const hiveUrl = pinnedHiveUrl(await resolveHiveUrl());
      checkActive(signal);
      const baseline = await readEntities(hiveUrl, signal);
      checkActive(signal);
      attempt = Object.freeze({ attemptId: crypto.randomUUID(), hiveUrl, name, baselineIds: Object.freeze(baseline.map(item => item.id)) });
      // Persist the attempt before sending. If that fails or causes cancellation,
      // POST is never invoked and the caller can safely report "not sent".
      options.onDispatched?.(attempt);
      checkActive(signal);
      dispatched = true;
      const response = await fetch(`${hiveUrl}/v1/entities`, {
        method: "POST", headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ name }), signal,
      });
      // Deliberately inspect late responses after timeout/cancellation. A valid ID
      // can help the caller recover this exact attempt, without repeating POST.
      const data = await readEnvelope(response);
      const id = data && typeof data === "object" ? (data as Record<string, unknown>).id : undefined;
      if (!validId(id)) throw new Error("Hive did not return a valid Entity ID.");
      if (attempt.baselineIds.includes(id)) throw new Error("Hive returned an Entity ID that existed before this creation attempt.");
      confirm(options, id, attempt);
      return id;
    }, CREATION_TIMEOUT_MS, options.signal);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new CreationError(detail, dispatched ? "unknown" : "not-sent", dispatched ? attempt : undefined);
  }
}

export async function listCreationCandidates(attempt: CreationAttempt, signal?: AbortSignal): Promise<CreationCandidate[]> {
  return bounded(async requestSignal => {
    const hiveUrl = pinnedHiveUrl(attempt.hiveUrl);
    const entities = await readEntities(hiveUrl, requestSignal);
    const baseline = new Set(attempt.baselineIds);
    // A matching new ID is only a candidate, never proof that this POST made it.
    return entities.filter(item => !item.is_hive && item.name === attempt.name && !baseline.has(item.id))
      .map(({ id, name }) => ({ id, name }));
  }, CANDIDATE_TIMEOUT_MS, signal);
}
