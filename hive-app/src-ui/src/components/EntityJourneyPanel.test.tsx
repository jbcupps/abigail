import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import EntityJourneyPanel from "./EntityJourneyPanel";
import { getEntityJourney, getQuestCatalog } from "../lib/questClient";
import { entityJourneyFixture, questCatalogFixture } from "../test/questFixture";

vi.mock("../lib/questClient", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/questClient")>(),
  getQuestCatalog: vi.fn(),
  getEntityJourney: vi.fn(),
}));

describe("Entity Journey panel", () => {
  beforeEach(() => {
    vi.mocked(getQuestCatalog).mockResolvedValue(questCatalogFixture());
    vi.mocked(getEntityJourney).mockResolvedValue(entityJourneyFixture());
  });
  afterEach(() => {
    cleanup();
    vi.resetAllMocks();
  });

  it("shows the post-creation arc without send actions or scoring fields", async () => {
    const onClose = vi.fn();
    render(<EntityJourneyPanel entityId="e1" entityName="Nova" onClose={onClose} />);

    expect(await screen.findByRole("heading", { name: "Nova's Journey" })).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Bond level progress, 120 XP" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Bond tier path" })).toBeInTheDocument();
    expect(screen.getByText("3 messages")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Getting Acquainted progress" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Seasonal quests" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Suggested first quests" })).toBeInTheDocument();
    expect(screen.getByText("Nothing is sent for you. Copy a prompt, or say whatever feels right.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /send/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/weight|deontology|teleology|areteology|welfare/i)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes on Escape and surfaces catalog failures", async () => {
    const onClose = vi.fn();
    vi.mocked(getQuestCatalog).mockRejectedValue(new Error("catalog offline"));
    render(<EntityJourneyPanel entityId="e1" entityName="Nova" onClose={onClose} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("catalog offline");
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });

  it("keeps family seasonal progress copy-only", async () => {
    render(<EntityJourneyPanel entityId="e1" entityName="Nova" onClose={vi.fn()} />);
    const quests = await screen.findByRole("region", { name: "Seasonal quests" });
    expect(within(quests).getByRole("article", { name: "Family Constellation" })).toBeInTheDocument();
    expect(within(quests).queryByRole("button")).not.toBeInTheDocument();
  });
});
