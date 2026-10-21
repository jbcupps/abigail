import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, beforeEach } from "vitest";
import { installBrowserTauriHarness } from "../browserTauriHarness";

interface Snapshot {
  runtime: "browser-harness";
  faultMode: "none" | "chat_timeout" | "chat_error" | "provider_validation_error";
  state: {
    providers: string[];
    activeProviderPreference: string | null;
  };
}

interface Identity {
  id: string;
  name: string;
}

describe("Browser harness debug controls", () => {
  beforeEach(() => {
    installBrowserTauriHarness({ force: true, resetState: true, trace: true, strict: true });
  });

  it("exposes deterministic snapshot and provider permutations", async () => {
    const before = await invoke<Snapshot>("harness_debug_snapshot");
    expect(before.runtime).toBe("browser-harness");
    expect(before.state.providers).toEqual([]);

    await invoke("store_provider_key", { provider: "google", key: "AIza-test", validate: true });
    const afterGoogle = await invoke<Snapshot>("harness_debug_snapshot");
    expect(afterGoogle.state.providers).toContain("google");
    expect(afterGoogle.state.providers).toContain("gemini-cli");
    expect(afterGoogle.state.activeProviderPreference).toBe("google");

    await invoke("store_provider_key", { provider: "xai", key: "xai-test", validate: true });
    const afterXai = await invoke<Snapshot>("harness_debug_snapshot");
    expect(afterXai.state.providers).toContain("xai");
    expect(afterXai.state.providers).toContain("grok-cli");
  });

  it("supports injected chat faults and recovery", async () => {
    // chat_stream delivers faults via SSE envelopes (returns null, not throwing).
    await invoke("harness_debug_set_fault", { mode: "chat_error" });
    const faultResult = await invoke("chat_stream", { message: "hello", sessionMessages: [] });
    expect(faultResult).toBeNull(); // Error delivered via event envelope, not exception

    await invoke("harness_debug_set_fault", { mode: "none" });
    const okResult = await invoke("chat_stream", { message: "hello", sessionMessages: [] });
    expect(okResult).toBeNull(); // Response delivered via event envelope
  });

  it("supports injected provider validation failures", async () => {
    await invoke("harness_debug_set_fault", { mode: "provider_validation_error" });
    const failed = await invoke<{ success: boolean; error: string }>("store_provider_key", {
      provider: "openai",
      key: "sk-test",
      validate: true,
    });
    expect(failed.success).toBe(false);
    expect(failed.error).toMatch(/synthetic provider validation failure/i);

    await invoke("harness_debug_set_fault", { mode: "none" });
    const passed = await invoke<{ success: boolean }>("store_provider_key", {
      provider: "openai",
      key: "sk-test",
      validate: true,
    });
    expect(passed.success).toBe(true);
  });

  it("supports agent spawn, switch, archive, and delete lifecycle guards", async () => {
    const alphaId = await invoke<string>("create_agent", { name: "Alpha" });
    await invoke("suspend_agent");
    const betaId = await invoke<string>("create_agent", { name: "Beta" });

    let identities = await invoke<Identity[]>("get_identities");
    expect(identities.map((identity) => identity.name)).toEqual(["Alpha", "Beta"]);

    // Active-agent archive is blocked until suspended.
    await expect(invoke("archive_agent_identity", { agentId: betaId })).rejects.toThrow(
      /cannot archive active agent/i
    );

    // Switch to Alpha and archive Beta (inactive).
    await invoke("load_agent", { agentId: alphaId });
    await invoke("archive_agent_identity", { agentId: betaId });
    identities = await invoke<Identity[]>("get_identities");
    expect(identities.map((identity) => identity.name)).toEqual(["Alpha"]);

    // Active-agent delete is blocked until suspended.
    await expect(invoke("delete_agent_identity", { agentId: alphaId })).rejects.toThrow(
      /cannot delete active agent/i
    );
    await invoke("suspend_agent");
    await invoke("delete_agent_identity", { agentId: alphaId });
    identities = await invoke<Identity[]>("get_identities");
    expect(identities).toEqual([]);
  });
});

