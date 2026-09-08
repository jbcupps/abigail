import { detectRuntimeMode } from "../runtimeMode";

// Resolve the Hive daemon base URL. In a packaged (native) build the Rust shell
// knows the real URL from the ABIGAIL_HIVE_URL env var and exposes it via the
// `get_hive_connection_info` command. In the browser dev/harness path we fall
// back to a query param, a Vite env var, then the standard local port.
const DEFAULT_HIVE_URL = "http://127.0.0.1:43141";

export interface HiveConnection {
  hiveUrl: string;
  /** Per-launch local control-plane token. Memory only — never localStorage. */
  authToken: string | null;
}

let cached: HiveConnection | null = null;

async function fromTauri(): Promise<HiveConnection | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const info = await invoke<{ hive_url: string; auth_token?: string | null }>(
      "get_hive_connection_info",
    );
    if (!info?.hive_url) return null;
    return {
      hiveUrl: info.hive_url,
      authToken: info.auth_token ?? null,
    };
  } catch {
    return null;
  }
}

function fromQueryOrEnv(): HiveConnection | null {
  const params = new URLSearchParams(window.location.search);
  const param = params.get("hiveUrl");

  const env = import.meta.env.VITE_HIVE_DAEMON_URL as string | undefined;
  const envToken = import.meta.env.VITE_HIVE_AUTH_TOKEN as string | undefined;
  const hiveUrl = param ?? env ?? null;
  if (!hiveUrl) return null;
  return {
    hiveUrl,
    authToken: envToken ?? null,
  };
}

export async function resolveHiveConnection(): Promise<HiveConnection> {
  if (cached) return cached;
  let conn: HiveConnection | null = null;
  if (detectRuntimeMode() === "native") {
    conn = await fromTauri();
    if (!conn?.authToken)
      throw new Error("Abigail is starting its secure connection…");
  }
  if (!conn) conn = fromQueryOrEnv();
  if (!conn) {
    conn = { hiveUrl: DEFAULT_HIVE_URL, authToken: null };
  }
  cached = {
    hiveUrl: conn.hiveUrl.replace(/\/+$/, ""),
    authToken: conn.authToken,
  };
  return cached;
}

export async function resolveHiveUrl(): Promise<string> {
  return (await resolveHiveConnection()).hiveUrl;
}

export async function restartHiveConnection(): Promise<void> {
  cached = null;
  if (detectRuntimeMode() === "native") {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("retry_hive_startup");
  }
}
