export type RuntimeMode = "native" | "browser-harness";

declare global {
  interface Window {
    isTauri?: boolean;
  }
}

export function detectRuntimeMode(): RuntimeMode {
  return window.isTauri ? "native" : "browser-harness";
}

export function isBrowserHarnessRuntime(): boolean {
  return detectRuntimeMode() === "browser-harness";
}

export function isHarnessDebugEnabled(): boolean {
  const fromQuery = new URLSearchParams(window.location.search).get("harnessDebug");
  if (fromQuery === "1" || fromQuery === "true") return true;
  if (fromQuery === "0" || fromQuery === "false") return false;
  return safeStorageGet("abigail.harnessDebug") === "1";
}

export function setHarnessDebugEnabled(enabled: boolean): void {
  safeStorageSet("abigail.harnessDebug", enabled ? "1" : "0");
}

export function isExperimentalUiEnabled(): boolean {
  const fromQuery = new URLSearchParams(window.location.search).get("experimentalUi");
  if (fromQuery === "1" || fromQuery === "true") return true;
  if (fromQuery === "0" || fromQuery === "false") return false;
  if (safeStorageGet("abigail.experimentalUi") === "1") return true;
  return import.meta.env.VITE_ENABLE_EXPERIMENTAL_UI === "1";
}

function safeStorageGet(key: string): string | null {
  const storage = window.localStorage as { getItem?: (k: string) => string | null };
  if (typeof storage?.getItem !== "function") return null;
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

function safeStorageSet(key: string, value: string): void {
  const storage = window.localStorage as { setItem?: (k: string, v: string) => void };
  if (typeof storage?.setItem !== "function") return;
  try {
    storage.setItem(key, value);
  } catch {
    // ignore storage write failures in restricted/browser-test environments
  }
}
