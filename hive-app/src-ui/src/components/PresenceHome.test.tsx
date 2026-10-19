import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import type { HiveStatus } from "../lib/daemonClient";
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
  default: ({ profile, baseUrl, onBack }: { profile: EntityProfile; baseUrl: string; onBack: () => void }) => <section aria-label="Opened conversation"><h1>{profile.name} conversation</h1><p>{baseUrl}</p><button onClick={onBack}>Return from conversation</button></section>,
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
    expect(mocks.savePresence).toHaveBeenCalledExactlyOnceWith("created-entity", defaultProfile("Sol"), false);
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
    await waitFor(() => expect(mocks.openPresence).toHaveBeenCalledExactlyOnceWith("ada"));

    fireEvent.click(screen.getByRole("button", { name: "Your Entities" }));
    await act(async () => { opening.resolve("http://127.0.0.1:43201"); });
    expect(screen.queryByRole("region", { name: "Opened conversation" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Open conversation/ })).toBeEnabled();
  });

  it("does not start an entity when a delayed profile load completes after navigation", async () => {
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
});
