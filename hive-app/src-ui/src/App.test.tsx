import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  hiveHealth: vi.fn(),
  openEntity: vi.fn(),
  showAppWindow: vi.fn(),
  genesisWorkspace: vi.fn(),
  genesisMounts: 0,
  genesisClosers: [] as Array<() => void>,
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

vi.mock("./components/GenesisWorkspace", () => ({
  default: (props: { resumeId?: string; onClose: () => void }) => {
    mocks.genesisWorkspace(props);
    const instance = useRef(0);
    if (instance.current === 0) {
      instance.current = ++mocks.genesisMounts;
      mocks.genesisClosers.push(props.onClose);
    }
    return <div data-testid="genesis-workspace">{instance.current}:{props.resumeId ?? "new"}</div>;
  },
}));

describe("Abigail Hive app", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.genesisMounts = 0;
    mocks.genesisClosers.length = 0;
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
    expect(screen.getByText("Entities (1)")).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });

  it("confirms workspace switches and ignores a superseded workspace close", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    await screen.findByRole("heading", { name: "Abigail Hive" });

    const launch = screen.getByRole("button", { name: "Create or resume an Entity" });
    fireEvent.click(launch);
    expect(await screen.findByTestId("genesis-workspace")).toHaveTextContent("1:new");

    fireEvent.click(launch);
    expect(screen.getByRole("dialog", { name: "Switch Genesis workspace?" })).toBeInTheDocument();
    expect(screen.getByTestId("genesis-workspace")).toHaveTextContent("1:new");
    fireEvent.click(screen.getByRole("button", { name: "Switch workspace" }));
    await waitFor(() => expect(screen.getByTestId("genesis-workspace")).toHaveTextContent("2:new"));

    act(() => mocks.genesisClosers[0]());
    await waitFor(() => expect(screen.getByTestId("genesis-workspace")).toHaveTextContent("2:new"));
  });
});
