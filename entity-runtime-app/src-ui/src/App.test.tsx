import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const mocks = vi.hoisted(() => ({
  entityHealth: vi.fn(),
  resolveEntityUrl: vi.fn(),
  showAppWindow: vi.fn(),
}));

vi.mock("./lib/connection", () => ({
  resolveEntityUrl: mocks.resolveEntityUrl,
}));

vi.mock("./lib/daemonClient", () => ({
  entityHealth: mocks.entityHealth,
}));

vi.mock("./lib/window", () => ({
  showAppWindow: mocks.showAppWindow,
}));

describe("Abigail Entity Runtime app", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.resolveEntityUrl.mockResolvedValue("http://127.0.0.1:43142");
    mocks.entityHealth.mockResolvedValue(true);
    vi.stubGlobal("fetch", vi.fn(async (url: string) => new Response(JSON.stringify({
      ok: true,
      data: url.includes("/v1/status")
        ? { entity_id: "ada", name: "Ada" }
        : { session_id: null, messages: [] },
    }), { headers: { "Content-Type": "application/json" } })));
  });

  afterEach(() => vi.unstubAllGlobals());

  it("opens from splash into the chat runtime", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText("Hi, I'm Ada. What can I help you with today?")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ada" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Type a message…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });
});
