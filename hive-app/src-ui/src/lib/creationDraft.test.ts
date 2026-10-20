import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { CREATION_DRAFT_EVENT, CREATION_DRAFT_KEY, clearCreationDraft, confirmCreationDraft, readCreationDraft, migrateCreationDraft, writeCreationDraft, type CreationDraft } from "./creationDraft";
import { withCreationLock } from "./presenceCreation";
const LEGACY_KEY = "abigail.first-signal.creation.v1";
const KEY = "f2e75c9c-d4a7-433e-a380-69e4a658208a";
const ID = "37c73c8e-f9b9-48c3-9e53-0b3b8ce09455";
const OTHER = "37c73c8e-f9b9-48c3-9e53-0b3b8ce09456";
function draft(): CreationDraft { return { version: 2, profile: defaultProfile("Sol"), attempt: { attemptId: KEY, creationKey: KEY, hiveUrl: "http://127.0.0.1:43141", name: "Sol", baselineIds: [] } }; }
function legacy(): CreationDraft { return { version: 1, profile: defaultProfile("Sol"), attempt: { attemptId: "old-attempt", hiveUrl: "http://127.0.0.1:43141", name: "Sol", baselineIds: ["existing"] }, recoveryEntityId: "selected" }; }
beforeEach(() => {
  sessionStorage.clear(); localStorage.clear();
  let held = false;
  vi.stubGlobal("navigator", { locks: { request: (_name: string, _options: unknown, callback: (lock: object | null) => unknown) => {
    if (held) return Promise.resolve(callback(null)); held = true;
    return Promise.resolve(callback({ name: "lock" })).finally(() => { held = false; });
  } } });
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); sessionStorage.clear(); localStorage.clear(); });

describe("durable browser creation journal", async () => {
  it("survives lost session storage and a fresh module without networking", async () => {
    const network = vi.fn(); vi.stubGlobal("fetch", network);
    const saved = await writeCreationDraft(draft()); expect(saved.revision).toBe(1);
    sessionStorage.clear(); vi.resetModules(); const reloaded = await import("./creationDraft");
    expect(reloaded.readCreationDraft()).toEqual(saved); expect(network).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem(CREATION_DRAFT_KEY)!)).toEqual(saved);
  });
  it("migrates a legacy session record unchanged in identity and keeps its manual recovery path", async () => {
    const original = legacy(); sessionStorage.setItem(LEGACY_KEY, JSON.stringify(original));
    expect(await migrateCreationDraft()).toEqual({ ...original, revision: 1 });
    expect(sessionStorage.getItem(LEGACY_KEY)).toBeNull(); expect(readCreationDraft()?.attempt.creationKey).toBeUndefined();
    sessionStorage.clear(); expect(readCreationDraft()?.version).toBe(1);
  });
  it("never erases the legacy source when durable migration fails", async () => {
    const raw = JSON.stringify(legacy()); sessionStorage.setItem(LEGACY_KEY, raw);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Disk blocked"); });
    await expect(migrateCreationDraft()).rejects.toThrow(/Disk blocked/); expect(sessionStorage.getItem(LEGACY_KEY)).toBe(raw);
    expect(localStorage.getItem(CREATION_DRAFT_KEY)).toBeNull();
  });
  it("retains both conflicting local and legacy records instead of overwriting either", async () => {
    const saved = await writeCreationDraft(draft()); const raw = JSON.stringify(legacy()); sessionStorage.setItem(LEGACY_KEY, raw);
    expect(() => readCreationDraft()).toThrow(/different creation/);
    expect(JSON.parse(localStorage.getItem(CREATION_DRAFT_KEY)!)).toEqual(saved); expect(sessionStorage.getItem(LEGACY_KEY)).toBe(raw);
  });
  it("does not resurrect a resolved legacy attempt from a duplicated tab's old session", async () => {
    const original = legacy();
    sessionStorage.setItem(LEGACY_KEY, JSON.stringify(original));
    const migrated = (await migrateCreationDraft())!;
    await clearCreationDraft(original.attempt.attemptId, migrated.revision);
    // A duplicated tab can retain its independent pre-migration session copy.
    sessionStorage.setItem(LEGACY_KEY, JSON.stringify(original));
    expect(readCreationDraft()).toBeNull();
    expect(await migrateCreationDraft()).toBeNull();
    await expect(writeCreationDraft(original)).rejects.toThrow(/already resolved/);
    expect(localStorage.getItem(CREATION_DRAFT_KEY)).toBeNull();
    const fresh = await writeCreationDraft(draft());
    expect(readCreationDraft()).toEqual(fresh);
  });
  it("keeps a legacy journal when its resolution marker cannot be stored", async () => {
    const saved = await writeCreationDraft(legacy());
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Quota"); });
    await expect(clearCreationDraft(saved.attempt.attemptId, saved.revision)).rejects.toThrow(/Quota/);
    expect(readCreationDraft()).toEqual(saved);
  });
  it("preserves edited profile and original name while adding a late exact confirmation", async () => {
    const saved = await writeCreationDraft(draft());
    const edited = await writeCreationDraft({ ...saved, profile: { ...saved.profile, name: "Edited name", purpose: "Current purpose" } });
    const confirmed = await confirmCreationDraft(KEY, ID);
    expect(confirmed).toEqual({ ...edited, revision: 3, entityId: ID });
    expect(confirmed?.attempt.name).toBe("Sol");
  });
  it("rejects stale profile updates and clears after another tab changes revision", async () => {
    const first = await writeCreationDraft(draft()); const second = await writeCreationDraft({ ...first, profile: { ...first.profile, purpose: "Other tab" } });
    await expect(writeCreationDraft({ ...first, profile: { ...first.profile, purpose: "Stale" } })).rejects.toThrow(/another tab/);
    await expect(clearCreationDraft(KEY, first.revision)).rejects.toThrow(/another tab/); expect(readCreationDraft()).toEqual(second);
  });
  it("does not resurrect a resolved record from a stale callback or write", async () => {
    const saved = await writeCreationDraft(draft()); await clearCreationDraft(KEY, saved.revision);
    expect(await confirmCreationDraft(KEY, ID)).toBeNull(); await expect(writeCreationDraft(saved)).rejects.toThrow(/stale operation/);
    expect(readCreationDraft()).toBeNull();
  });
  it("does not overwrite a different unfinished key or clear/confirm it from an older callback", async () => {
    const first = await writeCreationDraft(draft()); await clearCreationDraft(KEY);
    const second = await writeCreationDraft({ ...draft(), attempt: { ...draft().attempt, attemptId: OTHER, creationKey: OTHER } });
    expect(await confirmCreationDraft(KEY, ID)).toBeNull(); await clearCreationDraft(KEY);
    await expect(writeCreationDraft(first)).rejects.toThrow(/Another creation/); expect(readCreationDraft()).toEqual(second);
  });
  it.each([{ name: "Other" }, { hiveUrl: "http://127.0.0.1:9999" }, { creationKey: OTHER, attemptId: OTHER }])("never mutates dispatched identity fields %j", async change => {
    const saved = await writeCreationDraft(draft());
    await expect(writeCreationDraft({ ...saved, attempt: { ...saved.attempt, ...change } })).rejects.toThrow(); expect(readCreationDraft()).toEqual(saved);
  });
  it("rejects a different confirmed Entity without changing the original record", async () => {
    await writeCreationDraft(draft()); const first = await confirmCreationDraft(KEY, ID);
    await expect(confirmCreationDraft(KEY, OTHER)).rejects.toThrow(/already has a confirmed/); expect(readCreationDraft()).toEqual(first);
  });
  it("preserves confirmation if a current revision omits its ID", async () => {
    await writeCreationDraft(draft()); const confirmed = (await confirmCreationDraft(KEY, ID))!;
    const saved = await writeCreationDraft({ ...confirmed, entityId: undefined, profile: { ...confirmed.profile, purpose: "Edited" } });
    expect(saved.entityId).toBe(ID);
  });
  it("notifies only after successfully committed same-tab changes", async () => {
    const observed: (CreationDraft | null)[] = []; const changed = () => observed.push(readCreationDraft());
    window.addEventListener(CREATION_DRAFT_EVENT, changed);
    try { await writeCreationDraft(draft()); await confirmCreationDraft(KEY, ID); await clearCreationDraft(KEY); }
    finally { window.removeEventListener(CREATION_DRAFT_EVENT, changed); }
    expect(observed.map(value => value?.revision ?? null)).toEqual([1, 2, null]);
  });
  it("rejects competing writes and clears while another tab owns a transaction, before any storage event", async () => {
    const original = await writeCreationDraft(draft());
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    const owner = withCreationLock(async lease => {
      await writeCreationDraft({ ...original, profile: { ...original.profile, purpose: "Lock owner's edit" } }, lease);
      await gate;
    });
    await vi.waitFor(() => expect(readCreationDraft()?.revision).toBe(2));
    await expect(writeCreationDraft(original)).rejects.toThrow(/Another tab/);
    await expect(clearCreationDraft(KEY)).rejects.toThrow(/Another tab/);
    await expect(confirmCreationDraft(KEY, ID)).rejects.toThrow(/Another tab/);
    release(); await owner;
    await expect(writeCreationDraft(original)).rejects.toThrow(/another tab/);
    expect(readCreationDraft()?.profile.purpose).toBe("Lock owner's edit");
  });
});

describe("strict storage validation and failure preservation", async () => {
  it.each([
    null, [], { ...draft(), version: "2" }, { ...draft(), version: 3 }, { ...draft(), revision: -1 },
    { ...draft(), extra: true }, { ...draft(), profile: { name: "Sol" } },
    { ...draft(), attempt: { ...draft().attempt, creationKey: KEY.toUpperCase() } },
    { ...draft(), attempt: { ...draft().attempt, attemptId: OTHER } },
    { ...draft(), attempt: { ...draft().attempt, baselineIds: ["not-empty"] } },
    { ...draft(), attempt: { ...draft().attempt, hiveUrl: "http://user:password@localhost:1" } },
    { ...draft(), attempt: { ...draft().attempt, name: " padded " } },
    { ...draft(), attempt: { ...draft().attempt, name: "bad\nname" } },
    { ...draft(), attempt: { ...draft().attempt, name: "Sol\u0080" } },
    { ...draft(), entityId: "old-id" }, { ...draft(), recoveryEntityId: ID },
    { ...legacy(), attempt: { ...legacy().attempt, creationKey: KEY } },
  ])("retains invalid stored bytes %j and blocks mutations", async record => {
    const raw = JSON.stringify(record); localStorage.setItem(CREATION_DRAFT_KEY, raw);
    expect(() => readCreationDraft()).toThrow(); await expect(writeCreationDraft(draft())).rejects.toThrow();
    await expect(confirmCreationDraft(KEY, ID)).rejects.toThrow(); await expect(clearCreationDraft(KEY)).rejects.toThrow();
    expect(localStorage.getItem(CREATION_DRAFT_KEY)).toBe(raw);
  });
  it("retains malformed legacy bytes without migrating them", async () => {
    sessionStorage.setItem(LEGACY_KEY, "{broken"); expect(() => readCreationDraft()).toThrow(/could not be read/);
    expect(sessionStorage.getItem(LEGACY_KEY)).toBe("{broken"); expect(localStorage.length).toBe(0);
  });
  it("does not fall back to session storage when durable storage is inaccessible", async () => {
    vi.spyOn(window, "localStorage", "get").mockImplementation(() => { throw new Error("Storage blocked"); });
    expect(() => readCreationDraft()).toThrow(/Storage blocked/); await expect(writeCreationDraft(draft())).rejects.toThrow(/Storage blocked/);
    expect(sessionStorage.length).toBe(0);
  });
  it("retains the old journal after write, confirmation, or clear failure", async () => {
    const saved = await writeCreationDraft(draft()); const before = localStorage.getItem(CREATION_DRAFT_KEY);
    const write = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Quota"); });
    await expect(writeCreationDraft({ ...saved, profile: { ...saved.profile, purpose: "Edited" } })).rejects.toThrow(/Quota/);
    await expect(confirmCreationDraft(KEY, ID)).rejects.toThrow(/Quota/); write.mockRestore();
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => { throw new Error("Remove blocked"); });
    await expect(clearCreationDraft(KEY)).rejects.toThrow(/Remove blocked/); expect(localStorage.getItem(CREATION_DRAFT_KEY)).toBe(before);
  });
});
