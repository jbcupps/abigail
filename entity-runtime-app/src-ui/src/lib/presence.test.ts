import { afterEach, describe, expect, it, vi } from "vitest";
import { getRuntimePresence } from "./presence";

const baseCard = {
  name: "Ada",
  purpose: "Bring calm clarity",
  temperament: "playful",
  boundaries: "Ask before acting",
  identity: "aurora",
  crest: "constellation",
  motion: "lively",
};

afterEach(() => vi.unstubAllGlobals());

describe("getRuntimePresence", () => {
  it("rejects Presence cards that include extra keys", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(
          JSON.stringify({ ok: true, data: { ...baseCard, weights: 1 } }),
          { headers: { "Content-Type": "application/json" } },
        ),
      ),
    );

    await expect(
      getRuntimePresence("http://127.0.0.1:43142", new AbortController().signal),
    ).rejects.toThrow("invalid Presence card");
  });
});
