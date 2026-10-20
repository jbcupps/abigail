import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import GenesisStudio from "./GenesisStudio";
import type { GenesisSession } from "../lib/genesisClient";
import { genesisAssetBlob, getGenesisMedia, getGenesisMediaCapabilities, requestGenesisMedia, type GenesisAsset, type MediaCapabilities } from "../lib/genesisMedia";

vi.mock("../lib/genesisMedia", () => ({
  actOnGenesisJob: vi.fn(), approveGenesisAsset: vi.fn(), deleteGenesisAsset: vi.fn(),
  genesisAssetBlob: vi.fn(), getGenesisMedia: vi.fn(), getGenesisMediaCapabilities: vi.fn(), requestGenesisMedia: vi.fn(),
}));

const portrait = { asset_id: "approved-portrait", kind: "portrait", approved: true, version: 1 } as GenesisAsset;
const film = { asset_id: "approved-film", kind: "manifestation", approved: true, reference_asset_id: portrait.asset_id, version: 1 } as GenesisAsset;
const session = { id: "session-one", definition: { name: "Nova", portrait_asset_id: portrait.asset_id, manifestation_asset_id: film.asset_id } } as GenesisSession;
const capabilities: MediaCapabilities = { portrait_available: true, video_available: true, portrait_model: "gpt-image-2", video_model: "gen4_turbo", portrait_reservation_usd: 0.25, video_reservation_usd: 2, daily_budget_usd: 10, daily_job_limit: 20, video_seconds: [5, 10], notice: "Provider charges apply." };

beforeEach(() => {
  vi.mocked(getGenesisMediaCapabilities).mockResolvedValue(capabilities);
  vi.mocked(getGenesisMedia).mockResolvedValue({ jobs: [], assets: [portrait, film] });
  vi.mocked(genesisAssetBlob).mockResolvedValue(new Blob(["fixture"], { type: "image/png" }));
  vi.mocked(requestGenesisMedia).mockResolvedValue({});
  vi.stubGlobal("URL", class extends URL {
    static createObjectURL = vi.fn(() => "blob:private-visual");
    static revokeObjectURL = vi.fn();
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); vi.unstubAllGlobals(); });

function studio() { return render(<GenesisStudio session={session} busy={false} onUpdate={vi.fn().mockResolvedValue(undefined)} onRefresh={vi.fn().mockResolvedValue(undefined)} />); }

describe("Genesis studio generation contracts", () => {
  it("shows the full two-candidate reservation and regenerates portraits without a film reference", async () => {
    studio();
    await screen.findByText(/2 portrait candidates \$0.50 total/);
    fireEvent.click(screen.getByRole("checkbox", { name: "I approve the displayed generation budget for this request." }));
    fireEvent.click(screen.getByRole("button", { name: "Generate portrait candidates" }));
    await waitFor(() => expect(requestGenesisMedia).toHaveBeenCalledOnce());
    const request = vi.mocked(requestGenesisMedia).mock.calls[0][1];
    expect(request).toMatchObject({ kind: "portrait", candidates: 2, cost_confirmed: true });
    expect(request).not.toHaveProperty("reference_asset_id");
  });

  it("uses a supported duration and the canonical approved portrait for films", async () => {
    studio();
    expect(await screen.findByRole("combobox", { name: "Film duration" })).toHaveValue("5");
    expect(screen.getByRole("textbox", { name: "Film direction" })).toHaveAttribute("maxlength", "600");
    fireEvent.click(screen.getByRole("checkbox", { name: "I approve the displayed generation budget for this request." }));
    fireEvent.click(screen.getByRole("button", { name: "Generate manifestation" }));
    await waitFor(() => expect(requestGenesisMedia).toHaveBeenCalledOnce());
    expect(vi.mocked(requestGenesisMedia).mock.calls[0][1]).toMatchObject({ kind: "manifestation", seconds: 5, reference_asset_id: portrait.asset_id });
  });

  it("fades a finished film onto its portrait and replay restores the film", async () => {
    const { container } = studio();
    await waitFor(() => expect(container.querySelector(".genesis-manifestation video")).not.toBeNull());
    const video = container.querySelector(".genesis-manifestation video")!;
    fireEvent.ended(video);
    expect(video.parentElement).toHaveAttribute("aria-hidden", "true");
    expect(container.querySelector(".genesis-manifestation img")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Replay emergence" }));
    await waitFor(() => expect(container.querySelector(".genesis-manifestation video")?.parentElement).toHaveAttribute("aria-hidden", "false"));
    fireEvent.click(screen.getByRole("button", { name: "Skip animation" }));
    expect(container.querySelector(".genesis-manifestation video")).toBeNull();
  });
});
