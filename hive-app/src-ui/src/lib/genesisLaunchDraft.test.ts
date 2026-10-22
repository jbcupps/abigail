import { afterEach, describe, expect, it, vi } from "vitest";
import type { GenesisCreationSeed, GenesisSession } from "./genesisClient";
import {
  createGenesisLaunchService,
  dispatchGenesisLaunch,
  GENESIS_LAUNCH_DRAFT_KEY,
  readGenesisLaunchDraft,
} from "./genesisLaunchDraft";

const key = "11111111-1111-4111-8111-111111111111";
const otherKey = "22222222-2222-4222-8222-222222222222";
const seed: GenesisCreationSeed = {
  mode: "quick",
  name: "Nova",
  purpose: "Help our family notice small wonders",
  presence: { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" },
};
const session = (id = key) => ({ id } as GenesisSession);

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("First Signal launch journal", () => {
  it("round-trips a valid draft written before dispatch and retains it after an ambiguous failure", async () => {
    const dispatch = vi.fn(async () => {
      expect(readGenesisLaunchDraft()).toEqual({ version: 1, creation_key: key, seed });
      throw new TypeError("Network request failed");
    });

    await expect(dispatchGenesisLaunch(seed, key, dispatch)).rejects.toThrow("Network request failed");

    expect(readGenesisLaunchDraft()).toEqual({ version: 1, creation_key: key, seed });
  });

  it.each([
    ["malformed", "not-json"],
    ["unknown-version", JSON.stringify({ version: 2, creation_key: key, seed })],
  ])("retains a %s journal for explicit recovery", async (_label, raw) => {
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, raw);

    expect(() => readGenesisLaunchDraft()).toThrow(/saved First Signal/i);
    await expect(dispatchGenesisLaunch(seed, key, vi.fn(async () => session()))).rejects.toThrow(/saved First Signal/i);
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBe(raw);
  });

  it("keeps the original creation key immutable after an ambiguous dispatch", async () => {
    await expect(dispatchGenesisLaunch(seed, key, async () => { throw new Error("timeout"); })).rejects.toThrow("timeout");
    const dispatch = vi.fn();

    await expect(dispatchGenesisLaunch(seed, otherKey, dispatch)).rejects.toThrow(/creation key/i);

    expect(dispatch).not.toHaveBeenCalled();
    expect(readGenesisLaunchDraft()?.creation_key).toBe(key);
  });

  it("refuses a seed mismatch without replacing the saved reviewed input", async () => {
    await expect(dispatchGenesisLaunch(seed, key, async () => { throw new Error("timeout"); })).rejects.toThrow("timeout");
    const changed = { ...seed, purpose: "A different purpose" };
    const dispatch = vi.fn();

    await expect(dispatchGenesisLaunch(changed, key, dispatch)).rejects.toThrow(/reviewed input/i);

    expect(dispatch).not.toHaveBeenCalled();
    expect(readGenesisLaunchDraft()?.seed).toEqual(seed);
  });

  it("clears only after the returned session ID exactly matches the journal key", async () => {
    await expect(dispatchGenesisLaunch(seed, key, async () => session(otherKey))).rejects.toThrow(/different session/i);
    expect(readGenesisLaunchDraft()?.creation_key).toBe(key);

    await expect(dispatchGenesisLaunch(seed, key, async () => session())).resolves.toEqual(session());
    expect(readGenesisLaunchDraft()).toBeNull();
  });

  it("serializes concurrent journal access", async () => {
    let releaseFirst!: () => void;
    let active = 0;
    let maximumActive = 0;
    const firstGate = new Promise<void>((resolve) => { releaseFirst = resolve; });
    const dispatch = vi.fn(async () => {
      active += 1;
      maximumActive = Math.max(maximumActive, active);
      if (dispatch.mock.calls.length === 1) await firstGate;
      active -= 1;
      return session();
    });

    const first = dispatchGenesisLaunch(seed, key, dispatch);
    const second = dispatchGenesisLaunch(seed, key, dispatch);
    await vi.waitFor(() => expect(dispatch).toHaveBeenCalledTimes(1));
    releaseFirst();
    await Promise.all([first, second]);

    expect(dispatch).toHaveBeenCalledTimes(2);
    expect(maximumActive).toBe(1);
  });

  it("stops a stale rendered recovery after another view clears the saved draft", async () => {
    const rendered = { version: 1 as const, creation_key: key, seed };
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(rendered));
    const dispatch = vi.fn();
    const launch = createGenesisLaunchService(dispatch);
    localStorage.removeItem(GENESIS_LAUNCH_DRAFT_KEY);

    await expect(launch(seed)).rejects.toThrow(/saved creation changed/i);

    expect(dispatch).not.toHaveBeenCalled();
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBeNull();
  });

  it("keeps one generated key and exact retained seed across an ambiguous service retry", async () => {
    const dispatch = vi.fn()
      .mockRejectedValueOnce(new TypeError("Network request failed"))
      .mockImplementationOnce(async (_seed: GenesisCreationSeed, creationKey: string) => session(creationKey));
    const launch = createGenesisLaunchService(dispatch);

    await expect(launch(seed)).rejects.toThrow("Network request failed");
    const retained = readGenesisLaunchDraft()!;
    await expect(launch(retained.seed)).resolves.toEqual(session(retained.creation_key));

    expect(dispatch.mock.calls).toEqual([
      [retained.seed, retained.creation_key],
      [retained.seed, retained.creation_key],
    ]);
    expect(readGenesisLaunchDraft()).toBeNull();
  });

  it.each([
    ["noncanonical UUID", { version: 1, creation_key: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA", seed }],
    ["blank name", { version: 1, creation_key: key, seed: { ...seed, name: "   " } }],
    ["untrimmed name", { version: 1, creation_key: key, seed: { ...seed, name: " Nova" } }],
    ["long name", { version: 1, creation_key: key, seed: { ...seed, name: "N".repeat(81) } }],
    ["controlled name", { version: 1, creation_key: key, seed: { ...seed, name: "No\u0000va" } }],
    ["blank purpose", { version: 1, creation_key: key, seed: { ...seed, purpose: "\n\t" } }],
    ["untrimmed purpose", { version: 1, creation_key: key, seed: { ...seed, purpose: " Purpose " } }],
    ["long purpose", { version: 1, creation_key: key, seed: { ...seed, purpose: "P".repeat(2001) } }],
    ["controlled purpose", { version: 1, creation_key: key, seed: { ...seed, purpose: "Keep\u0000stories" } }],
    ["invalid mode", { version: 1, creation_key: key, seed: { ...seed, mode: "automatic" } }],
    ["invalid temperament", { version: 1, creation_key: key, seed: { ...seed, presence: { ...seed.presence, temperament: "bold" } } }],
    ["invalid palette", { version: 1, creation_key: key, seed: { ...seed, presence: { ...seed.presence, identity: "gold" } } }],
    ["invalid crest", { version: 1, creation_key: key, seed: { ...seed, presence: { ...seed.presence, crest: "crown" } } }],
    ["invalid motion", { version: 1, creation_key: key, seed: { ...seed, presence: { ...seed.presence, motion: "spin" } } }],
  ])("retains and rejects a semantically invalid journal: %s", (_label, invalid) => {
    const raw = JSON.stringify(invalid);
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, raw);

    expect(() => readGenesisLaunchDraft()).toThrow(/invalid format/i);
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBe(raw);
  });

  it("accepts document whitespace inside a trimmed purpose", () => {
    const valid = { version: 1 as const, creation_key: key, seed: { ...seed, purpose: "Keep stories\n\twith care" } };
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(valid));

    expect(readGenesisLaunchDraft()).toEqual(valid);
  });
});
