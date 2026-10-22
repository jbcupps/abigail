import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GenesisSession } from "../lib/genesisClient";
import { useGenesisEntities } from "./GenesisEntityPortrait";

const mocks = vi.hoisted(() => ({
  listGenesisSessions: vi.fn(),
}));

vi.mock("../lib/genesisClient", async () => {
  const actual = await vi.importActual<typeof import("../lib/genesisClient")>(
    "../lib/genesisClient",
  );
  return {
    ...actual,
    listGenesisSessions: mocks.listGenesisSessions,
  };
});

function receiptOnlySession(id: string): GenesisSession {
  return {
    id,
    updated_at: "2026-10-22T09:00:00Z",
    state: "review",
    awakening: undefined,
    effective_entity_id: "nova",
    definition: {
      entity_id: null,
      portrait_asset_id: null,
    },
    asset_ids: [],
  } as unknown as GenesisSession;
}

describe("Genesis Entity card refresh", () => {
  beforeEach(() => {
    mocks.listGenesisSessions.mockReset();
  });

  it("keeps the prior snapshot after a transient failure and replaces it on the next key", async () => {
    let rejectRefresh: ((reason: Error) => void) | undefined;
    const failedRefresh = new Promise<GenesisSession[]>((_, reject) => {
      rejectRefresh = reject;
    });
    mocks.listGenesisSessions
      .mockResolvedValueOnce([receiptOnlySession("prior-session")])
      .mockReturnValueOnce(failedRefresh)
      .mockResolvedValueOnce([receiptOnlySession("fresh-session")]);

    const { result, rerender } = renderHook(
      ({ refreshKey }) => useGenesisEntities(refreshKey),
      { initialProps: { refreshKey: "initial" } },
    );
    await waitFor(() => {
      expect(result.current.awaiting).toEqual({ nova: "prior-session" });
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
    });

    rerender({ refreshKey: "needs-provider" });
    await waitFor(() => expect(mocks.listGenesisSessions).toHaveBeenCalledTimes(2));
    await act(async () => {
      rejectRefresh?.(new Error("temporary Genesis list failure"));
      await failedRefresh.catch(() => undefined);
    });
    expect(result.current.awaiting).toEqual({ nova: "prior-session" });
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBe("temporary Genesis list failure");

    rerender({ refreshKey: "ready" });
    await waitFor(() => {
      expect(mocks.listGenesisSessions).toHaveBeenCalledTimes(3);
      expect(result.current.awaiting).toEqual({ nova: "fresh-session" });
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
    });
  });

  it("fails closed from an initial rejection through a consecutive failure and recovers on retry", async () => {
    mocks.listGenesisSessions
      .mockRejectedValueOnce(new Error("initial Genesis list failure"))
      .mockRejectedValueOnce(new Error("consecutive Genesis list failure"))
      .mockResolvedValueOnce([receiptOnlySession("recovered-session")]);

    const { result, rerender } = renderHook(
      ({ refreshKey }) => useGenesisEntities(refreshKey),
      { initialProps: { refreshKey: "initial" } },
    );
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBe("initial Genesis list failure");
      expect(result.current.awaiting).toEqual({});
    });

    rerender({ refreshKey: "retry-1" });
    await waitFor(() => {
      expect(mocks.listGenesisSessions).toHaveBeenCalledTimes(2);
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBe("consecutive Genesis list failure");
      expect(result.current.awaiting).toEqual({});
    });

    rerender({ refreshKey: "retry-2" });
    await waitFor(() => {
      expect(mocks.listGenesisSessions).toHaveBeenCalledTimes(3);
      expect(result.current.loading).toBe(false);
      expect(result.current.error).toBeNull();
      expect(result.current.awaiting).toEqual({ nova: "recovered-session" });
    });
  });
});
