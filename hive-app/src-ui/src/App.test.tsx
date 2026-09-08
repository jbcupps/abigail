import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  hiveHealth: vi.fn(),
  getSetupStatus: vi.fn(),
  setupHistory: vi.fn(),
  retrySetup: vi.fn(),
  cancelSetup: vi.fn(),
  openEntity: vi.fn(),
  showAppWindow: vi.fn(),
}));

vi.mock("./lib/daemonClient", () => ({
  getStatus: mocks.getStatus,
  hiveHealth: mocks.hiveHealth,
  getSetupStatus: mocks.getSetupStatus,
  setupHistory: mocks.setupHistory,
  retrySetup: mocks.retrySetup,
  cancelSetup: mocks.cancelSetup,
}));

vi.mock("./lib/entityWindow", () => ({
  openEntity: mocks.openEntity,
}));

vi.mock("./lib/window", () => ({
  showAppWindow: mocks.showAppWindow,
}));

describe("Abigail Hive app", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.hiveHealth.mockResolvedValue(true);
    mocks.getSetupStatus.mockResolvedValue({
      phase: "ready",
      message: "Ready",
      model: "qwen3.5:0.8b",
      active_provider: "local",
      active_model: "qwen3.5:0.8b",
    });
    mocks.setupHistory.mockResolvedValue({ messages: [] });
    mocks.getStatus.mockResolvedValue({
      entity_count: 2,
      entities: [
        {
          id: "hive",
          name: "Abigail Hive",
          birth_complete: true,
          birth_date: null,
          is_hive: true,
          immortal: true,
        },
        {
          id: "ada",
          name: "Ada",
          birth_complete: true,
          birth_date: null,
          is_hive: false,
          immortal: false,
        },
      ],
      ready_state: "ready",
      any_provider_configured: true,
      setup_complete: true,
      helper: {
        running: false,
        local_url: null,
      },
    });
  });

  it("opens from splash into the Hive dashboard", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(
      await screen.findByText("Your local guide to getting started."),
    ).toBeInTheDocument();
    expect(screen.getByText("Entities (1)")).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });

  it("keeps conversation unavailable until a real local completion is ready", async () => {
    mocks.getSetupStatus.mockResolvedValue({
      phase: "loading",
      message: "Loading your local model",
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    expect(
      await screen.findByText("Loading your local model"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "Message Abigail" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Pause startup" }));
    expect(mocks.cancelSetup).toHaveBeenCalledOnce();
  });

  it("shows a missing package failure without pretending a model works", async () => {
    mocks.getSetupStatus.mockResolvedValue({
      phase: "error",
      message: "Offline package is missing. Reinstall and Retry.",
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    expect(
      await screen.findByRole("button", { name: "Try again" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Offline package is missing/)).toBeInTheDocument();
  });

  it("keeps verified setup instructions correct even when model prose is wrong", async () => {
    mocks.setupHistory.mockResolvedValue({
      messages: [
        {
          role: "assistant",
          content:
            "An API account is a key. I already connected your cloud model.",
        },
      ],
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    await screen.findByText(/I already connected your cloud model/);
    expect(
      screen.getByText(/An API account is a developer account on the provider/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/An API key is a secret generated inside that account/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Current connection: local model"),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Anthropic ·/ })).toHaveAttribute(
      "href",
      "https://platform.claude.com/",
    );
    expect(screen.getByRole("link", { name: /OpenAI ·/ })).toHaveAttribute(
      "href",
      "https://platform.openai.com/",
    );
    expect(
      screen.getByText(
        /Enter it only in Abigail’s Connect a model form, never in chat/,
      ),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getAllByRole("button", { name: "Connect a model" })[0],
    );
    expect(
      screen.getByRole("dialog", { name: "Connect a model" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveAttribute(
      "type",
      "password",
    );
    expect(
      screen.getByRole("button", { name: "Test and connect" }),
    ).toBeDisabled();
  });
});
