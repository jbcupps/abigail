import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  canonicalLegacyPresenceText,
  clearLegacyFirstSignalJournal,
  readLegacyFirstSignalJournal,
  resolveLegacyCandidates,
  type LegacyFirstSignalJournal,
  type LegacyMigrationRequest,
} from "./legacyFirstSignal";
import { migrateLegacyFirstSignal } from "./questClient";

vi.mock("./connection", () => ({
  resolveHiveConnection: async () => ({
    base: "http://127.0.0.1:43141",
    token: "control-token",
  }),
}));

const V1_KEY = "abigail.first-signal.creation.v1";
const V2_KEY = "abigail.first-signal.creation.v2";
const RESOLVED_PREFIX = "abigail.first-signal.creation.resolved.v1:";
const CREATION_KEY = "11111111-1111-4111-8111-111111111111";
const ENTITY_ID = "22222222-2222-4222-8222-222222222222";
const OTHER_ID = "33333333-3333-4333-8333-333333333333";
const profile = {
  name: "Sol",
  purpose: "Plan our family garden.",
  temperament: "curious" as const,
  boundaries: "Ask before changing shared plans.",
  identity: "ember" as const,
};
const unkeyedAttempt = {
  attemptId: "old-attempt",
  hiveUrl: "http://127.0.0.1:43141",
  name: "Sol",
  baselineIds: [OTHER_ID],
};
const keyedAttempt = {
  attemptId: CREATION_KEY,
  creationKey: CREATION_KEY,
  hiveUrl: "http://127.0.0.1:43141",
  name: "Sol",
  baselineIds: [],
};

function v1(overrides: Record<string, unknown> = {}) {
  return {
    version: 1,
    profile,
    attempt: unkeyedAttempt,
    recoveryEntityId: ENTITY_ID,
    ...overrides,
  };
}

function v2(overrides: Record<string, unknown> = {}) {
  return {
    version: 2,
    revision: 4,
    profile,
    attempt: keyedAttempt,
    ...overrides,
  };
}

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ ok: status >= 200 && status < 300, data }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});

describe("legacy First Signal journal decoding", () => {
  it("mirrors Rust excerpt trimming rather than JavaScript FEFF trimming", () => {
    expect(canonicalLegacyPresenceText("\uFEFFSignal\uFEFF"))
      .toBe("\uFEFFSignal\uFEFF");
    expect(canonicalLegacyPresenceText("\u0085Signal\u0085")).toBe("Signal");
  });

  it("decodes the exact historical v1 session record without mutating storage", () => {
    const raw = JSON.stringify(v1());
    sessionStorage.setItem(V1_KEY, raw);
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    const removeItem = vi.spyOn(Storage.prototype, "removeItem");

    const journal = readLegacyFirstSignalJournal();

    expect(journal).toMatchObject({
      source: "session-v1",
      version: 1,
      revision: 0,
      profile,
      attempt: unkeyedAttempt,
      recoveryEntityId: ENTITY_ID,
    });
    expect(journal?.records).toEqual([{ storage: "session", key: V1_KEY, raw }]);
    expect(setItem).not.toHaveBeenCalled();
    expect(removeItem).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(V1_KEY)).toBe(raw);
  });

  it("decodes the exact historical keyed v2 local record", () => {
    const raw = JSON.stringify(v2({ entityId: ENTITY_ID }));
    localStorage.setItem(V2_KEY, raw);

    expect(readLegacyFirstSignalJournal()).toMatchObject({
      source: "local-v2",
      version: 2,
      revision: 4,
      profile,
      attempt: keyedAttempt,
      entityId: ENTITY_ID,
      records: [{ storage: "local", key: V2_KEY, raw }],
    });
  });

  it("accepts maximum astral migration text and attempt-key bytes", () => {
    const astral = "😀";
    const raw = JSON.stringify(v1({
      profile: {
        ...profile,
        purpose: astral.repeat(2000),
        boundaries: astral.repeat(2000),
      },
      attempt: {
        ...unkeyedAttempt,
        attemptId: astral.repeat(128),
      },
    }));
    sessionStorage.setItem(V1_KEY, raw);

    expect(readLegacyFirstSignalJournal()).toMatchObject({
      version: 1,
      profile: {
        purpose: astral.repeat(2000),
        boundaries: astral.repeat(2000),
      },
      attempt: { attemptId: astral.repeat(128) },
    });
    expect(sessionStorage.getItem(V1_KEY)).toBe(raw);
  });

  it("mirrors Rust trimming for a FEFF-delimited attempt key", () => {
    const attemptId = "\uFEFFold-attempt\uFEFF";
    const raw = JSON.stringify(v1({
      attempt: { ...unkeyedAttempt, attemptId },
    }));
    sessionStorage.setItem(V1_KEY, raw);

    expect(readLegacyFirstSignalJournal()).toMatchObject({
      attempt: { attemptId },
    });
    expect(sessionStorage.getItem(V1_KEY)).toBe(raw);
  });

  it.each([
    [
      "an attempt key over 512 UTF-8 bytes",
      v1({ attempt: { ...unkeyedAttempt, attemptId: "😀".repeat(129) } }),
      /attempt key.*retained/i,
    ],
    [
      "a disallowed text control",
      v1({ profile: { ...profile, purpose: "Plan\u0000private" } }),
      /brief.*retained/i,
    ],
  ])("retains and explains %s", (_label, value, message) => {
    const raw = JSON.stringify(value);
    sessionStorage.setItem(V1_KEY, raw);

    expect(() => readLegacyFirstSignalJournal()).toThrow(message);
    expect(sessionStorage.getItem(V1_KEY)).toBe(raw);
  });

  it.each([
    ["v1 malformed JSON", "session" as const, V1_KEY, "{broken"],
    ["v2 malformed JSON", "local" as const, V2_KEY, "{broken"],
    ["v1 unknown profile field", "session" as const, V1_KEY, JSON.stringify(v1({ profile: { ...profile, prompt: "private" } }))],
    ["v2 noncanonical key", "local" as const, V2_KEY, JSON.stringify(v2({ attempt: { ...keyedAttempt, creationKey: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA", attemptId: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA" } }))],
    ["v2 baseline IDs", "local" as const, V2_KEY, JSON.stringify(v2({ attempt: { ...keyedAttempt, baselineIds: [OTHER_ID] } }))],
  ])("retains and rejects %s", (_label, storage, key, raw) => {
    const area = storage === "session" ? sessionStorage : localStorage;
    area.setItem(key, raw);

    expect(() => readLegacyFirstSignalJournal()).toThrow(/retained/i);
    expect(area.getItem(key)).toBe(raw);
  });

  it("retains both conflicting records and refuses to choose between them", () => {
    const sessionRaw = JSON.stringify(v1());
    const localRaw = JSON.stringify(v2());
    sessionStorage.setItem(V1_KEY, sessionRaw);
    localStorage.setItem(V2_KEY, localRaw);

    expect(() => readLegacyFirstSignalJournal()).toThrow(/different legacy First Signal/i);
    expect(sessionStorage.getItem(V1_KEY)).toBe(sessionRaw);
    expect(localStorage.getItem(V2_KEY)).toBe(localRaw);
  });

  it("coalesces matching migrated v1 copies without changing either one", () => {
    const sessionRaw = JSON.stringify(v1());
    const localRaw = JSON.stringify(v1({ revision: 1 }));
    sessionStorage.setItem(V1_KEY, sessionRaw);
    localStorage.setItem(V2_KEY, localRaw);

    const journal = readLegacyFirstSignalJournal();

    expect(journal?.source).toBe("local-v2");
    expect(journal?.records).toEqual([
      { storage: "local", key: V2_KEY, raw: localRaw },
      { storage: "session", key: V1_KEY, raw: sessionRaw },
    ]);
    expect(localStorage.getItem(V2_KEY)).toBe(localRaw);
    expect(sessionStorage.getItem(V1_KEY)).toBe(sessionRaw);
  });

  it("ignores only a v1 session record covered by its exact resolution marker", () => {
    const record = v1();
    sessionStorage.setItem(V1_KEY, JSON.stringify(record));
    localStorage.setItem(
      `${RESOLVED_PREFIX}${encodeURIComponent(unkeyedAttempt.attemptId)}`,
      JSON.stringify(unkeyedAttempt),
    );

    expect(readLegacyFirstSignalJournal()).toBeNull();
    expect(sessionStorage.getItem(V1_KEY)).not.toBeNull();
  });
});

describe("legacy candidate resolution", () => {
  it("uses only an exact complete receipt for a keyed attempt and never POSTs", async () => {
    localStorage.setItem(V2_KEY, JSON.stringify(v2()));
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => json({
      creation_key: CREATION_KEY,
      state: "complete",
      name: "Sol",
      id: ENTITY_ID,
      directory: "/entities/sol",
    }));
    vi.stubGlobal("fetch", fetcher);

    await expect(resolveLegacyCandidates(readLegacyFirstSignalJournal()!)).resolves.toEqual([{
      id: ENTITY_ID,
      name: "Sol",
      resolution: "completed_receipt",
    }]);
    expect(fetcher).toHaveBeenCalledOnce();
    expect(fetcher.mock.calls[0][0]).toBe(`http://127.0.0.1:43141/v1/entity-creations/${CREATION_KEY}`);
    expect(fetcher.mock.calls[0][1]).toMatchObject({
      method: "GET",
      headers: { Accept: "application/json", Authorization: "Bearer control-token" },
    });
    expect(fetcher.mock.calls.some(([, init]) => init?.method === "POST")).toBe(false);
  });

  it.each(["absent", "reserved", "unavailable"])("does not offer a keyed %s receipt", async (state) => {
    localStorage.setItem(V2_KEY, JSON.stringify(v2()));
    vi.stubGlobal("fetch", vi.fn(async () => json({
      creation_key: CREATION_KEY,
      state,
      ...(state === "absent" ? {} : { name: "Sol" }),
      ...(state === "unavailable" ? { id: ENTITY_ID } : {}),
    })));

    await expect(resolveLegacyCandidates(readLegacyFirstSignalJournal()!)).resolves.toEqual([]);
  });

  it("rejects a keyed completed receipt without a canonical Entity ID", async () => {
    localStorage.setItem(V2_KEY, JSON.stringify(v2()));
    vi.stubGlobal("fetch", vi.fn(async () => json({
      creation_key: CREATION_KEY,
      state: "complete",
      name: "Sol",
      id: "not-a-canonical-entity-id",
      directory: "/entities/sol",
    })));

    await expect(resolveLegacyCandidates(readLegacyFirstSignalJournal()!))
      .rejects.toThrow(/invalid legacy creation receipt/i);
  });

  it("filters unkeyed candidates by exact name, non-Hive status, and the journal baseline", async () => {
    sessionStorage.setItem(V1_KEY, JSON.stringify(v1()));
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => json([
      { id: "hive", name: "Sol", is_hive: true },
      { id: OTHER_ID, name: "Sol", is_hive: false },
      { id: ENTITY_ID, name: "Sol", is_hive: false },
      { id: "different", name: "SOL", is_hive: false },
    ]));
    vi.stubGlobal("fetch", fetcher);

    await expect(resolveLegacyCandidates(readLegacyFirstSignalJournal()!)).resolves.toEqual([{
      id: ENTITY_ID,
      name: "Sol",
      resolution: "name_candidate",
    }]);
    expect(fetcher.mock.calls[0][0]).toBe("http://127.0.0.1:43141/v1/entities");
    expect(fetcher.mock.calls[0][1]).toMatchObject({ method: "GET" });
    expect(fetcher.mock.calls.some(([, init]) => init?.method === "POST")).toBe(false);
  });
});

describe("legacy migration client and exact cleanup", () => {
  const request: LegacyMigrationRequest = {
    attempt_key: CREATION_KEY,
    original_name: "Sol",
    purpose: profile.purpose,
    boundaries: profile.boundaries,
    presence: {
      temperament: "curious",
      identity: "ember",
      crest: "orbit",
      motion: "calm",
    },
  };

  it("posts the closed import request to the selected existing Entity", async () => {
    const result = {
      entity_id: ENTITY_ID,
      presence: { name: "Sol", purpose: profile.purpose, boundaries: profile.boundaries, ...request.presence },
      badges: ["first_spark", "named", "purposeful", "presence_chosen"],
      migrated_at: "2026-10-23T00:00:00Z",
    };
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => json(result));
    vi.stubGlobal("fetch", fetcher);

    await expect(migrateLegacyFirstSignal(ENTITY_ID, request)).resolves.toEqual(result);
    expect(fetcher).toHaveBeenCalledWith(
      `http://127.0.0.1:43141/v1/entities/${ENTITY_ID}/journey/legacy-first-signal`,
      expect.objectContaining({
        method: "POST",
        headers: {
          Accept: "application/json",
          Authorization: "Bearer control-token",
          "Content-Type": "application/json",
        },
        body: JSON.stringify(request),
      }),
    );
  });

  it("writes the exact resolution marker before removing a matching v1 record", async () => {
    const record = v1();
    sessionStorage.setItem(V1_KEY, JSON.stringify(record));
    const journal = readLegacyFirstSignalJournal()!;
    const writes: string[] = [];
    const originalSetItem = Storage.prototype.setItem;
    const originalRemoveItem = Storage.prototype.removeItem;
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key, value) {
      writes.push(`set:${key}`);
      return Reflect.apply(originalSetItem, this, [key, value]);
    });
    const remove = vi.spyOn(Storage.prototype, "removeItem").mockImplementation(function (this: Storage, key) {
      writes.push(`remove:${key}`);
      return Reflect.apply(originalRemoveItem, this, [key]);
    });

    await clearLegacyFirstSignalJournal(journal);

    const marker = `${RESOLVED_PREFIX}${encodeURIComponent(unkeyedAttempt.attemptId)}`;
    expect(localStorage.getItem(marker)).toBe(JSON.stringify(unkeyedAttempt));
    expect(sessionStorage.getItem(V1_KEY)).toBeNull();
    expect(writes.indexOf(`set:${marker}`)).toBeLessThan(writes.indexOf(`remove:${V1_KEY}`));
    set.mockRestore();
    remove.mockRestore();
  });

  it("marks a migrated v1 journal copied under the local v2 key", async () => {
    const raw = JSON.stringify(v1({ revision: 1 }));
    localStorage.setItem(V2_KEY, raw);
    const journal = readLegacyFirstSignalJournal()!;

    await clearLegacyFirstSignalJournal(journal);

    const marker = `${RESOLVED_PREFIX}${encodeURIComponent(unkeyedAttempt.attemptId)}`;
    expect(localStorage.getItem(marker)).toBe(JSON.stringify(unkeyedAttempt));
    expect(localStorage.getItem(V2_KEY)).toBeNull();
  });

  it("is idempotent when another tab already removed the exact records", async () => {
    const raw = JSON.stringify(v1());
    sessionStorage.setItem(V1_KEY, raw);
    const journal = readLegacyFirstSignalJournal()!;

    await clearLegacyFirstSignalJournal(journal);
    await expect(clearLegacyFirstSignalJournal(journal)).resolves.toBeUndefined();

    expect(sessionStorage.getItem(V1_KEY)).toBeNull();
    expect(localStorage.getItem(
      `${RESOLVED_PREFIX}${encodeURIComponent(unkeyedAttempt.attemptId)}`,
    )).toBe(JSON.stringify(unkeyedAttempt));
  });

  it("uses the historical creation Web Lock to serialize clear operations", async () => {
    const raw = JSON.stringify(v1());
    sessionStorage.setItem(V1_KEY, raw);
    const journal = readLegacyFirstSignalJournal()!;
    const previous = Object.getOwnPropertyDescriptor(navigator, "locks");
    const request = vi.fn(async (_name: string, work: () => Promise<void>) => work());
    Object.defineProperty(navigator, "locks", {
      configurable: true,
      value: { request },
    });

    try {
      await Promise.all([
        clearLegacyFirstSignalJournal(journal),
        clearLegacyFirstSignalJournal(journal),
      ]);
    } finally {
      if (previous) Object.defineProperty(navigator, "locks", previous);
      else Reflect.deleteProperty(navigator, "locks");
    }

    expect(request).toHaveBeenCalledTimes(2);
    expect(request).toHaveBeenCalledWith(
      "abigail:first-signal:creation",
      expect.any(Function),
    );
  });

  it("does not clear bytes that changed after the journal was reviewed", async () => {
    const original = JSON.stringify(v2());
    localStorage.setItem(V2_KEY, original);
    const journal = readLegacyFirstSignalJournal() as LegacyFirstSignalJournal;
    const changed = JSON.stringify(v2({ revision: 5 }));
    localStorage.setItem(V2_KEY, changed);

    await expect(clearLegacyFirstSignalJournal(journal)).rejects.toThrow(/changed/i);
    expect(localStorage.getItem(V2_KEY)).toBe(changed);
  });
});
