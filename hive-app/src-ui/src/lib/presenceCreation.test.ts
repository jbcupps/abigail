import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPresenceEntity, CreationError, listCreationCandidates, lookupCreationAttempt, type CreationAttempt } from "./presenceCreation";

const mocks = vi.hoisted(() => ({ resolveHiveUrl: vi.fn(), fetch: vi.fn() }));
vi.mock("./connection", () => ({ resolveHiveUrl: mocks.resolveHiveUrl }));
const HIVE = "http://127.0.0.1:43141";
const KEY = "f2e75c9c-d4a7-433e-a380-69e4a658208a";
const ID = "37c73c8e-f9b9-48c3-9e53-0b3b8ce09455";
const attempt: CreationAttempt = { attemptId: KEY, creationKey: KEY, hiveUrl: HIVE, name: "Sol", baselineIds: [] };
const absent = { creation_key: KEY, state: "absent" };
const complete = { creation_key: KEY, state: "complete", name: "Sol", id: ID, directory: "entities/test" };
const created = { creation_key: KEY, id: ID, directory: "entities/test" };
const success = (data: unknown, status = 200) => ({ ok: status >= 200 && status < 300, status, json: vi.fn().mockResolvedValue({ ok: true, data }) }) as unknown as Response;
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { resolve, promise }; }
function normal() { mocks.fetch.mockResolvedValueOnce(success(absent)).mockResolvedValueOnce(success(created)); }
const locks = vi.fn();

beforeEach(() => {
  mocks.resolveHiveUrl.mockReset().mockResolvedValue(HIVE);
  mocks.fetch.mockReset().mockRejectedValue(new Error("Unexpected request"));
  vi.stubGlobal("fetch", mocks.fetch);
  locks.mockReset().mockImplementation((_name, _options, callback) => Promise.resolve(callback({ name: "test-lock" })));
  vi.stubGlobal("navigator", { locks: { request: locks } });
  vi.spyOn(crypto, "randomUUID").mockReturnValue(KEY);
});
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("keyed creation protocol", () => {
  it("proves GET support then records the immutable key before POST", async () => {
    normal();
    const onConfirmed = vi.fn();
    const onDispatched = vi.fn(value => { expect(mocks.fetch).toHaveBeenCalledTimes(1); expect(Object.isFrozen(value)).toBe(true); });
    await expect(createPresenceEntity("Sol", false, { onDispatched, onConfirmed })).resolves.toBe(ID);
    expect(onDispatched).toHaveBeenCalledExactlyOnceWith(attempt);
    expect(mocks.fetch).toHaveBeenNthCalledWith(1, `${HIVE}/v1/entity-creations/${KEY}`, expect.any(Object));
    expect(mocks.fetch).toHaveBeenNthCalledWith(2, `${HIVE}/v1/entities`, expect.objectContaining({ method: "POST", body: JSON.stringify({ name: "Sol", creation_key: KEY }) }));
    expect(onConfirmed).toHaveBeenCalledExactlyOnceWith(ID, attempt);
  });
  it.each([404, 405, 503])("never downgrades or POSTs when lookup is unsupported/unavailable (%s)", async status => {
    mocks.fetch.mockResolvedValue(success(absent, status));
    await expect(createPresenceEntity("Sol", false)).rejects.toMatchObject({ outcome: "not-sent" });
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
  it.each([
    [], null, { ...absent, state: "missing" }, { ...absent, creation_key: "different" },
    { ...absent, id: ID }, { ...complete, name: "Another name" }, { ...complete, id: "not-uuid" },
    { ...complete, directory: "" }, { ...complete, extra: true },
    { creation_key: KEY, state: "reserved" }, { creation_key: KEY, state: "unavailable", name: "Sol", id: "" },
  ])("rejects malformed or mismatched lookup data without POST: %j", async value => {
    mocks.fetch.mockResolvedValue(success(value));
    await expect(createPresenceEntity("Sol", false, { attempt })).rejects.toMatchObject({ outcome: "not-sent" });
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
  it.each(["absent", "reserved"])("explicit retry of %s uses the original key/name and pinned Hive", async state => {
    mocks.fetch.mockResolvedValueOnce(success(state === "absent" ? absent : { creation_key: KEY, state, name: "Sol" })).mockResolvedValueOnce(success(created));
    await expect(createPresenceEntity("Edited display name", false, { attempt })).resolves.toBe(ID);
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenLastCalledWith(`${HIVE}/v1/entities`, expect.objectContaining({ body: JSON.stringify({ name: "Sol", creation_key: KEY }) }));
  });
  it("complete lookup recovers the exact ID without POST", async () => {
    mocks.fetch.mockResolvedValue(success(complete)); const onConfirmed = vi.fn();
    await expect(createPresenceEntity("Sol", false, { attempt, onConfirmed })).resolves.toBe(ID);
    expect(onConfirmed).toHaveBeenCalledExactlyOnceWith(ID, attempt);
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
  it("unavailable never creates a replacement or changes the key", async () => {
    mocks.fetch.mockResolvedValue(success({ creation_key: KEY, state: "unavailable", name: "Sol", id: ID }));
    await expect(createPresenceEntity("Sol", false, { attempt })).rejects.toThrow(/unavailable/);
    expect(mocks.fetch).toHaveBeenCalledOnce(); expect(crypto.randomUUID).not.toHaveBeenCalled();
  });
  it("does not adopt an existing generated key on a fresh creation", async () => {
    mocks.fetch.mockResolvedValue(success(complete));
    await expect(createPresenceEntity("Sol", false)).rejects.toMatchObject({ outcome: "not-sent" });
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
  it.each([{}, { ...created, creation_key: undefined }, { ...created, creation_key: "different" }, { ...created, id: "legacy-id" }, { ...created, directory: null }, { ...created, ignored: true }])("keeps malformed POST confirmation uncertain: %j", async value => {
    mocks.fetch.mockResolvedValueOnce(success(absent)).mockResolvedValueOnce(success(value)); const onConfirmed = vi.fn();
    await expect(createPresenceEntity("Sol", false, { onConfirmed })).rejects.toMatchObject({ outcome: "unknown", attempt });
    expect(onConfirmed).not.toHaveBeenCalled(); expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });
  it.each([400, 409, 410, 500])("does not repeat or re-key a failed POST (%s)", async status => {
    mocks.fetch.mockResolvedValueOnce(success(absent)).mockResolvedValueOnce(success(created, status));
    await expect(createPresenceEntity("Sol", false)).rejects.toMatchObject({ outcome: "unknown", attempt });
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });
  it("blocks POST when journal persistence fails", async () => {
    mocks.fetch.mockResolvedValue(success(absent));
    await expect(createPresenceEntity("Sol", false, { onDispatched: () => { throw new Error("Storage blocked"); } })).rejects.toMatchObject({ outcome: "not-sent" });
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
  it("checks cancellation after journaling before POST", async () => {
    mocks.fetch.mockResolvedValue(success(absent)); const controller = new AbortController();
    await expect(createPresenceEntity("Sol", false, { signal: controller.signal, onDispatched: () => controller.abort() })).rejects.toMatchObject({ outcome: "not-sent" });
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
  it("fails visibly when Web Locks are unavailable or another tab holds the lock", async () => {
    vi.stubGlobal("navigator", {});
    await expect(createPresenceEntity("Sol", false)).rejects.toThrow(/Web Locks/);
    vi.stubGlobal("navigator", { locks: { request: (_name: string, _options: unknown, callback: (lock: null) => unknown) => callback(null) } });
    await expect(createPresenceEntity("Sol", false)).rejects.toThrow(/Another tab/);
    expect(mocks.fetch).not.toHaveBeenCalled();
  });
  it("preview remains local and does not require Web Locks", async () => {
    vi.stubGlobal("navigator", {}); const onConfirmed = vi.fn();
    await expect(createPresenceEntity("Sol", true, { onConfirmed })).resolves.toBe(KEY);
    expect(onConfirmed).toHaveBeenCalledWith(KEY, undefined); expect(mocks.fetch).not.toHaveBeenCalled();
  });
  it.each(["", " padded ", "A\nB", "Sol\u0080", "Sol\u009f", "N".repeat(81)])("rejects invalid name %j before network or journal persistence", async name => {
    const onDispatched = vi.fn();
    await expect(createPresenceEntity(name, false, { onDispatched })).rejects.toMatchObject({ outcome: "not-sent" });
    expect(mocks.fetch).not.toHaveBeenCalled(); expect(onDispatched).not.toHaveBeenCalled();
  });
  it("never retries a legacy attempt through keyed POST", async () => {
    await expect(createPresenceEntity("Sol", false, { attempt: { ...attempt, creationKey: undefined } })).rejects.toThrow(/older unkeyed/);
    expect(mocks.fetch).not.toHaveBeenCalled();
  });
});

describe("bounded requests and late evidence", () => {
  it.each(["connection", "lookup fetch", "lookup body", "POST fetch", "POST body"])("bounds %s and prevents later side effects except exact POST evidence", async stage => {
    vi.useFakeTimers(); const gate = deferred<unknown>();
    if (stage === "connection") mocks.resolveHiveUrl.mockReturnValue(gate.promise);
    if (stage === "lookup fetch") mocks.fetch.mockReturnValueOnce(gate.promise);
    if (stage === "lookup body") { const value = success(null); value.json = () => gate.promise; mocks.fetch.mockResolvedValueOnce(value); }
    if (stage.startsWith("POST")) {
      mocks.fetch.mockResolvedValueOnce(success(absent));
      if (stage === "POST fetch") mocks.fetch.mockReturnValueOnce(gate.promise);
      else { const value = success(null); value.json = () => gate.promise; mocks.fetch.mockResolvedValueOnce(value); }
    }
    const onConfirmed = vi.fn();
    const result = createPresenceEntity("Sol", false, { onConfirmed }).catch(error => error);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(await result).toBeInstanceOf(CreationError);
    expect(await result).toMatchObject({ outcome: stage.startsWith("POST") ? "unknown" : "not-sent" });
    const calls = mocks.fetch.mock.calls.length;
    gate.resolve(stage === "connection" ? HIVE : stage.includes("body") ? { ok: true, data: stage.startsWith("POST") ? created : absent } : success(stage.startsWith("POST") ? created : absent));
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.fetch).toHaveBeenCalledTimes(calls);
    expect(onConfirmed).toHaveBeenCalledTimes(stage.startsWith("POST") ? 1 : 0);
    expect(vi.getTimerCount()).toBe(0);
  });
  it("aborted POST can record late exact evidence without resolving the stopped operation", async () => {
    const gate = deferred<Response>(); const controller = new AbortController(); const onConfirmed = vi.fn();
    mocks.fetch.mockResolvedValueOnce(success(absent)).mockReturnValueOnce(gate.promise);
    const result = createPresenceEntity("Sol", false, { signal: controller.signal, onConfirmed }).catch(error => error);
    await vi.waitFor(() => expect(mocks.fetch).toHaveBeenCalledTimes(2)); controller.abort();
    expect(await result).toMatchObject({ outcome: "unknown" });
    gate.resolve(success(created)); await vi.waitFor(() => expect(onConfirmed).toHaveBeenCalledWith(ID, attempt));
    expect(await result).toMatchObject({ outcome: "unknown" });
  });
  it("bounds exact lookup to five seconds and never sends POST", async () => {
    vi.useFakeTimers(); mocks.fetch.mockReturnValue(new Promise(() => undefined));
    const result = lookupCreationAttempt(attempt).catch(error => error);
    await vi.advanceTimersByTimeAsync(5_000); expect(await result).toMatchObject({ name: "TimeoutError" });
    expect(mocks.fetch).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0);
  });
  it("preaborted creation and lookup do not touch Hive", async () => {
    const controller = new AbortController(); controller.abort();
    await expect(createPresenceEntity("Sol", false, { signal: controller.signal })).rejects.toMatchObject({ outcome: "not-sent" });
    await expect(lookupCreationAttempt(attempt, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
    expect(mocks.fetch).not.toHaveBeenCalled();
  });
});

describe("legacy candidates remain suggestions only", () => {
  it("uses only the old exact name and baseline without any creation request", async () => {
    const legacy = { ...attempt, creationKey: undefined, baselineIds: ["old"] };
    mocks.fetch.mockResolvedValue(success([{ id: "old", name: "Sol", is_hive: false }, { id: "hive", name: "Sol", is_hive: true }, { id: "new", name: "Sol", is_hive: false }, { id: "other", name: "Other", is_hive: false }]));
    await expect(listCreationCandidates(legacy)).resolves.toEqual([{ id: "new", name: "Sol" }]);
    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${HIVE}/v1/entities`, expect.any(Object));
  });
  it("does not use name candidates for keyed attempts", async () => {
    await expect(listCreationCandidates(attempt)).rejects.toThrow(/exact creation key/); expect(mocks.fetch).not.toHaveBeenCalled();
  });
  it("rejects malformed legacy lists without offering partial results", async () => {
    mocks.fetch.mockResolvedValue(success([{ id: "id", name: "Sol", is_hive: false }, { id: "id", name: "Sol", is_hive: false }]));
    await expect(listCreationCandidates({ ...attempt, creationKey: undefined })).rejects.toThrow(/duplicate/);
  });
});
