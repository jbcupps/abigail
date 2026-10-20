import { detectRuntimeMode } from "../runtimeMode";
import { nativeHiveConnection } from "../../../../entity-runtime-app/src-ui/src/lib/nativeConnection";

// Resolve the Hive daemon base URL. In a packaged (native) build the Rust shell
// knows the real URL from the ABIGAIL_HIVE_URL env var and exposes it via the
// `get_hive_connection_info` command. In the browser dev/harness path we fall
// back to a query param, a Vite env var, then the standard local port.
const DEFAULT_HIVE_URL = "http://127.0.0.1:43141";

let cached: string | null = null;

function fromQueryOrEnv(): string | null {
  const param = new URLSearchParams(window.location.search).get("hiveUrl");
  if (param) return param;
  const env = import.meta.env.VITE_HIVE_DAEMON_URL as string | undefined;
  return env ?? null;
}

export async function resolveHiveUrl(): Promise<string> {
  if (detectRuntimeMode() === "native") return (await nativeHiveConnection()).hive_url;
  if (cached) return cached;
  let url = fromQueryOrEnv();
  if (!url) url = DEFAULT_HIVE_URL;
  cached = url.replace(/\/+$/, "");
  return cached;
}

// Desktop capability stays in memory, never in a media URL or browser storage.
export async function resolveGenesisToken(): Promise<string> {
  if (detectRuntimeMode() === "native") {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("get_genesis_token");
  }
  const token = import.meta.env.VITE_GENESIS_TOKEN as string | undefined;
  if (!token) throw new Error("Open Genesis in the Abigail app, or configure the local development capability.");
  return token;
}
