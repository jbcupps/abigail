import { describe, expect, it } from "vitest";
import type { GenesisSession } from "../lib/genesisClient";
import { awaitingFromSessions, portraitsFromSessions } from "./GenesisEntityPortrait";

function session(id: string, updatedAt: string, portrait: string | null, state = "ready", awakening?: string): GenesisSession {
  return { id, updated_at: updatedAt, state, awakening, definition: { entity_id: "nova", portrait_asset_id: portrait }, asset_ids: ["unapproved-candidate"] } as unknown as GenesisSession;
}

describe("persistent Entity portraits", () => {
  it("uses the canonical portrait and never substitutes an unapproved candidate", () => {
    expect(portraitsFromSessions([session("one", "2026-10-17", "approved")])).toEqual({ nova: { sessionId: "one", assetId: "approved" } });
    expect(portraitsFromSessions([session("one", "2026-10-17", null)])).toEqual({});
    expect(portraitsFromSessions([session("draft", "2026-10-17", "approved", "review")])).toEqual({});
  });

  it("retains the approved visual of an awakened Entity during intentional evolution", () => {
    expect(portraitsFromSessions([session("evolving", "2026-10-17", "approved", "review", "I am Nova")])).toEqual({ nova: { sessionId: "evolving", assetId: "approved" } });
  });

  it("routes Entities still being created back to their Genesis session", () => {
    expect(awaitingFromSessions([session("creating", "2026-10-17", null, "review")])).toEqual({ nova: "creating" });
    expect(awaitingFromSessions([session("born", "2026-10-17", null, "ready", "I am Nova")])).toEqual({});
  });

  it("does not resurrect an older image after a newer canonical deletion", () => {
    const older = session("older", "2026-10-16", "old-portrait");
    const newer = session("newer", "2026-10-17", null);
    expect(portraitsFromSessions([older, newer])).toEqual({});
  });
});
