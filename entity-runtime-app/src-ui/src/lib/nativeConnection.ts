// Native connection metadata is verified by the owning Rust shell against its
// pinned process instance. Browser previews deliberately do not use this bridge.
export interface ProcessIdentity {
  channel: string;
  profile_id: string;
  canonical_data_root: string;
  instance_id: string;
  hive_instance_id: string;
}

export interface HiveConnection {
  hive_url: string;
  process_identity?: ProcessIdentity | null;
}

export interface RuntimeConnection {
  runtime_url: string;
  hive_url?: string | null;
  process_identity?: ProcessIdentity | null;
  hive_process_identity?: ProcessIdentity | null;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const PROFILE_HEADER = "x-abigail-profile-id";
const INSTANCE_HEADER = "x-abigail-process-instance-id";
const VERIFICATION_TIMEOUT_MS = 10_000;

export function isNativeConnection(): boolean {
  return window.isTauri === true;
}

function origin(value: unknown): string {
  if (typeof value !== "string" || value !== value.trim()) throw new Error("Native connection did not provide a valid daemon address.");
  const url = new URL(value);
  if (!/^https?:$/.test(url.protocol) || url.username || url.password || url.search || url.hash || url.pathname !== "/") {
    throw new Error("Native connection did not provide a valid daemon address.");
  }
  return url.origin;
}

function identity(value: unknown): ProcessIdentity | undefined {
  if (value === undefined || value === null) return undefined;
  const candidate = value as Partial<ProcessIdentity>;
  if (!candidate || typeof candidate !== "object" || candidate.channel !== "beta"
    || typeof candidate.canonical_data_root !== "string" || !candidate.canonical_data_root
    || !UUID.test(candidate.profile_id ?? "") || !UUID.test(candidate.instance_id ?? "")
    || !UUID.test(candidate.hive_instance_id ?? "")) {
    throw new Error("Native connection returned invalid process ownership.");
  }
  return candidate as ProcessIdentity;
}

function checkActive(signal?: AbortSignal | null) {
  if (signal?.aborted) throw signal.reason ?? new DOMException("Request cancelled.", "AbortError");
}

async function invokeVerified<T>(command: string, args: Record<string, unknown>, signal?: AbortSignal | null, timeoutMs = VERIFICATION_TIMEOUT_MS): Promise<T> {
  checkActive(signal);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancel: (() => void) | undefined;
  let expired = false;
  const interrupted = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      expired = true;
      reject(new Error("Native connection operation timed out. Check its status before retrying."));
    }, timeoutMs);
    cancel = () => reject(signal?.reason ?? new DOMException("Request cancelled.", "AbortError"));
    signal?.addEventListener("abort", cancel, { once: true });
  });
  try {
    const operation = (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      checkActive(signal);
      if (expired) throw new Error("Native connection operation timed out before dispatch.");
      return invoke<T>(command, args);
    })();
    const value = await Promise.race([operation, interrupted]);
    checkActive(signal);
    return value;
  } catch (error) {
    checkActive(signal);
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`Native connection could not be verified. ${detail}`);
  } finally {
    clearTimeout(timer);
    if (cancel) signal?.removeEventListener("abort", cancel);
  }
}

export async function nativeHiveConnection(hiveUrl?: string, signal?: AbortSignal | null): Promise<HiveConnection> {
  const expected = hiveUrl === undefined ? undefined : origin(hiveUrl);
  let value: HiveConnection;
  try {
    value = await invokeVerified<HiveConnection>("get_hive_connection_info", expected ? { hiveUrl: expected } : {}, signal);
  } catch (error) {
    if (expected && !signal?.aborted) {
      throw new Error(`The saved Hive endpoint is stale or unavailable. Reopen the intended Hive; keep the existing key for any unfinished creation. ${error instanceof Error ? error.message : String(error)}`);
    }
    throw error;
  }
  const base = origin(value?.hive_url);
  if (expected && base !== expected) throw new Error("The saved Hive endpoint changed. Its creation key has been retained; no request was sent.");
  return { hive_url: base, process_identity: identity(value.process_identity) };
}

export async function nativeRuntimeConnection(runtimeUrl?: string, hiveUrl?: string, signal?: AbortSignal | null): Promise<RuntimeConnection> {
  const expected = runtimeUrl === undefined ? undefined : origin(runtimeUrl);
  const expectedHive = hiveUrl === undefined ? undefined : origin(hiveUrl);
  const value = await invokeVerified<RuntimeConnection>("get_runtime_connection_info", {
    ...(expected ? { runtimeUrl: expected } : {}),
    ...(expectedHive ? { hiveUrl: expectedHive } : {}),
  }, signal);
  const base = origin(value?.runtime_url);
  const hive = value.hive_url ? origin(value.hive_url) : undefined;
  if ((expected && base !== expected) || (expectedHive && hive !== expectedHive)) {
    throw new Error("The saved runtime endpoint changed. Reopen the selected Entity; no request was sent.");
  }
  return {
    runtime_url: base, hive_url: hive,
    process_identity: identity(value.process_identity),
    hive_process_identity: identity(value.hive_process_identity),
  };
}

export function ownershipHeaders(owner: ProcessIdentity | null | undefined, existing?: HeadersInit): Headers {
  const headers = new Headers(existing);
  // Never retain caller-supplied ownership when native metadata disagrees.
  headers.delete(PROFILE_HEADER);
  headers.delete(INSTANCE_HEADER);
  if (owner) {
    const verified = identity(owner)!;
    headers.set(PROFILE_HEADER, verified.profile_id);
    headers.set(INSTANCE_HEADER, verified.instance_id);
  }
  return headers;
}

export async function hiveRequestHeaders(hiveUrl: string, headers?: HeadersInit, signal?: AbortSignal | null): Promise<HeadersInit | undefined> {
  if (!isNativeConnection()) return headers;
  const connection = await nativeHiveConnection(hiveUrl, signal);
  return ownershipHeaders(connection.process_identity, headers);
}

export function verifiedRuntimeFetch(fetchFn: typeof fetch = globalThis.fetch.bind(globalThis)): typeof fetch {
  return async (input, init) => {
    if (!isNativeConnection()) return fetchFn(input, init);
    const request = typeof input === "string" || input instanceof URL ? null : input;
    const url = new URL(request ? request.url : String(input));
    const signal = init?.signal ?? request?.signal;
    checkActive(signal);
    const connection = await nativeRuntimeConnection(url.origin, undefined, signal);
    checkActive(signal);
    const original = new Headers(request?.headers);
    new Headers(init?.headers).forEach((value, key) => original.set(key, value));
    return fetchFn(input, { ...init, redirect: "error", headers: ownershipHeaders(connection.process_identity, original) });
  };
}

export async function nativeOpenEntity(entityId: string, hiveUrl?: string, signal?: AbortSignal): Promise<string> {
  const value = await invokeVerified<RuntimeConnection>("open_entity_connection", {
    entityId, ...(hiveUrl ? { hiveUrl: origin(hiveUrl) } : {}),
  }, signal, 55_000);
  const base = origin(value?.runtime_url);
  identity(value.process_identity);
  identity(value.hive_process_identity);
  return base;
}
