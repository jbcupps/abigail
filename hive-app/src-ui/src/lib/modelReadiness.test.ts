import { describe, expect, it } from "vitest";
import { modelSelectionLabel } from "./modelReadiness";
import type { HiveStatus } from "./daemonClient";

const status: HiveStatus = { entity_count: 0, entities: [], ready_state: "ready", any_provider_configured: true, setup_complete: true };
describe("Model selection presentation", () => {
  it("shows the resolved provider and model without inventing a live response", () => {
    expect(modelSelectionLabel({ ...status, model_connection: { provider: "openai", model: "fixture-model" } })).toBe("Model selected: OpenAI · fixture-model");
    expect(modelSelectionLabel({ ...status, model_connection: { provider: "codex-cli", model: null } })).toBe("Model selected: Codex (ChatGPT) · provider default");
    expect(modelSelectionLabel({ ...status, model_connection: { provider: "local" } })).toBe("Model selected: Local server · server-selected model");
  });
  it("does not turn Hive connectivity, stale selection or a fixture into model readiness", () => {
    expect(modelSelectionLabel({ ...status, any_provider_configured: false, model_connection: { provider: "openai", model: "old" } })).toBe("No model connected");
    expect(modelSelectionLabel({ ...status, ready_state: "needs_provider" })).toBe("No model connected");
    expect(modelSelectionLabel({ ...status, fixture: true })).toBe("Synthetic replies · no model connected");
    expect(modelSelectionLabel(null)).toBe("Model status unavailable");
    expect(modelSelectionLabel(status)).toContain("selection details unavailable");
  });
});
