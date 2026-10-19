import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { createPresenceEntity, loadPresence, openPresence, previewProfiles, savePresence } from "./presenceApi";

const mocks = vi.hoisted(() => ({
  resolveHiveUrl: vi.fn<() => Promise<string>>(),
  createEntity: vi.fn(),
  fetch: vi.fn<typeof fetch>(),
}));
vi.mock("./connection", () => ({ resolveHiveUrl: mocks.resolveHiveUrl }));
vi.mock("./daemonClient", () => ({ createEntity: mocks.createEntity }));

const PREVIEW_KEY = "abigail.first-signal.preview.v1";
const HIVE_URL = "http://127.0.0.1:43141";
const profile: EntityProfile = {
  ...defaultProfile("Sol"),
  purpose: "Plan my garden.",
  boundaries: "Ask before changing files.",
  identity: "lumen",
  temperament: "direct",
};

function response(envelope: unknown, status = 200): Response {
  return { ok: status >= 200 && status < 300, status, json: vi.fn().mockResolvedValue(envelope) } as unknown as Response;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const stages = ["connection", "fetch", "body"] as const;
function stallRequest(stage: typeof stages[number], data: unknown): () => void {
  if (stage === "connection") {
    const gate = deferred<string>();
    mocks.resolveHiveUrl.mockReturnValue(gate.promise);
    return () => gate.resolve(HIVE_URL);
  }
  if (stage === "fetch") {
    const gate = deferred<Response>();
    mocks.fetch.mockReturnValue(gate.promise);
    return () => gate.resolve(response({ ok: true, data }));
  }
  const gate = deferred<unknown>();
  const pendingBody = response(null);
  pendingBody.json = vi.fn().mockReturnValue(gate.promise);
  mocks.fetch.mockResolvedValue(pendingBody);
  return () => gate.resolve({ ok: true, data });
}

function expectNoRuntimeCalls() {
  expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
  expect(mocks.fetch).not.toHaveBeenCalled();
  expect(mocks.createEntity).not.toHaveBeenCalled();
}

beforeEach(() => {
  localStorage.clear();
  mocks.resolveHiveUrl.mockReset().mockResolvedValue(HIVE_URL);
  mocks.createEntity.mockReset().mockResolvedValue({ id: "created-entity", directory: "unused" });
  mocks.fetch.mockReset().mockRejectedValue(new Error("Unexpected network request"));
  vi.stubGlobal("fetch", mocks.fetch);
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("presence HTTP response validation", () => {
  it("saves only after confirmation and reloads the profile through the real wrapper", async () => {
    mocks.fetch
      .mockResolvedValueOnce(response({ ok: true, data: { ...profile, revision: 2 } }))
      .mockResolvedValueOnce(response({ ok: true, data: profile }));

    await expect(savePresence("entity/with spaces", profile, false)).resolves.toBeUndefined();
    await expect(loadPresence("entity/with spaces", false)).resolves.toEqual(profile);

    const endpoint = `${HIVE_URL}/v1/entities/entity%2Fwith%20spaces/presence`;
    expect(mocks.fetch).toHaveBeenNthCalledWith(1, endpoint, expect.objectContaining({
      method: "PUT",
      headers: expect.objectContaining({ "Content-Type": "application/json" }),
      body: JSON.stringify(profile),
    }));
    expect(mocks.fetch).toHaveBeenNthCalledWith(2, endpoint, expect.any(Object));
    expect(localStorage.getItem(PREVIEW_KEY)).toBeNull();
  });

  it.each([
    ["null", null],
    ["an array", []],
    ["a string", "saved"],
    ["missing ok", { data: profile }],
    ["string ok", { ok: "true", data: profile }],
    ["numeric ok", { ok: 1, data: profile }],
    ["false ok", { ok: false, data: profile }],
    ["missing data", { ok: true }],
  ])("rejects a malformed envelope with %s", async (_label, envelope) => {
    mocks.fetch.mockResolvedValue(response(envelope));
    await expect(loadPresence("sol", false)).rejects.toThrow(/Hive request failed/);
  });

  it("rejects HTTP failure even when its JSON claims success", async () => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: profile }, 503));
    await expect(loadPresence("sol", false)).rejects.toThrow(/503/);
  });

  it("surfaces a daemon error and rejects non-JSON responses", async () => {
    mocks.fetch.mockResolvedValueOnce(response({ ok: false, error: "Profile unavailable" }, 404));
    await expect(loadPresence("sol", false)).rejects.toThrow("Profile unavailable");
    const brokenJson = response(null);
    brokenJson.json = vi.fn().mockRejectedValue(new SyntaxError("Invalid JSON"));
    mocks.fetch.mockResolvedValueOnce(brokenJson);
    await expect(loadPresence("sol", false)).rejects.toThrow("Invalid JSON");
  });

  it("rejects an incomplete loaded profile", async () => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: { name: "Sol" } }));
    await expect(loadPresence("sol", false)).rejects.toThrow(/invalid visual profile/);
  });

  it.each([
    ["null", null],
    ["different name", { ...profile, name: "Other" }],
    ["different purpose", { ...profile, purpose: "Another purpose" }],
    ["different temperament", { ...profile, temperament: "curious" }],
    ["different boundaries", { ...profile, boundaries: "Another boundary" }],
    ["different identity", { ...profile, identity: "ember" }],
  ])("does not confirm a save with %s", async (_label, echo) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: echo }));
    await expect(savePresence("sol", profile, false)).rejects.toThrow(/did not confirm/);
  });

  it("keeps the save confirmation deadline at five seconds", async () => {
    vi.useFakeTimers();
    const release = stallRequest("fetch", profile);
    const settled = vi.fn();
    const outcome = savePresence("sol", profile, false).then(() => null, (error: unknown) => error);
    void outcome.then(settled);

    await vi.advanceTimersByTimeAsync(4_999);
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    const error = await outcome;
    expect(error).toBeInstanceOf(DOMException);
    expect(error).toMatchObject({ name: "TimeoutError" });
    expect(mocks.fetch.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);

    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(await outcome).toBe(error);
  });

  it("returns the actual created entity ID", async () => {
    await expect(createPresenceEntity("Sol", false)).resolves.toBe("created-entity");
    expect(mocks.createEntity).toHaveBeenCalledExactlyOnceWith("Sol");
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it.each([null, {}, { id: "" }, { id: "   " }, { id: " padded " }, { id: 42 }])("rejects an unconfirmed creation result %j", async (result) => {
    mocks.createEntity.mockResolvedValue(result);
    await expect(createPresenceEntity("Sol", false)).rejects.toThrow(/valid Entity ID/);
  });

  it.each(["http://127.0.0.1:45123", "https://entity.example.test/runtime/"])("opens a validated HTTP(S) address %s", async (local_url) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: { local_url } }));
    await expect(openPresence("entity/with spaces")).resolves.toBe(local_url);
    expect(mocks.fetch).toHaveBeenCalledWith(`${HIVE_URL}/v1/entities/entity%2Fwith%20spaces/open`, expect.objectContaining({ method: "POST" }));
  });

  it.each([null, {}, { local_url: "" }, { local_url: "   " }, { local_url: " http://127.0.0.1:45123 " }, { local_url: 42 }, { local_url: "/runtime" }, { local_url: "javascript:alert(1)" }, { local_url: "file:///runtime" }])("rejects an unusable open result %j", async (data) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data }));
    await expect(openPresence("sol")).rejects.toThrow(/usable Entity address/);
  });
});

describe("preview storage remains local and recoverable", () => {
  it("creates, saves, edits and reopens a preview without runtime calls", async () => {
    const id = await createPresenceEntity("Sol", true);
    expect(id).toEqual(expect.any(String));
    expect(id.trim()).not.toBe("");
    await savePresence(id, profile, true);
    expect(previewProfiles()).toEqual([{ id, profile }]);
    const edited = { ...profile, identity: "ember" as const };
    await savePresence(id, edited, true);
    expect(previewProfiles()).toEqual([{ id, profile: edited }]);
    await expect(loadPresence(id, true)).resolves.toEqual(edited);
    expectNoRuntimeCalls();
  });

  it("rejects an invalid profile before overwriting existing preview data", async () => {
    const raw = JSON.stringify([{ id: "sol", profile }]);
    localStorage.setItem(PREVIEW_KEY, raw);
    const write = vi.spyOn(Storage.prototype, "setItem");

    await expect(savePresence("sol", { ...profile, name: "" }, true)).rejects.toThrow(/invalid/);
    expect(write).not.toHaveBeenCalled();
    expect(localStorage.getItem(PREVIEW_KEY)).toBe(raw);
    expectNoRuntimeCalls();
  });

  it("rejects a quota failure while retaining the last saved preview", async () => {
    const raw = JSON.stringify([{ id: "sol", profile }]);
    localStorage.setItem(PREVIEW_KEY, raw);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new DOMException("Storage full", "QuotaExceededError"); });

    await expect(savePresence("sol", { ...profile, name: "Edited" }, true)).rejects.toMatchObject({ name: "QuotaExceededError" });
    expect(localStorage.getItem(PREVIEW_KEY)).toBe(raw);
    expectNoRuntimeCalls();
  });

  it.each([
    ["malformed JSON", "{broken"],
    ["a non-array", JSON.stringify({ id: "sol", profile })],
    ["an invalid profile", JSON.stringify([{ id: "sol", profile: { name: "Sol" } }])],
    ["an empty ID", JSON.stringify([{ id: "", profile }])],
  ])("rejects reading and replacing corrupt storage containing %s", async (_label, raw) => {
    localStorage.setItem(PREVIEW_KEY, raw);
    const write = vi.spyOn(Storage.prototype, "setItem");

    expect(() => previewProfiles()).toThrow();
    await expect(loadPresence("sol", true)).rejects.toThrow();
    await expect(savePresence("sol", profile, true)).rejects.toThrow();
    expect(write).not.toHaveBeenCalled();
    expect(localStorage.getItem(PREVIEW_KEY)).toBe(raw);
    expectNoRuntimeCalls();
  });

  it("does not fall back to the network for a missing preview", async () => {
    await expect(loadPresence("missing", true)).rejects.toThrow(/no longer saved/);
    expectNoRuntimeCalls();
  });
});

const operations = [
  { label: "load", deadline: 5_000, data: profile, run: (signal?: AbortSignal) => loadPresence("sol", false, signal) },
  { label: "open", deadline: 55_000, data: { local_url: "http://127.0.0.1:45123" }, run: (signal?: AbortSignal) => openPresence("sol", signal) },
];

describe.each(operations)("$label cancellation and deadline", ({ run, data, deadline }) => {
  beforeEach(() => vi.useFakeTimers());

  it("rejects a preaborted request before resolving the connection or fetching", async () => {
    const controller = new AbortController();
    controller.abort();
    const error: unknown = await run(controller.signal).catch((reason: unknown) => reason);
    expect(error).toBeInstanceOf(DOMException);
    expect(error).toMatchObject({ name: "AbortError" });
    expectNoRuntimeCalls();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each(stages)("settles cancellation during %s even when that operation ignores abort", async (stage) => {
    const release = stallRequest(stage, data);
    const controller = new AbortController();
    const outcome = run(controller.signal).then(() => null, (error: unknown) => error);
    await vi.advanceTimersByTimeAsync(0);
    controller.abort();

    const error = await outcome;
    expect(error).toBeInstanceOf(DOMException);
    expect(error).toMatchObject({ name: "AbortError" });
    expect(vi.getTimerCount()).toBe(0);
    if (stage !== "connection") expect(mocks.fetch.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);

    release();
    await vi.advanceTimersByTimeAsync(0);
    if (stage === "connection") expect(mocks.fetch).not.toHaveBeenCalled();
    expect(await outcome).toBe(error);
  });

  it.each(stages)(`enforces the ${deadline / 1_000}-second deadline during %s even when that operation ignores abort`, async (stage) => {
    const release = stallRequest(stage, data);
    const settled = vi.fn();
    const outcome = run().then(() => null, (error: unknown) => error);
    void outcome.then(settled);

    if (deadline > 5_000) {
      await vi.advanceTimersByTimeAsync(5_000);
      expect(settled).not.toHaveBeenCalled();
    }
    await vi.advanceTimersByTimeAsync(deadline - (deadline > 5_000 ? 5_000 : 0) - 1);
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    const error = await outcome;
    expect(error).toBeInstanceOf(DOMException);
    expect(error).toMatchObject({ name: "TimeoutError" });
    expect(vi.getTimerCount()).toBe(0);
    if (stage !== "connection") expect(mocks.fetch.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);

    release();
    await vi.advanceTimersByTimeAsync(0);
    if (stage === "connection") expect(mocks.fetch).not.toHaveBeenCalled();
    expect(await outcome).toBe(error);
  });

  it("clears the deadline and detaches caller cancellation after success", async () => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data }));
    const controller = new AbortController();
    await run(controller.signal);

    expect(vi.getTimerCount()).toBe(0);
    controller.abort();
    expect(mocks.fetch.mock.calls[0]?.[1]?.signal?.aborted).toBe(false);
  });
});
