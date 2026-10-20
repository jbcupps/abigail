import { resolveGenesisToken, resolveHiveUrl } from "./connection";
import { hiveRequestHeaders, isNativeConnection, nativeHiveConnection } from "../../../../entity-runtime-app/src-ui/src/lib/nativeConnection";

export interface GenesisConnection { hiveUrl: string; profileId: string | null }
export interface GenesisRequestOptions { connection?: GenesisConnection; signal?: AbortSignal }
export interface GenesisSeed { mode: GenesisMode; name: string; purpose: string }

export type GenesisMode = "guided" | "voice" | "markdown" | "hybrid" | "quick" | "deep";
export type GenesisState = "draft" | "discovering" | "review" | "portrait_pending" | "portrait_approved" | "manifesting" | "ready" | "failed" | "cancelled";
export interface Personality {
  description: string;
  empathy?: number; curiosity?: number; assertiveness?: number; humor?: number;
  creativity?: number; analytical_reasoning?: number; emotional_expressiveness?: number;
  independence?: number; sociability?: number;
  fears: string[]; aspirations: string[]; communication_style: string; response_patterns: string[];
}
export interface EntityDefinition {
  schema_version: number;
  entity_id?: string | null; owner_id: string; name: string; status: string;
  identity: string; personality: Personality; values: string[]; purpose: string;
  boundaries: string[]; appearance: string; visual_style: string; voice_profile: string;
  lore: string[]; relationships: string[]; fictional_memories: string[]; capabilities: string[];
  permissions: Record<string, unknown>; runtime_configuration: Record<string, unknown>;
  portrait_asset_id?: string | null; manifestation_asset_id?: string | null;
  created_at: string; updated_at: string; revision: number;
}
export type EditableField = "name" | "identity" | "personality" | "values" | "purpose" | "boundaries" | "appearance" | "visual_style" | "voice_profile" | "lore" | "relationships" | "fictional_memories" | "capabilities";
export interface FieldUpdate { field: EditableField; value: unknown; source_ref?: string }
export interface GenesisProposal { id: string; field: EditableField; value: unknown; rationale: string; source_ref?: string | null; status: "pending" | "accepted" | "rejected" }
export interface GenesisConflict { id: string; field: string; confirmed_value: unknown; proposed_value: unknown; proposal_id: string; resolved: boolean }
export interface GenesisSource { id: string; filename: string; content: string; created_at: string; committed: boolean; unrecognized_sections: string[] }
export interface GenesisMessage { id: string; role: string; content: string; created_at: string }
export interface GenesisRevision { revision: number; reason: string; created_at: string; definition: EntityDefinition }
export interface EssenceProgress { identity: number; personality: number; purpose: number; memories_lore: number; appearance: number; voice_presence: number }
export interface GenesisSession {
  /** Client-side origin binding. Never part of the canonical definition. */
  connection?: GenesisConnection;
  id: string; owner_id: string; mode: GenesisMode; state: GenesisState;
  definition: EntityDefinition; messages: GenesisMessage[]; proposals: GenesisProposal[];
  conflicts: GenesisConflict[]; sources: GenesisSource[]; revisions: GenesisRevision[];
  asset_ids: string[]; job_ids: string[]; revision: number; created_at: string; updated_at: string;
  composer_draft: string; last_error?: string | null;
  essence?: EssenceProgress; awakening?: string | null; choices?: string[];
  retention_days?: number | null;
}

export const GENESIS_MODES: { value: GenesisMode; label: string; detail: string }[] = [
  { value: "guided", label: "Guided conversation", detail: "Discover one meaningful choice at a time." },
  { value: "quick", label: "Quick Genesis", detail: "Begin with a spark. Review a complete proposal." },
  { value: "deep", label: "Deep Genesis", detail: "Explore motives, boundaries, lore, and presence." },
  { value: "voice", label: "Voice", detail: "Speak when your browser supports it; review your words." },
  { value: "markdown", label: "Markdown", detail: "Bring a biography, character sheet, or previous export." },
  { value: "hybrid", label: "Hybrid", detail: "Move freely between documents, speech, and text." },
];

export async function genesisRequest<T>(path: string, body?: unknown, method = body === undefined ? "GET" : "POST", options: GenesisRequestOptions = {}): Promise<T> {
  const signal = options.signal ? AbortSignal.any([options.signal, AbortSignal.timeout(120_000)]) : AbortSignal.timeout(120_000);
  signal.throwIfAborted();
  const { base, token, connection } = await resolveGenesisConnection(options.connection);
  const headers = await hiveRequestHeaders(base, { Accept: "application/json", Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, signal);
  signal.throwIfAborted();
  const response = await fetch(`${base}/v1/genesis${path}`, {
    method,
    headers, redirect: "error",
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal,
  });
  const envelope = await response.json() as { ok: boolean; data?: T; error?: string };
  if (!response.ok || !envelope.ok || envelope.data === undefined) throw new Error(envelope.error || `Genesis request failed (${response.status}).`);
  const bind = (value: unknown) => value && typeof value === "object" && "definition" in value && "id" in value ? { ...value, connection } : value;
  return (Array.isArray(envelope.data) ? envelope.data.map(bind) : bind(envelope.data)) as T;
}

export async function resolveGenesisConnection(expected?: GenesisConnection): Promise<{ base: string; token: string; connection: GenesisConnection }> {
  const base = await resolveHiveUrl();
  const url = new URL(base);
  if (url.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)
      || url.username || url.password || url.search || url.hash || (url.pathname !== "/" && url.pathname !== "")) {
    throw new Error("Genesis requires a local Hive connection. Use the Abigail app or a local development Hive.");
  }
  const owner = isNativeConnection() ? (await nativeHiveConnection(url.origin)).process_identity : null;
  const connection = { hiveUrl: url.origin, profileId: owner?.profile_id ?? null };
  // A verified preview profile may restart on a new loopback port. Its durable
  // profile ID, not a dead process's address, owns the creation receipt.
  const sameNativeProfile = isNativeConnection() && expected?.profileId != null && expected.profileId === connection.profileId;
  if (expected && (expected.profileId !== connection.profileId || (expected.hiveUrl !== connection.hiveUrl && !sameNativeProfile))) throw new Error("This creation belongs to another Hive connection. Reopen its original Hive to resume; no request was sent.");
  const token = await resolveGenesisToken();
  if (!token) throw new Error("Genesis requires an authenticated Hive connection. Reopen Abigail or configure the development Genesis token.");
  return { base: url.origin, token, connection };
}

export const listGenesisSessions = () => genesisRequest<GenesisSession[]>("/sessions");
export const createGenesisSession = (mode: GenesisMode) => genesisRequest<GenesisSession>("/sessions", { mode });
export const createOrResumeGenesis = (key: string, seed: GenesisSeed, options: GenesisRequestOptions) => genesisRequest<GenesisSession>("/sessions", { mode: seed.mode, creation_key: key, seed }, "POST", options);
export const getGenesisSession = (id: string, connection?: GenesisConnection) => genesisRequest<GenesisSession>(`/sessions/${encodeURIComponent(id)}`, undefined, "GET", { connection });
export const deleteGenesisSession = (id: string) => genesisRequest<unknown>(`/sessions/${encodeURIComponent(id)}`, undefined, "DELETE");
export const genesisAction = (session: GenesisSession, action: string, args: Record<string, unknown> = {}, signal?: AbortSignal) => genesisRequest<GenesisSession>(`/sessions/${encodeURIComponent(session.id)}/actions`, { action, expected_revision: session.revision, ...args }, "POST", { connection: session.connection, signal });
export const sendGenesisMessage = (session: GenesisSession, text: string) => genesisRequest<GenesisSession>(`/sessions/${encodeURIComponent(session.id)}/messages`, { text, expected_revision: session.revision }, "POST", { connection: session.connection });
export const importGenesisMarkdown = (session: GenesisSession, filename: string, content: string) => genesisRequest<GenesisSession>(`/sessions/${encodeURIComponent(session.id)}/import`, { filename, content, expected_revision: session.revision }, "POST", { connection: session.connection });
export const exportGenesisMarkdown = (id: string) => genesisRequest<string>(`/sessions/${encodeURIComponent(id)}/export`);

export function essenceFor(definition: EntityDefinition): EssenceProgress {
  const present = (values: unknown[]) => Math.floor(values.filter((value) => Array.isArray(value) ? value.length > 0 : typeof value === "string" ? value.trim().length > 0 : value === false ? false : value != null).length / values.length * 100);
  const scores = ["empathy", "curiosity", "assertiveness", "humor", "creativity", "analytical_reasoning", "emotional_expressiveness", "independence", "sociability"] as const;
  return {
    identity: present([definition.name, definition.identity]),
    personality: present([definition.personality.description, scores.some((score) => definition.personality[score] != null), definition.values]),
    purpose: present([definition.purpose, definition.boundaries]),
    memories_lore: present([definition.lore, definition.fictional_memories, definition.relationships]),
    appearance: present([definition.appearance, definition.visual_style]),
    voice_presence: present([definition.voice_profile, definition.personality.communication_style]),
  };
}

export function displayGenesisValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(String).join("; ");
  return JSON.stringify(value, null, 2);
}
