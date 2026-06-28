import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  hiveHealth: vi.fn(),
  openEntity: vi.fn(),
  showAppWindow: vi.fn(),
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

describe("Abigail Hive app", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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

  it("opens from splash into the Hive dashboard", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByRole("heading", { name: "Abigail Hive" })).toBeInTheDocument();
    expect(screen.getByText("Your family's private AI coordinator.")).toBeInTheDocument();
    expect(screen.getByText("Entities (2)")).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });
});
