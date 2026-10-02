import { resolveHiveUrl } from "./connection";

// Thin typed wrapper over the Hive daemon HTTP API. Every JSON route returns the
// universal `{ ok, data?, error? }` envelope; `/health` returns plain text.
export interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

export interface EntityInfo {
  id: string;
  name: string;
  birth_complete: boolean;
  birth_date?: string | null;
  is_hive: boolean;
  immortal: boolean;
}

export interface CreateEntityResult {
  id: string;
  directory: string;
}

async function hiveFetch(path: string, init?: RequestInit): Promise<Response> {
  const base = await resolveHiveUrl();
  return fetch(`${base}${path}`, { ...init, signal: init?.signal ?? AbortSignal.timeout(30_000) });
}

async function unwrap<T>(res: Response, path: string): Promise<T> {
  const envelope = (await res.json()) as ApiEnvelope<T>;
  if (!envelope.ok) throw new Error(envelope.error ?? `Request to ${path} failed`);
  if (envelope.data === undefined) throw new Error(`Response from ${path} missing data`);
  return envelope.data;
}

export async function hiveHealth(): Promise<boolean> {
  try {
    return (await hiveFetch("/health", { signal: AbortSignal.timeout(2_000) })).ok;
  } catch {
    return false;
  }
}

export async function listEntities(): Promise<EntityInfo[]> {
  const res = await hiveFetch("/v1/entities", { headers: { Accept: "application/json" } });
  return unwrap<EntityInfo[]>(res, "/v1/entities");
}

export async function createEntity(name: string): Promise<CreateEntityResult> {
  const res = await hiveFetch("/v1/entities", {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ name }),
  });
  return unwrap<CreateEntityResult>(res, "/v1/entities");
}

export async function completeEntitySetup(entityId: string, purpose?: string): Promise<void> {
  await hivePost(`/v1/entities/${encodeURIComponent(entityId)}/birth`, {
    path: "quickstart",
    choices: [],
    purpose: purpose?.trim() || "A helpful, honest companion for everyday family life.",
  });
}

async function hivePost<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  const res = await hiveFetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(body),
    signal,
  });
  return unwrap<T>(res, path);
}

export interface HelperInfo {
  running: boolean;
  local_url?: string | null;
}

export interface HiveStatus {
  entity_count: number;
  entities: EntityInfo[];
  ready_state: string; // "needs_provider" | "ready"
  any_provider_configured: boolean;
  setup_complete: boolean;
  helper?: HelperInfo | null;
}

export async function getStatus(): Promise<HiveStatus> {
  const res = await hiveFetch("/v1/status", { headers: { Accept: "application/json" }, signal: AbortSignal.timeout(5_000) });
  return unwrap<HiveStatus>(res, "/v1/status");
}

export interface BestModel {
  provider?: string | null;
  model?: string | null;
  tier?: string | null;
  reason?: string | null;
}

export async function getBestModel(): Promise<BestModel> {
  const res = await hiveFetch("/v1/providers/best", { headers: { Accept: "application/json" } });
  return unwrap<BestModel>(res, "/v1/providers/best");
}

export interface CliProviderDetection {
  provider: string;
  on_path: boolean;
  is_official: boolean;
  is_authenticated: boolean;
  auth_hint?: string | null;
}

export async function detectCliProviders(): Promise<CliProviderDetection[]> {
  const res = await hiveFetch("/v1/providers/detect", { headers: { Accept: "application/json" } });
  const data = await unwrap<{ providers: CliProviderDetection[] }>(res, "/v1/providers/detect");
  return data.providers;
}

export interface ProviderModel {
  id: string;
  display_name?: string | null;
}

export async function discoverModels(provider: string, apiKey: string): Promise<ProviderModel[]> {
  const data = await hivePost<{ provider: string; models: { model_id: string; display_name?: string }[] }>(
    "/v1/providers/models",
    { provider, api_key: apiKey },
  );
  return data.models.map((model) => ({ id: model.model_id, display_name: model.display_name }));
}

export async function storeSecret(key: string, value: string): Promise<void> {
  await hivePost<string>("/v1/secrets", { key, value });
}

export interface HiveDefault {
  provider?: string | null;
  model?: string | null;
}

export async function setHiveDefault(provider?: string, model?: string): Promise<HiveDefault> {
  return hivePost<HiveDefault>("/v1/providers/hive-default", { provider, model }, AbortSignal.timeout(75_000));
}

export async function connectLocalModel(baseUrl: string): Promise<{ base_url: string; model: string }> {
  return hivePost("/v1/providers/local", { base_url: baseUrl.trim() });
}
