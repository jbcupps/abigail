import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { genesisRequest, resolveGenesisConnection } from "./genesisClient";
import { resolveGenesisToken, resolveHiveUrl } from "./connection";
import { hiveRequestHeaders, isNativeConnection, nativeHiveConnection } from "../../../../entity-runtime-app/src-ui/src/lib/nativeConnection";
vi.mock("./connection", () => ({ resolveHiveUrl: vi.fn(), resolveGenesisToken: vi.fn() }));
vi.mock("../../../../entity-runtime-app/src-ui/src/lib/nativeConnection", () => ({ hiveRequestHeaders: vi.fn(), isNativeConnection: vi.fn(), nativeHiveConnection: vi.fn() }));

beforeEach(() => { vi.mocked(resolveHiveUrl).mockResolvedValue("http://127.0.0.1:43141"); vi.mocked(resolveGenesisToken).mockResolvedValue("private-capability"); vi.mocked(isNativeConnection).mockReturnValue(false); vi.mocked(hiveRequestHeaders).mockImplementation(async (_base, headers) => headers); });
afterEach(() => { vi.resetAllMocks(); vi.unstubAllGlobals(); });

describe("private Genesis requests", () => {
  it("binds native requests to ownership headers and forbids redirects", async () => {
    vi.mocked(isNativeConnection).mockReturnValue(true);
    vi.mocked(nativeHiveConnection).mockResolvedValue({ hive_url: "http://127.0.0.1:43141", process_identity: { profile_id: "profile-a" } as never });
    vi.mocked(hiveRequestHeaders).mockImplementation(async (_base, headers) => { const result = new Headers(headers); result.set("x-abigail-profile-id", "profile-a"); result.set("x-abigail-process-instance-id", "instance-a"); return result; });
    const fetcher = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ok: true, data: { id: "draft", definition: {} } }) }); vi.stubGlobal("fetch", fetcher);
    await expect(genesisRequest("/sessions/draft")).resolves.toMatchObject({ connection: { hiveUrl: "http://127.0.0.1:43141", profileId: "profile-a" } });
    const init = fetcher.mock.calls[0][1];
    expect(init.redirect).toBe("error"); expect(init.headers.get("x-abigail-profile-id")).toBe("profile-a"); expect(init.headers.get("Authorization")).toBe("Bearer private-capability");
  });
  it("refuses a saved creation from another profile before obtaining its token or sending", async () => {
    vi.mocked(isNativeConnection).mockReturnValue(true);
    vi.mocked(nativeHiveConnection).mockResolvedValue({ hive_url: "http://127.0.0.1:43141", process_identity: { profile_id: "profile-b" } as never });
    const fetcher = vi.fn(); vi.stubGlobal("fetch", fetcher);
    await expect(genesisRequest("/sessions", {}, "POST", { connection: { hiveUrl: "http://127.0.0.1:43141", profileId: "profile-a" } })).rejects.toThrow("another Hive connection");
    expect(resolveGenesisToken).not.toHaveBeenCalled(); expect(fetcher).not.toHaveBeenCalled();
  });
  it("does not send a cancelled creation request", async () => {
    const controller = new AbortController(); controller.abort(); const fetcher = vi.fn(); vi.stubGlobal("fetch", fetcher);
    await expect(genesisRequest("/sessions", {}, "POST", { signal: controller.signal })).rejects.toThrow();
    expect(fetcher).not.toHaveBeenCalled();
  });
  it("resumes a verified native profile after its port changes, keeping the same creation key", async () => {
    vi.mocked(isNativeConnection).mockReturnValue(true);
    vi.mocked(resolveHiveUrl).mockResolvedValue("http://127.0.0.1:44999");
    vi.mocked(nativeHiveConnection).mockResolvedValue({ hive_url: "http://127.0.0.1:44999", process_identity: { profile_id: "profile-a" } as never });
    const fetcher = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ok: true, data: { id: "original-key", definition: {} } }) }); vi.stubGlobal("fetch", fetcher);
    await expect(genesisRequest("/sessions", { creation_key: "original-key" }, "POST", { connection: { hiveUrl: "http://127.0.0.1:44001", profileId: "profile-a" } })).resolves.toMatchObject({ id: "original-key", connection: { hiveUrl: "http://127.0.0.1:44999", profileId: "profile-a" } });
    expect(fetcher.mock.calls[0][0]).toBe("http://127.0.0.1:44999/v1/genesis/sessions");
    expect(JSON.parse(fetcher.mock.calls[0][1].body)).toEqual({ creation_key: "original-key" });
  });
  it("does not move a browser journal to a different origin", async () => {
    const fetcher = vi.fn(); vi.stubGlobal("fetch", fetcher);
    await expect(genesisRequest("/sessions", {}, "POST", { connection: { hiveUrl: "http://127.0.0.1:44999", profileId: null } })).rejects.toThrow("another Hive connection");
    expect(fetcher).not.toHaveBeenCalled();
  });
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
