import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { genesisRequest, resolveGenesisConnection } from "./genesisClient";
import { resolveGenesisToken, resolveHiveUrl } from "./connection";
vi.mock("./connection", () => ({ resolveHiveUrl: vi.fn(), resolveGenesisToken: vi.fn() }));

beforeEach(() => { vi.mocked(resolveHiveUrl).mockResolvedValue("http://127.0.0.1:43141"); vi.mocked(resolveGenesisToken).mockResolvedValue("private-capability"); });
afterEach(() => { vi.resetAllMocks(); vi.unstubAllGlobals(); });

describe("private Genesis requests", () => {
  it.each(["https://evil.example", "http://localhost.evil.example", "http://user:password@127.0.0.1", "http://127.0.0.1?token=x", "http://127.0.0.1/forward", "http://127.0.0.1#fragment"])("rejects unsafe connection %s before obtaining a capability", async (url) => {
    vi.mocked(resolveHiveUrl).mockResolvedValue(url);
    await expect(resolveGenesisConnection()).rejects.toThrow("local Hive connection");
    expect(resolveGenesisToken).not.toHaveBeenCalled();
  });
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
});
