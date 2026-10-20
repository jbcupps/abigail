import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import GenesisNotifications from "./GenesisNotifications";
import { genesisRequest } from "../lib/genesisClient";

vi.mock("../lib/genesisClient", () => ({ genesisRequest: vi.fn() }));
afterEach(() => { cleanup(); vi.useRealTimers(); vi.clearAllMocks(); });

describe("Genesis background notifications", () => {
  it("notifies after navigation even when a fast job finishes between polls", async () => {
    vi.useFakeTimers();
    vi.mocked(genesisRequest).mockResolvedValueOnce([]).mockResolvedValue([
      { job_id: "fast-film", name: "Nova", kind: "manifestation", status: "completed" },
    ]);
    const open = vi.fn();
    await act(async () => { render(<GenesisNotifications onOpen={open} />); });
    expect(screen.queryByRole("status")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(screen.getByRole("status")).toHaveTextContent("Nova’s manifestation is ready to review.");
    fireEvent.click(screen.getByRole("button", { name: "Open Genesis" }));
    expect(open).toHaveBeenCalledOnce();
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("does not repeat old completed work when Hive opens", async () => {
    vi.useFakeTimers();
    vi.mocked(genesisRequest).mockResolvedValue([{ job_id: "old", name: "Nova", kind: "portrait", status: "completed" }]);
    await act(async () => { render(<GenesisNotifications onOpen={vi.fn()} />); });
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(screen.queryByRole("status")).toBeNull();
  });
});
