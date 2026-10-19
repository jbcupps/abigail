import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { CREATION_DRAFT_EVENT, clearCreationDraft, confirmCreationDraft, readCreationDraft, writeCreationDraft, type CreationDraft } from "./creationDraft";

const KEY = "abigail.first-signal.creation.v1";

function draft(): CreationDraft {
  return {
    version: 1,
    profile: { ...defaultProfile("Juniper"), purpose: "Help plan my garden." },
    attempt: {
      attemptId: "attempt-one",
      hiveUrl: "http://127.0.0.1:43141",
      name: "Juniper",
      baselineIds: ["hive", "existing-entity"],
    },
  };
}

beforeEach(() => {
  sessionStorage.clear();
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  sessionStorage.clear();
  localStorage.clear();
});

describe("tab-local creation journal", () => {
  it("roundtrips through sessionStorage and a fresh module without localStorage or network", async () => {
    const network = vi.fn();
    vi.stubGlobal("fetch", network);
    expect(readCreationDraft()).toBeNull();
    const submitted = draft();
    writeCreationDraft(submitted);
    expect(readCreationDraft()).toEqual(submitted);
    vi.resetModules();
    const reloaded = await import("./creationDraft");
    expect(reloaded.readCreationDraft()).toEqual(submitted);
    expect(localStorage.length).toBe(0);
    expect(network).not.toHaveBeenCalled();
  });

  it("keeps the latest edited profile and distinct recovery target during late confirmation", () => {
    const original = draft();
    writeCreationDraft(original);
    const edited = {
      ...original,
      profile: { ...original.profile, name: "Juni", purpose: "Plan the greenhouse first." },
      recoveryEntityId: "selected-existing-entity",
    };
    writeCreationDraft(edited);
    const result = confirmCreationDraft(original.attempt.attemptId, "exact-created-entity");
    expect(result).toEqual({ ...edited, entityId: "exact-created-entity" });
    expect(readCreationDraft()).toEqual(result);
    expect(result?.attempt.name).toBe("Juniper");
  });

  it("does not erase confirmed or selected IDs when a stale render omits them", () => {
    const original = draft();
    writeCreationDraft({ ...original, recoveryEntityId: "selected-existing-entity" });
    confirmCreationDraft(original.attempt.attemptId, "exact-created-entity");
    writeCreationDraft({ ...original, profile: { ...original.profile, purpose: "Latest edit" } });
    expect(readCreationDraft()).toEqual({
      ...original,
      profile: { ...original.profile, purpose: "Latest edit" },
      recoveryEntityId: "selected-existing-entity",
      entityId: "exact-created-entity",
    });
  });

  it("permits an explicit recovery selection change without changing exact confirmation", () => {
    const original = draft();
    writeCreationDraft({ ...original, entityId: "exact-created", recoveryEntityId: "candidate-one" });
    writeCreationDraft({ ...original, recoveryEntityId: "candidate-two" });
    expect(readCreationDraft()).toEqual({ ...original, entityId: "exact-created", recoveryEntityId: "candidate-two" });
  });

  it("does not let stale callbacks overwrite, confirm, or clear a different attempt", () => {
    const first = draft();
    writeCreationDraft(first);
    clearCreationDraft(first.attempt.attemptId);
    const second = { ...first, attempt: { ...first.attempt, attemptId: "attempt-two" } };
    writeCreationDraft(second);
    const before = sessionStorage.getItem(KEY);
    expect(confirmCreationDraft(first.attempt.attemptId, "late-created-id")).toBeNull();
    clearCreationDraft(first.attempt.attemptId);
    expect(() => writeCreationDraft(first)).toThrow(/Another creation is unfinished/);
    expect(sessionStorage.getItem(KEY)).toBe(before);
    expect(readCreationDraft()).toEqual(second);
  });

  it("clears only its matching attempt and makes absent confirmation a no-op", () => {
    expect(confirmCreationDraft("absent-attempt", "created-id")).toBeNull();
    writeCreationDraft(draft());
    clearCreationDraft("unrelated-attempt");
    expect(readCreationDraft()).toEqual(draft());
    clearCreationDraft(draft().attempt.attemptId);
    expect(readCreationDraft()).toBeNull();
  });

  it.each([
    ["Hive address", { hiveUrl: "http://127.0.0.1:43142" }],
    ["submitted name", { name: "Another name" }],
    ["baseline", { baselineIds: ["different-entity"] }],
  ])("keeps the dispatched %s immutable", (_label, change) => {
    const original = draft();
    writeCreationDraft(original);
    const before = sessionStorage.getItem(KEY);
    expect(() => writeCreationDraft({ ...original, attempt: { ...original.attempt, ...change } })).toThrow(/cannot be changed/);
    expect(sessionStorage.getItem(KEY)).toBe(before);
  });

  it("rejects a different exact confirmation instead of replacing a known ID", () => {
    const original = draft();
    writeCreationDraft(original);
    confirmCreationDraft(original.attempt.attemptId, "first-confirmed-id");
    const before = sessionStorage.getItem(KEY);
    expect(() => confirmCreationDraft(original.attempt.attemptId, "different-id")).toThrow(/already has a confirmed/);
    expect(() => writeCreationDraft({ ...original, entityId: "different-id" })).toThrow(/already has a confirmed/);
    expect(sessionStorage.getItem(KEY)).toBe(before);
  });
});

describe("invalid records remain available for recovery", () => {
  const invalidRecords: [string, unknown][] = [
    ["missing version", { ...draft(), version: undefined }],
    ["unknown version", { ...draft(), version: 2 }],
    ["null record", null],
    ["array record", []],
    ["unknown record field", { ...draft(), completed: true }],
    ["incomplete profile", { ...draft(), profile: { name: "Juniper" } }],
    ["unknown profile field", { ...draft(), profile: { ...draft().profile, instructions: "not a profile field" } }],
    ["invalid identity", { ...draft(), profile: { ...draft().profile, identity: "unknown" } }],
    ["empty attempt ID", { ...draft(), attempt: { ...draft().attempt, attemptId: "" } }],
    ["padded attempt ID", { ...draft(), attempt: { ...draft().attempt, attemptId: " attempt " } }],
    ["control in attempt ID", { ...draft(), attempt: { ...draft().attempt, attemptId: "attempt\n" } }],
    ["unknown attempt field", { ...draft(), attempt: { ...draft().attempt, completed: true } }],
    ["invalid URL", { ...draft(), attempt: { ...draft().attempt, hiveUrl: "not a URL" } }],
    ["non-HTTP URL", { ...draft(), attempt: { ...draft().attempt, hiveUrl: "file:///tmp/hive" } }],
    ["URL query", { ...draft(), attempt: { ...draft().attempt, hiveUrl: "http://127.0.0.1:43141?other=1" } }],
    ["URL fragment", { ...draft(), attempt: { ...draft().attempt, hiveUrl: "http://127.0.0.1:43141#other" } }],
    ["unpinned URL slash", { ...draft(), attempt: { ...draft().attempt, hiveUrl: "http://127.0.0.1:43141/" } }],
    ["blank name", { ...draft(), attempt: { ...draft().attempt, name: " " } }],
    ["padded name", { ...draft(), attempt: { ...draft().attempt, name: " Juniper " } }],
    ["control in name", { ...draft(), attempt: { ...draft().attempt, name: "Juni\u0000per" } }],
    ["overlong name", { ...draft(), attempt: { ...draft().attempt, name: "x".repeat(81) } }],
    ["missing baseline", { ...draft(), attempt: { ...draft().attempt, baselineIds: undefined } }],
    ["invalid baseline ID", { ...draft(), attempt: { ...draft().attempt, baselineIds: [" "] } }],
    ["duplicate baseline IDs", { ...draft(), attempt: { ...draft().attempt, baselineIds: ["hive", "hive"] } }],
    ["empty Entity ID", { ...draft(), entityId: "" }],
    ["invalid Entity ID type", { ...draft(), entityId: 42 }],
    ["null Entity ID", { ...draft(), entityId: null }],
    ["empty recovery ID", { ...draft(), recoveryEntityId: "" }],
  ];
  it.each(invalidRecords)("rejects %s on read and refuses every mutation", (_label, record) => {
    const raw = JSON.stringify(record);
    sessionStorage.setItem(KEY, raw);
    expect(() => readCreationDraft()).toThrow(/record is invalid/);
    expect(() => writeCreationDraft(draft())).toThrow(/record is invalid/);
    expect(() => confirmCreationDraft(draft().attempt.attemptId, "created-id")).toThrow(/record is invalid/);
    expect(() => clearCreationDraft(draft().attempt.attemptId)).toThrow(/record is invalid/);
    expect(sessionStorage.getItem(KEY)).toBe(raw);
  });

  it.each(["", "{unfinished JSON"])("keeps malformed bytes %j unchanged", raw => {
    sessionStorage.setItem(KEY, raw);
    expect(() => readCreationDraft()).toThrow(/could not be read/);
    expect(() => writeCreationDraft(draft())).toThrow(/could not be read/);
    expect(() => confirmCreationDraft(draft().attempt.attemptId, "created-id")).toThrow(/could not be read/);
    expect(() => clearCreationDraft(draft().attempt.attemptId)).toThrow(/could not be read/);
    expect(sessionStorage.getItem(KEY)).toBe(raw);
  });

  it("rejects invalid outgoing data and IDs without modifying saved bytes", () => {
    writeCreationDraft(draft());
    const before = sessionStorage.getItem(KEY);
    expect(() => writeCreationDraft({ ...draft(), entityId: " bad-id " })).toThrow(/record is invalid/);
    expect(() => confirmCreationDraft("", "created-id")).toThrow(/valid attempt and Entity IDs/);
    expect(() => confirmCreationDraft(draft().attempt.attemptId, "\n")).toThrow(/valid attempt and Entity IDs/);
    expect(() => clearCreationDraft(" ")).toThrow(/valid attempt ID/);
    expect(sessionStorage.getItem(KEY)).toBe(before);
  });
});

describe("storage failures", () => {
  it("propagates inaccessible sessionStorage without falling back to another store", () => {
    const getStorage = vi.spyOn(window, "sessionStorage", "get").mockImplementation(() => {
      throw new DOMException("Storage unavailable", "SecurityError");
    });
    expect(() => readCreationDraft()).toThrow(/Storage unavailable/);
    expect(() => writeCreationDraft(draft())).toThrow(/Storage unavailable/);
    expect(localStorage.length).toBe(0);
    getStorage.mockRestore();
    expect(readCreationDraft()).toBeNull();
  });

  it("never writes or removes after a failed read", () => {
    writeCreationDraft(draft());
    const before = sessionStorage.getItem(KEY);
    const read = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("Read blocked"); });
    const write = vi.spyOn(Storage.prototype, "setItem");
    const remove = vi.spyOn(Storage.prototype, "removeItem");
    expect(() => writeCreationDraft(draft())).toThrow(/Read blocked/);
    expect(() => confirmCreationDraft(draft().attempt.attemptId, "created-id")).toThrow(/Read blocked/);
    expect(() => clearCreationDraft(draft().attempt.attemptId)).toThrow(/Read blocked/);
    expect(write).not.toHaveBeenCalled();
    expect(remove).not.toHaveBeenCalled();
    read.mockRestore();
    expect(sessionStorage.getItem(KEY)).toBe(before);
  });

  it("preserves the prior draft when writing or confirming exceeds quota", () => {
    const original = draft();
    writeCreationDraft(original);
    const before = sessionStorage.getItem(KEY);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new DOMException("Quota exceeded", "QuotaExceededError"); });
    expect(() => writeCreationDraft({ ...original, profile: { ...original.profile, purpose: "Edited" } })).toThrow(/Quota exceeded/);
    expect(() => confirmCreationDraft(original.attempt.attemptId, "created-id")).toThrow(/Quota exceeded/);
    expect(sessionStorage.getItem(KEY)).toBe(before);
    expect(readCreationDraft()).toEqual(original);
  });

  it("surfaces clear failures and retains the saved record", () => {
    writeCreationDraft(draft());
    const before = sessionStorage.getItem(KEY);
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => { throw new Error("Remove blocked"); });
    expect(() => clearCreationDraft(draft().attempt.attemptId)).toThrow(/Remove blocked/);
    expect(sessionStorage.getItem(KEY)).toBe(before);
  });
});

describe("same-tab change notifications", () => {
  it("notifies after successful writes, confirmation, and matching clear with committed data", () => {
    const original = draft();
    const observed: (CreationDraft | null)[] = [];
    const onChange = () => { observed.push(readCreationDraft()); };
    window.addEventListener(CREATION_DRAFT_EVENT, onChange);
    try {
      writeCreationDraft(original);
      const edited = { ...original, profile: { ...original.profile, purpose: "Latest edit" } };
      writeCreationDraft(edited);
      confirmCreationDraft(original.attempt.attemptId, "confirmed-id");
      clearCreationDraft(original.attempt.attemptId);
      expect(observed).toEqual([original, edited, { ...edited, entityId: "confirmed-id" }, null]);
    } finally {
      window.removeEventListener(CREATION_DRAFT_EVENT, onChange);
    }
  });

  it("does not notify for stale or absent callbacks", () => {
    writeCreationDraft(draft());
    const onChange = vi.fn();
    window.addEventListener(CREATION_DRAFT_EVENT, onChange);
    try {
      clearCreationDraft("different-attempt");
      expect(confirmCreationDraft("different-attempt", "late-id")).toBeNull();
      expect(() => writeCreationDraft({ ...draft(), attempt: { ...draft().attempt, attemptId: "different-attempt" } })).toThrow(/Another creation/);
      expect(onChange).not.toHaveBeenCalled();
      clearCreationDraft(draft().attempt.attemptId);
      onChange.mockClear();
      clearCreationDraft(draft().attempt.attemptId);
      expect(confirmCreationDraft(draft().attempt.attemptId, "late-id")).toBeNull();
      expect(onChange).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener(CREATION_DRAFT_EVENT, onChange);
    }
  });

  it("does not announce writes, confirmations, or clears that failed", () => {
    writeCreationDraft(draft());
    const onChange = vi.fn();
    window.addEventListener(CREATION_DRAFT_EVENT, onChange);
    try {
      vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Write failed"); });
      vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => { throw new Error("Remove failed"); });
      expect(() => writeCreationDraft(draft())).toThrow(/Write failed/);
      expect(() => confirmCreationDraft(draft().attempt.attemptId, "confirmed-id")).toThrow(/Write failed/);
      expect(() => clearCreationDraft(draft().attempt.attemptId)).toThrow(/Remove failed/);
      expect(onChange).not.toHaveBeenCalled();
      expect(readCreationDraft()).toEqual(draft());
    } finally {
      window.removeEventListener(CREATION_DRAFT_EVENT, onChange);
    }
  });
});
