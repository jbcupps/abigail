import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { CREATION_DRAFT_KEY, clearCreationDraft, readCreationDraft, writeCreationDraft, type CreationDraft } from "../lib/creationDraft";
import FirstSignal from "./FirstSignal";
const mocks = vi.hoisted(() => ({ resolveHiveUrl: vi.fn(), fetch: vi.fn() }));
vi.mock("../lib/connection", () => ({ resolveHiveUrl: mocks.resolveHiveUrl }));
const HIVE = "http://127.0.0.1:43141";
const KEY = "f2e75c9c-d4a7-433e-a380-69e4a658208a";
const ID = "37c73c8e-f9b9-48c3-9e53-0b3b8ce09455";
const OTHER = "37c73c8e-f9b9-48c3-9e53-0b3b8ce09456";
const profile = { ...defaultProfile("Sol"), purpose: "Plan my garden." };
const attempt = { attemptId: KEY, creationKey: KEY, hiveUrl: HIVE, name: "Sol", baselineIds: [] };
const draft = (): CreationDraft => ({ version: 2, profile, attempt });
const success = (data: unknown, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => ({ ok: true, data }) }) as Response;
const created = { id: ID, directory: "entities/test", creation_key: KEY };
const complete = { ...created, name: "Sol", state: "complete" };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { resolve, promise }; }
const calls = (method: string) => mocks.fetch.mock.calls.filter(([, init]) => (init?.method ?? "GET") === method);
async function click(name: string) { await act(async () => { fireEvent.click(screen.getByRole("button", { name })); }); }
function renderBuilder() { const events = { onClose: vi.fn(), onSaved: vi.fn(), onPersisted: vi.fn(), onBusyChange: vi.fn() }; return { ...render(<FirstSignal preview={false} {...events} />), events }; }
async function create() {
  fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Sol" } });
  fireEvent.change(screen.getByRole("textbox", { name: "What would you like to explore together?" }), { target: { value: profile.purpose } });
  await click("Continue"); await click("Continue"); await click("Create Entity");
}
function normalTransport() {
  mocks.fetch.mockImplementation(async (url: string, init?: RequestInit) => {
    if (url.includes("/v1/entity-creations/")) return success({ creation_key: KEY, state: "absent" });
    if (init?.method === "POST") return success(created);
    if (init?.method === "PUT") return success(JSON.parse(init.body as string));
    throw new Error("Unexpected request");
  });
}
function externalChange(value: CreationDraft | null) {
  if (value) localStorage.setItem(CREATION_DRAFT_KEY, JSON.stringify(value)); else localStorage.removeItem(CREATION_DRAFT_KEY);
  window.dispatchEvent(new StorageEvent("storage", { key: CREATION_DRAFT_KEY, storageArea: localStorage, newValue: value ? JSON.stringify(value) : null }));
}
beforeEach(() => {
  sessionStorage.clear(); localStorage.clear(); mocks.fetch.mockReset(); mocks.resolveHiveUrl.mockReset().mockResolvedValue(HIVE);
  vi.stubGlobal("fetch", mocks.fetch);
  let held = false;
  vi.stubGlobal("navigator", { locks: { request: (_name: string, _options: unknown, callback: (value: object | null) => unknown) => {
    if (held) return Promise.resolve(callback(null)); held = true;
    return Promise.resolve(callback({ name: "lock" })).finally(() => { held = false; });
  } } });
  vi.spyOn(crypto, "randomUUID").mockReturnValue(KEY);
  vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  normalTransport();
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); sessionStorage.clear(); localStorage.clear(); });

describe("exact keyed recovery through the real frontend client", () => {
  it("creates, confirms, saves and reveals only after both writes succeed", async () => {
    const { events } = renderBuilder(); await create();
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(mocks.fetch.mock.calls[0][0]).toBe(`${HIVE}/v1/entity-creations/${KEY}`);
    expect(JSON.parse(calls("POST")[0][1].body)).toEqual({ name: "Sol", creation_key: KEY });
    expect(calls("PUT")).toHaveLength(1); expect(readCreationDraft()).toBeNull();
    expect(events.onSaved).not.toHaveBeenCalled(); await click("Continue to conversation");
    expect(events.onSaved).toHaveBeenCalledWith({ id: ID, profile, hiveUrl: HIVE });
  });
  it("blocks an older daemon before POST and displays an actual error state", async () => {
    mocks.fetch.mockResolvedValue(success({}, 404)); renderBuilder(); await create();
    expect(screen.getByRole("alert")).toHaveTextContent("No creation request was sent");
    expect(calls("POST")).toHaveLength(0); expect(readCreationDraft()).toBeNull();
    expect(screen.getByRole("img", { name: "Creation needs attention" }).parentElement).toHaveClass("state-error");
  });
  it("restores a lost POST across tab close and uses exact lookup without automatically saving", async () => {
    mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "absent" })).mockRejectedValueOnce(new Error("Connection lost"));
    const first = renderBuilder(); await create();
    expect(readCreationDraft()).toMatchObject({ version: 2, attempt }); first.unmount(); sessionStorage.clear();
    mocks.fetch.mockResolvedValueOnce(success(complete)); const second = renderBuilder();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    await click("Check this creation"); expect(readCreationDraft()?.entityId).toBe(ID);
    expect(calls("POST")).toHaveLength(1); expect(calls("PUT")).toHaveLength(0);
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
    await click("Save brief to confirmed Entity");
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument(); expect(second.events.onPersisted).toHaveBeenCalledOnce();
  });
  it.each(["absent", "reserved"])("explicitly retries %s with unchanged original name and key despite edited display name", async state => {
    await writeCreationDraft(draft()); renderBuilder(); await click("Back"); await click("Back");
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Edited Sol" } });
    await click("Continue"); await click("Continue");
    mocks.fetch.mockResolvedValueOnce(success(state === "absent" ? { creation_key: KEY, state } : { creation_key: KEY, state, name: "Sol" }));
    await click("Retry the same creation safely");
    expect(JSON.parse(calls("POST")[0][1].body)).toEqual({ name: "Sol", creation_key: KEY });
    expect(calls("PUT")).toHaveLength(0); expect(readCreationDraft()?.profile.name).toBe("Edited Sol");
    await click("Save brief to confirmed Entity");
    expect(JSON.parse(calls("PUT")[0][1].body).name).toBe("Edited Sol"); expect(screen.getByRole("heading", { name: "Meet Edited Sol." })).toBeInTheDocument();
  });
  it("already complete retry confirms without another POST or automatic brief save", async () => {
    await writeCreationDraft(draft()); mocks.fetch.mockResolvedValue(success(complete)); renderBuilder();
    await click("Retry the same creation safely");
    expect(readCreationDraft()?.entityId).toBe(ID); expect(calls("POST")).toHaveLength(0); expect(calls("PUT")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeEnabled();
  });
  it("unavailable retains the attempt and offers no replacement", async () => {
    await writeCreationDraft(draft()); mocks.fetch.mockResolvedValue(success({ creation_key: KEY, state: "unavailable", name: "Sol", id: ID })); renderBuilder();
    await click("Check this creation");
    expect(screen.getByText(/No replacement or new key/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry the same creation safely" })).not.toBeInTheDocument();
    expect(readCreationDraft()?.attempt.creationKey).toBe(KEY); expect(calls("POST")).toHaveLength(0);
  });
  it("a malformed exact lookup never treats a matching name as confirmation", async () => {
    await writeCreationDraft(draft()); mocks.fetch.mockResolvedValue(success({ ...complete, creation_key: OTHER })); renderBuilder();
    await click("Check this creation"); expect(readCreationDraft()?.entityId).toBeUndefined(); expect(calls("PUT")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
  });
  it("keeps the journal through retry conflict without rekeying", async () => {
    await writeCreationDraft(draft()); mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "reserved", name: "Sol" })).mockResolvedValueOnce(success({}, 409)); renderBuilder();
    await click("Retry the same creation safely"); expect(readCreationDraft()?.attempt.creationKey).toBe(KEY);
    expect(calls("POST")).toHaveLength(1); expect(screen.getByRole("alert")).toHaveTextContent("409");
  });
});

describe("stop, deadlines, late callbacks, and cross-tab edits", () => {
  it("Stop is immediate; late POST confirmation is evidence only", async () => {
    const post = deferred<Response>(); mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "absent" })).mockReturnValueOnce(post.promise);
    const { events } = renderBuilder(); await create();
    expect(screen.getByRole("img", { name: "Creating Entity" }).parentElement).toHaveClass("state-thinking");
    await click("Stop waiting"); expect(screen.queryByRole("button", { name: "Stop waiting" })).not.toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Waiting stopped/ }).parentElement).toHaveClass("state-interrupted");
    await act(async () => post.resolve(success(created)));
    expect(readCreationDraft()?.entityId).toBe(ID); expect(calls("PUT")).toHaveLength(0);
    expect(events.onPersisted).not.toHaveBeenCalled(); expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
  });
  it("unmounted POST can preserve exact late evidence but never save or reveal", async () => {
    const post = deferred<Response>(); mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "absent" })).mockReturnValueOnce(post.promise);
    const first = renderBuilder(); await create(); first.unmount(); await act(async () => post.resolve(success(created)));
    expect(readCreationDraft()?.entityId).toBe(ID); expect(calls("PUT")).toHaveLength(0); expect(first.events.onPersisted).not.toHaveBeenCalled();
    sessionStorage.clear(); renderBuilder(); expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeEnabled();
  });
  it("Stop before the capability probe completes never dispatches POST", async () => {
    const probe = deferred<Response>(); mocks.fetch.mockReturnValueOnce(probe.promise); renderBuilder(); await create(); await click("Stop waiting");
    await act(async () => probe.resolve(success({ creation_key: KEY, state: "absent" })));
    expect(readCreationDraft()).toBeNull(); expect(calls("POST")).toHaveLength(0); expect(screen.getByRole("button", { name: "Create Entity" })).toBeEnabled();
  });
  it("bounds a stalled POST and retains the key for a deliberate retry", async () => {
    vi.useFakeTimers(); mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "absent" })).mockReturnValueOnce(new Promise(() => undefined)); renderBuilder(); await create();
    await act(async () => vi.advanceTimersByTimeAsync(30_000));
    expect(screen.getByRole("alert")).toHaveTextContent(/timed out/); expect(readCreationDraft()?.attempt.creationKey).toBe(KEY);
    expect(screen.getByRole("button", { name: "Retry the same creation safely" })).toBeEnabled(); expect(calls("POST")).toHaveLength(1);
  });
  it("bounds a stalled brief save and reuses the confirmed Entity", async () => {
    vi.useFakeTimers(); await writeCreationDraft({ ...draft(), entityId: ID }); mocks.fetch.mockReturnValueOnce(new Promise(() => undefined)); renderBuilder();
    await click("Save brief to confirmed Entity"); await act(async () => vi.advanceTimersByTimeAsync(5_000));
    expect(screen.getByRole("alert")).toHaveTextContent(/saving its brief is not confirmed/); expect(readCreationDraft()?.entityId).toBe(ID);
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument(); expect(calls("POST")).toHaveLength(0);
  });
  it("late stopped brief-save response never reveals or clears the record", async () => {
    await writeCreationDraft({ ...draft(), entityId: ID }); const saving = deferred<Response>(); mocks.fetch.mockReturnValueOnce(saving.promise); const { events } = renderBuilder();
    await click("Save brief to confirmed Entity"); await click("Stop waiting");
    await act(async () => saving.resolve(success(profile))); expect(readCreationDraft()?.entityId).toBe(ID);
    expect(events.onPersisted).not.toHaveBeenCalled(); expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
  });
  it("cross-tab changes stop an in-flight POST, preserve newer edits, and require reload", async () => {
    const post = deferred<Response>(); mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "absent" })).mockReturnValueOnce(post.promise); renderBuilder(); await create();
    const saved = readCreationDraft()!; const newer = { ...saved, revision: saved.revision! + 1, profile: { ...profile, purpose: "Other tab's purpose" } };
    await act(async () => externalChange(newer)); await act(async () => post.resolve(success(created)));
    expect(readCreationDraft()?.profile.purpose).toBe("Other tab's purpose"); expect(calls("PUT")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeDisabled(); await click("Reload saved creation");
    expect(screen.getByText("Other tab's purpose")).toBeInTheDocument(); await click("Save brief to confirmed Entity");
    expect(JSON.parse(calls("PUT")[0][1].body).purpose).toBe("Other tab's purpose");
  });
  it("a stale callback cannot overwrite a different tab's new key", async () => {
    const post = deferred<Response>(); mocks.fetch.mockResolvedValueOnce(success({ creation_key: KEY, state: "absent" })).mockReturnValueOnce(post.promise); renderBuilder(); await create();
    const other = { ...draft(), revision: 1, attempt: { ...attempt, attemptId: OTHER, creationKey: OTHER }, profile: { ...profile, name: "Other" } };
    await act(async () => externalChange(other)); await act(async () => post.resolve(success(created)));
    expect(readCreationDraft()).toEqual(other); expect(calls("PUT")).toHaveLength(0); expect(screen.queryByRole("heading", { name: /^Meet / })).not.toBeInTheDocument();
  });
  it("a removed cross-tab record blocks stale save and cannot be resurrected", async () => {
    await writeCreationDraft({ ...draft(), entityId: ID }); renderBuilder(); await act(async () => externalChange(null)); await click("Reload saved creation");
    expect(screen.getAllByRole("alert").some(element => element.textContent?.includes("record was removed"))).toBe(true);
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeDisabled(); expect(readCreationDraft()).toBeNull(); expect(mocks.fetch).not.toHaveBeenCalled();
  });
  it("holds the shared lock through PUT so competing edits or clears cannot pass before storage events", async () => {
    await writeCreationDraft({ ...draft(), entityId: ID }); const saving = deferred<Response>(); mocks.fetch.mockReturnValueOnce(saving.promise); renderBuilder();
    await click("Save brief to confirmed Entity"); const inFlight = readCreationDraft()!;
    await expect(writeCreationDraft({ ...inFlight, profile: { ...profile, purpose: "Concurrent edit" } })).rejects.toThrow(/Another tab/);
    await expect(clearCreationDraft(KEY)).rejects.toThrow(/Another tab/);
    expect(readCreationDraft()?.profile).toEqual(profile); expect(calls("PUT")).toHaveLength(1);
    await act(async () => saving.resolve(success(profile)));
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument(); expect(readCreationDraft()).toBeNull();
  });
  it("checks the journal after PUT even when a bypassing writer's storage event has not arrived", async () => {
    await writeCreationDraft({ ...draft(), entityId: ID }); const saving = deferred<Response>(); mocks.fetch.mockReturnValueOnce(saving.promise); const { events } = renderBuilder();
    await click("Save brief to confirmed Entity"); const saved = readCreationDraft()!;
    const newer = { ...saved, revision: saved.revision! + 1, profile: { ...profile, purpose: "Unannounced newer edit" } };
    localStorage.setItem(CREATION_DRAFT_KEY, JSON.stringify(newer));
    await act(async () => saving.resolve(success(profile)));
    expect(readCreationDraft()).toEqual(newer); expect(events.onPersisted).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reload saved creation" })).toBeInTheDocument();
  });
  it("blocks POST if durable journal persistence fails", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage blocked"); }); renderBuilder(); await create();
    expect(screen.getByRole("alert")).toHaveTextContent("No creation request was sent"); expect(calls("POST")).toHaveLength(0);
  });
  it("makes unavailable cross-tab coordination visible without a network request", async () => {
    vi.stubGlobal("navigator", {}); renderBuilder(); await create();
    expect(screen.getByRole("alert")).toHaveTextContent("Web Locks"); expect(mocks.fetch).not.toHaveBeenCalled(); expect(screen.getByRole("button", { name: "Create Entity" })).toBeEnabled();
  });
});

describe("legacy unkeyed recovery remains separate", () => {
  it("migrates, checks name candidates without confirmation, and only saves an explicitly reviewed target", async () => {
    const legacy: CreationDraft = { version: 1, profile, attempt: { attemptId: "legacy", hiveUrl: HIVE, name: "Sol", baselineIds: ["old"] } };
    sessionStorage.setItem("abigail.first-signal.creation.v1", JSON.stringify(legacy));
    mocks.fetch.mockResolvedValueOnce(success([{ id: "old", name: "Sol", is_hive: false }, { id: "candidate", name: "Sol", is_hive: false }])).mockResolvedValueOnce(success({ ...profile, purpose: "Existing purpose" }));
    renderBuilder(); expect(sessionStorage.length).toBe(0); expect(readCreationDraft()?.version).toBe(1);
    expect(screen.queryByRole("button", { name: "Retry the same creation safely" })).not.toBeInTheDocument();
    await click("Check Hive for results"); expect(readCreationDraft()?.entityId).toBeUndefined(); expect(calls("POST")).toHaveLength(0);
    await click("Review Entity candidate"); const recovery = screen.getByRole("region", { name: "Creation recovery" });
    expect(within(recovery).getByText(/Existing purpose/)).toBeInTheDocument(); expect(calls("PUT")).toHaveLength(0);
    await click("Save this brief to this Entity");
    expect(calls("PUT")).toHaveLength(1); expect(readCreationDraft()?.entityId).toBeUndefined(); expect(readCreationDraft()?.recoveryEntityId).toBe("candidate");
    expect(calls("POST")).toHaveLength(0);
  });
});
