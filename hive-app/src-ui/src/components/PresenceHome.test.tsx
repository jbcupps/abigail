import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import type { HiveStatus } from "../lib/daemonClient";
import { writeCreationDraft } from "../lib/creationDraft";
import PresenceHome from "./PresenceHome";

const mocks = vi.hoisted(() => ({
  loadPresence: vi.fn(),
  openPresence: vi.fn(),
  previewEntities: vi.fn(),
  createPresenceEntity: vi.fn(),
  savePresence: vi.fn(),
}));

vi.mock("../lib/presenceApi", () => mocks);
vi.mock("../../../../entity-runtime-app/src-ui/src/components/ChatPanel", () => ({
  default: ({ profile, baseUrl, profileNotice, onBack }: { profile: EntityProfile; baseUrl: string; profileNotice?: string; onBack: () => void }) => <section aria-label="Opened conversation"><h1>{profile.name} conversation</h1><p>{baseUrl}</p>{profileNotice && <p>{profileNotice}</p>}<button onClick={onBack}>Return from conversation</button></section>,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

const status: HiveStatus = {
  entity_count: 1,
  entities: [{ id: "ada", name: "Ada", birth_complete: true, is_hive: false, immortal: false }],
  ready_state: "ready",
  any_provider_configured: true,
  setup_complete: true,
  helper: { running: true, local_url: "http://127.0.0.1:43200" },
};

describe("Presence Home pending operations", () => {
  let originalUrl: string;

  beforeEach(() => {
    sessionStorage.clear();
    originalUrl = window.location.href;
    mocks.loadPresence.mockReset().mockResolvedValue(defaultProfile("Ada"));
    mocks.openPresence.mockReset().mockResolvedValue("http://127.0.0.1:43201");
    mocks.previewEntities.mockReset().mockReturnValue([]);
    mocks.createPresenceEntity.mockReset().mockResolvedValue("created-entity");
    mocks.savePresence.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  });

  afterEach(() => {
    cleanup();
    sessionStorage.clear();
    window.history.replaceState({}, "", originalUrl);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps navigation blocked throughout both entity creation and profile saving", async () => {
    const creation = deferred<string>();
    const saving = deferred<void>();
    mocks.createPresenceEntity.mockReturnValueOnce(creation.promise);
    mocks.savePresence.mockReturnValueOnce(saving.promise);
    const onProviders = vi.fn();
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={onProviders} />);
    fireEvent.click(screen.getByRole("button", { name: /A new presence/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Sol" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Create Entity" }));

    const home = screen.getByRole("link", { name: "Abigail Hive home" });
    expect(home).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("button", { name: "Your Entities" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Model connections" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Ask Abigail" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "What should they keep in mind?" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));
    fireEvent.click(screen.getByRole("button", { name: "Model connections" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask Abigail" }));
    expect(fireEvent.click(home)).toBe(false);
    expect(screen.getByRole("heading", { name: "Make it yours." })).toBeInTheDocument();
    expect(onProviders).not.toHaveBeenCalled();
    expect(mocks.openPresence).not.toHaveBeenCalled();

    await act(async () => { creation.resolve("created-entity"); });
    expect(mocks.savePresence).toHaveBeenCalledExactlyOnceWith("created-entity", defaultProfile("Sol"), false, expect.objectContaining({ signal: expect.any(AbortSignal) }));
    expect(screen.getByRole("button", { name: "Your Entities" })).toBeDisabled();
    expect(home).toHaveAttribute("aria-disabled", "true");

    await act(async () => { saving.resolve(); });
    expect(screen.getByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Your Entities" })).toBeEnabled();
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
    await waitFor(() => expect(mocks.openPresence).toHaveBeenCalledWith("ada", expect.any(AbortSignal), undefined));

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
    expect(mocks.openPresence).toHaveBeenCalledOnce();
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
    fireEvent.click(screen.getByRole("button", { name: "Edit identity" }));
    fireEvent.click(screen.getByRole("button", { name: /A new presence/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Sol" } });

    await act(async () => { loading.resolve(defaultProfile("Ada")); });
    expect(screen.getByText("CREATE AN ENTITY")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "What should we call them?" })).toHaveValue("Sol");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("button", { name: "Create Entity" })).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: "Edit identity" }));
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
    writeCreationDraft({
      version: 1, profile: defaultProfile("Sol"), entityId: "ada",
      attempt: { attemptId: "resume-on-original-hive", hiveUrl: originalHive, name: "Sol", baselineIds: [] },
    });
    mocks.openPresence.mockImplementation(async (_id: string, _signal: AbortSignal, hiveUrl?: string) => hiveUrl === originalHive ? originalRuntime : otherRuntime);
    render(<PresenceHome preview={false} status={status} onRefresh={vi.fn()} onProviders={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /An unfinished presence/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save brief to confirmed Entity" }));
    expect(await screen.findByRole("heading", { name: "Meet Sol." })).toBeInTheDocument();
    expect(mocks.savePresence).toHaveBeenCalledExactlyOnceWith("ada", defaultProfile("Sol"), false, expect.objectContaining({ hiveUrl: originalHive }));
    fireEvent.click(screen.getByRole("button", { name: "Continue to conversation" }));

    expect(await screen.findByRole("heading", { name: "Sol conversation" })).toBeInTheDocument();
    expect(mocks.openPresence).toHaveBeenCalledExactlyOnceWith("ada", expect.any(AbortSignal), originalHive);
    expect(screen.getByText(originalRuntime)).toBeInTheDocument();
    expect(screen.queryByText(otherRuntime)).not.toBeInTheDocument();
    expect(mocks.createPresenceEntity).not.toHaveBeenCalled();
    expect(new URLSearchParams(window.location.search).get("hiveUrl")).toBe(currentHive);
  });
});
