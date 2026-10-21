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

// The control token stays in memory, never in a URL or browser storage. The
// native shell only releases it after the daemon proves it holds the same one.
export async function resolveHiveToken(): Promise<string> {
  if (detectRuntimeMode() === "native") {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("get_hive_control_token");
  }
  const token = import.meta.env.VITE_HIVE_CONTROL_TOKEN as string | undefined;
  if (!token) throw new Error("Open this in the Abigail app, or configure the local development Hive token.");
  return token;
}

// Only a plain loopback origin may receive the control token.
export function localHiveOrigin(base: string): string {
  const url = new URL(base);
  if (url.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)
      || url.username || url.password || url.search || url.hash || (url.pathname !== "/" && url.pathname !== "")) {
    throw new Error("Abigail needs a local Hive connection. Use the Abigail app or a local development Hive.");
  }
  return url.origin;
}

export async function resolveHiveConnection(): Promise<{ base: string; token: string }> {
  const base = localHiveOrigin(await resolveHiveUrl());
  return { base, token: await resolveHiveToken() };
}
