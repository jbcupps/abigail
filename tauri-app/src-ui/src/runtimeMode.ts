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
  return window.localStorage.getItem("abigail.harnessDebug") === "1";
}

export function setHarnessDebugEnabled(enabled: boolean): void {
  window.localStorage.setItem("abigail.harnessDebug", enabled ? "1" : "0");
}

