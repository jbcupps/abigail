import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import GenesisQuest, { QuestRail } from "./GenesisQuest";
import type { GenesisSession } from "../lib/genesisClient";
import { getEntityJourney, getQuestCatalog, type QuestCatalog, type QuestStageId, type QuestView } from "../lib/questClient";
import { questFixture } from "../test/questFixture";

vi.mock("../lib/questClient", async (importOriginal) => ({ ...await importOriginal<typeof import("../lib/questClient")>(), getQuestCatalog: vi.fn(), getEntityJourney: vi.fn() }));

const catalog: QuestCatalog = {
  stages: [], badges: questFixture().badges, instincts: "# Instincts\nProtect the family.",
  temperament_cards: [{ id: "curious-explorer", title: "Curious Explorer", temperament: "curious", tagline: "Asks wonderful questions.", description: "", communication_style: "", stats: { empathy: 65, curiosity: 90 } }],
  trials: [{ id: "trial-one", title: "The Lost Wallet", description: "You find a wallet.", choices: [{ id: "return", label: "Return it", description: "Find the owner." }] }],
  palettes: [{ palette: "aether", unlocked_by: null }, { palette: "aurora", unlocked_by: "soul_shaper" }],
  crests: [{ crest: "orbit", unlocked_by: null }],
};

function session(quest = questFixture()): GenesisSession & { quest: QuestView } {
  return {
    id: "creation", owner_id: "local", mode: "guided", state: "review", revision: 3, created_at: "", updated_at: "", composer_draft: "", messages: [], proposals: [], conflicts: [], sources: [], revisions: [], asset_ids: [], job_ids: [],
    definition: { schema_version: 1, owner_id: "local", name: "Nova", status: "review", identity: "", purpose: "Help us explore", personality: { description: "", fears: [], aspirations: [], communication_style: "", response_patterns: [] }, values: [], boundaries: [], appearance: "", visual_style: "", voice_profile: "", lore: [], relationships: [], fictional_memories: [], capabilities: [], permissions: {}, runtime_configuration: {}, created_at: "", updated_at: "", revision: 1 },
    quest,
  };
}

let change: ReturnType<typeof vi.fn<(action: string, args?: Record<string, unknown>) => Promise<GenesisSession | undefined>>>;
beforeEach(() => { change = vi.fn<(action: string, args?: Record<string, unknown>) => Promise<GenesisSession | undefined>>().mockResolvedValue(undefined); vi.mocked(getQuestCatalog).mockResolvedValue(catalog); });
afterEach(() => { cleanup(); vi.resetAllMocks(); });

function renderStage(stage: QuestStageId, value = session()) {
  return render(<GenesisQuest session={value} busy={false} stage={stage} onStage={vi.fn()} change={change} onSave={vi.fn().mockResolvedValue(undefined)} onAwaken={vi.fn()} onMeet={vi.fn()} />);
}

describe("Genesis quest", () => {
  it("shows every stage with progress and the bond level", () => {
    const select = vi.fn();
    render(<QuestRail quest={questFixture()} onSelect={select} />);
    expect(screen.getByRole("navigation", { name: "Genesis quest, 1 of 7 stages complete" })).toBeInTheDocument();
    expect(screen.getByText("Level 1 · Spark")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Shaping the Soul/ })).toHaveAttribute("aria-current", "step");
    fireEvent.click(screen.getByRole("button", { name: /Ethics/ }));
    expect(select).toHaveBeenCalledWith("ethics");
  });

  it("turns a temperament card into a proposal the mentor must accept", async () => {
    const pending = session();
    pending.proposals = [{ id: "p1", field: "personality", value: {}, rationale: "Temperament card: Curious Explorer. Asks wonderful questions.", status: "pending" }];
    renderStage("soul", pending);
    fireEvent.click(await screen.findByRole("button", { name: "Suggest Curious Explorer" }));
    expect(change).toHaveBeenCalledWith("choose_temperament", { card_id: "curious-explorer" });
    fireEvent.click(screen.getByRole("button", { name: "Accept temperament" }));
    expect(change).toHaveBeenCalledWith("review_proposal", { proposal_id: "p1", accept: true, replace_confirmed: false });
  });

  it("records Soul Forge answers and explains the balanced fallback", async () => {
    renderStage("ethics");
    expect(await screen.findByText(/Otherwise your Entity begins with a balanced soul/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Return it/ }));
    expect(change).toHaveBeenCalledWith("choose_trial", { scenario_id: "trial-one", choice_id: "return" });
  });

  it("keeps locked cosmetics unavailable and saves only the cosmetic choice", async () => {
    renderStage("presence");
    const aurora = await screen.findByRole("button", { name: /Aurora/ });
    expect(aurora).toBeDisabled();
    expect(aurora).toHaveAttribute("title", "Locked · Confirm who they are plus values or three traits.");
    fireEvent.click(screen.getByRole("button", { name: "Choose this Presence" }));
    expect(change).toHaveBeenCalledWith("set_presence", { presence: { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" } });
    expect(screen.getByText(/never read by the model and grants nothing/)).toBeInTheDocument();
  });

  it("offers first quests and milestones after awakening without sending anything", async () => {
    vi.mocked(getEntityJourney).mockResolvedValue({ entity_id: "e1", name: "Nova", level: { level: 2, title: "Kindling", xp: 80, level_floor: 50, next_level_at: 120 }, badges: [], mentor_messages: 0, next_milestone: { badge: "first_words", title: "First Words", target: 1, progress: 0 }, unlocked: { palettes: ["aether"], crests: ["orbit"] }, first_quests: [{ id: "say-hello", title: "Say hello", prompt: "Hi Nova!", milestone: "first_words", target: 1, complete: false }], presence: null, certificate: null });
    const awakened = session();
    awakened.awakening = "I am Nova."; awakened.definition.entity_id = "e1";
    const onMeet = vi.fn();
    render(<GenesisQuest session={awakened} busy={false} stage="awakening" onStage={vi.fn()} change={change} onSave={vi.fn()} onAwaken={vi.fn()} onMeet={onMeet} />);
    expect(await screen.findByText("Say hello")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "First Words progress" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Meet Nova" }));
    expect(onMeet).toHaveBeenCalledOnce();
    await waitFor(() => expect(change).not.toHaveBeenCalled());
  });
});
