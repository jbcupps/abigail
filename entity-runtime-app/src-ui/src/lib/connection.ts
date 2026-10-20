import { detectRuntimeMode } from "../runtimeMode";
import { nativeRuntimeConnection } from "./nativeConnection";

// Resolve the Entity Runtime daemon base URL. In a packaged (native) build the
// Rust shell knows the real URL from the ABIGAIL_ENTITY_URL env var (set per
// entity when the Hive opens it) and exposes it via `get_runtime_connection_info`.
// In the browser dev/harness path we fall back to a query param, a Vite env var,
// then the standard local port.
const DEFAULT_ENTITY_URL = "http://127.0.0.1:43142";

let cached: string | null = null;

function fromQueryOrEnv(): string | null {
  const param = new URLSearchParams(window.location.search).get("entityUrl");
  if (param) return param;
  const env = import.meta.env.VITE_ENTITY_DAEMON_URL as string | undefined;
  return env ?? null;
}

export async function resolveEntityUrl(): Promise<string> {
  if (detectRuntimeMode() === "native") return (await nativeRuntimeConnection()).runtime_url;
  if (cached) return cached;
  let url = fromQueryOrEnv();
  if (!url) url = DEFAULT_ENTITY_URL;
  cached = url.replace(/\/+$/, "");
  return cached;
}
