import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GenesisCreationSeed } from "../lib/genesisClient";
import { GENESIS_LAUNCH_DRAFT_KEY, type GenesisLaunchDraft } from "../lib/genesisLaunchDraft";
import { resolveLegacyCandidates } from "../lib/legacyFirstSignal";
import { getQuestCatalog, migrateLegacyFirstSignal } from "../lib/questClient";
import GenesisFirstSignal from "./GenesisFirstSignal";

vi.mock("../lib/questClient", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/questClient")>(),
  getQuestCatalog: vi.fn(),
  migrateLegacyFirstSignal: vi.fn(),
}));
vi.mock("../lib/legacyFirstSignal", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/legacyFirstSignal")>(),
  resolveLegacyCandidates: vi.fn(),
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
const legacyKey = "abigail.first-signal.creation.v1";
const legacyResolvedKey = "abigail.first-signal.creation.resolved.v1:old-attempt";
const legacyProfile = {
  name: "Sol",
  purpose: "Plan our family garden.",
  temperament: "curious" as const,
  boundaries: "Ask before changing shared plans.",
  identity: "ember" as const,
};
const legacyAttempt = {
  attemptId: "old-attempt",
  hiveUrl: "http://127.0.0.1:43141",
  name: "Sol",
  baselineIds: ["old-entity"],
};
const legacyJournal = {
  version: 1,
  profile: legacyProfile,
  attempt: legacyAttempt,
};

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  vi.mocked(getQuestCatalog).mockResolvedValue(catalog);
  vi.mocked(resolveLegacyCandidates).mockResolvedValue([]);
  vi.mocked(migrateLegacyFirstSignal).mockResolvedValue({
    entity_id: "candidate",
    presence: {
      name: "Sol",
      purpose: legacyProfile.purpose,
      boundaries: legacyProfile.boundaries,
      temperament: "curious",
      identity: "ember",
      crest: "orbit",
      motion: "calm",
    },
    badges: ["first_spark", "named", "purposeful", "presence_chosen"],
    migrated_at: "2026-10-23T00:00:00Z",
  });
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
  localStorage.clear();
  sessionStorage.clear();
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

  it("blocks a current draft while malformed legacy bytes remain retained", async () => {
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(recoveryDraft));
    sessionStorage.setItem(legacyKey, "{broken");
    const onBegin = vi.fn(async () => undefined);

    render(<GenesisFirstSignal onBegin={onBegin} />);

    const recoveryError = screen.getByRole("region", {
      name: "Legacy First Signal recovery error",
    });
    expect(within(recoveryError).getByRole("alert")).toHaveTextContent(/retained.*recovery/i);
    const resume = screen.getByRole("button", { name: "Resume First Signal" });
    expect(resume).toBeDisabled();
    fireEvent.click(resume);
    expect(onBegin).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(legacyKey)).toBe("{broken");
    expect(localStorage.getItem(GENESIS_LAUNCH_DRAFT_KEY))
      .toBe(JSON.stringify(recoveryDraft));
  });

  it("blocks a current draft while conflicting legacy bytes remain retained", async () => {
    const creationKey = "22222222-2222-4222-8222-222222222222";
    const localLegacy = JSON.stringify({
      version: 2,
      revision: 1,
      profile: legacyProfile,
      attempt: {
        attemptId: creationKey,
        creationKey,
        hiveUrl: "http://127.0.0.1:43141",
        name: "Sol",
        baselineIds: [],
      },
    });
    const sessionLegacy = JSON.stringify(legacyJournal);
    localStorage.setItem(GENESIS_LAUNCH_DRAFT_KEY, JSON.stringify(recoveryDraft));
    localStorage.setItem("abigail.first-signal.creation.v2", localLegacy);
    sessionStorage.setItem(legacyKey, sessionLegacy);
    const onBegin = vi.fn(async () => undefined);

    render(<GenesisFirstSignal onBegin={onBegin} />);

    expect(screen.getByRole("region", {
      name: "Legacy First Signal recovery error",
    })).toHaveTextContent(/different legacy First Signal/i);
    const resume = screen.getByRole("button", { name: "Resume First Signal" });
    expect(resume).toBeDisabled();
    fireEvent.click(resume);
    expect(onBegin).not.toHaveBeenCalled();
    expect(localStorage.getItem("abigail.first-signal.creation.v2")).toBe(localLegacy);
    expect(sessionStorage.getItem(legacyKey)).toBe(sessionLegacy);
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

  it("blocks a new launch and requires Check, Review, and Migrate for an unkeyed legacy brief", async () => {
    sessionStorage.setItem(legacyKey, JSON.stringify(legacyJournal));
    vi.mocked(resolveLegacyCandidates).mockResolvedValue([{
      id: "candidate",
      name: "Sol",
      resolution: "name_candidate",
    }]);
    const onBegin = vi.fn();

    render(<GenesisFirstSignal onBegin={onBegin} />);

    const recovery = screen.getByRole("region", { name: "Legacy First Signal recovery" });
    expect(within(recovery).getByText(legacyProfile.purpose)).toBeInTheDocument();
    expect(within(recovery).getByText(legacyProfile.boundaries)).toBeInTheDocument();
    expect(within(recovery).getByText(/matching name is only a candidate/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Begin Genesis Quest" })).toBeDisabled();
    expect(getQuestCatalog).not.toHaveBeenCalled();

    fireEvent.click(within(recovery).getByRole("button", { name: "Check retained First Signal" }));
    await waitFor(() => expect(resolveLegacyCandidates).toHaveBeenCalledOnce());
    expect(within(recovery).queryByRole("button", { name: "Migrate this brief" })).not.toBeInTheDocument();
    fireEvent.click(await within(recovery).findByRole("button", { name: "Review Sol" }));
    expect(within(recovery).getByText(/Selected existing Entity: Sol/)).toBeInTheDocument();
    fireEvent.click(within(recovery).getByRole("button", { name: "Migrate this brief" }));

    await waitFor(() => expect(migrateLegacyFirstSignal).toHaveBeenCalledWith("candidate", {
      attempt_key: "old-attempt",
      original_name: "Sol",
      purpose: legacyProfile.purpose,
      boundaries: legacyProfile.boundaries,
      presence: {
        temperament: "curious",
        identity: "ember",
        crest: "orbit",
        motion: "calm",
      },
    }));
    expect(onBegin).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(legacyKey)).toBeNull();
    expect(localStorage.getItem(legacyResolvedKey)).toBe(JSON.stringify(legacyAttempt));
    expect(await screen.findByText(/migrated into canonical Presence and journey progress/i)).toBeInTheDocument();
  });

  it("accepts Hive's canonical normalization of reviewed legacy text", async () => {
    const legacyWithControls = {
      ...legacyJournal,
      profile: {
        ...legacyProfile,
        purpose: "  Plan\r\nour\t family garden.  ",
        boundaries: "\tAsk before.\r\nConfirm\t changes.\t",
      },
    };
    sessionStorage.setItem(legacyKey, JSON.stringify(legacyWithControls));
    vi.mocked(resolveLegacyCandidates).mockResolvedValue([{
      id: "candidate",
      name: "Sol",
      resolution: "name_candidate",
    }]);
    vi.mocked(migrateLegacyFirstSignal).mockResolvedValue({
      entity_id: "candidate",
      presence: {
        name: "Sol",
        purpose: "Plan\nour family garden.",
        boundaries: "Ask before.\nConfirm changes.",
        temperament: "curious",
        identity: "ember",
        crest: "orbit",
        motion: "calm",
      },
      badges: ["first_spark", "named", "purposeful", "presence_chosen"],
      migrated_at: "2026-10-23T00:00:00Z",
    });

    render(<GenesisFirstSignal onBegin={vi.fn()} />);
    const recovery = screen.getByRole("region", { name: "Legacy First Signal recovery" });
    fireEvent.click(within(recovery).getByRole("button", { name: "Check retained First Signal" }));
    fireEvent.click(await within(recovery).findByRole("button", { name: "Review Sol" }));
    fireEvent.click(within(recovery).getByRole("button", { name: "Migrate this brief" }));

    expect(await screen.findByText(/migrated into canonical Presence/i)).toBeInTheDocument();
    expect(sessionStorage.getItem(legacyKey)).toBeNull();
  });

  it("mirrors Rust purpose trimming when validating applicable badges", async () => {
    const legacyWithRustSignificantPurpose = {
      ...legacyJournal,
      profile: {
        ...legacyProfile,
        purpose: "\uFEFF",
        boundaries: "",
      },
    };
    sessionStorage.setItem(
      legacyKey,
      JSON.stringify(legacyWithRustSignificantPurpose),
    );
    vi.mocked(resolveLegacyCandidates).mockResolvedValue([{
      id: "candidate",
      name: "Sol",
      resolution: "name_candidate",
    }]);
    vi.mocked(migrateLegacyFirstSignal).mockResolvedValue({
      entity_id: "candidate",
      presence: {
        name: "Sol",
        purpose: "\uFEFF",
        boundaries: "",
        temperament: "curious",
        identity: "ember",
        crest: "orbit",
        motion: "calm",
      },
      badges: ["first_spark", "named", "purposeful", "presence_chosen"],
      migrated_at: "2026-10-23T00:00:00Z",
    });

    render(<GenesisFirstSignal onBegin={vi.fn()} />);
    const recovery = screen.getByRole("region", { name: "Legacy First Signal recovery" });
    fireEvent.click(within(recovery).getByRole("button", { name: "Check retained First Signal" }));
    fireEvent.click(await within(recovery).findByRole("button", { name: "Review Sol" }));
    fireEvent.click(within(recovery).getByRole("button", { name: "Migrate this brief" }));

    expect(await screen.findByText(/migrated into canonical Presence/i)).toBeInTheDocument();
    expect(sessionStorage.getItem(legacyKey)).toBeNull();
  });

  it("retains a legacy journal when migration fails and identifies a keyed completed receipt", async () => {
    const creationKey = "22222222-2222-4222-8222-222222222222";
    localStorage.setItem("abigail.first-signal.creation.v2", JSON.stringify({
      version: 2,
      revision: 1,
      profile: legacyProfile,
      attempt: {
        attemptId: creationKey,
        creationKey,
        hiveUrl: "http://127.0.0.1:43141",
        name: "Sol",
        baselineIds: [],
      },
    }));
    vi.mocked(resolveLegacyCandidates).mockResolvedValue([{
      id: "candidate",
      name: "Sol",
      resolution: "completed_receipt",
    }]);
    vi.mocked(migrateLegacyFirstSignal).mockRejectedValue(new Error("migration interrupted"));

    render(<GenesisFirstSignal onBegin={vi.fn()} />);
    const recovery = screen.getByRole("region", { name: "Legacy First Signal recovery" });
    fireEvent.click(within(recovery).getByRole("button", { name: "Check retained First Signal" }));
    const review = await within(recovery).findByRole("button", { name: "Review Sol" });
    expect(recovery).toHaveTextContent("Exact completed receipt");
    fireEvent.click(review);
    fireEvent.click(within(recovery).getByRole("button", { name: "Migrate this brief" }));

    expect(await within(recovery).findByRole("alert")).toHaveTextContent("migration interrupted");
    expect(localStorage.getItem("abigail.first-signal.creation.v2")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Begin Genesis Quest" })).toBeDisabled();
  });

  it("retains a legacy journal when the canonical response differs from the reviewed brief", async () => {
    sessionStorage.setItem(legacyKey, JSON.stringify(legacyJournal));
    vi.mocked(resolveLegacyCandidates).mockResolvedValue([{
      id: "candidate",
      name: "Sol",
      resolution: "name_candidate",
    }]);
    vi.mocked(migrateLegacyFirstSignal).mockResolvedValue({
      entity_id: "candidate",
      presence: {
        name: "Sol",
        purpose: "A different purpose",
        boundaries: legacyProfile.boundaries,
        temperament: "curious",
        identity: "ember",
        crest: "orbit",
        motion: "calm",
      },
      badges: ["first_spark", "named", "presence_chosen"],
      migrated_at: "2026-10-23T00:00:00Z",
    });

    render(<GenesisFirstSignal onBegin={vi.fn()} />);
    const recovery = screen.getByRole("region", { name: "Legacy First Signal recovery" });
    fireEvent.click(within(recovery).getByRole("button", { name: "Check retained First Signal" }));
    fireEvent.click(await within(recovery).findByRole("button", { name: "Review Sol" }));
    fireEvent.click(within(recovery).getByRole("button", { name: "Migrate this brief" }));

    expect(await within(recovery).findByRole("alert")).toHaveTextContent(/did not return.*canonical/i);
    expect(sessionStorage.getItem(legacyKey)).not.toBeNull();
    expect(screen.getByRole("button", { name: "Begin Genesis Quest" })).toBeDisabled();
  });
});
