import { isEntityProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { resolveHiveUrl } from "./connection";
import { createEntity, type EntityInfo } from "./daemonClient";

const PREVIEW_KEY = "abigail.first-signal.preview.v1";
export interface SavedPresence { id: string; profile: EntityProfile }
export function previewProfiles(): SavedPresence[] {
  const raw = localStorage.getItem(PREVIEW_KEY);
  if (!raw) return [];
  const value: unknown = JSON.parse(raw);
  if (!Array.isArray(value) || value.some(item => !item || typeof item.id !== "string" || !isEntityProfile(item.profile))) throw new Error("Preview storage could not be read. Export any open brief before clearing this site's storage.");
  return value as SavedPresence[];
}
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${await resolveHiveUrl()}${path}`, init);
  const envelope = await response.json();
  if (!response.ok || !envelope.ok || envelope.data === undefined) throw new Error(envelope.error || `Hive request failed (${response.status}).`);
  return envelope.data as T;
}
export async function loadPresence(id: string, preview: boolean): Promise<EntityProfile> {
  if (preview) {
    const found = previewProfiles().find(item => item.id === id);
    if (!found) throw new Error("This preview identity is no longer saved.");
    return found.profile;
  }
  const data = await request<unknown>(`/v1/entities/${encodeURIComponent(id)}/presence`);
  if (!isEntityProfile(data)) throw new Error("Hive returned an invalid visual profile.");
  return data;
}
export async function createPresenceEntity(name: string, preview: boolean): Promise<string> {
  return preview ? crypto.randomUUID() : (await createEntity(name)).id;
}
export async function savePresence(id: string, profile: EntityProfile, preview: boolean): Promise<void> {
  if (preview) {
    const rows = previewProfiles().filter(item => item.id !== id);
    localStorage.setItem(PREVIEW_KEY, JSON.stringify([...rows, { id, profile }]));
    return;
  }
  await request(`/v1/entities/${encodeURIComponent(id)}/presence`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(profile) });
}
export async function openPresence(id: string): Promise<string> {
  const data = await request<{ local_url: string }>(`/v1/entities/${encodeURIComponent(id)}/open`, { method: "POST" });
  if (!data.local_url) throw new Error("Hive did not return an Entity address.");
  return data.local_url;
}
export function previewEntities(): EntityInfo[] {
  return previewProfiles().map(({ id, profile }) => ({ id, name: profile.name, birth_complete: false, is_hive: false, immortal: false }));
}
