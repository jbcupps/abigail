import { detectRuntimeMode } from "../runtimeMode";

// Resolve the Hive daemon base URL. In a packaged (native) build the Rust shell
// knows the real URL from the ABIGAIL_HIVE_URL env var and exposes it via the
// `get_hive_connection_info` command. In the browser dev/harness path we fall
// back to a query param, a Vite env var, then the standard local port.
const DEFAULT_HIVE_URL = "http://127.0.0.1:43141";

let cached: string | null = null;

async function fromTauri(): Promise<string | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const info = await invoke<{ hive_url: string }>("get_hive_connection_info");
    return info?.hive_url ?? null;
  } catch {
    return null;
  }
}

function fromQueryOrEnv(): string | null {
  const param = new URLSearchParams(window.location.search).get("hiveUrl");
  if (param) return param;
  const env = import.meta.env.VITE_HIVE_DAEMON_URL as string | undefined;
  return env ?? null;
}

export async function resolveHiveUrl(): Promise<string> {
  if (cached) return cached;
  let url: string | null = null;
  if (detectRuntimeMode() === "native") {
    url = await fromTauri();
  }
  if (!url) url = fromQueryOrEnv();
  if (!url) url = DEFAULT_HIVE_URL;
  cached = url.replace(/\/+$/, "");
  return cached;
}
