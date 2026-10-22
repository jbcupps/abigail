import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GenesisCreationSeed } from "../lib/genesisClient";
import { GENESIS_LAUNCH_DRAFT_KEY } from "../lib/genesisLaunchDraft";
import { getQuestCatalog } from "../lib/questClient";
import GenesisFirstSignal from "./GenesisFirstSignal";

vi.mock("../lib/questClient", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/questClient")>(),
  getQuestCatalog: vi.fn(),
}));

const catalog = {
  stages: [], badges: [], temperament_cards: [], trials: [], instincts: "",
  palettes: [
    { palette: "aether" as const, unlocked_by: null },
    { palette: "aurora" as const, unlocked_by: "soul_shaper" as const },
  ],
  crests: [
    { crest: "orbit" as const, unlocked_by: null },
    { crest: "lantern" as const, unlocked_by: "instinct_keeper" as const },
  ],
};

beforeEach(() => {
  localStorage.clear();
  vi.mocked(getQuestCatalog).mockResolvedValue(catalog);
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
  localStorage.clear();
});

describe("Genesis First Signal", () => {
  it("requires a trimmed name and purpose", async () => {
    const onBegin = vi.fn(async (_seed: GenesisCreationSeed) => undefined);
    render(<GenesisFirstSignal onBegin={onBegin} />);
    const begin = await screen.findByRole("button", { name: "Begin Genesis Quest" });
    expect(begin).toBeDisabled();

    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "  Nova  " } });
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: "  Notice small wonders  " } });
    fireEvent.click(begin);

    await waitFor(() => expect(onBegin).toHaveBeenCalledOnce());
    expect(onBegin.mock.calls[0][0].name).toBe("Nova");
    expect(onBegin.mock.calls[0][0].purpose).toBe("Notice small wonders");
  });

  it("previews canonical Presence choices and submits only the creation seed", async () => {
    const onBegin = vi.fn(async (_seed: GenesisCreationSeed) => undefined);
    render(<GenesisFirstSignal onBegin={onBegin} />);
    await screen.findByRole("button", { name: "Begin Genesis Quest" });
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Nova" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: "Help our family explore" } });

    expect(screen.getByRole("img", { name: "Nova crest preview" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Aurora/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Lantern/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Begin Genesis Quest" }));

    await waitFor(() => expect(onBegin).toHaveBeenCalledWith({
      mode: "quick",
      name: "Nova",
      purpose: "Help our family explore",
      presence: { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" },
    }));
    expect(JSON.stringify(onBegin.mock.calls[0][0])).not.toMatch(/permission|runtime/i);
  });

  it("resumes the exact reviewed seed from a saved journal", async () => {
    const saved: GenesisCreationSeed = {
      mode: "quick",
      name: "Aster",
      purpose: "Keep family stories",
      presence: { temperament: "steady", identity: "aether", crest: "orbit", motion: "still" },
    };
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify({
      version: 1,
      creation_key: "11111111-1111-4111-8111-111111111111",
      seed: saved,
    }));
    const onBegin = vi.fn(async (_seed: GenesisCreationSeed) => undefined);

    render(<GenesisFirstSignal onBegin={onBegin} />);
    const resume = await screen.findByRole("button", { name: "Resume First Signal" });
    expect(screen.getByRole("textbox", { name: "Name" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Purpose" })).toBeDisabled();
    fireEvent.click(resume);

    await waitFor(() => expect(onBegin).toHaveBeenCalledWith(saved));
  });

  it("retains and surfaces an unreadable saved journal", async () => {
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, "not-json");
    const onBegin = vi.fn();

    render(<GenesisFirstSignal onBegin={onBegin} />);

    expect(await screen.findByRole("alert")).toHaveTextContent(/retained for recovery/i);
    expect(screen.getByRole("button", { name: "Begin Genesis Quest" })).toBeDisabled();
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBe("not-json");
  });
});
