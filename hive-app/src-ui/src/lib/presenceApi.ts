import { isEntityProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { resolveHiveUrl } from "./connection";
import type { EntityInfo } from "./daemonClient";
export { createPresenceEntity, listCreationCandidates, CreationError, type CreationAttempt, type CreationCandidate, type CreationOptions } from "./presenceCreation";

const PREVIEW_KEY = "abigail.first-signal.preview.v1";
const REQUEST_TIMEOUT_MS = 5_000;
// Cold runtime startup can spend up to 45 seconds checking daemon health.
const OPEN_TIMEOUT_MS = 55_000;
export interface SavedPresence { id: string; profile: EntityProfile; hiveUrl?: string }
function isEntityId(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0 && value === value.trim();
}
export function previewProfiles(): SavedPresence[] {
  const raw = localStorage.getItem(PREVIEW_KEY);
  if (!raw) return [];
  const value: unknown = JSON.parse(raw);
  if (!Array.isArray(value) || value.some(item => !item || !isEntityId(item.id) || !isEntityProfile(item.profile))) throw new Error("Preview storage could not be read. Export any open brief before clearing this site's storage.");
  return value as SavedPresence[];
}
async function request(path: string, init?: RequestInit, signal?: AbortSignal, timeoutMs = REQUEST_TIMEOUT_MS, hiveUrl?: string): Promise<unknown> {
  if (signal?.aborted) throw new DOMException("Hive request was cancelled.", "AbortError");
  const controller = new AbortController();
  const cancel = () => controller.abort(new DOMException("Hive request was cancelled.", "AbortError"));
  signal?.addEventListener("abort", cancel, { once: true });
  const timeout = setTimeout(() => controller.abort(new DOMException(`Hive request timed out after ${timeoutMs / 1_000} seconds.`, "TimeoutError")), timeoutMs);
  const interrupted = new Promise<never>((_, reject) => {
    controller.signal.addEventListener("abort", () => reject(controller.signal.reason), { once: true });
  });
  try {
    const operation = (async () => {
      const base = hiveUrl ?? await resolveHiveUrl();
      if (controller.signal.aborted) throw controller.signal.reason;
      const response = await fetch(`${base}${path}`, { ...init, signal: controller.signal });
      const value: unknown = await response.json();
      const envelope = value && typeof value === "object" ? value as Record<string, unknown> : null;
      if (!response.ok || envelope?.ok !== true || envelope.data === undefined) {
        throw new Error(typeof envelope?.error === "string" ? envelope.error : `Hive request failed (${response.status}).`);
      }
      return envelope.data;
    })();
    // Include URL resolution and body parsing in the deadline. Racing also keeps
    // the UI responsive if a transport does not settle promptly after abort.
    return await Promise.race([operation, interrupted]);
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", cancel);
  }
}
export async function loadPresence(id: string, preview: boolean, signal?: AbortSignal, hiveUrl?: string): Promise<EntityProfile> {
  if (signal?.aborted) throw new DOMException("Hive request was cancelled.", "AbortError");
  if (preview) {
    const found = previewProfiles().find(item => item.id === id);
    if (!found) throw new Error("This preview identity is no longer saved.");
    return found.profile;
  }
  const data = await request(`/v1/entities/${encodeURIComponent(id)}/presence`, undefined, signal, REQUEST_TIMEOUT_MS, hiveUrl);
  if (!isEntityProfile(data)) throw new Error("Hive returned an invalid visual profile.");
  return data;
}
export async function savePresence(id: string, profile: EntityProfile, preview: boolean, options: { signal?: AbortSignal; hiveUrl?: string } = {}): Promise<void> {
  if (options.signal?.aborted) throw new DOMException("Hive request was cancelled.", "AbortError");
  if (!isEntityId(id) || !isEntityProfile(profile)) throw new Error("The visual profile is invalid and was not saved.");
  if (preview) {
    const rows = previewProfiles().filter(item => item.id !== id);
    localStorage.setItem(PREVIEW_KEY, JSON.stringify([...rows, { id, profile }]));
    return;
  }
  const confirmed = await request(`/v1/entities/${encodeURIComponent(id)}/presence`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(profile) }, options.signal, REQUEST_TIMEOUT_MS, options.hiveUrl);
  if (!isEntityProfile(confirmed) || confirmed.name !== profile.name || confirmed.purpose !== profile.purpose || confirmed.temperament !== profile.temperament || confirmed.boundaries !== profile.boundaries || confirmed.identity !== profile.identity) {
    throw new Error("Hive did not confirm the submitted visual profile. Saving could not be confirmed.");
  }
}
export async function openPresence(id: string, signal?: AbortSignal, hiveUrl?: string): Promise<string> {
  const data = await request(`/v1/entities/${encodeURIComponent(id)}/open`, { method: "POST" }, signal, OPEN_TIMEOUT_MS, hiveUrl);
  const address = data && typeof data === "object" ? (data as Record<string, unknown>).local_url : undefined;
  if (typeof address !== "string" || !address.trim() || address !== address.trim()) throw new Error("Hive did not return a usable Entity address.");
  try {
    const url = new URL(address);
    if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error();
  } catch {
    throw new Error("Hive did not return a usable Entity address.");
  }
  return address;
}
export function previewEntities(): EntityInfo[] {
  return previewProfiles().map(({ id, profile }) => ({ id, name: profile.name, birth_complete: false, is_hive: false, immortal: false }));
}
