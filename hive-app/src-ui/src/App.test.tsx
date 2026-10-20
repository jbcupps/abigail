import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  hiveHealth: vi.fn(),
  openEntity: vi.fn(),
  showAppWindow: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("./lib/daemonClient", () => ({
  getStatus: mocks.getStatus,
  hiveHealth: mocks.hiveHealth,
}));

vi.mock("./lib/entityWindow", () => ({
  openEntity: mocks.openEntity,
}));

vi.mock("./lib/window", () => ({
  showAppWindow: mocks.showAppWindow,
}));

vi.mock("./lib/presenceApi", () => ({
  loadPresence: vi.fn().mockResolvedValue({ name: "Ada", purpose: "", temperament: "gentle", boundaries: "", identity: "aether" }),
  preparePresence: vi.fn().mockResolvedValue(undefined),
  openPresence: vi.fn().mockResolvedValue("http://127.0.0.1:43142"),
  previewEntities: vi.fn().mockReturnValue([]),
}));

describe("Abigail Hive app", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete window.isTauri;
    mocks.hiveHealth.mockResolvedValue(true);
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

  afterEach(() => { delete window.isTauri; Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); });

  it("shows native startup refusal without opening the dashboard or falling back", async () => {
    window.isTauri = true;
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: mocks.invoke } });
    mocks.invoke.mockRejectedValue(new Error("Preview Hive failed. Cleanup unconfirmed; no automatic restart."));
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    expect(await screen.findByText(/Cleanup unconfirmed; no automatic restart/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(mocks.hiveHealth).not.toHaveBeenCalled();
    expect(mocks.getStatus).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Create an Entity" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Model connections" })).not.toBeInTheDocument();
  });

  it("opens from splash into the Hive dashboard", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByRole("link", { name: "Abigail Hive home" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Model connections" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Your Entities 1" })).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });
});
