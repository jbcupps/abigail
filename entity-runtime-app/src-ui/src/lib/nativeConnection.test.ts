import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nativeHiveConnection, nativeOpenEntity, nativeRuntimeConnection, ownershipHeaders, verifiedRuntimeFetch } from "./nativeConnection";
import { resolveEntityUrl } from "./connection";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), fetch: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

const RUNTIME = "http://127.0.0.1:44142";
const HIVE = "http://127.0.0.1:44141";
const owner = {
  channel: "beta", canonical_data_root: "C:\\synthetic-preview",
  profile_id: "11111111-1111-4111-8111-111111111111",
  instance_id: "22222222-2222-4222-8222-222222222222",
  hive_instance_id: "33333333-3333-4333-8333-333333333333",
};
const hiveOwner = { ...owner, instance_id: owner.hive_instance_id };
const connection = { runtime_url: RUNTIME, hive_url: HIVE, process_identity: owner, hive_process_identity: hiveOwner };

beforeEach(() => {
  window.isTauri = true;
  window.history.replaceState({}, "", "/");
  mocks.invoke.mockReset().mockResolvedValue(connection);
  mocks.fetch.mockReset().mockResolvedValue(new Response("{}"));
});
afterEach(() => { delete window.isTauri; vi.useRealTimers(); vi.unstubAllEnvs(); });

describe("native process-owned transport", () => {
  it.each([
    ["/v1/status", "GET"],
    ["/v1/chat/history?limit=200", "GET"],
    ["/v1/chat/stream", "POST"],
    ["/v1/chat", "POST"],
    ["/v1/chat/cancel", "POST"],
  ])("verifies and labels %s before HTTP", async (path, method) => {
    await verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}${path}`, {
      method, headers: { Accept: "text/event-stream", "x-abigail-profile-id": "untrusted" },
    });
    expect(mocks.invoke).toHaveBeenCalledWith("get_runtime_connection_info", { runtimeUrl: RUNTIME });
    const [, init] = mocks.fetch.mock.calls[0];
    const headers = new Headers(init.headers);
    expect(headers.get("x-abigail-profile-id")).toBe(owner.profile_id);
    expect(headers.get("x-abigail-process-instance-id")).toBe(owner.instance_id);
    expect(headers.get("Accept")).toBe("text/event-stream");
    expect(init.redirect).toBe("error");
  });

  it("checks ownership again for a later request", async () => {
    const request = verifiedRuntimeFetch(mocks.fetch);
    await request(`${RUNTIME}/v1/status`);
    mocks.invoke.mockRejectedValueOnce(new Error("Live instance changed"));
    await expect(request(`${RUNTIME}/v1/chat/cancel`, { method: "POST" })).rejects.toThrow(/instance changed/);
    expect(mocks.fetch).toHaveBeenCalledTimes(1);
  });

  it("does not fall back to browser addresses after native resolution fails", async () => {
    window.history.replaceState({}, "", "/?entityUrl=http://127.0.0.1:49999");
    vi.stubEnv("VITE_ENTITY_DAEMON_URL", "http://127.0.0.1:48888");
    mocks.invoke.mockRejectedValue(new Error("No owned runtime"));
    await expect(resolveEntityUrl()).rejects.toThrow(/No owned runtime/);
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("rejects a different returned endpoint before HTTP", async () => {
    mocks.invoke.mockResolvedValue({ ...connection, runtime_url: "http://127.0.0.1:49999" });
    await expect(verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}/v1/chat`)).rejects.toThrow(/endpoint changed/);
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("rejects malformed process metadata before HTTP", async () => {
    mocks.invoke.mockResolvedValue({ ...connection, process_identity: { ...owner, instance_id: "" } });
    await expect(verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}/v1/chat`)).rejects.toThrow(/invalid process ownership/);
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("removes caller ownership headers for a verified stable connection", async () => {
    mocks.invoke.mockResolvedValue({ runtime_url: RUNTIME, process_identity: null });
    await verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}/v1/status`, { headers: {
      "x-abigail-profile-id": "stale", "x-abigail-process-instance-id": "stale", Accept: "application/json",
    } });
    const headers = new Headers(mocks.fetch.mock.calls[0][1].headers);
    expect(headers.has("x-abigail-profile-id")).toBe(false);
    expect(headers.has("x-abigail-process-instance-id")).toBe(false);
    expect(headers.get("Accept")).toBe("application/json");
  });

  it("uses the Hive instance for a Runtime appearance request", async () => {
    const info = await nativeRuntimeConnection(RUNTIME, HIVE);
    const headers = ownershipHeaders(info.hive_process_identity);
    expect(info.hive_url).toBe(HIVE);
    expect(headers.get("x-abigail-process-instance-id")).toBe(owner.hive_instance_id);
    expect(mocks.invoke).toHaveBeenCalledWith("get_runtime_connection_info", { runtimeUrl: RUNTIME, hiveUrl: HIVE });
  });

  it("does not send after cancellation while IPC is outstanding", async () => {
    let finish!: (value: typeof connection) => void;
    mocks.invoke.mockReturnValue(new Promise(resolve => { finish = resolve; }));
    const controller = new AbortController();
    const result = verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}/v1/chat/stream`, { method: "POST", signal: controller.signal });
    const assertion = expect(result).rejects.toHaveProperty("name", "AbortError");
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledOnce());
    controller.abort();
    await assertion;
    finish(connection);
    await Promise.resolve();
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("bounds a missing IPC response without sending a later request", async () => {
    vi.useFakeTimers();
    mocks.invoke.mockReturnValue(new Promise(() => undefined));
    const result = verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}/v1/chat`);
    const assertion = expect(result).rejects.toThrow(/timed out/);
    await vi.advanceTimersByTimeAsync(10_001);
    await assertion;
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("opens an Entity through the native pinning command", async () => {
    await expect(nativeOpenEntity("entity-id", HIVE)).resolves.toBe(RUNTIME);
    expect(mocks.invoke).toHaveBeenCalledWith("open_entity_connection", { entityId: "entity-id", hiveUrl: HIVE });
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("refuses stale Hive endpoints without silently using the returned address", async () => {
    mocks.invoke.mockResolvedValue({ hive_url: "http://127.0.0.1:49999", process_identity: hiveOwner });
    await expect(nativeHiveConnection(HIVE)).rejects.toThrow(/endpoint changed/);
  });

  it("leaves browser fixture transport unchanged", async () => {
    window.isTauri = false;
    const init = { method: "POST", headers: { "X-Fixture": "synthetic" }, body: "{}" };
    await verifiedRuntimeFetch(mocks.fetch)(`${RUNTIME}/v1/chat/stream`, init);
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenCalledWith(`${RUNTIME}/v1/chat/stream`, init);
  });
});
