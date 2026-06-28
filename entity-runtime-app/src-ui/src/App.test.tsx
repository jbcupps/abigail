import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
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
  });

  it("opens from splash into the chat runtime", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /skip/i }));

    expect(await screen.findByText("Hi — what can I help you with today?")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Type a message…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(mocks.showAppWindow).toHaveBeenCalledOnce();
  });
});
