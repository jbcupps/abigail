import { detectRuntimeMode } from "../runtimeMode";

// Resolve the Entity Runtime daemon base URL. In a packaged (native) build the
// Rust shell knows the real URL from the ABIGAIL_ENTITY_URL env var (set per
// entity when the Hive opens it) and exposes it via `get_runtime_connection_info`.
// In the browser dev/harness path we fall back to a query param, a Vite env var,
// then the standard local port.
const DEFAULT_ENTITY_URL = "http://127.0.0.1:43142";

let cached: string | null = null;

async function fromTauri(): Promise<string | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const info = await invoke<{ runtime_url: string }>("get_runtime_connection_info");
    return info?.runtime_url ?? null;
  } catch {
    return null;
  }
}

function fromQueryOrEnv(): string | null {
  const param = new URLSearchParams(window.location.search).get("entityUrl");
  if (param) return param;
  const env = import.meta.env.VITE_ENTITY_DAEMON_URL as string | undefined;
  return env ?? null;
}

export async function resolveEntityUrl(): Promise<string> {
  if (cached) return cached;
  let url: string | null = null;
  if (detectRuntimeMode() === "native") {
    url = await fromTauri();
  }
  if (!url) url = fromQueryOrEnv();
  if (!url) url = DEFAULT_ENTITY_URL;
  cached = url.replace(/\/+$/, "");
  return cached;
}
