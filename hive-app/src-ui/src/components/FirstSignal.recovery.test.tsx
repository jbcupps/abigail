import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { readCreationDraft } from "../lib/creationDraft";
import FirstSignal from "./FirstSignal";

const mocks = vi.hoisted(() => ({
  resolveHiveUrl: vi.fn<() => Promise<string>>(),
  fetch: vi.fn<typeof fetch>(),
}));
vi.mock("../lib/connection", () => ({ resolveHiveUrl: mocks.resolveHiveUrl }));
vi.mock("../../../../entity-runtime-app/src-ui/src/presence/Presence", () => ({ default: () => <div /> }));

const HIVE = "http://127.0.0.1:43141";
const OTHER_HIVE = "http://127.0.0.1:43999";
const profile = { ...defaultProfile("Sol"), purpose: "An unsent personal design brief." };
const baseline = [{ id: "already-here", name: "Sol", is_hive: false }];
const callbacks = () => ({ onClose: vi.fn(), onSaved: vi.fn(), onPersisted: vi.fn(), onBusyChange: vi.fn() });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function success(data: unknown): Response {
  return { ok: true, status: 200, json: vi.fn().mockResolvedValue({ ok: true, data }) } as unknown as Response;
}

function calls(method: string) {
  return mocks.fetch.mock.calls.filter(([, init]) => (init?.method ?? "GET") === method);
}

async function flush() {
  await act(async () => { await Promise.resolve(); });
}

async function click(name: string | RegExp) {
  await act(async () => { fireEvent.click(screen.getByRole("button", { name })); });
}

function renderBuilder() {
  const events = callbacks();
  return { ...render(<FirstSignal preview={false} {...events} />), events };
}

async function fillAndCreate() {
  fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: profile.name } });
  fireEvent.change(screen.getByRole("textbox", { name: "What would you like to explore together?" }), { target: { value: profile.purpose } });
  await click("Continue");
  await click("Continue");
  await click("Create Entity");
}

function normalTransport() {
  mocks.fetch.mockImplementation(async (_url, init) => {
    if (init?.method === "POST") return success({ id: "confirmed-entity", directory: "unused" });
    if (init?.method === "PUT") return success(JSON.parse(init.body as string));
    return success(baseline);
  });
}

async function pendingCreation() {
  const post = deferred<Response>();
  mocks.fetch.mockResolvedValueOnce(success(baseline)).mockReturnValueOnce(post.promise);
  const view = renderBuilder();
  await fillAndCreate();
  expect(calls("POST")).toHaveLength(1);
  return { post, ...view };
}

async function restoredCandidate() {
  const previous = await pendingCreation();
  previous.unmount();
  const view = renderBuilder();
  mocks.fetch.mockResolvedValueOnce(success([...baseline, { id: "selected-id", name: "Sol", is_hive: false }]))
    .mockResolvedValueOnce(success(defaultProfile("Existing identity")));
  await click("Check Hive for results");
  await click("Review Entity selected-id");
  return view;
}

function markdownFile(read: () => Promise<string>) {
  const file = new File(["Purpose: A proposed change."], "brief.md", { type: "text/markdown" });
  Object.defineProperty(file, "text", { value: read });
  return file;
}

beforeEach(() => {
  vi.useFakeTimers();
  sessionStorage.clear();
  localStorage.clear();
  mocks.resolveHiveUrl.mockReset().mockResolvedValue(HIVE);
  mocks.fetch.mockReset().mockRejectedValue(new Error("Unexpected network request"));
  vi.stubGlobal("fetch", mocks.fetch);
});

afterEach(async () => {
  cleanup();
  await flush();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  sessionStorage.clear();
  localStorage.clear();
});

describe("First Signal creation recovery with real transport and journal", () => {
  it.each(["cancel", "timeout"] as const)("allows safe retry after %s before POST and ignores late baseline completion", async (reason) => {
    const listing = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(listing.promise);
    renderBuilder();
    await fillAndCreate();
    expect(calls("POST")).toHaveLength(0);

    if (reason === "cancel") await click("Stop waiting");
    else await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });

    expect(screen.getByRole("alert")).toHaveTextContent("No creation request was sent");
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeEnabled();
    expect(readCreationDraft()).toBeNull();
    expect(screen.getByText(profile.purpose)).toBeInTheDocument();
    await act(async () => listing.resolve(success(baseline)));
    expect(calls("POST")).toHaveLength(0);

    normalTransport();
    await click("Create Entity");
    expect(calls("POST")).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
  });

  it("bounds connection resolution before dispatch and keeps the brief available", async () => {
    const address = deferred<string>();
    mocks.resolveHiveUrl.mockReturnValueOnce(address.promise);
    renderBuilder();
    await fillAndCreate();
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });

    expect(screen.getByRole("alert")).toHaveTextContent("No creation request was sent");
    expect(mocks.fetch).not.toHaveBeenCalled();
    await act(async () => address.resolve(HIVE));
    expect(mocks.fetch).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeEnabled();
  });

  it.each(["cancel", "timeout"] as const)("blocks duplicate creation after %s once POST was dispatched", async (reason) => {
    await pendingCreation();
    if (reason === "cancel") await click("Stop waiting");
    else await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });

    expect(screen.getByRole("heading", { name: "Creation outcome not confirmed" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    expect(readCreationDraft()).toMatchObject({ profile, attempt: { hiveUrl: HIVE, name: "Sol" } });
    await click("Create Entity");
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT")).toHaveLength(0);
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
  });

  it("uses one total 30-second deadline across the baseline and POST", async () => {
    const listing = deferred<Response>();
    const post = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(listing.promise).mockReturnValueOnce(post.promise);
    renderBuilder();
    await fillAndCreate();
    await act(async () => { await vi.advanceTimersByTimeAsync(20_000); listing.resolve(success(baseline)); });
    expect(calls("POST")).toHaveLength(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(9_999); });
    expect(screen.getByRole("button", { name: "Stop waiting" })).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });

    expect(screen.queryByRole("button", { name: "Stop waiting" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    expect(calls("POST")).toHaveLength(1);
  });

  it.each(["rejected fetch", "missing Entity ID"])("keeps an uncertain creation blocked after %s, including on remount", async (failure) => {
    const { post, unmount, events } = await pendingCreation();
    await act(async () => {
      if (failure === "rejected fetch") post.reject(new Error("Connection closed after dispatch."));
      else post.resolve(success({ directory: "unused" }));
    });

    expect(screen.getByRole("heading", { name: "Creation outcome not confirmed" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    expect(readCreationDraft()).toMatchObject({ profile, attempt: { hiveUrl: HIVE } });
    expect(readCreationDraft()?.entityId).toBeUndefined();
    expect(calls("PUT")).toHaveLength(0);
    expect(events.onPersisted).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();

    unmount();
    renderBuilder();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    expect(screen.getByText(profile.purpose)).toBeInTheDocument();
    expect(calls("POST")).toHaveLength(1);
  });

  it("records late exact confirmation after stop without saving until explicit finish", async () => {
    const { post, events } = await pendingCreation();
    await click("Stop waiting");
    await act(async () => post.resolve(success({ id: "late-exact-id" })));

    expect(readCreationDraft()?.entityId).toBe("late-exact-id");
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeEnabled();
    expect(calls("PUT")).toHaveLength(0);
    expect(events.onPersisted).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();

    mocks.fetch.mockResolvedValueOnce(success(profile));
    await click("Save brief to confirmed Entity");
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT")[0][0]).toBe(`${HIVE}/v1/entities/late-exact-id/presence`);
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(readCreationDraft()).toBeNull();
    expect(events.onPersisted).toHaveBeenCalledOnce();
  });

  it("retains the confirmed ID after canceling a save and ignores its late success", async () => {
    const saving = deferred<Response>();
    mocks.fetch.mockResolvedValueOnce(success(baseline))
      .mockResolvedValueOnce(success({ id: "created-once" }))
      .mockReturnValueOnce(saving.promise);
    const { events } = renderBuilder();
    await fillAndCreate();
    expect(calls("PUT")).toHaveLength(1);
    await click("Stop waiting");
    await act(async () => saving.resolve(success(profile)));

    expect(readCreationDraft()?.entityId).toBe("created-once");
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeEnabled();
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
    expect(events.onPersisted).not.toHaveBeenCalled();
    mocks.fetch.mockResolvedValueOnce(success(profile));
    await click("Save brief to confirmed Entity");
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT").map(([url]) => url)).toEqual([
      `${HIVE}/v1/entities/created-once/presence`, `${HIVE}/v1/entities/created-once/presence`,
    ]);
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
  });

  it("updates only the journal when confirmation arrives after unmount", async () => {
    const { post, unmount, events } = await pendingCreation();
    unmount();
    await act(async () => post.resolve(success({ id: "confirmed-after-unmount" })));

    expect(readCreationDraft()?.entityId).toBe("confirmed-after-unmount");
    expect(calls("PUT")).toHaveLength(0);
    expect(events.onPersisted).not.toHaveBeenCalled();
    expect(events.onSaved).not.toHaveBeenCalled();
    renderBuilder();
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeEnabled();
    expect(screen.getByText(profile.purpose)).toBeInTheDocument();
  });

  it("restores the draft and creation block after remount, then absorbs a late confirmation", async () => {
    const { post, unmount } = await pendingCreation();
    const originalAttempt = readCreationDraft()?.attempt.attemptId;
    unmount();
    renderBuilder();
    expect(screen.getByText(profile.purpose)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "What should they keep in mind?" }), { target: { value: "Edits made in the restored builder." } });

    await act(async () => post.resolve(success({ id: "late-for-restored-tab" })));

    expect(readCreationDraft()?.attempt.attemptId).toBe(originalAttempt);
    expect(readCreationDraft()?.profile.boundaries).toBe("Edits made in the restored builder.");
    expect(screen.getByRole("textbox", { name: "What should they keep in mind?" })).toHaveValue("Edits made in the restored builder.");
    expect(screen.getByRole("button", { name: "Save brief to confirmed Entity" })).toBeEnabled();
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT")).toHaveLength(0);

    mocks.resolveHiveUrl.mockResolvedValue(OTHER_HIVE);
    mocks.fetch.mockResolvedValueOnce(success({ ...profile, boundaries: "Edits made in the restored builder." }));
    await click("Save brief to confirmed Entity");
    expect(calls("PUT")[0][0]).toBe(`${HIVE}/v1/entities/late-for-restored-tab/presence`);
    expect(JSON.parse(calls("PUT")[0][1]?.body as string)).toMatchObject({ boundaries: "Edits made in the restored builder." });
    expect(calls("POST")).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(readCreationDraft()).toBeNull();
  });

  it.each([{ kind: "empty", rows: [] }, { kind: "ambiguous", rows: [
    { id: "candidate-a", name: "Sol", is_hive: false },
    { id: "candidate-b", name: "Sol", is_hive: false },
  ] }])("never adopts or retries creation from an $kind candidate list", async ({ rows }) => {
    await pendingCreation();
    await click("Stop waiting");
    mocks.fetch.mockResolvedValueOnce(success([...baseline, ...rows]));
    await click("Check Hive for results");

    if (rows.length === 0) expect(screen.getByText(/This does not prove nothing was created/)).toBeInTheDocument();
    else {
      expect(screen.getAllByRole("button", { name: /^Review Entity / })).toHaveLength(2);
      expect(screen.getByText(/A matching name does not prove/)).toBeInTheDocument();
    }
    expect(readCreationDraft()?.entityId).toBeUndefined();
    expect(readCreationDraft()?.recoveryEntityId).toBeUndefined();
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeDisabled();
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT")).toHaveLength(0);
  });

  it.each(["original-late-id", "selected-id"])("reviews and saves an explicit candidate on the pinned Hive, retaining late confirmation %s", async (lateId) => {
    const { post, events } = await pendingCreation();
    await click("Stop waiting");
    const originalAttempt = readCreationDraft()?.attempt.attemptId;
    mocks.resolveHiveUrl.mockResolvedValue(OTHER_HIVE);
    mocks.fetch.mockResolvedValueOnce(success([...baseline, { id: "selected-id", name: "Sol", is_hive: false }]))
      .mockResolvedValueOnce(success({ ...defaultProfile("Existing identity"), purpose: "An existing saved brief." }))
      .mockResolvedValueOnce(success(profile));

    await click("Check Hive for results");
    await click("Review Entity selected-id");
    expect(screen.getByText("Saved purpose: An existing saved brief.")).toBeInTheDocument();
    expect(calls("PUT")).toHaveLength(0);
    await click("Save this brief to this Entity");

    expect(calls("PUT")[0][0]).toBe(`${HIVE}/v1/entities/selected-id/presence`);
    expect(mocks.fetch.mock.calls.every(([url]) => String(url).startsWith(HIVE))).toBe(true);
    expect(readCreationDraft()).toMatchObject({ attempt: { attemptId: originalAttempt }, recoveryEntityId: "selected-id" });
    expect(readCreationDraft()?.entityId).toBeUndefined();
    expect(screen.getByRole("status")).toHaveTextContent("is still unconfirmed");

    await act(async () => post.resolve(success({ id: lateId })));

    expect(readCreationDraft()).toMatchObject({ entityId: lateId, recoveryEntityId: "selected-id" });
    if (lateId === "selected-id") {
      expect(screen.getByRole("status")).not.toHaveTextContent("confirmed a different Entity");
      expect(screen.getByRole("status")).toHaveTextContent("Its unfinished record is still kept in this tab.");
    } else expect(screen.getByRole("status")).toHaveTextContent(`confirmed a different Entity: ${lateId}`);
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT")).toHaveLength(1);
    await click("Continue to conversation");
    expect(events.onSaved).toHaveBeenCalledExactlyOnceWith({ id: "selected-id", profile, hiveUrl: HIVE });
  });

  it.each(["intention", "character"])("locks editable %s controls while saving a restored recovery candidate", async (step) => {
    const { events } = await restoredCandidate();
    await click("Back");
    let controls: HTMLElement[];
    if (step === "intention") {
      await click("Back");
      fireEvent.change(screen.getByLabelText("Import Markdown brief"), {
        target: { files: [markdownFile(() => Promise.resolve("Purpose: A proposed change."))] },
      });
      await flush();
      controls = [
        screen.getByRole("textbox", { name: "What should we call them?" }),
        screen.getByRole("textbox", { name: "What would you like to explore together?" }),
        screen.getByLabelText("Import Markdown brief"),
        screen.getByRole("button", { name: "Apply proposed settings" }),
        screen.getByRole("button", { name: "Discard" }),
      ];
    } else {
      controls = [
        ...within(screen.getByRole("group", { name: "Visual identity" })).getAllByRole("button"),
        ...screen.getAllByRole("radio"),
      ];
    }
    controls.forEach(control => expect(control).toBeEnabled());
    const saving = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(saving.promise);
    await click("Save this brief to this Entity");

    controls.forEach(control => expect(control).toBeDisabled());
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop waiting" })).toBeEnabled();
    expect(calls("PUT")).toHaveLength(1);
    expect(JSON.parse(calls("PUT")[0][1]?.body as string)).toEqual(profile);
    expect(readCreationDraft()?.profile).toEqual(profile);

    await act(async () => saving.resolve(success(profile)));
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(events.onPersisted).toHaveBeenCalledOnce();
    await click("Continue to conversation");
    expect(events.onSaved).toHaveBeenCalledExactlyOnceWith({ id: "selected-id", profile, hiveUrl: HIVE });
  });

  it.each(["while saving", "after stopping"])("ignores an older Markdown read that completes %s, then allows a fresh import", async (timing) => {
    await restoredCandidate();
    await click("Back");
    await click("Back");
    const reading = deferred<string>();
    fireEvent.change(screen.getByLabelText("Import Markdown brief"), {
      target: { files: [markdownFile(() => reading.promise)] },
    });
    const saving = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(saving.promise);
    await click("Save this brief to this Entity");
    if (timing === "after stopping") await click("Stop waiting");
    await act(async () => reading.resolve("Purpose: Stale imported purpose."));

    expect(screen.queryByRole("heading", { name: "Review proposed settings" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "What would you like to explore together?" })).toHaveValue(profile.purpose);
    expect(readCreationDraft()?.profile).toEqual(profile);
    expect(calls("PUT")).toHaveLength(1);
    if (timing === "while saving") await click("Stop waiting");

    expect(screen.getByLabelText("Import Markdown brief")).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Import Markdown brief"), {
      target: { files: [markdownFile(() => Promise.resolve("Purpose: A fresh imported purpose."))] },
    });
    await flush();
    expect(screen.getByRole("heading", { name: "Review proposed settings" })).toBeInTheDocument();
    await click("Apply proposed settings");
    await act(async () => saving.resolve(success(profile)));
    expect(screen.getByRole("textbox", { name: "What would you like to explore together?" })).toHaveValue("A fresh imported purpose.");
    expect(readCreationDraft()?.profile.purpose).toBe("A fresh imported purpose.");
    expect(screen.queryByRole("heading", { name: "Meet Sol." })).not.toBeInTheDocument();
    expect(calls("PUT")).toHaveLength(1);
    expect(calls("POST")).toHaveLength(1);
  });

  it("blocks POST when the attempt cannot be recorded before dispatch", async () => {
    mocks.fetch.mockResolvedValueOnce(success(baseline));
    const storage = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new DOMException("Tab storage unavailable", "QuotaExceededError"); });
    renderBuilder();
    await fillAndCreate();

    expect(screen.getByRole("alert")).toHaveTextContent("No creation request was sent");
    expect(calls("POST")).toHaveLength(0);
    expect(readCreationDraft()).toBeNull();
    expect(screen.getByText(profile.purpose)).toBeInTheDocument();
    storage.mockRestore();
    normalTransport();
    await click("Create Entity");
    expect(calls("POST")).toHaveLength(1);
  });

  it("ignores the old pre-dispatch operation after a new safe retry starts", async () => {
    const oldBaseline = deferred<Response>();
    const newPost = deferred<Response>();
    mocks.fetch.mockReturnValueOnce(oldBaseline.promise).mockResolvedValueOnce(success(baseline)).mockReturnValueOnce(newPost.promise);
    renderBuilder();
    await fillAndCreate();
    await click("Stop waiting");
    await click("Create Entity");
    const currentAttempt = readCreationDraft()?.attempt.attemptId;
    expect(calls("POST")).toHaveLength(1);

    await act(async () => oldBaseline.resolve(success(baseline)));

    expect(screen.getByRole("button", { name: "Stop waiting" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(readCreationDraft()?.attempt.attemptId).toBe(currentAttempt);
    expect(calls("POST")).toHaveLength(1);
    expect(calls("PUT")).toHaveLength(0);
  });
});
