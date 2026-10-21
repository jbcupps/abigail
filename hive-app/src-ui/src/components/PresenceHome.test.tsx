import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import type { HiveStatus } from "../lib/daemonClient";
import { writeCreationDraft } from "../lib/creationDraft";
import PresenceHome from "./PresenceHome";
import { ModelRequiredError } from "../lib/modelReadiness";
vi.mock("../lib/connection", () => ({ resolveHiveUrl: async () => "http://127.0.0.1:43141" }));
import { createOrResumeGenesis, genesisAction, resolveGenesisConnection, type GenesisSession } from "../lib/genesisClient";
vi.mock("../lib/genesisClient", async original => ({ ...await original<typeof import("../lib/genesisClient")>(), createOrResumeGenesis: vi.fn(), genesisAction: vi.fn(), resolveGenesisConnection: vi.fn() }));

const mocks = vi.hoisted(() => ({
  loadPresence: vi.fn(),
  openPresence: vi.fn(),
  preparePresence: vi.fn(),
  requirePresenceReady: vi.fn(),
  previewEntities: vi.fn(),
  createPresenceEntity: vi.fn(),
  savePresence: vi.fn(),
  chatMounted: vi.fn(),
  chatDisposed: vi.fn(),
}));

vi.mock("../lib/presenceApi", () => mocks);
vi.mock("../../../../entity-runtime-app/src-ui/src/components/ChatPanel", () => ({
  default: ({ profile, baseUrl, expectedEntityId, profileNotice, onBack }: { profile: EntityProfile; baseUrl: string; expectedEntityId?: string; profileNotice?: string; onBack: () => void }) => {
    useEffect(() => {
      mocks.chatMounted(baseUrl);
      return () => { mocks.chatDisposed(baseUrl); };
    }, [baseUrl]);
    return <section aria-label="Opened conversation" data-expected-entity-id={expectedEntityId}><h1>{profile.name} conversation</h1><p>{baseUrl}</p>{profileNotice && <p>{profileNotice}</p>}<button onClick={onBack}>Return from conversation</button></section>;
  },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

const status: HiveStatus = {
  entity_count: 2,
  entities: [
    { id: "ada", name: "Ada", birth_complete: true, is_hive: false, immortal: false },
    { id: "hive-entity", name: "Abigail Hive", birth_complete: true, is_hive: true, immortal: true },
  ],
  ready_state: "ready",
  any_provider_configured: true,
  setup_complete: true,
  helper: { running: true, local_url: "http://127.0.0.1:43200" },
};
const currentHive = "http://127.0.0.1:43141";

describe("Presence Home pending operations", () => {
  let originalUrl: string;

  beforeEach(() => {
    delete window.isTauri;
    sessionStorage.clear();
    localStorage.clear();
    originalUrl = window.location.href;
    mocks.loadPresence.mockReset().mockResolvedValue(defaultProfile("Ada"));
    mocks.preparePresence.mockReset().mockResolvedValue(undefined);
    mocks.requirePresenceReady.mockReset().mockImplementation(async (_signal: AbortSignal, hiveUrl?: string) => hiveUrl ?? currentHive);
    mocks.openPresence.mockReset().mockResolvedValue("http://127.0.0.1:43201");
    mocks.previewEntities.mockReset().mockReturnValue([]);
    mocks.createPresenceEntity.mockReset().mockResolvedValue("created-entity");
    mocks.savePresence.mockReset().mockResolvedValue(undefined);
    mocks.chatMounted.mockReset();
    mocks.chatDisposed.mockReset();
    vi.mocked(resolveGenesisConnection).mockReset().mockResolvedValue({ base: currentHive, token: "fixture", connection: { hiveUrl: currentHive, profileId: null } });
    vi.mocked(createOrResumeGenesis).mockReset();
    vi.mocked(genesisAction).mockReset();
    const heldLocks = new Set<string>();
    vi.stubGlobal("navigator", { locks: { request: async (name: string, _options: unknown, callback: (lock: object | null) => unknown) => {
      if (heldLocks.has(name)) return callback(null);
      heldLocks.add(name);
      try { return await callback({ name, mode: "exclusive" }); }
      finally { heldLocks.delete(name); }
    } } });
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  });

  afterEach(() => {
    delete window.isTauri;
    cleanup();
    sessionStorage.clear();
    localStorage.clear();
    window.history.replaceState({}, "", originalUrl);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("passes the selected stable Entity ID to chat even when its displayed profile has been renamed", async () => {
    mocks.loadPresence.mockResolvedValue(defaultProfile("Renamed visual identity"));
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("region", { name: "Opened conversation" })).toHaveAttribute("data-expected-entity-id", "ada");
    expect(await screen.findByRole("heading", { name: "Renamed visual identity conversation" })).toBeInTheDocument();
  });

  it("opens and pins the native helper through the Entity connection command", async () => {
    window.isTauri = true;
    mocks.openPresence.mockResolvedValue("http://127.0.0.1:44200");
    const view = render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    await screen.findByRole("heading", { name: "Abigail Hive conversation" });
    expect(mocks.openPresence).toHaveBeenCalledWith("hive-entity", expect.any(AbortSignal));
    expect(screen.getByText("http://127.0.0.1:44200")).toBeInTheDocument();
    view.rerender(<PresenceHome preview={false} status={{ ...status, helper: { running: true, local_url: "http://127.0.0.1:49999" } }} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(screen.getByText("http://127.0.0.1:44200")).toBeInTheDocument();
    expect(screen.queryByText("http://127.0.0.1:49999")).not.toBeInTheDocument();
    expect(mocks.openPresence).toHaveBeenCalledOnce();
  });

  it("shows a refused native helper connection without opening its reported port", async () => {
    window.isTauri = true;
    mocks.openPresence.mockRejectedValue(new Error("Pinned Entity instance changed. Reopen Hive."));
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Pinned Entity instance changed");
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    expect(mocks.chatMounted).not.toHaveBeenCalled();
  });

  it("pins the helper's authoritative Hive Entity ID across port changes", async () => {
    const helperStatus = status;
    const view = render(<PresenceHome preview={false} status={helperStatus} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(screen.getByRole("region", { name: "Opened conversation" })).toHaveAttribute("data-expected-entity-id", "hive-entity");
    view.rerender(<PresenceHome preview={false} status={{ ...helperStatus, helper: { running: true, local_url: "http://127.0.0.1:50002" } }} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Opened conversation" })).toHaveAttribute("data-expected-entity-id", "hive-entity");
    expect(screen.getByText("http://127.0.0.1:50002")).toBeInTheDocument();
    view.rerender(<PresenceHome preview={false} status={{ ...status, entities: status.entities.filter(entity => !entity.is_hive) }} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Opened conversation" })).toHaveAttribute("data-expected-entity-id", "hive-entity");
    expect(screen.queryByText(/Abigail Hive's identity is unavailable/)).not.toBeInTheDocument();
  });

  it.each(["missing", "empty", "whitespace"])("does not open a helper with a URL but a %s authoritative Entity ID", kind => {
    const entities = kind === "missing" ? status.entities.filter(entity => !entity.is_hive)
      : status.entities.map(entity => entity.is_hive ? { ...entity, id: kind === "empty" ? "" : "   " } : entity);
    const onRefresh = vi.fn();
    const view = render(<PresenceHome preview={false} status={{ ...status, entities }} onRefresh={onRefresh} onProviders={vi.fn()} />);
    const ask = screen.getByRole("button", { name: "Ask Abigail" });
    expect(ask).toBeDisabled();
    expect(screen.getByText(/Abigail Hive's identity is unavailable/)).toBeInTheDocument();
    fireEvent.click(ask);
    expect(mocks.chatMounted).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh Hive" }));
    expect(onRefresh).toHaveBeenCalledOnce();
    view.rerender(<PresenceHome preview={false} status={status} onRefresh={onRefresh} onProviders={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Ask Abigail" })).toBeEnabled();
    expect(mocks.chatMounted).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(screen.getByRole("region", { name: "Opened conversation" })).toHaveAttribute("data-expected-entity-id", "hive-entity");
  });

  it("keeps navigation blocked throughout both entity creation and profile saving", async () => {
    const creation = deferred<void>();
    const saving = deferred<void>();
    vi.mocked(createOrResumeGenesis).mockImplementationOnce(async (id, seed, options) => {
      await creation.promise;
      return { id, state: "ready", connection: options.connection, definition: { entity_id: "created-entity", name: seed.name, purpose: seed.purpose } } as GenesisSession;
    });
    mocks.savePresence.mockReturnValueOnce(saving.promise);
    const onProviders = vi.fn();
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={onProviders} />);
    fireEvent.click(screen.getByRole("button", { name: /A new presence/ }));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Sol" } });
    fireEvent.change(screen.getByRole("textbox", { name: "What would you like to explore together?" }), { target: { value: "Explore" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));

    const home = screen.getByRole("link", { name: "Abigail Hive home" });
    expect(home).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("button", { name: "Your Entities" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Settings" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Model connections" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Ask Abigail" })).toBeDisabled();
    expect(screen.queryByRole("textbox", { name: "What should they keep in mind?" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));
    fireEvent.click(screen.getByRole("button", { name: "Model connections" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(fireEvent.click(home)).toBe(false);
    expect(screen.getByRole("button", { name: "Stop waiting" })).toBeInTheDocument();
    expect(onProviders).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();

    await act(async () => { creation.resolve(); });
    await waitFor(() => expect(mocks.savePresence).toHaveBeenCalledExactlyOnceWith("created-entity", { ...defaultProfile("Sol"), purpose: "Explore" }, false, expect.objectContaining({ signal: expect.any(AbortSignal) })));
    expect(screen.getByRole("button", { name: "Your Entities" })).toBeDisabled();
    expect(home).toHaveAttribute("aria-disabled", "true");

    await act(async () => { saving.resolve(); });
    expect(await screen.findByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Your Entities" })).toBeEnabled());
    expect(home).toHaveAttribute("aria-disabled", "false");
  });

  it("returns home in-app without losing the selected Hive URL", () => {
    window.history.replaceState({}, "", "/?hiveUrl=http%3A%2F%2F127.0.0.1%3A43999&session=review");
    const search = window.location.search;
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /A new presence/ }));

    expect(fireEvent.click(screen.getByRole("link", { name: "Abigail Hive home" }))).toBe(false);
    expect(window.location.search).toBe(search);
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "What brings you here?" })).not.toBeInTheDocument();
  });

  it("ignores an entity-open response that arrives after the user returns home", async () => {
    const opening = deferred<string>();
    mocks.openPresence.mockReturnValueOnce(opening.promise);
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    await waitFor(() => expect(mocks.openPresence).toHaveBeenCalledWith("ada", expect.any(AbortSignal), currentHive));

    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));
    await act(async () => { opening.resolve("http://127.0.0.1:43201"); });
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Open conversation/ })).toBeEnabled();
  });

  it("ignores a delayed profile after navigation while opening text independently", async () => {
    const loading = deferred<EntityProfile>();
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    await waitFor(() => expect(mocks.loadPresence).toHaveBeenCalledOnce());
    mocks.loadPresence.mockReturnValueOnce(loading.promise);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    fireEvent.click(screen.getByRole("link", { name: "Abigail Hive home" }));

    await act(async () => { loading.resolve(defaultProfile("Ada")); });
    expect(mocks.openPresence).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
  });

  it("ignores errors from an abandoned open operation", async () => {
    const opening = deferred<string>();
    mocks.openPresence.mockReturnValueOnce(opening.promise);
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    await waitFor(() => expect(mocks.openPresence).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));

    await act(async () => { opening.reject(new Error("Late connection failure")); });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
  });

  it("keeps a new draft separate from an earlier pending identity edit", async () => {
    const loading = deferred<EntityProfile>();
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    await waitFor(() => expect(mocks.loadPresence).toHaveBeenCalledOnce());
    mocks.loadPresence.mockReturnValueOnce(loading.promise);
    fireEvent.click(screen.getByRole("button", { name: "Edit appearance" }));
    fireEvent.click(screen.getByRole("button", { name: /A new presence/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Sol" } });

    await act(async () => { loading.resolve(defaultProfile("Ada")); });
    expect(screen.getByText("CREATE AN ENTITY")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "What should we call them?" })).toHaveValue("Sol");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("button", { name: "Awaken Entity" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save identity" })).not.toBeInTheDocument();
  });

  it("keeps the helper conversation when an earlier entity-open operation finishes", async () => {
    const opening = deferred<string>();
    mocks.openPresence.mockReturnValueOnce(opening.promise);
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    await waitFor(() => expect(mocks.openPresence).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));

    await act(async () => { opening.resolve("http://127.0.0.1:43201"); });
    expect(screen.getByRole("heading", { name: "Abigail Hive conversation" })).toBeInTheDocument();
    expect(screen.getByText("http://127.0.0.1:43200")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Ada conversation" })).not.toBeInTheDocument();
  });

  it("opens text immediately while appearance is loading, then restores the saved identity", async () => {
    const loading = deferred<EntityProfile>();
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    await waitFor(() => expect(mocks.loadPresence).toHaveBeenCalledOnce());
    mocks.loadPresence.mockReturnValueOnce(loading.promise);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("heading", { name: "Ada conversation" })).toBeInTheDocument();
    expect(screen.getByText("Loading saved appearance…")).toBeInTheDocument();

    await act(async () => { loading.resolve({ ...defaultProfile("Vesper"), identity: "ember" }); });
    expect(screen.getByRole("heading", { name: "Vesper conversation" })).toBeInTheDocument();
    expect(screen.queryByText("Loading saved appearance…")).not.toBeInTheDocument();
  });

  it("keeps the conversation usable with an explicit fallback when saved appearance fails", async () => {
    mocks.loadPresence.mockRejectedValue(new Error("Appearance service unavailable"));
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(await screen.findByText("Saved appearance unavailable. Default shown.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("heading", { name: "Ada conversation" })).toBeInTheDocument();
    expect(await screen.findByText(/your saved brief has not been changed/)).toBeInTheDocument();
    expect(mocks.savePresence).not.toHaveBeenCalled();
  });

  it("does not offer a default edit form when the saved profile cannot be read", async () => {
    mocks.loadPresence.mockRejectedValue(new Error("Appearance service unavailable"));
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit appearance" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Appearance service unavailable");
    expect(screen.queryByRole("textbox", { name: "What should we call them?" })).not.toBeInTheDocument();
    expect(mocks.openPresence).not.toHaveBeenCalled();
  });

  it("restores other cards without waiting for a slow profile", async () => {
    const loading = deferred<EntityProfile>();
    mocks.loadPresence.mockImplementation((id: string) => id === "ada" ? loading.promise : Promise.resolve(defaultProfile("Vesper")));
    const twoEntities = { ...status, entities: [...status.entities, { ...status.entities[0], id: "second", name: "Second" }] };
    render(<PresenceHome preview={false} status={twoEntities} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(await screen.findByRole("heading", { name: "Vesper" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ada" })).toBeInTheDocument();
  });

  it("shows a newly saved preview when returning home directly from the reveal", async () => {
    mocks.previewEntities.mockReturnValue([]);
    mocks.savePresence.mockImplementation(async () => {
      mocks.previewEntities.mockReturnValue([{ ...status.entities[0], id: "created-entity", name: "Sol" }]);
      mocks.loadPresence.mockResolvedValue(defaultProfile("Sol"));
    });
    render(<PresenceHome preview status={null} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /A new presence/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Sol" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Save preview identity" }));
    expect(await screen.findByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));
    expect(await screen.findByRole("heading", { name: "Sol" })).toBeInTheDocument();
  });

  it("continues a recovered creation through its original Hive when the current Hive has the same Entity ID", async () => {
    const originalHive = "http://127.0.0.1:43151";
    const currentHive = "http://127.0.0.1:43161";
    const originalRuntime = "http://127.0.0.1:43251";
    const otherRuntime = "http://127.0.0.1:43261";
    window.history.replaceState({}, "", `/?hiveUrl=${encodeURIComponent(currentHive)}`);
    await writeCreationDraft({
      version: 1, profile: defaultProfile("Sol"), entityId: "ada",
      attempt: { attemptId: "resume-on-original-hive", hiveUrl: originalHive, name: "Sol", baselineIds: [] },
    });
    mocks.openPresence.mockImplementation(async (_id: string, _signal: AbortSignal, hiveUrl?: string) => hiveUrl === originalHive ? originalRuntime : otherRuntime);
    render(<PresenceHome preview={false} status={{ ...status, ready_state: "needs_provider", any_provider_configured: false }} onRefresh={vi.fn()} onProviders={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /An unfinished presence/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save brief to confirmed Entity" }));
    expect(await screen.findByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(mocks.savePresence).toHaveBeenCalledExactlyOnceWith("ada", defaultProfile("Sol"), false, expect.objectContaining({ hiveUrl: originalHive }));
    fireEvent.click(screen.getByRole("button", { name: "Continue to conversation" }));

    expect(await screen.findByRole("heading", { name: "Sol conversation" })).toBeInTheDocument();
    expect(mocks.requirePresenceReady).toHaveBeenCalledExactlyOnceWith(expect.any(AbortSignal), originalHive);
    expect(mocks.preparePresence).toHaveBeenCalledExactlyOnceWith("ada", expect.any(AbortSignal), originalHive);
    expect(mocks.openPresence).toHaveBeenCalledExactlyOnceWith("ada", expect.any(AbortSignal), originalHive);
    expect(screen.getByText(originalRuntime)).toBeInTheDocument();
    expect(screen.queryByText(otherRuntime)).not.toBeInTheDocument();
    expect(mocks.createPresenceEntity).not.toHaveBeenCalled();
    expect(new URLSearchParams(window.location.search).get("hiveUrl")).toBe(currentHive);
  });
  it("keeps provider setup available and prevents opening without a configured model", async () => {
    const onProviders = vi.fn();
    render(<PresenceHome preview={false} status={{ ...status, ready_state: "needs_provider", any_provider_configured: false }} onRefresh={vi.fn()} onProviders={onProviders} />);
    expect(screen.getByRole("button", { name: "Choose model to chat" })).toBeEnabled();
    expect(screen.getByRole("status", { name: "Model connection" })).toHaveTextContent("No model connected");
    expect(screen.queryByRole("button", { name: "Ask Abigail" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Connect a model" }));
    expect(onProviders).toHaveBeenCalledOnce();
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Choose model to chat" }));
    expect(onProviders).toHaveBeenCalledTimes(2);
  });

  it("shows the actual saved model separately from Hive connectivity", () => {
    render(<PresenceHome preview={false} status={{ ...status, model_connection: { provider: "openai", model: "selected-fixture-model" } }} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(screen.getByText("HIVE CONNECTED")).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Model connection" })).toHaveTextContent("Model selected: OpenAI · selected-fixture-model");
  });

  it("offers setup after a fresh missing-model response without starting or changing the Entity", async () => {
    mocks.requirePresenceReady.mockRejectedValueOnce(new ModelRequiredError(currentHive));
    const onProviders = vi.fn();
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={onProviders} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("status", { name: "Conversation needs a model" })).toHaveTextContent("Your saved identity is still here");
    fireEvent.click(screen.getByRole("button", { name: "Choose model for this conversation" }));
    expect(onProviders).toHaveBeenCalledOnce();
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();
    expect(mocks.savePresence).not.toHaveBeenCalled();
    expect(mocks.createPresenceEntity).not.toHaveBeenCalled();
  });

  it("does not offer to configure this Hive for a creation pinned to another Hive", async () => {
    mocks.requirePresenceReady.mockRejectedValueOnce(new ModelRequiredError("http://127.0.0.1:43991"));
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("status", { name: "Conversation needs a model" })).toHaveTextContent("different Hive");
    expect(screen.queryByRole("button", { name: "Choose model for this conversation" })).not.toBeInTheDocument();
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();
  });

  it("does not open the runtime after failed setup", async () => {
    mocks.preparePresence.mockRejectedValueOnce(new Error("Entity setup could not be confirmed"));
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("setup could not be confirmed");
    expect(mocks.openPresence).not.toHaveBeenCalled();
  });

  it.each(["Continue to conversation", "Skip reveal"])("checks the pinned Hive after saving without a model via %s", async (action) => {
    await writeCreationDraft({
      version: 1, profile: defaultProfile("Sol"), entityId: "ada",
      attempt: { attemptId: "confirmed-without-model", hiveUrl: currentHive, name: "Sol", baselineIds: [] },
    });
    mocks.requirePresenceReady.mockRejectedValueOnce(new ModelRequiredError(currentHive));
    render(<PresenceHome preview={false} status={{ ...status, ready_state: "needs_provider", any_provider_configured: false }} onRefresh={vi.fn()} onProviders={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /An unfinished presence/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save brief to confirmed Entity" }));
    expect(await screen.findByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(mocks.savePresence).toHaveBeenCalledExactlyOnceWith("ada", defaultProfile("Sol"), false, expect.objectContaining({ hiveUrl: currentHive }));
    expect(mocks.requirePresenceReady).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: action }));
    expect(await screen.findByRole("status", { name: "Conversation needs a model" })).toHaveTextContent("Your saved identity is still here");
    expect(screen.queryByText("Could not complete that step.")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose model for this conversation" })).toBeEnabled();
    expect(mocks.requirePresenceReady).toHaveBeenCalledExactlyOnceWith(expect.any(AbortSignal), currentHive);
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
  });

  it("keeps identity editing and saving available without checking a model", async () => {
    render(<PresenceHome preview={false} status={{ ...status, ready_state: "needs_provider", any_provider_configured: false }} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit appearance" }));
    expect(await screen.findByRole("textbox", { name: "What should we call them?" })).toHaveValue("Ada");
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Edited Ada" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Save identity" }));
    expect(await screen.findByRole("heading", { name: "Meet Edited Ada." })).toBeInTheDocument();
    expect(mocks.savePresence).toHaveBeenCalledOnce();
    expect(mocks.requirePresenceReady).not.toHaveBeenCalled();
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();
  });

  it("uses the exact readiness-checked Hive for appearance, setup and opening", async () => {
    const checkedHive = "http://127.0.0.1:43991";
    mocks.requirePresenceReady.mockResolvedValueOnce(checkedHive);
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    await waitFor(() => expect(mocks.loadPresence).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    expect(await screen.findByRole("region", { name: "Opened conversation" })).toBeInTheDocument();
    expect(mocks.loadPresence).toHaveBeenLastCalledWith("ada", false, expect.any(AbortSignal), checkedHive);
    expect(mocks.preparePresence).toHaveBeenCalledExactlyOnceWith("ada", expect.any(AbortSignal), checkedHive);
    expect(mocks.openPresence).toHaveBeenCalledExactlyOnceWith("ada", expect.any(AbortSignal), checkedHive);
  });

  it("ignores readiness confirmation after navigation without preparing or opening", async () => {
    const readiness = deferred<string>();
    mocks.requirePresenceReady.mockReturnValueOnce(readiness.promise);
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Open conversation/ }));
    const signal = mocks.requirePresenceReady.mock.calls[0][0] as AbortSignal;
    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));
    expect(signal.aborted).toBe(true);
    await act(async () => { readiness.resolve(currentHive); });
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
  });

  it("replaces an active helper connection when polling reports a different port", () => {
    const props = { preview: false, onRefresh: vi.fn(), onProviders: vi.fn() };
    const view = render(<PresenceHome {...props} status={status} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(screen.getByText("http://127.0.0.1:43200")).toBeInTheDocument();

    view.rerender(<PresenceHome {...props} status={{ ...status, helper: { running: true, local_url: "http://127.0.0.1:43299" } }} />);
    expect(screen.getByText("http://127.0.0.1:43299")).toBeInTheDocument();
    expect(screen.queryByText("http://127.0.0.1:43200")).not.toBeInTheDocument();
    expect(mocks.chatDisposed).toHaveBeenCalledExactlyOnceWith("http://127.0.0.1:43200");
    expect(mocks.chatMounted.mock.calls).toEqual([["http://127.0.0.1:43200"], ["http://127.0.0.1:43299"]]);
    expect(mocks.openPresence).not.toHaveBeenCalled();
    expect(mocks.preparePresence).not.toHaveBeenCalled();
  });

  it.each([
    ["stopped helper", { ...status, helper: { running: false, local_url: status.helper!.local_url } }],
    ["missing helper address", { ...status, helper: { running: true, local_url: null } }],
    ["provider disconnected", { ...status, ready_state: "needs_provider", any_provider_configured: false }],
    ["unconfigured status", { ...status, any_provider_configured: false }],
  ] satisfies Array<[string, HiveStatus]>)("unmounts the active helper for %s and restores only its connection when ready", (_label, unavailable) => {
    const props = { preview: false, onRefresh: vi.fn(), onProviders: vi.fn() };
    const view = render(<PresenceHome {...props} status={status} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));

    view.rerender(<PresenceHome {...props} status={unavailable} />);
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Abigail Hive is unavailable." })).toBeInTheDocument();
    expect(screen.getByText(/No messages will be sent automatically/)).toBeInTheDocument();
    expect(mocks.chatDisposed).toHaveBeenCalledExactlyOnceWith("http://127.0.0.1:43200");
    expect(screen.queryByRole("button", { name: "Ask Abigail" })).not.toBeInTheDocument();
    if (unavailable.ready_state === "needs_provider") {
      fireEvent.click(within(screen.getByRole("status", { name: "Model connection" })).getByRole("button", { name: "Connect a model" }));
      expect(props.onProviders).toHaveBeenCalledOnce();
    }

    view.rerender(<PresenceHome {...props} status={status} />);
    expect(screen.getByRole("region", { name: "Opened conversation" })).toBeInTheDocument();
    expect(mocks.chatMounted).toHaveBeenCalledTimes(2);
    expect(mocks.openPresence).not.toHaveBeenCalled();
    expect(mocks.preparePresence).not.toHaveBeenCalled();
    expect(mocks.requirePresenceReady).not.toHaveBeenCalled();
  });

  it("does not reopen a recovered helper after the user returns home", () => {
    const props = { preview: false, onRefresh: vi.fn(), onProviders: vi.fn() };
    const view = render(<PresenceHome {...props} status={status} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    view.rerender(<PresenceHome {...props} status={{ ...status, helper: { running: false } }} />);
    fireEvent.click(screen.getByRole("button", { name: "Back to Your Entities" }));
    view.rerender(<PresenceHome {...props} status={status} />);
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
    expect(mocks.chatMounted).toHaveBeenCalledOnce();
  });

  it("labels an explicit synthetic HTTP fixture separately from local design preview", () => {
    render(<PresenceHome preview={false} status={{ ...status, fixture: true, any_provider_configured: false }} onRefresh={vi.fn()} onProviders={vi.fn()} />);
    expect(screen.getAllByText("SYNTHETIC TRANSPORT")).toHaveLength(2);
    expect(screen.getByText(/No model or provider is connected to this fixture/)).toBeInTheDocument();
    expect(screen.queryByText("HIVE CONNECTED")).not.toBeInTheDocument();
    expect(screen.queryByText("LOCAL DESIGN PREVIEW")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(screen.getByRole("region", { name: "Opened conversation" })).toBeInTheDocument();
    expect(screen.getByText(/Replies are scripted test data/)).toBeInTheDocument();
  });

});
