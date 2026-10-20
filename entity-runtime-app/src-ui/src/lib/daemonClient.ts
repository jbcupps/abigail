import { resolveEntityUrl } from "./connection";
import { verifiedRuntimeFetch } from "./nativeConnection";

// Thin wrapper over the Entity Runtime daemon HTTP API. Chat itself goes through
// EntityHttpChatGateway (added in Phase 1); this client covers health/status.
export interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

async function entityFetch(path: string, init?: RequestInit): Promise<Response> {
  const base = await resolveEntityUrl();
  return verifiedRuntimeFetch()(`${base}${path}`, init);
}

export async function entityHealth(): Promise<boolean> {
  try {
    return (await entityFetch("/health")).ok;
  } catch {
    return false;
  }
}

async function getEntityJson(path: string, signal?: AbortSignal): Promise<unknown> {
  const res = await entityFetch(path, { headers: { Accept: "application/json" }, signal });
  if (!res.ok) throw new Error(`Entity status request failed (${res.status}).`);
  const json: unknown = await res.json();
  // Status may or may not be wrapped in the standard envelope; handle both.
  if (json && typeof json === "object" && "ok" in json) {
    const envelope = json as ApiEnvelope<unknown>;
    if (!envelope.ok) throw new Error(envelope.error ?? "Status request failed");
    return envelope.data ?? {};
  }
  return json;
}

export function getStatus(signal?: AbortSignal): Promise<unknown> {
  return getEntityJson("/v1/status", signal);
}

export function getSessionStatus(signal?: AbortSignal): Promise<unknown> {
  return getEntityJson("/v1/session/status", signal);
}
