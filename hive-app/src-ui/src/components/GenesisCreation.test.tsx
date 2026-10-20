import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import GenesisCreation from "./GenesisCreation";
import { createOrResumeGenesis, genesisAction, resolveGenesisConnection, type GenesisSession } from "../lib/genesisClient";
import { savePresence } from "../lib/presenceApi";
import { readGenesisDraft } from "../lib/genesisDraft";
import { defaultProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";

vi.mock("../lib/genesisClient", async original => ({ ...await original<typeof import("../lib/genesisClient")>(), createOrResumeGenesis: vi.fn(), genesisAction: vi.fn(), resolveGenesisConnection: vi.fn() }));
vi.mock("../lib/presenceApi", async original => ({ ...await original<typeof import("../lib/presenceApi")>(), savePresence: vi.fn() }));
vi.mock("./GenesisWorkspace", () => ({ default: ({ initialSession, onCreated, onMeet }: { initialSession?: GenesisSession; onCreated: (session: GenesisSession) => void; onMeet: (session: GenesisSession) => void }) => initialSession ? <div><p>Discovery for {initialSession.id}</p><button onClick={() => onCreated({ ...initialSession, state: "ready", definition: { ...initialSession.definition, entity_id: "entity-one", name: "Reviewed Vesper", purpose: "Reviewed purpose" } })}>Approve explored identity</button></div> : <button onClick={() => onMeet({ id: "other-creation", state: "ready", connection: { hiveUrl: "http://127.0.0.1:43141", profileId: null }, definition: { entity_id: "existing-entity" } } as GenesisSession)}>Return to existing</button> }));
const connection = { hiveUrl: "http://127.0.0.1:43141", profileId: null };
let server: GenesisSession | undefined;
function props() { return { preview: false, onClose: vi.fn(), onSaved: vi.fn(), onPersisted: vi.fn(), onReturn: vi.fn(), onBusyChange: vi.fn() }; }
beforeEach(() => {
  server = undefined; localStorage.clear(); sessionStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  vi.stubGlobal("navigator", { locks: { request: (_name: string, _options: unknown, callback: (lock: object) => unknown) => Promise.resolve(callback({})) } });
  vi.mocked(resolveGenesisConnection).mockResolvedValue({ base: connection.hiveUrl, token: "fixture", connection });
  vi.mocked(createOrResumeGenesis).mockImplementation(async (id, seed) => {
    expect(readGenesisDraft()?.key).toBe(id); // journal must be durable before dispatch
    server ??= { id, connection, state: "draft", revision: 1, definition: { name: seed.name, purpose: seed.purpose } } as GenesisSession;
    return structuredClone(server);
  });
  vi.mocked(genesisAction).mockImplementation(async session => {
    server = { ...session, state: "ready", definition: { ...session.definition, entity_id: "entity-one" } };
    return structuredClone(server);
  });
  vi.mocked(savePresence).mockResolvedValue(undefined);
});
afterEach(() => { cleanup(); vi.resetAllMocks(); vi.unstubAllGlobals(); localStorage.clear(); sessionStorage.clear(); });
function review() {
  fireEvent.change(screen.getByRole("textbox", { name: "What should we call them?" }), { target: { value: "Vesper" } });
  fireEvent.change(screen.getByRole("textbox", { name: "What would you like to explore together?" }), { target: { value: "Explore tiny worlds" } });
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}
it("approves only the reviewed name and purpose, then reveals the confirmed Entity", async () => {
  const callbacks = props(); render(<GenesisCreation {...callbacks} />); review();
  expect(screen.getByText(/Awaken approves this name and purpose/)).toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox", { name: "What should they keep in mind?" }), { target: { value: "Presentation-only note" } });
  fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));
  await screen.findByRole("heading", { name: "Meet Vesper." });
  expect(createOrResumeGenesis).toHaveBeenCalledWith(expect.any(String), { mode: "quick", name: "Vesper", purpose: "Explore tiny worlds" }, expect.objectContaining({ connection }));
  expect(savePresence).toHaveBeenCalledWith("entity-one", expect.objectContaining({ boundaries: "Presentation-only note" }), false, expect.objectContaining({ hiveUrl: connection.hiveUrl }));
  expect(readGenesisDraft()).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Start a conversation" }));
  expect(callbacks.onSaved).toHaveBeenCalledWith(expect.objectContaining({ id: "entity-one", hiveUrl: connection.hiveUrl }));
});
it("reopens an uncertain creation with the exact original key and input", async () => {
  vi.mocked(createOrResumeGenesis).mockRejectedValueOnce(new Error("Response lost"));
  const view = render(<GenesisCreation {...props()} />); review(); fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));
  await screen.findByRole("alert"); const original = readGenesisDraft(); expect(original).not.toBeNull(); view.unmount();
  render(<GenesisCreation {...props()} />);
  expect(createOrResumeGenesis).toHaveBeenCalledTimes(1); // no automatic replay on mount
  fireEvent.click(screen.getByRole("button", { name: "Resume awakening" }));
  await screen.findByRole("heading", { name: "Meet Vesper." });
  expect(createOrResumeGenesis).toHaveBeenLastCalledWith(original!.key, original!.seed, expect.anything());
});
it("recovers a lost activation response without reactivating the ready Entity", async () => {
  vi.mocked(genesisAction).mockImplementationOnce(async session => { server = { ...session, state: "ready", definition: { ...session.definition, entity_id: "entity-one" } }; throw new Error("Activation response lost"); });
  const view = render(<GenesisCreation {...props()} />); review(); fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));
  await screen.findByRole("alert"); view.unmount(); render(<GenesisCreation {...props()} />);
  fireEvent.click(screen.getByRole("button", { name: "Resume awakening" }));
  await screen.findByRole("heading", { name: "Meet Vesper." });
  expect(genesisAction).toHaveBeenCalledTimes(1);
});
it("keeps the confirmed Entity and draft when appearance saving fails", async () => {
  vi.mocked(savePresence).mockRejectedValueOnce(new Error("Brief save interrupted"));
  render(<GenesisCreation {...props()} />); review(); fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));
  await screen.findByRole("alert"); expect(readGenesisDraft()).not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Resume awakening" }));
  await screen.findByRole("heading", { name: "Meet Vesper." });
  expect(genesisAction).toHaveBeenCalledTimes(1); expect(savePresence).toHaveBeenCalledTimes(2);
  expect(vi.mocked(savePresence).mock.calls.map(call => call[0])).toEqual(["entity-one", "entity-one"]);
});
it("stopping waiting ignores late success and retains the same recovery key", async () => {
  let release!: (session: GenesisSession) => void;
  vi.mocked(createOrResumeGenesis).mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
  render(<GenesisCreation {...props()} />); review(); fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));
  await waitFor(() => expect(readGenesisDraft()).not.toBeNull()); const record = readGenesisDraft()!;
  fireEvent.click(screen.getByRole("button", { name: "Stop waiting" }));
  release({ id: record.key, connection, state: "ready", definition: { entity_id: "entity-one" } } as GenesisSession);
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Waiting stopped"));
  expect(savePresence).not.toHaveBeenCalled(); expect(genesisAction).not.toHaveBeenCalled(); expect(readGenesisDraft()?.key).toBe(record.key);
});
it("explores the same draft and saves approved canonical changes with the chosen presentation", async () => {
  render(<GenesisCreation {...props()} />); review(); fireEvent.click(screen.getByRole("button", { name: "Explore this character first" }));
  await screen.findByText(/^Discovery for/); const original = readGenesisDraft()!;
  expect(genesisAction).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Approve explored identity" }));
  await screen.findByRole("heading", { name: "Meet Reviewed Vesper." });
  expect(savePresence).toHaveBeenCalledWith("entity-one", { ...original.profile, name: "Reviewed Vesper", purpose: "Reviewed purpose" }, false, expect.anything());
  expect(createOrResumeGenesis).toHaveBeenCalledTimes(1);
});
it("keeps existing presentation-only data out of Genesis", async () => {
  render(<GenesisCreation {...props()} existing={{ id: "legacy", profile: { ...defaultProfile("Old alias"), purpose: "Old visual intention" } }} />);
  fireEvent.click(screen.getByRole("button", { name: "Continue" })); fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(screen.getByText(/This brief is saved as design metadata/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Save identity" }));
  await screen.findByRole("heading", { name: "Meet Old alias." });
  expect(createOrResumeGenesis).not.toHaveBeenCalled(); expect(genesisAction).not.toHaveBeenCalled();
});
it("browsing an existing Entity preserves a different unfinished creation and its appearance", async () => {
  vi.mocked(createOrResumeGenesis).mockRejectedValueOnce(new Error("Response lost"));
  const view = render(<GenesisCreation {...props()} />); review(); fireEvent.click(screen.getByRole("button", { name: "Awaken Entity" }));
  await screen.findByRole("alert"); const original = readGenesisDraft(); view.unmount();
  const callbacks = props(); render(<GenesisCreation {...callbacks} browse />);
  fireEvent.click(screen.getByRole("button", { name: "Return to existing" }));
  expect(callbacks.onReturn).toHaveBeenCalledWith("existing-entity", connection.hiveUrl);
  expect(readGenesisDraft()).toEqual(original); expect(savePresence).not.toHaveBeenCalled();
  expect(genesisAction).not.toHaveBeenCalled();
});
