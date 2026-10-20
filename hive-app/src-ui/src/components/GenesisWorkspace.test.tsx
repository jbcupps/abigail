import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import GenesisWorkspace from "./GenesisWorkspace";
import { createGenesisSession, genesisAction, getGenesisSession, importGenesisMarkdown, listGenesisSessions, sendGenesisMessage, type GenesisSession } from "../lib/genesisClient";

vi.mock("../lib/genesisClient", async (importOriginal) => ({ ...await importOriginal<typeof import("../lib/genesisClient")>(), createGenesisSession: vi.fn(), genesisAction: vi.fn(), getGenesisSession: vi.fn(), listGenesisSessions: vi.fn(), sendGenesisMessage: vi.fn(), importGenesisMarkdown: vi.fn(), deleteGenesisSession: vi.fn(), exportGenesisMarkdown: vi.fn() }));
vi.mock("./GenesisStudio", () => ({ default: ({ onUpdate }: { onUpdate: (updates: unknown[]) => Promise<void> }) => <button type="button" onClick={() => void onUpdate([{ field: "visual_style", value: "Watercolor" }])}>Set studio style</button> }));
vi.mock("./GenesisVoice", () => ({ default: () => <p>Voice controls</p> }));
vi.mock("../lib/entityWindow", () => ({ openEntity: vi.fn() }));

let saved: GenesisSession;
beforeEach(() => {
  saved = {
    id: "creation", owner_id: "local", mode: "guided", state: "discovering", revision: 1, created_at: "2026-10-17T12:00:00Z", updated_at: "2026-10-17T12:00:00Z", composer_draft: "",
    messages: [], proposals: [], conflicts: [], sources: [], revisions: [], asset_ids: [], job_ids: [],
    definition: { schema_version: 1, owner_id: "local", name: "Nova", status: "discovering", identity: "A thoughtful companion", purpose: "Help our family explore", personality: { description: "Curious", fears: [], aspirations: [], communication_style: "Warm", response_patterns: [] }, values: [], boundaries: [], appearance: "", visual_style: "", voice_profile: "", lore: [], relationships: [], fictional_memories: [], capabilities: [], permissions: {}, runtime_configuration: {}, created_at: "2026-10-17T12:00:00Z", updated_at: "2026-10-17T12:00:00Z", revision: 1 },
  };
  vi.mocked(listGenesisSessions).mockResolvedValue([]);
  vi.mocked(createGenesisSession).mockImplementation(async () => structuredClone(saved));
  vi.mocked(getGenesisSession).mockImplementation(async () => structuredClone(saved));
  vi.mocked(genesisAction).mockImplementation(async (_session, action, args = {}) => {
    saved = structuredClone(saved); saved.revision += 1;
    if (action === "set_mode") saved.mode = args.mode as GenesisSession["mode"];
    if (action === "save_draft") saved.composer_draft = args.text as string;
    if (action === "set_retention") saved.retention_days = args.days as number | null;
    if (action === "update") for (const item of args.updates as { field: string; value: unknown }[]) Object.assign(saved.definition, { [item.field]: item.value });
    return structuredClone(saved);
  });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

async function start(onClose = vi.fn()) {
  render(<GenesisWorkspace onClose={onClose} onCreated={vi.fn()} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Begin Genesis" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Begin Genesis" }));
  await screen.findByRole("textbox", { name: "Tell me what you imagine" });
}

describe("Genesis creation workflow", () => {
  it("returns an existing ready Entity without creating or activating again", async () => {
    saved.state = "ready"; saved.definition.entity_id = "existing-entity";
    const meet = vi.fn(); render(<GenesisWorkspace initialSession={saved} onClose={vi.fn()} onCreated={vi.fn()} onMeet={meet} />);
    fireEvent.click(screen.getByRole("button", { name: "Meet Nova" }));
    await waitFor(() => expect(meet).toHaveBeenCalledWith(saved));
    expect(createGenesisSession).not.toHaveBeenCalled(); expect(genesisAction).not.toHaveBeenCalled();
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
    await waitFor(() => expect(getGenesisSession).toHaveBeenCalledWith("creation", undefined));
    expect(screen.getByRole("textbox", { name: "Tell me what you imagine" })).toHaveValue("Keep this idea");
  });

  it("saves pending definition edits even after an independent style change", async () => {
    const close = vi.fn(); await start(close);
    fireEvent.click(screen.getByRole("button", { name: "Definition" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Aster" } });
    fireEvent.click(screen.getByText("Optional visual studio"));
    fireEvent.click(screen.getByRole("button", { name: "Set studio style" }));
    await waitFor(() => expect(saved.definition.visual_style).toBe("Watercolor"));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save and exit" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Save and exit" }));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(saved.definition.name).toBe("Aster");
  });
});
