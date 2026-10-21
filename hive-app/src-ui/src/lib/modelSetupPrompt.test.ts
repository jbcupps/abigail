import { beforeEach, describe, expect, it } from "vitest";
import { rememberModelSetupPrompt, shouldOfferModelSetup } from "./modelSetupPrompt";
import type { HiveStatus } from "./daemonClient";

const status: HiveStatus = { entity_count: 1, entities: [{ id: "hive-fixture", name: "Hive", birth_complete: true, is_hive: true, immortal: true }], ready_state: "needs_provider", any_provider_configured: false, setup_complete: false };
beforeEach(() => localStorage.clear());
describe("First model setup invitation", () => {
  it("remembers the invitation for this Hive without granting a connection or touching drafts", () => {
    localStorage.setItem("unrelated-identity-fixture", "keep these bytes");
    expect(shouldOfferModelSetup(status, false)).toBe(true);
    rememberModelSetupPrompt(status);
    expect(shouldOfferModelSetup(status, false)).toBe(false);
    expect(status.any_provider_configured).toBe(false);
    expect(localStorage.getItem("unrelated-identity-fixture")).toBe("keep these bytes");
    expect(shouldOfferModelSetup({ ...status, entities: [{ ...status.entities[0], id: "another-hive" }] }, false)).toBe(true);
  });
  it("leaves unfinished creation, configured models and synthetic previews alone", () => {
    expect(shouldOfferModelSetup(status, true)).toBe(false);
    expect(shouldOfferModelSetup({ ...status, any_provider_configured: true }, false)).toBe(false);
    expect(shouldOfferModelSetup({ ...status, fixture: true }, false)).toBe(false);
    expect(shouldOfferModelSetup({ ...status, entities: [] }, false)).toBe(false);
  });
});
