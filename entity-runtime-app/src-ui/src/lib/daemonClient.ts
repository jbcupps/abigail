import { resolveEntityUrl } from "./connection";

// Thin wrapper over the Entity Runtime daemon HTTP API. Chat itself goes through
// EntityHttpChatGateway (added in Phase 1); this client covers health/status.
export interface ApiEnvelope<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

async function entityFetch(path: string, init?: RequestInit): Promise<Response> {
  const base = await resolveEntityUrl();
  return fetch(`${base}${path}`, init);
}

export async function entityHealth(): Promise<boolean> {
  try {
    return (await entityFetch("/health")).ok;
  } catch {
    return false;
  }
}

export async function getStatus(): Promise<unknown> {
  const res = await entityFetch("/v1/status", { headers: { Accept: "application/json" } });
  const json: unknown = await res.json();
  // Status may or may not be wrapped in the standard envelope; handle both.
  if (json && typeof json === "object" && "ok" in json) {
    const envelope = json as ApiEnvelope<unknown>;
    if (!envelope.ok) throw new Error(envelope.error ?? "Status request failed");
    return envelope.data ?? {};
  }
  return json;
}
