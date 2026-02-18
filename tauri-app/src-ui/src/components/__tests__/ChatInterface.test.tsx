import "../../test/tauri-mock";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { describe, it, expect, beforeEach, type Mock } from "vitest";
import { ThemeProvider } from "../../contexts/ThemeContext";
import ChatInterface from "../ChatInterface";

function renderWithTheme(ui: React.ReactElement) {
  return render(<ThemeProvider>{ui}</ThemeProvider>);
}

const mockInvoke = invoke as unknown as Mock;

const defaultRouterStatus = {
  id_provider: "local_http",
  id_url: "http://localhost:11434",
  ego_configured: true,
  ego_provider: "openai",
  superego_configured: false,
  routing_mode: "tier_based",
  council_providers: 0,
};

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "get_router_status":
        return Promise.resolve(defaultRouterStatus);
      case "get_ollama_status":
        return Promise.resolve({
          managed: false,
          running: false,
          port: 11434,
          model_ready: false,
        });
      case "list_missing_skill_secrets":
        return Promise.resolve([]);
      case "chat_stream":
        return Promise.resolve("assistant fallback reply");
      default:
        return Promise.resolve(null);
    }
  });
});

describe("ChatInterface", () => {
  it("renders without crashing", async () => {
    renderWithTheme(<ChatInterface />);
    await waitFor(() => {
      expect(screen.getByPlaceholderText(/type|message|ask/i)).toBeInTheDocument();
    });
  });

  it("shows the message input placeholder", async () => {
    renderWithTheme(<ChatInterface />);
    const input = await screen.findByPlaceholderText(/type|message|ask/i);
    expect(input).toBeInTheDocument();
  });

  it("shows fallback reply when no stream tokens arrive", async () => {
    const user = userEvent.setup();
    renderWithTheme(<ChatInterface />);

    const input = await screen.findByPlaceholderText(/type|message|ask/i);
    await user.type(input, "hello");
    await user.click(screen.getByRole("button", { name: /send/i }));

    await waitFor(() => {
      expect(screen.getByText(/assistant fallback reply/i)).toBeInTheDocument();
    });
  });
});
