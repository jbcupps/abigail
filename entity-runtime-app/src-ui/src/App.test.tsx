import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  entityHealth: vi.fn(),
  getStatus: vi.fn(),
  getSessionStatus: vi.fn(),
  fetch: vi.fn(),
  profileResponse: vi.fn(),
  resolveEntityUrl: vi.fn(),
  showAppWindow: vi.fn(),
}));

vi.mock("./lib/connection", () => ({
  resolveEntityUrl: mocks.resolveEntityUrl,
}));

vi.mock("./lib/daemonClient", () => ({
  entityHealth: mocks.entityHealth,
  getStatus: mocks.getStatus,
  getSessionStatus: mocks.getSessionStatus,
}));

vi.mock("./lib/window", () => ({
  showAppWindow: mocks.showAppWindow,
}));

describe("Abigail Entity Runtime app", () => {
  const profile = {
    name: "Vesper",
    purpose: "Explore ideas together.",
    temperament: "curious",
    boundaries: "Ask before taking action.",
    identity: "ember",
  };

  beforeEach(() => {
    vi.clearAllMocks();
    window.history.replaceState({}, "", "/");
    vi.stubGlobal("fetch", mocks.fetch);
    mocks.resolveEntityUrl.mockResolvedValue("http://127.0.0.1:43142");
    mocks.entityHealth.mockResolvedValue(true);
    mocks.getStatus.mockResolvedValue({ entity_id: "entity-123", name: "Vesper" });
    mocks.getSessionStatus.mockResolvedValue({ lease: { entity_id: "entity-123", hive_url: "http://127.0.0.1:43141" } });
    mocks.profileResponse.mockReset().mockResolvedValue({ ok: true, json: async () => ({ ok: true, data: profile }) });
    mocks.fetch.mockReset().mockImplementation(async (url: string) => {
      if (url.endsWith("/presence")) return mocks.profileResponse();
      const data = url.includes("/v1/status")
        ? { entity_id: "entity-123", name: "Vesper" }
        : { session_id: "restored-session", messages: [] };
      return { ok: true, json: async () => ({ ok: true, data }) };
    });
  });

  afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); });

  it("opens from splash into the chat runtime", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText("Hi, I'm Vesper. What can I help you with today?")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });

  it("restores the saved visual identity from Hive when an Entity window is reopened", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByRole("heading", { name: "Vesper", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("Explore ideas together.")).toBeInTheDocument();
    expect((await screen.findByRole("img", { name: "Ready when you are" })).closest(".presence-art")).toHaveClass("identity-ember");
    expect(mocks.fetch).toHaveBeenCalledWith("http://127.0.0.1:43141/v1/entities/entity-123/presence", expect.objectContaining({
      headers: { Accept: "application/json" }, signal: expect.any(AbortSignal),
    }));
    expect(screen.queryByText(/Visual profile unavailable/)).not.toBeInTheDocument();
  });

  it("waits for an initial identity before requesting conversation history", async () => {
    let confirm!: (value: unknown) => void;
    mocks.getStatus.mockReturnValue(new Promise(resolve => { confirm = resolve; }));
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    expect(await screen.findByText("Confirming your Entity.")).toBeInTheDocument();
    expect(mocks.fetch).not.toHaveBeenCalled();
    expect(screen.queryByRole("textbox", { name: "YOUR MESSAGE" })).not.toBeInTheDocument();
    await act(async () => confirm({ entity_id: "entity-123", name: "Vesper" }));
    expect(await screen.findByText("Hi, I'm Vesper. What can I help you with today?")).toBeInTheDocument();
  });

  it("offers an explicit identity retry after a stalled status lookup without opening a conversation", async () => {
    vi.useFakeTimers();
    try {
      mocks.getStatus.mockReturnValue(new Promise(() => {}));
      render(<App />);
      fireEvent.click(screen.getByRole("button", { name: /skip/i }));
      await act(async () => {});
      expect(mocks.getStatus).toHaveBeenCalledOnce();
      await act(async () => vi.advanceTimersByTimeAsync(5_000));
      expect(screen.getByText("Your Entity's identity could not be confirmed. No conversation has been opened.")).toBeInTheDocument();
      expect(mocks.fetch).not.toHaveBeenCalled();
      mocks.getStatus.mockResolvedValue({ entity_id: "entity-123", name: "Vesper" });
      await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
      expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeEnabled();
    } finally { vi.useRealTimers(); }
  });

  it("rejects a later runtime identity that differs from its initial profile identity", async () => {
    mocks.fetch.mockImplementation(async (url: string) => {
      if (url.endsWith("/presence")) return mocks.profileResponse();
      const data = url.includes("/v1/status")
        ? { entity_id: "other-entity", name: "Other Entity" }
        : { session_id: "other-session", messages: [{ role: "assistant", content: "Other Entity's synthetic history" }] };
      return { ok: true, json: async () => ({ ok: true, data }) };
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/different Entity/i);
    expect(screen.queryByText("Other Entity's synthetic history")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeDisabled();
    expect(mocks.fetch.mock.calls.some(([url]) => String(url).includes("/chat/stream"))).toBe(false);
  });

  it("restores the same identity, visual profile and saved history after a same-origin app remount on a new runtime port", async () => {
    mocks.fetch.mockImplementation(async (url: string) => {
      if (url.endsWith("/presence")) return mocks.profileResponse();
      const data = url.includes("/v1/status")
        ? { entity_id: "entity-123", name: "Vesper" }
        : { session_id: "stable-custom-session", messages: [{ role: "assistant", content: "Persistent synthetic fixture history" }] };
      return { ok: true, json: async () => ({ ok: true, data }) };
    });
    const first = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    await screen.findByText("Persistent synthetic fixture history");
    await screen.findByText("Explore ideas together.");
    first.unmount();
    mocks.fetch.mockClear();
    window.history.replaceState({}, "", "/?build=next");
    mocks.resolveEntityUrl.mockResolvedValue("http://127.0.0.1:50001");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));
    expect(await screen.findByText("Persistent synthetic fixture history")).toBeInTheDocument();
    expect(await screen.findByText("Explore ideas together.")).toBeInTheDocument();
    expect((await screen.findByRole("img", { name: "Ready when you are" })).closest(".presence-art")).toHaveClass("identity-ember");
    expect(mocks.fetch).toHaveBeenCalledWith("http://127.0.0.1:50001/v1/chat/history?limit=200", expect.any(Object));
    expect(mocks.fetch).toHaveBeenCalledWith("http://127.0.0.1:43141/v1/entities/entity-123/presence", expect.any(Object));
    expect(mocks.fetch.mock.calls.every(([, init]) => !init?.method || init.method === "GET")).toBe(true);
  });

  it("uses an explicitly configured Hive address for the profile lookup", async () => {
    window.history.replaceState({}, "", "/?hiveUrl=http%3A%2F%2F127.0.0.1%3A43151");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    await screen.findByText("Explore ideas together.");
    expect(mocks.fetch).toHaveBeenCalledWith("http://127.0.0.1:43151/v1/entities/entity-123/presence", expect.any(Object));
  });

  it.each(["query", "environment"])("restores from a %s override without waiting on session discovery", async (source) => {
    const hiveUrl = "http://127.0.0.1:43151";
    if (source === "query") window.history.replaceState({}, "", `/?hiveUrl=${encodeURIComponent(hiveUrl)}`);
    else vi.stubEnv("VITE_HIVE_DAEMON_URL", hiveUrl);
    mocks.getSessionStatus.mockReturnValue(new Promise(() => {}));
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText("Explore ideas together.")).toBeInTheDocument();
    expect(mocks.fetch).toHaveBeenCalledWith(`${hiveUrl}/v1/entities/entity-123/presence`, expect.any(Object));
    expect(mocks.getSessionStatus).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeInTheDocument();
  });

  it("uses the native runtime session's Hive address when no override is provided", async () => {
    mocks.getSessionStatus.mockResolvedValue({ lease: { entity_id: "entity-123", hive_url: "http://127.0.0.1:43161" } });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    await screen.findByText("Explore ideas together.");
    expect(mocks.fetch).toHaveBeenCalledWith("http://127.0.0.1:43161/v1/entities/entity-123/presence", expect.any(Object));
  });

  it("ignores a session lease for a different Entity", async () => {
    mocks.getSessionStatus.mockResolvedValue({ lease: { entity_id: "other-entity", hive_url: "http://127.0.0.1:43161" } });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    await screen.findByText("Explore ideas together.");
    expect(mocks.fetch).toHaveBeenCalledWith("http://127.0.0.1:43141/v1/entities/entity-123/presence", expect.any(Object));
  });

  it("keeps chat available with the runtime name when the saved profile cannot be loaded", async () => {
    mocks.profileResponse.mockRejectedValue(new Error("Hive offline"));
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText("Visual profile unavailable. Using the default appearance.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Vesper", level: 1 })).toBeInTheDocument();
    expect((await screen.findByRole("img", { name: "Ready when you are" })).closest(".presence-art")).toHaveClass("identity-aether");
    expect(screen.getByRole("textbox", { name: "YOUR MESSAGE" })).toBeInTheDocument();
  });

  it("rejects malformed saved profile data instead of rendering it as a restored identity", async () => {
    mocks.profileResponse.mockResolvedValue({ ok: true, json: async () => ({ ok: true, data: { ...profile, identity: "unknown" } }) });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText(/Visual profile unavailable/)).toBeInTheDocument();
    expect((await screen.findByRole("img", { name: "Ready when you are" })).closest(".presence-art")).toHaveClass("identity-aether");
  });

  it("does not issue a profile request to a non-HTTP address", async () => {
    window.history.replaceState({}, "", "/?hiveUrl=file%3A%2F%2F%2Fprofile");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText(/Visual profile unavailable/)).toBeInTheDocument();
    expect(mocks.fetch.mock.calls.some(([url]) => String(url).endsWith("/presence"))).toBe(false);
  });
});
