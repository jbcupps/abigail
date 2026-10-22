import { afterEach, describe, expect, it, vi } from "vitest";
import type { GenesisCreationSeed, GenesisSession } from "./genesisClient";
import {
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
    await expect(dispatchGenesisLaunch(seed, key, vi.fn())).rejects.toThrow(/saved First Signal/i);
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
});
