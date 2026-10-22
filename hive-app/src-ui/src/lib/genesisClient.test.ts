import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createOrResumeGenesis, genesisRequest, type GenesisCreationSeed } from "./genesisClient";
import { dispatchGenesisLaunch, GENESIS_LAUNCH_DRAFT_KEY, type GenesisLaunchDraft } from "./genesisLaunchDraft";
import { resolveHiveConnection } from "./connection";
vi.mock("./connection", () => ({ resolveHiveConnection: vi.fn() }));

beforeEach(() => {
  localStorage.clear();
  vi.mocked(resolveHiveConnection).mockResolvedValue({ base: "http://127.0.0.1:43141", token: "private-capability" });
});
afterEach(() => { vi.resetAllMocks(); vi.unstubAllGlobals(); localStorage.clear(); });

describe("private Genesis requests", () => {
  it("puts the capability in the authorization header, never the URL", async () => {
    const fetcher = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ok: true, data: [] }) });
    vi.stubGlobal("fetch", fetcher);
    await expect(genesisRequest("/sessions")).resolves.toEqual([]);
    expect(fetcher.mock.calls[0][0]).toBe("http://127.0.0.1:43141/v1/genesis/sessions");
    expect(fetcher.mock.calls[0][1].headers.Authorization).toBe("Bearer private-capability");
  });
  it("preserves safe daemon errors for actionable conflict feedback", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 409, json: async () => ({ ok: false, error: "This creation changed. Reload before saving." }) }));
    await expect(genesisRequest("/sessions/test/actions", {})).rejects.toThrow("Reload before saving");
  });
  it("posts only the keyed canonical creation seed", async () => {
    const seed: GenesisCreationSeed = {
      mode: "quick",
      name: "Nova",
      purpose: "Help our family notice small wonders",
      presence: { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" },
    };
    const fetcher = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ok: true, data: { id: "creation-key" } }) });
    vi.stubGlobal("fetch", fetcher);

    await createOrResumeGenesis(seed, "creation-key");

    expect(JSON.parse(fetcher.mock.calls[0][1].body)).toEqual({
      mode: "quick",
      creation_key: "creation-key",
      seed,
    });
  });

  it("keeps the rendered recovery key through the journal and typed session response", async () => {
    const creationKey = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const seed: GenesisCreationSeed = {
      mode: "quick",
      name: "Nova",
      purpose: "Help our family notice small wonders",
      presence: { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" },
    };
    const draft: GenesisLaunchDraft = { version: 1, creation_key: creationKey, seed };
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(draft));
    const fetcher = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, data: { id: creationKey, creation_seed: seed } }),
    });
    vi.stubGlobal("fetch", fetcher);

    const created = await dispatchGenesisLaunch(seed, creationKey, { expectedDraft: draft });

    expect(JSON.parse(fetcher.mock.calls[0][1].body)).toEqual({
      mode: "quick",
      creation_key: creationKey,
      seed,
    });
    expect(created.creation_seed).toEqual(seed);
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBeNull();
  });
});
