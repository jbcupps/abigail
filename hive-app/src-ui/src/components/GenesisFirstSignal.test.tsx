import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GenesisCreationSeed } from "../lib/genesisClient";
import { GENESIS_LAUNCH_DRAFT_KEY, type GenesisLaunchDraft } from "../lib/genesisLaunchDraft";
import { getQuestCatalog } from "../lib/questClient";
import GenesisFirstSignal from "./GenesisFirstSignal";

vi.mock("../lib/questClient", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/questClient")>(),
  getQuestCatalog: vi.fn(),
}));

const catalog = {
  stages: [], badges: [], tiers: [], temperament_cards: [], trials: [], instincts: "",
  palettes: [
    { palette: "aether" as const, unlocked_by: null },
    { palette: "aurora" as const, unlocked_by: "soul_shaper" as const },
  ],
  crests: [
    { crest: "orbit" as const, unlocked_by: null },
    { crest: "lantern" as const, unlocked_by: "instinct_keeper" as const },
  ],
};
const recoveryKey = "11111111-1111-4111-8111-111111111111";
const recoverySeed: GenesisCreationSeed = {
  mode: "quick",
  name: "Aster",
  purpose: "Keep family stories",
  presence: { temperament: "steady", identity: "aether", crest: "orbit", motion: "still" },
};
const recoveryDraft: GenesisLaunchDraft = {
  version: 1,
  creation_key: recoveryKey,
  seed: recoverySeed,
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
    expect(onBegin.mock.calls[0]).toHaveLength(1);
    expect(JSON.stringify(onBegin.mock.calls[0][0])).not.toMatch(/permission|runtime/i);
  });

  it("resumes the exact reviewed seed from a saved journal", async () => {
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(recoveryDraft));
    const onBegin = vi.fn(async (_seed: GenesisCreationSeed) => undefined);

    render(<GenesisFirstSignal onBegin={onBegin} />);
    const resume = await screen.findByRole("button", { name: "Resume First Signal" });
    expect(screen.getByRole("textbox", { name: "Name" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Purpose" })).toBeDisabled();
    fireEvent.click(resume);

    await waitFor(() => expect(onBegin).toHaveBeenCalledWith(recoverySeed));
    expect(onBegin.mock.calls[0]).toHaveLength(1);
  });

  it("retains and surfaces an unreadable saved journal", async () => {
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, "not-json");
    const onBegin = vi.fn();

    render(<GenesisFirstSignal onBegin={onBegin} />);

    expect(await screen.findByRole("alert")).toHaveTextContent(/retained for recovery/i);
    expect(screen.getByRole("button", { name: "Begin Genesis Quest" })).toBeDisabled();
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY)).toBe("not-json");
  });

  it("replays a reviewed journal without waiting for the Quest catalog", async () => {
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(recoveryDraft));
    vi.mocked(getQuestCatalog).mockRejectedValue(new Error("catalog unavailable"));
    const onBegin = vi.fn(async () => undefined);

    render(<GenesisFirstSignal onBegin={onBegin} />);
    const resume = await screen.findByRole("button", { name: "Resume First Signal" });
    expect(resume).toBeEnabled();
    expect(getQuestCatalog).not.toHaveBeenCalled();
    fireEvent.click(resume);

    await waitFor(() => expect(onBegin).toHaveBeenCalledWith(recoverySeed));
    expect(onBegin.mock.calls[0]).toHaveLength(1);
  });

  it("freezes to the retained exact draft after an ambiguous failure in the same mount", async () => {
    const onBegin = vi.fn()
      .mockImplementationOnce(async (submittedSeed: GenesisCreationSeed) => {
        localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify({
          version: 1,
          creation_key: recoveryKey,
          seed: submittedSeed,
        }));
        throw new TypeError("Network request failed");
      })
      .mockResolvedValueOnce(undefined);
    render(<GenesisFirstSignal onBegin={onBegin} />);
    await screen.findByRole("button", { name: "Begin Genesis Quest" });
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: " Nova " } });
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: " Keep family stories " } });
    fireEvent.click(screen.getByRole("button", { name: "Begin Genesis Quest" }));

    expect(await screen.findByText(/may already have reached Hive/i)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Name" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Nova");
    expect(screen.getByRole("textbox", { name: "Purpose" })).toBeDisabled();
    const firstCall = onBegin.mock.calls[0];
    fireEvent.click(screen.getByRole("button", { name: "Resume First Signal" }));

    await waitFor(() => expect(onBegin).toHaveBeenCalledTimes(2));
    expect(firstCall).toEqual([expect.objectContaining({ name: "Nova", purpose: "Keep family stories" })]);
    expect(onBegin.mock.calls[1]).toEqual([firstCall[0]]);
  });

  it("retries Quest catalog loading for a new editable signal", async () => {
    vi.mocked(getQuestCatalog)
      .mockRejectedValueOnce(new Error("catalog unavailable"))
      .mockResolvedValueOnce(catalog);
    render(<GenesisFirstSignal onBegin={vi.fn()} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("catalog unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Retry Presence choices" }));

    await waitFor(() => expect(getQuestCatalog).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("button", { name: "Begin Genesis Quest" })).toBeInTheDocument();
  });
});
