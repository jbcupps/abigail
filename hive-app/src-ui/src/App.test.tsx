import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

type MockGenesisEntities = {
  portraits: Record<string, unknown>;
  awaiting: Record<string, string>;
  loading?: boolean;
  error?: string | null;
};

const mocks = vi.hoisted(() => ({
  completeEntitySetup: vi.fn(),
  connectLocalModel: vi.fn(),
  detectCliProviders: vi.fn(),
  getStatus: vi.fn(),
  hiveHealth: vi.fn(),
  openEntity: vi.fn(),
  showAppWindow: vi.fn(),
  genesisWorkspace: vi.fn(),
  genesisMounts: 0,
  genesisClosers: [] as Array<() => void>,
  genesisEntities: { portraits: {}, awaiting: { ada: "session-a" } } as MockGenesisEntities,
  genesisEntitiesForKey: null as ((refreshKey: unknown) => MockGenesisEntities) | null,
  genesisRefreshKeys: [] as unknown[],
}));

function statusWithUnbornAda(readyState: "ready" | "needs_provider") {
  return {
    entity_count: 3,
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
        birth_complete: false,
        birth_date: null,
        is_hive: false,
        immortal: false,
      },
      {
        id: "grace",
        name: "Grace",
        birth_complete: true,
        birth_date: null,
        is_hive: false,
        immortal: false,
      },
    ],
    ready_state: readyState,
    any_provider_configured: readyState === "ready",
    setup_complete: readyState === "ready",
    helper: { running: false, local_url: null },
  };
}

vi.mock("./lib/daemonClient", () => ({
  completeEntitySetup: mocks.completeEntitySetup,
  connectLocalModel: mocks.connectLocalModel,
  detectCliProviders: mocks.detectCliProviders,
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

vi.mock("./components/GenesisEntityPortrait", () => ({
  default: ({ name }: { name: string }) => <span>{name} portrait</span>,
  useGenesisEntities: (refreshKey: unknown) => {
    mocks.genesisRefreshKeys.push(refreshKey);
    return mocks.genesisEntitiesForKey?.(refreshKey) ?? mocks.genesisEntities;
  },
}));

describe("Abigail Hive app", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.genesisMounts = 0;
    mocks.genesisClosers.length = 0;
    mocks.genesisEntities = { portraits: {}, awaiting: { ada: "session-a" } };
    mocks.genesisEntitiesForKey = null;
    mocks.genesisRefreshKeys.length = 0;
    mocks.hiveHealth.mockResolvedValue(true);
    mocks.detectCliProviders.mockResolvedValue([]);
    mocks.connectLocalModel.mockResolvedValue({ model: "family-model" });
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

    fireEvent.click(screen.getByRole("button", { name: "Continue Ada's Genesis" }));
    expect(screen.getByRole("dialog", { name: "Switch Genesis workspace?" })).toBeInTheDocument();
    expect(screen.getByTestId("app-shell")).toHaveAttribute("inert");
    expect(screen.getByTestId("genesis-workspace")).toHaveTextContent("1:new");
    // A second trigger cannot silently replace the request already being confirmed.
    fireEvent.click(launch);
    fireEvent.click(screen.getByRole("button", { name: "Switch workspace" }));
    await waitFor(() => expect(screen.getByTestId("genesis-workspace")).toHaveTextContent("2:session-a"));

    act(() => mocks.genesisClosers[0]());
    await waitFor(() => expect(screen.getByTestId("genesis-workspace")).toHaveTextContent("2:session-a"));
  });

  it("blocks generic setup while an unborn card's Genesis linkage is still loading", async () => {
    mocks.genesisEntities = {
      portraits: {},
      awaiting: {},
      loading: true,
      error: null,
    };
    mocks.getStatus.mockResolvedValue(statusWithUnbornAda("ready"));

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    await screen.findByRole("heading", { name: "Abigail Hive" });

    expect(screen.getByText("Checking Genesis linkage…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Checking Genesis…" })).toBeDisabled();
    expect(mocks.completeEntitySetup).not.toHaveBeenCalled();
    expect(mocks.openEntity).not.toHaveBeenCalled();
  });

  it("retries an initial ready-state linkage failure into Continue Genesis without generic setup", async () => {
    mocks.getStatus.mockResolvedValue(statusWithUnbornAda("ready"));
    const recovered: MockGenesisEntities = {
      portraits: {},
      awaiting: { ada: "recovered-ready-session" },
      loading: false,
      error: null,
    };
    const failed: MockGenesisEntities = {
      portraits: {},
      awaiting: {},
      loading: false,
      error: "initial Genesis list failure",
    };
    mocks.genesisEntitiesForKey = (refreshKey) =>
      String(refreshKey).includes("retry=1") ? recovered : failed;

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    await screen.findByRole("heading", { name: "Abigail Hive" });

    expect(screen.getByText("Genesis linkage could not be checked.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry Genesis check" }));
    expect(await screen.findByRole("button", { name: "Continue Ada's Genesis" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Continue Ada's Genesis" }));
    expect(await screen.findByTestId("genesis-workspace")).toHaveTextContent(
      "1:recovered-ready-session",
    );
    expect(mocks.completeEntitySetup).not.toHaveBeenCalled();
    expect(mocks.openEntity).not.toHaveBeenCalled();
  });

  it("recovers consecutive linkage failures into the provider gate and Continue Genesis", async () => {
    const failedRefreshSnapshot: MockGenesisEntities = {
      portraits: {},
      awaiting: {},
      loading: false,
      error: "Genesis list failure",
    };
    const needsProviderSnapshot: MockGenesisEntities = {
      portraits: {},
      awaiting: { ada: "provider-gated-session" },
      loading: false,
      error: null,
    };
    const readySnapshot: MockGenesisEntities = {
      portraits: {},
      awaiting: { ada: "fresh-receipt-session" },
      loading: false,
      error: null,
    };
    mocks.genesisEntitiesForKey = (refreshKey) => {
      const key = String(refreshKey);
      if (key.includes("ready")) return readySnapshot;
      if (key.includes("retry=2")) return needsProviderSnapshot;
      return failedRefreshSnapshot;
    };
    const needsProvider = statusWithUnbornAda("needs_provider");
    mocks.getStatus
      .mockResolvedValueOnce(needsProvider)
      .mockResolvedValue(statusWithUnbornAda("ready"));

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    await screen.findByRole("heading", { name: "Abigail Hive" });

    fireEvent.click(screen.getByRole("button", { name: "Retry Genesis check" }));
    await waitFor(() =>
      expect(
        mocks.genesisRefreshKeys.some((key) => String(key).includes("retry=1")),
      ).toBe(true),
    );
    expect(screen.getByRole("button", { name: "Retry Genesis check" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Retry Genesis check" }));

    expect(screen.getByText("Connect a model to continue Genesis")).toBeInTheDocument();
    const setupButton = screen.getByRole("button", { name: "Connect a model to continue" });
    expect(setupButton).toBeEnabled();
    expect(screen.getByRole("button", { name: "Create or resume an Entity" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Open Grace" })).toBeDisabled();

    fireEvent.click(setupButton);
    expect(await screen.findByRole("dialog", { name: "Connect a model" })).toBeInTheDocument();
    expect(screen.queryByTestId("genesis-workspace")).not.toBeInTheDocument();
    expect(mocks.completeEntitySetup).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /^Use a local model/ }));
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(mocks.connectLocalModel).toHaveBeenCalledOnce());
    await waitFor(() =>
      expect(mocks.genesisRefreshKeys.some((key) => String(key).includes("ready"))).toBe(true),
    );
    await screen.findByRole("button", { name: "Continue Ada's Genesis" });
    fireEvent.click(screen.getByRole("button", { name: "Done" }));

    fireEvent.click(screen.getByRole("button", { name: "Continue Ada's Genesis" }));
    expect(await screen.findByTestId("genesis-workspace")).toHaveTextContent("1:fresh-receipt-session");
    expect(mocks.completeEntitySetup).not.toHaveBeenCalled();
  });
});
