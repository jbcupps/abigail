import "../../test/tauri-mock";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import SanctumDrawer from "../SanctumDrawer";

const mockInvoke = invoke as unknown as Mock;

describe("SanctumDrawer", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("falls back to Conscience when staff tab becomes unavailable", async () => {
    const user = userEvent.setup();
    const pollers: Array<() => Promise<void>> = [];
    vi.spyOn(globalThis, "setInterval").mockImplementation((fn: TimerHandler) => {
      pollers.push(fn as () => Promise<void>);
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    vi.spyOn(globalThis, "clearInterval").mockImplementation(() => {});

    let backendHealthy = true;
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "get_orchestration_backend_status") {
        return Promise.resolve({ healthy: backendHealthy });
      }
      return Promise.resolve(null);
    });

    render(
      <SanctumDrawer
        open
        onClose={() => {}}
        onDisconnect={() => {}}
      />
    );

    const staffTab = await screen.findByRole("tab", { name: /staff/i });
    await user.click(staffTab);
    expect(staffTab).toHaveAttribute("aria-selected", "true");

    backendHealthy = false;
    await act(async () => {
      await pollers[0]?.();
    });

    await waitFor(() => {
      expect(screen.queryByRole("tab", { name: /staff/i })).not.toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /conscience/i })).toHaveAttribute("aria-selected", "true");
      expect(screen.getByText(/ethical reflection/i)).toBeInTheDocument();
    });
  });

  it("shows browser sessions and clear controls in the Browser Session tab", async () => {
    const user = userEvent.setup();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "get_orchestration_backend_status") {
        return Promise.resolve({ healthy: false });
      }
      if (cmd === "list_browser_sessions") {
        return Promise.resolve([
          {
            entity_id: "entity-123",
            profile_dir: "E:/Agents/abigail/data/identities/entity-123/browser_profile",
            active_in_process: true,
            last_used_at_utc: "2026-03-06T15:00:00.000Z",
            last_action: "login_with_oauth",
            current_url: "https://mail.google.com",
            page_title: "Inbox",
            cookie_count: 6,
          },
        ]);
      }
      if (cmd === "clear_browser_session") {
        return Promise.resolve(null);
      }
      return Promise.resolve(null);
    });

    render(
      <SanctumDrawer
        open
        onClose={() => {}}
        onDisconnect={() => {}}
      />
    );

    await user.click(screen.getByRole("tab", { name: /browser session/i }));

    expect(await screen.findByText(/persistent playwright profiles/i)).toBeInTheDocument();
    expect(await screen.findByText(/entity entity-123/i)).toBeInTheDocument();
    expect(await screen.findByText(/https:\/\/mail\.google\.com/i)).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /clear session/i })).toBeInTheDocument();
  });
});
