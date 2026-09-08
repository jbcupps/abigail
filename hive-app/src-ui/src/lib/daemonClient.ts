import { restartHiveConnection, resolveHiveConnection } from "./connection";

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
  const { hiveUrl, authToken } = await resolveHiveConnection();
  const headers = new Headers(init?.headers ?? undefined);
  if (authToken && !headers.has("Authorization")) {
    headers.set("Authorization", `Bearer ${authToken}`);
  }
  return fetch(`${hiveUrl}${path}`, { ...init, headers });
}

async function unwrap<T>(res: Response, path: string): Promise<T> {
  if (!res.ok)
    throw new Error(`Request failed (HTTP ${res.status}). Please retry.`);
  const envelope = (await res.json()) as ApiEnvelope<T>;
  if (!envelope.ok)
    throw new Error(envelope.error ?? `Request to ${path} failed`);
  if (envelope.data === undefined)
    throw new Error(`Response from ${path} missing data`);
  return envelope.data;
}

export async function hiveHealth(): Promise<boolean> {
  try {
    return (await hiveFetch("/health")).ok;
  } catch {
    return false;
  }
}

export async function listEntities(): Promise<EntityInfo[]> {
  const res = await hiveFetch("/v1/entities", {
    headers: { Accept: "application/json" },
  });
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

async function hivePost<T>(path: string, body: unknown): Promise<T> {
  const res = await hiveFetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(body),
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
  const res = await hiveFetch("/v1/status", {
    headers: { Accept: "application/json" },
  });
  return unwrap<HiveStatus>(res, "/v1/status");
}

export interface BestModel {
  provider?: string | null;
  model?: string | null;
  tier?: string | null;
  reason?: string | null;
}

export async function getBestModel(): Promise<BestModel> {
  const res = await hiveFetch("/v1/providers/best", {
    headers: { Accept: "application/json" },
  });
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
  const res = await hiveFetch("/v1/providers/detect", {
    headers: { Accept: "application/json" },
  });
  const data = await unwrap<{ providers: CliProviderDetection[] }>(
    res,
    "/v1/providers/detect",
  );
  return data.providers;
}

export interface ProviderModel {
  model_id: string;
  display_name?: string | null;
}

export async function discoverModels(
  provider: string,
  apiKey: string,
): Promise<ProviderModel[]> {
  const data = await hivePost<{ provider: string; models: ProviderModel[] }>(
    "/v1/providers/models",
    { provider, api_key: apiKey },
  );
  return data.models;
}

export async function storeSecret(key: string, value: string): Promise<void> {
  await hivePost<string>("/v1/secrets", { key, value });
}

export interface HiveDefault {
  provider?: string | null;
  model?: string | null;
}

export async function setHiveDefault(
  provider?: string,
  model?: string,
): Promise<HiveDefault> {
  return hivePost<HiveDefault>("/v1/providers/hive-default", {
    provider,
    model,
  });
}

export interface SetupStatus {
  phase: string;
  message: string;
  model: string;
  active_provider: string;
  active_model: string;
}
export interface SetupMessage {
  role: string;
  content: string;
}
export async function getSetupStatus(): Promise<SetupStatus> {
  return unwrap(await hiveFetch("/v1/setup"), "/v1/setup");
}
export const retrySetup = async () => {
  await restartHiveConnection();
  return hivePost<SetupStatus>("/v1/setup/retry", {});
};
export const cancelSetup = () => hivePost<SetupStatus>("/v1/setup/cancel", {});
export const useLocalModel = () => hivePost<SetupStatus>("/v1/setup/local", {});
export const activateSetupModel = (
  provider: string,
  model: string,
  apiKey: string,
) =>
  hivePost<SetupStatus>("/v1/setup/activate", {
    provider,
    model,
    api_key: apiKey,
  });
export async function setupHistory(): Promise<{ messages: SetupMessage[] }> {
  return unwrap(await hiveFetch("/v1/setup/chat"), "/v1/setup/chat");
}
export const sendSetupMessage = (message: string) =>
  hivePost<{ messages: SetupMessage[] }>("/v1/setup/chat", { message });
