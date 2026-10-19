import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPresenceEntity, CreationError, listCreationCandidates, type CreationAttempt } from "./presenceCreation";

const mocks = vi.hoisted(() => ({
  resolveHiveUrl: vi.fn<() => Promise<string>>(),
  fetch: vi.fn<typeof fetch>(),
}));
vi.mock("./connection", () => ({ resolveHiveUrl: mocks.resolveHiveUrl }));

const HIVE_URL = "http://127.0.0.1:43141";
const baseline = [{ id: "existing", name: "Sol", is_hive: false }];
const created = { id: "created-entity", directory: "unused" };
const attempt: CreationAttempt = Object.freeze({
  attemptId: "f2e75c9c-d4a7-433e-a380-69e4a658208a",
  hiveUrl: HIVE_URL,
  name: "Sol",
  baselineIds: Object.freeze(["existing"]),
});

function response(envelope: unknown, status = 200): Response {
  return { ok: status >= 200 && status < 300, status, json: vi.fn().mockResolvedValue(envelope) } as unknown as Response;
}

function success(data: unknown): Response {
  return response({ ok: true, data });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const stages = ["connection", "baseline fetch", "baseline body", "POST fetch", "POST body"] as const;
function stallCreation(stage: typeof stages[number]): () => void {
  if (stage === "connection") {
    const gate = deferred<string>();
    mocks.resolveHiveUrl.mockReturnValue(gate.promise);
    mocks.fetch.mockResolvedValueOnce(success(baseline)).mockResolvedValueOnce(success(created));
    return () => gate.resolve(HIVE_URL);
  }
  if (stage === "baseline fetch") {
    const gate = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(gate.promise).mockResolvedValueOnce(success(created));
    return () => gate.resolve(success(baseline));
  }
  if (stage === "baseline body") {
    const gate = deferred<unknown>();
    const pendingBody = success(null);
    pendingBody.json = vi.fn().mockReturnValue(gate.promise);
    mocks.fetch.mockResolvedValueOnce(pendingBody).mockResolvedValueOnce(success(created));
    return () => gate.resolve({ ok: true, data: baseline });
  }
  mocks.fetch.mockResolvedValueOnce(success(baseline));
  if (stage === "POST fetch") {
    const gate = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(gate.promise);
    return () => gate.resolve(success(created));
  }
  const gate = deferred<unknown>();
  const pendingBody = success(null);
  pendingBody.json = vi.fn().mockReturnValue(gate.promise);
  mocks.fetch.mockResolvedValueOnce(pendingBody);
  return () => gate.resolve({ ok: true, data: created });
}

function expectCreationError(value: unknown, outcome: "not-sent" | "unknown") {
  expect(value).toBeInstanceOf(CreationError);
  expect(value).toMatchObject({ outcome });
}

beforeEach(() => {
  mocks.resolveHiveUrl.mockReset().mockResolvedValue(HIVE_URL);
  mocks.fetch.mockReset().mockRejectedValue(new Error("Unexpected network request"));
  vi.stubGlobal("fetch", mocks.fetch);
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("bounded Entity creation", () => {
  it("pins one Hive, reads the baseline before POST, and confirms the same immutable attempt", async () => {
    const originalRows = [
      { id: "existing", name: "Sol", is_hive: false, extra: "ignored" },
      { id: "legacy-name", name: ` ${"L".repeat(81)} `, is_hive: false, extra: "ignored" },
    ];
    mocks.fetch.mockResolvedValueOnce(success(originalRows)).mockResolvedValueOnce(success(created));
    const onConfirmed = vi.fn();
    const onDispatched = vi.fn((value: CreationAttempt) => {
      expect(mocks.fetch).toHaveBeenCalledTimes(1);
      expect(Object.isFrozen(value)).toBe(true);
      expect(Object.isFrozen(value.baselineIds)).toBe(true);
      originalRows[0].id = "changed-after-baseline";
      mocks.resolveHiveUrl.mockResolvedValue("http://other-hive.example.test");
    });

    await expect(createPresenceEntity("Sol", false, { onDispatched, onConfirmed })).resolves.toBe(created.id);

    const dispatched = onDispatched.mock.calls[0][0];
    expect(dispatched).toEqual({ attemptId: expect.any(String), hiveUrl: HIVE_URL, name: "Sol", baselineIds: ["existing", "legacy-name"] });
    expect(dispatched.attemptId.trim()).not.toBe("");
    expect(onDispatched).toHaveBeenCalledOnce();
    expect(onConfirmed).toHaveBeenCalledExactlyOnceWith(created.id, dispatched);
    expect(onConfirmed.mock.calls[0][1]).toBe(dispatched);
    expect(mocks.resolveHiveUrl).toHaveBeenCalledOnce();
    expect(mocks.fetch).toHaveBeenNthCalledWith(1, `${HIVE_URL}/v1/entities`, expect.objectContaining({ headers: expect.objectContaining({ Accept: "application/json" }) }));
    expect(mocks.fetch).toHaveBeenNthCalledWith(2, `${HIVE_URL}/v1/entities`, expect.objectContaining({ method: "POST", body: JSON.stringify({ name: "Sol" }) }));
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });

  it("creates a local preview ID without a baseline, dispatch, or network call", async () => {
    const onDispatched = vi.fn();
    const onConfirmed = vi.fn();
    const id = await createPresenceEntity("Sol", true, { onDispatched, onConfirmed });

    expect(id.trim()).not.toBe("");
    expect(onConfirmed).toHaveBeenCalledExactlyOnceWith(id, undefined);
    expect(onDispatched).not.toHaveBeenCalled();
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("rejects invalid names without silently trimming or contacting Hive", async () => {
    for (const name of ["", " padded ", "Line\nbreak", "Null\u0000byte", "N".repeat(81)]) {
      const error: unknown = await createPresenceEntity(name, false).catch((reason: unknown) => reason);
      expectCreationError(error, "not-sent");
    }
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it.each([false, true])("classifies an already cancelled operation as not sent (preview: %s)", async (preview) => {
    const controller = new AbortController();
    controller.abort();
    const onDispatched = vi.fn();
    const onConfirmed = vi.fn();
    const error: unknown = await createPresenceEntity("Sol", preview, { signal: controller.signal, onDispatched, onConfirmed }).catch((reason: unknown) => reason);

    expectCreationError(error, "not-sent");
    expect(onDispatched).not.toHaveBeenCalled();
    expect(onConfirmed).not.toHaveBeenCalled();
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("does not dispatch if the baseline is unsuccessful or any row is malformed", async () => {
    const malformed = [
      response({ ok: true, data: baseline }, 503),
      response({ ok: "true", data: baseline }),
      success({ entities: baseline }),
      success([{ id: "", name: "Sol", is_hive: false }]),
      success([{ id: "existing\u0000id", name: "Sol", is_hive: false }]),
      success([{ id: "existing\u007fid", name: "Sol", is_hive: false }]),
      success([...baseline, ...baseline]),
      success([{ id: "existing", name: 42, is_hive: false }]),
      success([{ id: "existing", name: "Sol", is_hive: "false" }]),
    ];
    for (const reply of malformed) {
      mocks.fetch.mockReset().mockResolvedValue(reply);
      const onDispatched = vi.fn();
      const onConfirmed = vi.fn();
      const error: unknown = await createPresenceEntity("Sol", false, { onDispatched, onConfirmed }).catch((reason: unknown) => reason);
      expectCreationError(error, "not-sent");
      expect(mocks.fetch).toHaveBeenCalledOnce();
      expect(onDispatched).not.toHaveBeenCalled();
      expect(onConfirmed).not.toHaveBeenCalled();
    }
  });

  it("does not dispatch when recording the attempt throws", async () => {
    mocks.fetch.mockResolvedValueOnce(success(baseline));
    const onConfirmed = vi.fn();
    const error: unknown = await createPresenceEntity("Sol", false, {
      onDispatched: () => { throw new Error("Journal full"); },
      onConfirmed,
    }).catch((reason: unknown) => reason);

    expectCreationError(error, "not-sent");
    expect(error).toMatchObject({ message: "Journal full" });
    expect(mocks.fetch).toHaveBeenCalledOnce();
    expect(onConfirmed).not.toHaveBeenCalled();
  });

  it("rechecks cancellation after recording the attempt and before invoking POST", async () => {
    mocks.fetch.mockResolvedValueOnce(success(baseline));
    const controller = new AbortController();
    const onDispatched = vi.fn(() => controller.abort());
    const onConfirmed = vi.fn();
    const error: unknown = await createPresenceEntity("Sol", false, { signal: controller.signal, onDispatched, onConfirmed }).catch((reason: unknown) => reason);

    expectCreationError(error, "not-sent");
    expect(onDispatched).toHaveBeenCalledOnce();
    expect(mocks.fetch).toHaveBeenCalledOnce();
    expect(onConfirmed).not.toHaveBeenCalled();
  });

  it("classifies all failures after POST invocation as unknown, without retrying", async () => {
    const badJson = success(null);
    badJson.json = vi.fn().mockRejectedValue(new SyntaxError("Broken response JSON"));
    const failures: (() => Promise<Response>)[] = [
      () => { throw new Error("Synchronous fetch failure"); },
      () => Promise.reject(new Error("Disconnected")),
      () => Promise.resolve(response({ ok: false, error: "Creation failed" }, 500)),
      () => Promise.resolve(response({ ok: "true", data: created })),
      () => Promise.resolve(success({ id: " " })),
      () => Promise.resolve(success({ id: "created\u0000entity" })),
      () => Promise.resolve(badJson),
    ];
    for (const failure of failures) {
      mocks.fetch.mockReset().mockResolvedValueOnce(success(baseline)).mockImplementationOnce(failure);
      const onDispatched = vi.fn();
      const onConfirmed = vi.fn();
      const error: unknown = await createPresenceEntity("Sol", false, { onDispatched, onConfirmed }).catch((reason: unknown) => reason);
      expectCreationError(error, "unknown");
      expect(error).toHaveProperty("attempt", onDispatched.mock.calls[0][0]);
      expect(mocks.fetch).toHaveBeenCalledTimes(2);
      expect(onConfirmed).not.toHaveBeenCalled();
    }
  });

  it("preserves a confirmed ID when its evidence observer throws", async () => {
    mocks.fetch.mockResolvedValueOnce(success(baseline)).mockResolvedValueOnce(success(created));
    const onConfirmed = vi.fn(() => { throw new Error("Observer failed"); });
    await expect(createPresenceEntity("Sol", false, { onConfirmed })).resolves.toBe(created.id);
    expect(onConfirmed).toHaveBeenCalledOnce();
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });

  it("rejects a stale POST ID already present in the baseline without confirming it", async () => {
    mocks.fetch.mockResolvedValueOnce(success(baseline)).mockResolvedValueOnce(success({ id: "existing" }));
    const onDispatched = vi.fn();
    const onConfirmed = vi.fn();
    const error: unknown = await createPresenceEntity("Sol", false, { onDispatched, onConfirmed }).catch((reason: unknown) => reason);

    expectCreationError(error, "unknown");
    expect(error).toHaveProperty("attempt", onDispatched.mock.calls[0][0]);
    expect(onConfirmed).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });

  it.each(stages)("bounds the complete creation at 30 seconds while %s ignores cancellation", async (stage) => {
    vi.useFakeTimers();
    const release = stallCreation(stage);
    const onDispatched = vi.fn();
    const onConfirmed = vi.fn();
    const settled = vi.fn();
    const outcome = createPresenceEntity("Sol", false, { onDispatched, onConfirmed }).then(() => null, (error: unknown) => error);
    void outcome.then(settled);

    await vi.advanceTimersByTimeAsync(29_999);
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    const error = await outcome;
    const dispatched = stage.startsWith("POST");
    expectCreationError(error, dispatched ? "unknown" : "not-sent");
    expect(vi.getTimerCount()).toBe(0);
    const callsAtDeadline = mocks.fetch.mock.calls.length;

    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(await outcome).toBe(error);
    expect(mocks.fetch).toHaveBeenCalledTimes(callsAtDeadline);
    if (dispatched) {
      const originalAttempt = onDispatched.mock.calls[0][0];
      expect(error).toHaveProperty("attempt", originalAttempt);
      expect(onConfirmed).toHaveBeenCalledExactlyOnceWith(created.id, originalAttempt);
      expect(onConfirmed.mock.calls[0][1]).toBe(originalAttempt);
    } else {
      expect(onDispatched).not.toHaveBeenCalled();
      expect(onConfirmed).not.toHaveBeenCalled();
    }
  });

  it("keeps an aborted POST unknown while delivering late confirmation only as correlated evidence", async () => {
    vi.useFakeTimers();
    const release = stallCreation("POST fetch");
    const controller = new AbortController();
    const onDispatched = vi.fn();
    const onConfirmed = vi.fn(() => { throw new Error("Late observer failed"); });
    const outcome = createPresenceEntity("Sol", false, { signal: controller.signal, onDispatched, onConfirmed }).then(() => null, (error: unknown) => error);
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
    controller.abort();

    const error = await outcome;
    expectCreationError(error, "unknown");
    expect(mocks.fetch.mock.calls[1][1]?.signal?.aborted).toBe(true);
    expect(onConfirmed).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);

    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(onConfirmed).toHaveBeenCalledExactlyOnceWith(created.id, onDispatched.mock.calls[0][0]);
    expect(await outcome).toBe(error);
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });

  it("never confirms an invalid late POST response", async () => {
    vi.useFakeTimers();
    const pending = deferred<Response>();
    mocks.fetch.mockResolvedValueOnce(success(baseline)).mockReturnValueOnce(pending.promise);
    const onConfirmed = vi.fn();
    const outcome = createPresenceEntity("Sol", false, { onConfirmed }).then(() => null, (error: unknown) => error);
    await vi.advanceTimersByTimeAsync(30_000);
    const error = await outcome;
    expectCreationError(error, "unknown");

    pending.resolve(success({ id: "" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(onConfirmed).not.toHaveBeenCalled();
    expect(await outcome).toBe(error);
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
  });

  it("clears the deadline and caller abort listener after normal confirmation", async () => {
    vi.useFakeTimers();
    mocks.fetch.mockResolvedValueOnce(success(baseline)).mockResolvedValueOnce(success(created));
    const controller = new AbortController();
    await createPresenceEntity("Sol", false, { signal: controller.signal });
    expect(vi.getTimerCount()).toBe(0);
    controller.abort();
    expect(mocks.fetch.mock.calls[1][1]?.signal?.aborted).toBe(false);
  });
});

describe("creation candidates are read-only suggestions", () => {
  it("uses the pinned Hive and returns only exact-name, non-Hive IDs absent from the baseline", async () => {
    mocks.resolveHiveUrl.mockResolvedValue("http://other-hive.example.test");
    mocks.fetch.mockResolvedValue(success([
      { id: "existing", name: "Sol", is_hive: false },
      { id: "hive", name: "Sol", is_hive: true },
      { id: "wrong-case", name: "sol", is_hive: false },
      { id: "other-name", name: "Other", is_hive: false },
      { id: "candidate-one", name: "Sol", is_hive: false, extra: "ignored" },
      { id: "candidate-two", name: "Sol", is_hive: false },
    ]));

    await expect(listCreationCandidates(attempt)).resolves.toEqual([{ id: "candidate-one", name: "Sol" }, { id: "candidate-two", name: "Sol" }]);
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).toHaveBeenCalledExactlyOnceWith(`${HIVE_URL}/v1/entities`, expect.any(Object));
    expect(mocks.fetch.mock.calls[0][1]?.method).toBeUndefined();
  });

  it("returns an empty candidate list without retrying creation", async () => {
    mocks.fetch.mockResolvedValue(success(baseline));
    await expect(listCreationCandidates(attempt)).resolves.toEqual([]);
    expect(mocks.fetch).toHaveBeenCalledOnce();
    expect(mocks.fetch.mock.calls[0][1]?.method).toBeUndefined();
  });

  it("rejects malformed lists rather than offering partial candidate evidence", async () => {
    for (const data of [null, {}, [null], [...baseline, ...baseline], [{ id: " padded ", name: "Sol", is_hive: false }], [{ id: "new\u007fid", name: "Sol", is_hive: false }], [{ id: "new", name: null, is_hive: false }], [{ id: "new", name: "Sol", is_hive: 0 }]]) {
      mocks.fetch.mockReset().mockResolvedValue(success(data));
      await expect(listCreationCandidates(attempt)).rejects.toThrow(/invalid Entity list|duplicate Entity IDs/);
      expect(mocks.fetch).toHaveBeenCalledOnce();
    }
  });

  it("rejects a preaborted lookup without contacting either Hive", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(listCreationCandidates(attempt, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
    expect(mocks.resolveHiveUrl).not.toHaveBeenCalled();
    expect(mocks.fetch).not.toHaveBeenCalled();
  });

  it("settles cancellation even when candidate fetch ignores abort", async () => {
    vi.useFakeTimers();
    const pending = deferred<Response>();
    mocks.fetch.mockReturnValue(pending.promise);
    const controller = new AbortController();
    const outcome = listCreationCandidates(attempt, controller.signal).then(() => null, (error: unknown) => error);
    controller.abort();
    const error = await outcome;
    expect(error).toBeInstanceOf(DOMException);
    expect(error).toMatchObject({ name: "AbortError" });
    expect(mocks.fetch.mock.calls[0][1]?.signal?.aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);

    pending.resolve(success([{ id: "new", name: "Sol", is_hive: false }]));
    await vi.advanceTimersByTimeAsync(0);
    expect(await outcome).toBe(error);
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });

  it("includes candidate response parsing in its five-second deadline", async () => {
    vi.useFakeTimers();
    const pending = deferred<unknown>();
    const pendingBody = success(null);
    pendingBody.json = vi.fn().mockReturnValue(pending.promise);
    mocks.fetch.mockResolvedValue(pendingBody);
    const settled = vi.fn();
    const outcome = listCreationCandidates(attempt).then(() => null, (error: unknown) => error);
    void outcome.then(settled);

    await vi.advanceTimersByTimeAsync(4_999);
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    const error = await outcome;
    expect(error).toBeInstanceOf(DOMException);
    expect(error).toMatchObject({ name: "TimeoutError" });
    expect(mocks.fetch.mock.calls[0][1]?.signal?.aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);

    pending.resolve({ ok: true, data: [{ id: "new", name: "Sol", is_hive: false }] });
    await vi.advanceTimersByTimeAsync(0);
    expect(await outcome).toBe(error);
    expect(mocks.fetch).toHaveBeenCalledOnce();
  });
});
