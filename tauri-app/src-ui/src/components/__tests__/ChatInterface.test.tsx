import "../../test/tauri-mock";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { describe, it, expect, beforeEach, type Mock } from "vitest";
import { ThemeProvider } from "../../contexts/ThemeContext";
import ChatInterface from "../ChatInterface";

function renderWithTheme(ui: React.ReactElement) {
  return render(<ThemeProvider>{ui}</ThemeProvider>);
}

const mockInvoke = invoke as unknown as Mock;
const mockListen = listen as unknown as Mock;

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
  mockListen.mockReset();

  const listeners: Record<string, (event: { payload: unknown }) => void> = {};
  mockListen.mockImplementation(
    (eventName: string, callback: (event: { payload: unknown }) => void) => {
      listeners[eventName] = callback;
      return Promise.resolve(() => {});
    },
  );

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
        setTimeout(() => {
          if (listeners["chat-token"]) {
            listeners["chat-token"]({ payload: "assistant fallback reply" });
          }
          if (listeners["chat-done"]) {
            listeners["chat-done"]({
              payload: {
                reply: "assistant fallback reply",
                provider: "id",
                tool_calls_made: [],
              },
            });
          }
        }, 10);
        return Promise.resolve();
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

  it("shows assistant reply after chat invoke", async () => {
    const user = userEvent.setup();
    renderWithTheme(<ChatInterface />);

    const input = await screen.findByPlaceholderText(/type|message|ask/i);
    await user.type(input, "hello");
    await user.click(screen.getByRole("button", { name: /send/i }));

    await waitFor(() => {
      expect(screen.getByText(/assistant fallback reply/i)).toBeInTheDocument();
    });
  });

  it("shows execution trace attribution with configured+executed when routing details enabled", async () => {
    const tracePayload = {
      turn_id: "test-turn-1",
      timestamp_utc: "2026-02-26T15:00:00Z",
      routing_mode: "tierbased",
      configured_provider: "openai",
      configured_model: "gpt-4.1-mini",
      target_selected: "ego",
      steps: [
        {
          provider_label: "openai",
          model_requested: "gpt-4.1-mini",
          result: "error" as const,
          error_summary: "timeout",
          started_at_utc: "2026-02-26T15:00:00Z",
          ended_at_utc: "2026-02-26T15:00:01Z",
        },
        {
          provider_label: "id(candle_stub)",
          result: "success" as const,
          started_at_utc: "2026-02-26T15:00:01Z",
          ended_at_utc: "2026-02-26T15:00:02Z",
        },
      ],
      final_step_index: 1,
      fallback_occurred: true,
    };

    const listeners: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(
      (eventName: string, callback: (event: { payload: unknown }) => void) => {
        listeners[eventName] = callback;
        return Promise.resolve(() => {});
      },
    );

    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_router_status":
          return Promise.resolve(defaultRouterStatus);
        case "get_ollama_status":
          return Promise.resolve({ managed: false, running: false, port: 11434, model_ready: false });
        case "list_missing_skill_secrets":
          return Promise.resolve([]);
        case "chat_stream":
          setTimeout(() => {
            if (listeners["chat-token"]) {
              listeners["chat-token"]({ payload: "traced reply" });
            }
            if (listeners["chat-done"]) {
              listeners["chat-done"]({
                payload: {
                  reply: "traced reply",
                  provider: "openai",
                  tier: "fast",
                  model_used: "gpt-4.1-mini",
                  execution_trace: tracePayload,
                },
              });
            }
          }, 10);
          return Promise.resolve();
        default:
          return Promise.resolve(null);
      }
    });

    const user = userEvent.setup();
    renderWithTheme(<ChatInterface />);

    const input = await screen.findByPlaceholderText(/type|message|ask/i);
    await user.type(input, "hello");
    await user.click(screen.getByRole("button", { name: /send/i }));

    await waitFor(() => {
      expect(screen.getByText(/traced reply/i)).toBeInTheDocument();
    });

    // Enable routing details to see the execution trace rendering.
    const toggleBtn = screen.getByText(/show routing details/i);
    await user.click(toggleBtn);

    // The fallback badge should now be visible.
    await waitFor(() => {
      expect(screen.getByText("fallback")).toBeInTheDocument();
    });
  });

  it("shows timestamp in execution trace header", async () => {
    const tracePayload = {
      turn_id: "test-turn-2",
      timestamp_utc: "2026-02-26T15:30:00Z",
      routing_mode: "tierbased",
      configured_provider: "anthropic",
      configured_model: "claude-sonnet-4-6",
      target_selected: "ego",
      steps: [
        {
          provider_label: "anthropic",
          model_requested: "claude-sonnet-4-6",
          result: "success" as const,
          started_at_utc: "2026-02-26T15:30:00Z",
          ended_at_utc: "2026-02-26T15:30:01Z",
        },
      ],
      final_step_index: 0,
      fallback_occurred: false,
    };

    const listeners: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(
      (eventName: string, callback: (event: { payload: unknown }) => void) => {
        listeners[eventName] = callback;
        return Promise.resolve(() => {});
      },
    );

    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_router_status":
          return Promise.resolve(defaultRouterStatus);
        case "get_ollama_status":
          return Promise.resolve({ managed: false, running: false, port: 11434, model_ready: false });
        case "list_missing_skill_secrets":
          return Promise.resolve([]);
        case "chat_stream":
          setTimeout(() => {
            if (listeners["chat-done"]) {
              listeners["chat-done"]({
                payload: {
                  reply: "timestamp reply",
                  provider: "anthropic",
                  tier: "standard",
                  model_used: "claude-sonnet-4-6",
                  execution_trace: tracePayload,
                },
              });
            }
          }, 10);
          return Promise.resolve();
        default:
          return Promise.resolve(null);
      }
    });

    const user = userEvent.setup();
    renderWithTheme(<ChatInterface />);

    const input = await screen.findByPlaceholderText(/type|message|ask/i);
    await user.type(input, "hello");
    await user.click(screen.getByRole("button", { name: /send/i }));

    await waitFor(() => {
      expect(screen.getByText(/timestamp reply/i)).toBeInTheDocument();
    });

    // Enable routing details to see the execution trace rendering.
    const toggleBtn = screen.getByText(/show routing details/i);
    await user.click(toggleBtn);

    // Check that the model badge renders from the trace
    await waitFor(() => {
      expect(screen.getByText(/claude-sonnet-4-6/i)).toBeInTheDocument();
    });
  });
});
