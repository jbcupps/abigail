import "../../test/tauri-mock";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import ProviderDrawer from "../ProviderDrawer";

const mockInvoke = invoke as unknown as Mock;

describe("ProviderDrawer", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "get_stored_providers") return Promise.resolve(["openai"]);
      if (cmd === "get_router_status") return Promise.resolve({ routing_mode: "TierBased", ego_provider: "openai" });
      if (cmd === "detect_cli_providers_full") return Promise.resolve([
        {
          provider_name: "claude-cli",
          binary: "claude",
          on_path: true,
          is_official: true,
          is_authenticated: true,
          version: "1.0.0",
          auth_hint: null,
        },
      ]);
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the drawer when open", () => {
    render(<ProviderDrawer onClose={() => {}} />);
    const drawer = screen.getByTestId("provider-drawer");
    expect(drawer).toBeInTheDocument();
  });

  it("calls get_stored_providers and get_router_status on mount", async () => {
    render(<ProviderDrawer onClose={() => {}} />);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_stored_providers");
      expect(mockInvoke).toHaveBeenCalledWith("get_router_status");
      expect(mockInvoke).toHaveBeenCalledWith("detect_cli_providers_full");
    });
  });

  it("shows API providers with ready badges", async () => {
    render(<ProviderDrawer onClose={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("OpenAI")).toBeInTheDocument();
      expect(screen.getByText("[READY]")).toBeInTheDocument();
    });
  });

  it("fires onClose when backdrop is clicked", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ProviderDrawer onClose={onClose} />);

    const backdrop = screen.getByTestId("provider-drawer-backdrop");
    await user.click(backdrop);
    expect(onClose).toHaveBeenCalled();
  });

  it("fires onClose when close button is clicked", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ProviderDrawer onClose={onClose} />);

    const closeBtn = screen.getByLabelText("Close drawer");
    await user.click(closeBtn);
    expect(onClose).toHaveBeenCalled();
  });

  it("switches to CLI tab and shows detected tools", async () => {
    const user = userEvent.setup();
    render(<ProviderDrawer onClose={() => {}} />);

    const cliTab = await screen.findByRole("tab", { name: /cli tools/i });
    await user.click(cliTab);

    await waitFor(() => {
      expect(screen.getByText("claude")).toBeInTheDocument();
      expect(screen.getByText("Official")).toBeInTheDocument();
      expect(screen.getByText("Authed")).toBeInTheDocument();
    });
  });

  it("shows active provider indicator", async () => {
    render(<ProviderDrawer onClose={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("openai")).toBeInTheDocument();
      expect(screen.getByText("TierBased")).toBeInTheDocument();
    });
  });
});
