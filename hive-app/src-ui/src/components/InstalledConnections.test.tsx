import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import InstalledConnections from "./InstalledConnections";
import { detectCliProviders } from "../lib/daemonClient";

vi.mock("../lib/daemonClient", () => ({ detectCliProviders: vi.fn() }));
afterEach(() => vi.resetAllMocks());

const tool = (provider: string, authentication: boolean | null) => ({ provider, on_path: true, is_official: true, is_authenticated: authentication === true, authentication });

describe("Installed connections", () => {
  it("distinguishes available, signed out, unknown and unsupported without connecting", async () => {
    vi.mocked(detectCliProviders).mockResolvedValue({ providers: [tool("claude-cli", false), tool("codex-cli", true), tool("grok-cli", null), tool("gemini-cli", true)] });
    const select = vi.fn();
    render(<InstalledConnections busy={false} onSelect={select} />);
    expect(await screen.findByText("Not signed in")).toBeInTheDocument();
    expect(screen.getByText("Available · signed in")).toBeInTheDocument();
    expect(screen.getByText("Installed · sign-in unknown")).toBeInTheDocument();
    expect(screen.getByText("Installed · not supported in Abigail yet")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use Claude" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use Gemini" })).not.toBeInTheDocument();
    expect(screen.getByText(/claude auth login/)).toBeInTheDocument();
    expect(select).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Use ChatGPT" }));
    expect(select).toHaveBeenCalledExactlyOnceWith("codex-cli");
  });

  it("shows preview presence without pretending accounts are checked or enabling them", async () => {
    vi.mocked(detectCliProviders).mockResolvedValue({ presence_only: true, unavailable_reason: "CLI accounts are unavailable in this isolated preview.", providers: [tool("codex-cli", null)] });
    render(<InstalledConnections busy={false} onSelect={vi.fn()} />);
    expect(await screen.findByText("Installed · account not checked")).toBeInTheDocument();
    expect(screen.getByText(/CLI accounts are unavailable/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Use / })).not.toBeInTheDocument();
    expect(screen.queryByText("Not signed in")).not.toBeInTheDocument();
  });

  it("keeps discovery errors visible and retries without stale presence claims", async () => {
    vi.mocked(detectCliProviders).mockRejectedValueOnce(new Error("Discovery unavailable")).mockResolvedValueOnce({ providers: [tool("grok-cli", null)] });
    render(<InstalledConnections busy={false} onSelect={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Discovery unavailable");
    expect(screen.queryByText("Not found in a supported location")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByRole("button", { name: "Use Grok" })).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(detectCliProviders).toHaveBeenCalledTimes(2);
  });

  it("never offers an unrecognized executable even if auth was reported true", async () => {
    vi.mocked(detectCliProviders).mockResolvedValue({ providers: [{ ...tool("claude-cli", true), is_official: false }] });
    render(<InstalledConnections busy={false} onSelect={vi.fn()} />);
    expect(await screen.findByText("Installed · version not recognized")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Use / })).not.toBeInTheDocument();
  });

  it("treats old daemon false auth as unknown and makes the account check explicit", async () => {
    vi.mocked(detectCliProviders).mockResolvedValue({ providers: [{ provider: "codex-cli", on_path: true, is_official: true, is_authenticated: false }] });
    render(<InstalledConnections busy={false} onSelect={vi.fn()} />);
    expect(await screen.findByText("Installed · sign-in unknown")).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Installed AI tools" })).getByRole("button", { name: "Use ChatGPT" })).toBeEnabled();
  });
});
