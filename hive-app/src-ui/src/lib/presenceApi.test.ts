import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { createPresenceEntity, loadPresence, openPresence, preparePresence, previewProfiles, requirePresenceReady, savePresence } from "./presenceApi";

const mocks = vi.hoisted(() => ({
  resolveHiveUrl: vi.fn<() => Promise<string>>(),
  fetch: vi.fn<typeof fetch>(),
}));
vi.mock("./connection", () => ({ resolveHiveUrl: mocks.resolveHiveUrl }));

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
}

beforeEach(() => {
  localStorage.clear();
  mocks.resolveHiveUrl.mockReset().mockResolvedValue(HIVE_URL);
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

  it("pins a recovered profile save and reload to the attempt's Hive address", async () => {
    const pinnedUrl = "http://127.0.0.1:43151";
    mocks.resolveHiveUrl.mockResolvedValue("http://127.0.0.1:43161");
    mocks.fetch.mockResolvedValue(response({ ok: true, data: profile }));
    const controller = new AbortController();

    await savePresence("sol", profile, false, { signal: controller.signal, hiveUrl: pinnedUrl });
    await expect(loadPresence("sol", false, controller.signal, pinnedUrl)).resolves.toEqual(profile);

    expect(mocks.fetch.mock.calls.map(([url]) => url)).toEqual([
      `${pinnedUrl}/v1/entities/sol/presence`, `${pinnedUrl}/v1/entities/sol/presence`,
    ]);
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
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

  it("re-exports creation with an exact-key capability check before POST", async () => {
    const key = "a1e11a11-1111-4111-8111-111111111111";
    const id = "b2e22b22-2222-4222-8222-222222222222";
    vi.spyOn(crypto, "randomUUID").mockReturnValue(key);
    vi.stubGlobal("navigator", { locks: { request: async (_name: string, _options: unknown, run: (lock: object) => Promise<string>) => run({}) } });
    mocks.fetch.mockResolvedValueOnce(response({ ok: true, data: { creation_key: key, state: "absent" } }))
      .mockResolvedValueOnce(response({ ok: true, data: { id, directory: "synthetic", creation_key: key } }));
    await expect(createPresenceEntity("Sol", false)).resolves.toBe(id);
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
    expect(mocks.fetch).toHaveBeenLastCalledWith(`${HIVE_URL}/v1/entities`, expect.objectContaining({ method: "POST", body: JSON.stringify({ name: "Sol", creation_key: key }) }));
  });

  it.each(["http://127.0.0.1:45123", "https://entity.example.test/runtime/"])("opens a validated HTTP(S) address %s", async (local_url) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: { local_url } }));
    await expect(openPresence("entity/with spaces")).resolves.toBe(local_url);
    expect(mocks.fetch).toHaveBeenCalledWith(`${HIVE_URL}/v1/entities/entity%2Fwith%20spaces/open`, expect.objectContaining({ method: "POST" }));
  });

  it("opens only the supplied Hive even when the current connection points elsewhere", async () => {
    const hiveA = "http://127.0.0.1:43151";
    const runtimeA = "http://127.0.0.1:45151";
    mocks.resolveHiveUrl.mockResolvedValue("http://127.0.0.1:43152");
    mocks.fetch.mockResolvedValue(response({ ok: true, data: { local_url: runtimeA } }));

    await expect(openPresence("same-entity-id", undefined, hiveA)).resolves.toBe(runtimeA);

    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${hiveA}/v1/entities/same-entity-id/open`, expect.objectContaining({ method: "POST" }));
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
  });

  it("does not fall back to the current Hive when the supplied Hive fails to open", async () => {
    const hiveA = "http://127.0.0.1:43151";
    mocks.resolveHiveUrl.mockResolvedValue("http://127.0.0.1:43152");
    mocks.fetch
      .mockRejectedValueOnce(new Error("Pinned Hive unavailable"))
      .mockResolvedValue(response({ ok: true, data: { local_url: "http://127.0.0.1:45152" } }));

    await expect(openPresence("same-entity-id", undefined, hiveA)).rejects.toThrow("Pinned Hive unavailable");

    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${hiveA}/v1/entities/same-entity-id/open`, expect.objectContaining({ method: "POST" }));
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
  });

  it.each([null, {}, { local_url: "" }, { local_url: "   " }, { local_url: " http://127.0.0.1:45123 " }, { local_url: 42 }, { local_url: "/runtime" }, { local_url: "javascript:alert(1)" }, { local_url: "file:///runtime" }])("rejects an unusable open result %j", async (data) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data }));
    await expect(openPresence("sol")).rejects.toThrow(/usable Entity address/);
  });
});

describe("presence model readiness", () => {
  it("checks only the supplied Hive and returns its exact address", async () => {
    const pinned = "http://127.0.0.1:43151";
    mocks.resolveHiveUrl.mockResolvedValue("http://127.0.0.1:43161");
    mocks.fetch.mockResolvedValue(response({ ok: true, data: { ready_state: "ready", any_provider_configured: true } }));

    await expect(requirePresenceReady(undefined, pinned)).resolves.toBe(pinned);

    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${pinned}/v1/status`, expect.objectContaining({ signal: expect.any(AbortSignal) }));
    expect(mocks.fetch.mock.calls[0][1]?.method).toBeUndefined();
  });

  it("discovers the current Hive once and returns the checked address for subsequent operations", async () => {
    const discovered = "http://127.0.0.1:43171";
    mocks.resolveHiveUrl.mockResolvedValueOnce(discovered).mockResolvedValue("http://127.0.0.1:43181");
    mocks.fetch.mockResolvedValue(response({ ok: true, data: { ready_state: "ready", any_provider_configured: true } }));

    await expect(requirePresenceReady()).resolves.toBe(discovered);

    expect(mocks.resolveHiveUrl).toHaveBeenCalledOnce();
    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${discovered}/v1/status`, expect.any(Object));
  });

  it("permits the explicitly labeled synthetic transport without claiming a configured model", async () => {
    mocks.fetch.mockResolvedValue(response({
      ok: true, data: { ready_state: "ready", any_provider_configured: false, fixture: true },
    }));

    await expect(requirePresenceReady(undefined, HIVE_URL)).resolves.toBe(HIVE_URL);

    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${HIVE_URL}/v1/status`, expect.any(Object));
  });

  it.each([
    { ready_state: "needs_provider", any_provider_configured: false },
    { ready_state: "needs_provider", any_provider_configured: true },
    { ready_state: "needs_provider", any_provider_configured: false, fixture: true },
    { ready_state: "ready", any_provider_configured: false },
    { ready_state: "ready", any_provider_configured: false, fixture: false },
    { ready_state: "ready", any_provider_configured: false, fixture: "true" },
    { ready_state: "ready", any_provider_configured: false, fixture: 1 },
  ])("keeps conversation closed when model readiness is absent: %j", async (status) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: status }));

    await expect(requirePresenceReady()).rejects.toThrow("Connect a model in this Hive before opening a conversation. Your saved identity is still here.");
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });

  it.each([
    null, [], "ready", {},
    { ready_state: "ready" },
    { ready_state: "ready", fixture: true },
    { any_provider_configured: true },
    { ready_state: "ready", any_provider_configured: "true" },
    { ready_state: "ready", any_provider_configured: 1 },
    { ready_state: "starting", any_provider_configured: true },
  ])("rejects an invalid or missing readiness status: %j", async (status) => {
    mocks.fetch.mockResolvedValue(response({ ok: true, data: status }));

    await expect(requirePresenceReady()).rejects.toThrow("Hive did not confirm model readiness.");
  });

  it("rejects a daemon status error without trusting success fields", async () => {
    mocks.fetch.mockResolvedValue(response({
      ok: false, error: "Hive is restarting", data: { ready_state: "ready", any_provider_configured: true },
    }, 503));

    await expect(requirePresenceReady()).rejects.toThrow("Hive is restarting");
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });

  it("does not fall back to the current Hive when the supplied readiness check fails", async () => {
    const pinned = "http://127.0.0.1:43151";
    mocks.fetch.mockRejectedValueOnce(new Error("Pinned Hive unavailable"))
      .mockResolvedValue(response({ ok: true, data: { ready_state: "ready", any_provider_configured: true } }));

    await expect(requirePresenceReady(undefined, pinned)).rejects.toThrow("Pinned Hive unavailable");

    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${pinned}/v1/status`, expect.any(Object));
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
  { label: "readiness", deadline: 5_000, data: { ready_state: "ready", any_provider_configured: true }, run: (signal?: AbortSignal) => requirePresenceReady(signal) },
  { label: "load", deadline: 5_000, data: profile, run: (signal?: AbortSignal) => loadPresence("sol", false, signal) },
  { label: "save", deadline: 5_000, data: profile, run: (signal?: AbortSignal) => savePresence("sol", profile, false, { signal }) },
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


describe("current main quickstart integration", () => {
  it("leaves an already configured Entity unchanged", async () => {
    mocks.fetch.mockResolvedValueOnce(response({ ok: true, data: { id: "ada", birth_complete: true } }));
    await preparePresence("ada");
    expect(mocks.fetch).toHaveBeenCalledOnce();
    expect(mocks.fetch.mock.calls[0][1]?.method).toBeUndefined();
  });

  it("finishes default setup on the pinned Hive and verifies it before opening", async () => {
    const pinned = "http://127.0.0.1:45999";
    mocks.fetch.mockResolvedValueOnce(response({ ok: true, data: { id: "ada", birth_complete: false } }))
      .mockResolvedValueOnce(response({ ok: true, data: { certificate: {} } }))
      .mockResolvedValueOnce(response({ ok: true, data: { id: "ada", birth_complete: true } }));
    await preparePresence("ada", undefined, pinned);
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch.mock.calls.map(([url]) => String(url))).toEqual([
      `${pinned}/v1/entities/ada`, `${pinned}/v1/entities/ada/birth`, `${pinned}/v1/entities/ada`,
    ]);
    expect(JSON.parse(mocks.fetch.mock.calls[1][1]?.body as string)).toEqual({
      path: "quickstart", choices: [], purpose: "A helpful, honest companion for everyday family life.",
    });
  });

  it("does not claim setup when the acknowledgement is followed by an incomplete Entity", async () => {
    mocks.fetch.mockResolvedValueOnce(response({ ok: true, data: { id: "ada", birth_complete: false } }))
      .mockResolvedValueOnce(response({ ok: true, data: {} }))
      .mockResolvedValueOnce(response({ ok: true, data: { id: "ada", birth_complete: false } }));
    await expect(preparePresence("ada")).rejects.toThrow("setup is not confirmed");
    expect(mocks.resolveHiveUrl).toHaveBeenCalledOnce();
  });

  it("rejects the wrong Entity before writing any setup", async () => {
    mocks.fetch.mockResolvedValueOnce(response({ ok: true, data: { id: "other", birth_complete: false } }));
    await expect(preparePresence("ada")).rejects.toThrow("setup state");
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });

  it("bounds connection discovery before starting setup", async () => {
    vi.useFakeTimers();
    const gate = deferred<string>();
    mocks.resolveHiveUrl.mockReturnValue(gate.promise);
    const pending = expect(preparePresence("ada")).rejects.toThrow("timed out");
    await vi.advanceTimersByTimeAsync(5000);
    await pending;
    gate.resolve(HIVE_URL);
    await Promise.resolve();
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("stops before birth when navigation cancels the setup read", async () => {
    const gate = deferred<Response>();
    mocks.fetch.mockReturnValue(gate.promise);
    const controller = new AbortController();
    const pending = expect(preparePresence("ada", controller.signal)).rejects.toThrow();
    await Promise.resolve();
    controller.abort();
    await pending;
    gate.resolve(response({ ok: true, data: { id: "ada", birth_complete: false } }));
    await Promise.resolve();
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
});
