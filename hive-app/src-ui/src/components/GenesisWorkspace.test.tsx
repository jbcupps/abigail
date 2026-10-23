import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import GenesisWorkspace from "./GenesisWorkspace";
import { createGenesisSession, createOrResumeGenesis, genesisAction, getGenesisSession, importGenesisMarkdown, listGenesisSessions, sendGenesisMessage, type GenesisSession } from "../lib/genesisClient";
import { GENESIS_LAUNCH_DRAFT_KEY, type GenesisLaunchDraft } from "../lib/genesisLaunchDraft";
import { getEntityJourney, getQuestCatalog } from "../lib/questClient";
import { questFixture } from "../test/questFixture";

vi.mock("../lib/genesisClient", async (importOriginal) => ({ ...await importOriginal<typeof import("../lib/genesisClient")>(), createGenesisSession: vi.fn(), createOrResumeGenesis: vi.fn(), genesisAction: vi.fn(), getGenesisSession: vi.fn(), listGenesisSessions: vi.fn(), sendGenesisMessage: vi.fn(), importGenesisMarkdown: vi.fn(), deleteGenesisSession: vi.fn(), exportGenesisMarkdown: vi.fn() }));
vi.mock("./GenesisStudio", () => ({ default: ({ onUpdate }: { onUpdate: (updates: unknown[]) => Promise<void> }) => <button type="button" onClick={() => void onUpdate([{ field: "visual_style", value: "Watercolor" }])}>Set studio style</button> }));
vi.mock("./GenesisVoice", () => ({ default: () => <p>Voice controls</p> }));
vi.mock("../lib/entityWindow", () => ({ openEntity: vi.fn() }));
vi.mock("../lib/questClient", async (importOriginal) => ({ ...await importOriginal<typeof import("../lib/questClient")>(), getQuestCatalog: vi.fn(), getEntityJourney: vi.fn() }));

let saved: GenesisSession;
beforeEach(() => {
  localStorage.clear();
  saved = {
    id: "creation", owner_id: "local", mode: "guided", state: "discovering", revision: 1, created_at: "2026-10-17T12:00:00Z", updated_at: "2026-10-17T12:00:00Z", composer_draft: "",
    messages: [], proposals: [], conflicts: [], sources: [], revisions: [], asset_ids: [], job_ids: [],
    definition: { schema_version: 1, owner_id: "local", name: "Nova", status: "discovering", identity: "A thoughtful companion", purpose: "Help our family explore", personality: { description: "Curious", fears: [], aspirations: [], communication_style: "Warm", response_patterns: [] }, values: [], boundaries: [], appearance: "", visual_style: "", voice_profile: "", lore: [], relationships: [], fictional_memories: [], capabilities: [], permissions: {}, runtime_configuration: {}, created_at: "2026-10-17T12:00:00Z", updated_at: "2026-10-17T12:00:00Z", revision: 1 },
  };
  vi.mocked(listGenesisSessions).mockResolvedValue([]);
  vi.mocked(createGenesisSession).mockImplementation(async () => structuredClone(saved));
  vi.mocked(createOrResumeGenesis).mockImplementation(async (_seed, creationKey) => ({ ...structuredClone(saved), id: creationKey }));
  vi.mocked(getGenesisSession).mockImplementation(async () => structuredClone(saved));
  vi.mocked(getQuestCatalog).mockResolvedValue({
    stages: [], badges: [], tiers: [], temperament_cards: [], trials: [], instincts: "",
    long_arc_quests: [],
    palettes: [{ palette: "aether", unlocked_by: null }],
    crests: [{ crest: "orbit", unlocked_by: null }],
  });
  vi.mocked(genesisAction).mockImplementation(async (_session, action, args = {}) => {
    saved = structuredClone(saved); saved.revision += 1;
    if (action === "set_mode") saved.mode = args.mode as GenesisSession["mode"];
    if (action === "save_draft") saved.composer_draft = args.text as string;
    if (action === "set_retention") saved.retention_days = args.days as number | null;
    if (action === "update") for (const item of args.updates as { field: string; value: unknown }[]) Object.assign(saved.definition, { [item.field]: item.value });
    return structuredClone(saved);
  });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); localStorage.clear(); });

async function start(onClose = vi.fn()) {
  render(<GenesisWorkspace onClose={onClose} onCreated={vi.fn()} />);
  fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: saved.definition.name } });
  fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: saved.definition.purpose } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Begin Genesis Quest" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Begin Genesis Quest" }));
  fireEvent.click(await screen.findByRole("button", { name: "Discovery" }));
  await screen.findByRole("textbox", { name: "Tell me what you imagine" });
}

describe("Genesis creation workflow", () => {
  it("enters the existing Quest after a keyed First Signal succeeds", async () => {
    saved.quest = questFixture();
    render(<GenesisWorkspace onClose={vi.fn()} onCreated={vi.fn()} />);
    fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: "Nova" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: "Help our family explore" } });
    fireEvent.click(screen.getByRole("button", { name: "Begin Genesis Quest" }));

    expect(await screen.findByRole("region", { name: "Genesis quest" })).toBeInTheDocument();
    expect(createOrResumeGenesis).toHaveBeenCalledWith(expect.objectContaining({
      mode: "quick",
      name: "Nova",
      purpose: "Help our family explore",
      presence: expect.objectContaining({ temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" }),
    }), expect.stringMatching(/^[0-9a-f-]{36}$/));
  });

  it("stops a stale recovery view instead of dispatching a new request key", async () => {
    const draft: GenesisLaunchDraft = {
      version: 1,
      creation_key: "11111111-1111-4111-8111-111111111111",
      seed: {
        mode: "quick",
        name: "Nova",
        purpose: "Help our family explore",
        presence: { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" },
      },
    };
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(draft));
    render(<GenesisWorkspace onClose={vi.fn()} onCreated={vi.fn()} />);
    const resume = await screen.findByRole("button", { name: "Resume First Signal" });
    localStorage.removeItem(GENESIS_LAUNCH_DRAFT_KEY);

    fireEvent.click(resume);

    expect(await screen.findByRole("alert")).toHaveTextContent(/saved creation changed/i);
    expect(createOrResumeGenesis).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Resume First Signal" })).toBeEnabled();
  });

  it("freezes and retries the exact retained request after an ambiguous failure", async () => {
    saved.quest = questFixture();
    vi.mocked(createOrResumeGenesis)
      .mockRejectedValueOnce(new TypeError("Network request failed"))
      .mockImplementationOnce(async (_seed, creationKey) => ({ ...structuredClone(saved), id: creationKey }));
    render(<GenesisWorkspace onClose={vi.fn()} onCreated={vi.fn()} />);
    fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: " Nova " } });
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: " Help our family explore " } });
    fireEvent.click(screen.getByRole("button", { name: "Begin Genesis Quest" }));

    const resume = await screen.findByRole("button", { name: "Resume First Signal" });
    expect(screen.getByRole("textbox", { name: "Name" })).toBeDisabled();
    const retained = JSON.parse(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)!) as GenesisLaunchDraft;
    fireEvent.click(resume);

    expect(await screen.findByRole("region", { name: "Genesis quest" })).toBeInTheDocument();
    expect(createOrResumeGenesis).toHaveBeenCalledTimes(2);
    expect(vi.mocked(createOrResumeGenesis).mock.calls[0]).toEqual([retained.seed, retained.creation_key]);
    expect(vi.mocked(createOrResumeGenesis).mock.calls[1]).toEqual([retained.seed, retained.creation_key]);
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBeNull();
  });

  it("replays an exact reviewed journal before the Quest catalog is available", async () => {
    saved.quest = questFixture();
    const draft: GenesisLaunchDraft = {
      version: 1,
      creation_key: "22222222-2222-4222-8222-222222222222",
      seed: {
        mode: "quick",
        name: "Aster",
        purpose: "Keep family stories",
        presence: { temperament: "steady", identity: "aether", crest: "orbit", motion: "still" },
      },
    };
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(draft));
    vi.mocked(getQuestCatalog).mockRejectedValue(new Error("catalog unavailable"));
    render(<GenesisWorkspace onClose={vi.fn()} onCreated={vi.fn()} />);

    const resume = await screen.findByRole("button", { name: "Resume First Signal" });
    expect(getQuestCatalog).not.toHaveBeenCalled();
    fireEvent.click(resume);

    expect(await screen.findByRole("region", { name: "Genesis quest" })).toBeInTheDocument();
    expect(createOrResumeGenesis).toHaveBeenCalledWith(draft.seed, draft.creation_key);
  });

  it("changes automatic retention only after an explicit save and permits turning it off", async () => {
    await start();
    fireEvent.click(screen.getByText("Creation record retention"));
    const retention = screen.getByRole("combobox", { name: "Keep this creation record" });
    expect(retention).toHaveValue("keep");
    expect(screen.getByRole("button", { name: "Save retention" })).toBeDisabled();
    fireEvent.change(retention, { target: { value: "30" } });
    expect(genesisAction).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save retention" }));
    await waitFor(() => expect(saved.retention_days).toBe(30));
    await waitFor(() => expect(retention).toBeEnabled());
    fireEvent.change(retention, { target: { value: "keep" } });
    fireEvent.click(screen.getByRole("button", { name: "Save retention" }));
    await waitFor(() => expect(saved.retention_days).toBeNull());
    expect(genesisAction).toHaveBeenLastCalledWith(expect.anything(), "set_retention", { days: null });
  });

  it("shows an incomplete import's durable error while preserving its source and usable suggestions", async () => {
    vi.mocked(importGenesisMarkdown).mockImplementation(async () => {
      saved = { ...saved, revision: 2, last_error: "The selected model is busy. Try again shortly.",
        sources: [{ id: "source1", filename: "story.md", content: "A librarian of shared stories", created_at: saved.created_at, committed: false, unrecognized_sections: [] }],
        proposals: [{ id: "p1", field: "lore", value: ["A librarian of shared stories"], rationale: "A preserved suggestion", source_ref: "source1", status: "pending" }],
      };
      return structuredClone(saved);
    });
    await start();
    fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
    const file = new File(["A librarian of shared stories"], "story.md", { type: "text/markdown" });
    Object.defineProperty(file, "text", { value: async () => "A librarian of shared stories" });
    fireEvent.change(screen.getByLabelText(/Choose Markdown document/), { target: { files: [file] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("The selected model is busy");
    expect(screen.getByText(/Your source is saved, but interpretation could not finish/)).toBeInTheDocument();
    expect(screen.queryByText(/^Import preview saved\./)).not.toBeInTheDocument();
    expect(screen.getByText("story.md · Preview")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Include in import" })).toBeEnabled();
    expect(saved.definition.name).toBe("Nova");
    cleanup();
    vi.mocked(listGenesisSessions).mockResolvedValue([structuredClone(saved)]);
    render(<GenesisWorkspace onClose={vi.fn()} onCreated={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Resume Nova" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The selected model is busy");
  });

  it("keeps unsent words across mode switching and persists them before exit", async () => {
    const close = vi.fn(); await start(close);
    fireEvent.change(screen.getByRole("textbox", { name: "Tell me what you imagine" }), { target: { value: "A guardian of shared stories" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Creation mode" }), { target: { value: "hybrid" } });
    await screen.findByText("Voice controls");
    expect(screen.getByRole("textbox", { name: "Tell me what you imagine" })).toHaveValue("A guardian of shared stories");
    fireEvent.click(screen.getByRole("button", { name: "Save and exit" }));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(saved.composer_draft).toBe("A guardian of shared stories");
  });

  it("requires an explicit replacement for conflicting suggestions", async () => {
    saved.proposals = [{ id: "p1", field: "name", value: "Aster", rationale: "Another possible name", status: "pending" }];
    saved.conflicts = [{ id: "c1", field: "name", confirmed_value: "Nova", proposed_value: "Aster", proposal_id: "p1", resolved: false }];
    await start();
    expect(screen.getByRole("button", { name: "Awaken Entity" })).toBeDisabled();
    expect(genesisAction).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Replace confirmed choice" }));
    await waitFor(() => expect(genesisAction).toHaveBeenCalledWith(expect.anything(), "review_proposal", { proposal_id: "p1", accept: true, replace_confirmed: true }));
  });

  it("can reject every imported suggestion without deleting its source", async () => {
    saved.sources = [{ id: "source1", filename: "story.md", content: "# Name\nAster", created_at: saved.created_at, committed: false, unrecognized_sections: [] }];
    saved.proposals = [{ id: "p1", field: "name", value: "Aster", rationale: "Imported name", source_ref: "source1", status: "pending" }];
    await start(); fireEvent.click(screen.getByRole("button", { name: "Markdown" }));
    fireEvent.click(screen.getByRole("button", { name: "Keep current identity" }));
    await waitFor(() => expect(genesisAction).toHaveBeenCalledWith(expect.anything(), "commit_import", { source_id: "source1", proposal_ids: [], replace_confirmed: false }));
  });

  it("reloads after a failed request while preserving the composer", async () => {
    vi.mocked(sendGenesisMessage).mockRejectedValue(new Error("Please reload the saved session."));
    await start();
    fireEvent.change(screen.getByRole("textbox", { name: "Tell me what you imagine" }), { target: { value: "Keep this idea" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue discovery" }));
    fireEvent.click(await screen.findByRole("button", { name: "Reload saved session" }));
    await waitFor(() => expect(getGenesisSession).toHaveBeenCalledWith(vi.mocked(createOrResumeGenesis).mock.calls[0][1]));
    expect(screen.getByRole("textbox", { name: "Tell me what you imagine" })).toHaveValue("Keep this idea");
  });

  it("offers Meet only after awakening and celebrates the first awakening", async () => {
    vi.mocked(getEntityJourney).mockRejectedValue(new Error("offline"));
    vi.mocked(getQuestCatalog).mockResolvedValue({ stages: [], badges: [], tiers: [], temperament_cards: [], trials: [], palettes: [], crests: [], instincts: "", long_arc_quests: [] });
    saved.definition.entity_id = "allocated";
    saved.state = "review";
    saved.quest = questFixture();
    const activation = vi.mocked(genesisAction).getMockImplementation()!;
    vi.mocked(genesisAction).mockImplementation(async (session, action, args) => {
      const next = await activation(session, action, args);
      if (action !== "activate") return next;
      saved = { ...next, state: "ready", awakening: "I am Nova.", quest: questFixture({ badges: [{ badge: "first_light", title: "First Light", hint: "Awaken your Entity.", xp: 20, requirement: null, unlocks: { palettes: [], crests: [] }, earned_at: "now" }] }) };
      return structuredClone(saved);
    });
    await start();
    expect(screen.getByRole("navigation", { name: /Genesis quest/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Meet Nova" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Apply identity refinements" }));
    const dialog = await screen.findByRole("dialog", { name: "Nova has awakened" });
    expect(dialog).toHaveTextContent("First Light");
    fireEvent.click(screen.getByRole("button", { name: "Stay in Genesis" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Meet Nova" })).toBeInTheDocument();
  });

  it("saves pending definition edits even after an independent style change", async () => {
    const close = vi.fn(); await start(close);
    fireEvent.click(screen.getByRole("button", { name: "Definition" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Aster" } });
    fireEvent.click(screen.getByRole("button", { name: "Set studio style" }));
    await waitFor(() => expect(saved.definition.visual_style).toBe("Watercolor"));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save and exit" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Save and exit" }));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(saved.definition.name).toBe("Aster");
  });
});
