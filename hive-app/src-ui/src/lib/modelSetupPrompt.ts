import type { HiveStatus } from "./daemonClient";

/** Display preference only. This never grants account access or routing consent. */
export function modelSetupPromptKey(status: HiveStatus): string | null {
  const hiveId = status.entities.find(entity => entity.is_hive)?.id;
  return hiveId ? `abigail.model-setup-seen.v1:${hiveId}` : null;
}

export function shouldOfferModelSetup(status: HiveStatus, unfinishedCreation: boolean): boolean {
  if (status.fixture || unfinishedCreation || status.any_provider_configured || status.ready_state !== "needs_provider") return false;
  const key = modelSetupPromptKey(status);
  if (!key) return false;
  try { return localStorage.getItem(key) !== "seen"; }
  catch { return false; } // Settings and the readiness action remain available.
}

export function rememberModelSetupPrompt(status: HiveStatus): void {
  const key = modelSetupPromptKey(status);
  if (key) {
    try { localStorage.setItem(key, "seen"); }
    catch { /* A display preference must not block creation or connection. */ }
  }
}
