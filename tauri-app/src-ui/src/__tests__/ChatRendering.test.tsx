/**
 * Frontend rendering parity tests for ChatInterface.
 *
 * Verifies that ChatResponse data from the backend is rendered correctly:
 *  - Reply content displayed
 *  - Tier badge and model metadata shown
 *  - Error states handled
 *  - Streaming token assembly
 */
import { describe, it, expect, beforeEach } from "vitest";

// Must import tauri-mock BEFORE any component that uses @tauri-apps/api
import "../test/tauri-mock";

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Mock } from "vitest";

// ChatInterface needs ThemeContext
import { ThemeProvider } from "../contexts/ThemeContext";
import ChatInterface from "../components/ChatInterface";

const mockInvoke = invoke as unknown as Mock;
const mockListen = listen as unknown as Mock;

function renderChat() {
  return render(
    <ThemeProvider>
      <ChatInterface />
    </ThemeProvider>,
  );
}

describe("ChatRendering parity", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockListen.mockReset();

    // Default mocks for router status and other init calls
    mockInvoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_router_status":
          return Promise.resolve(
            JSON.stringify({
              id_provider: "candle_stub",
              id_url: null,
              ego_configured: true,
              ego_provider: "openai",
              superego_configured: false,
              routing_mode: "tier_based",
            }),
          );
        case "list_missing_skill_secrets":
          return Promise.resolve([]);
        case "detect_ollama":
          return Promise.resolve(
            JSON.stringify({ managed: false, running: false, port: 11434, model_ready: false }),
          );
        case "probe_local_llm":
          return Promise.resolve(false);
        case "get_stored_providers":
          return Promise.resolve([]);
        case "get_cli_server_status":
          return Promise.resolve(JSON.stringify({ running: false }));
        case "get_memory_disclosure_settings":
          return Promise.resolve(JSON.stringify({ enabled: true }));
        case "get_active_provider":
          return Promise.resolve("openai");
        case "get_ego_model":
          return Promise.resolve(null);
        case "get_model_registry":
          return Promise.resolve({ models: [] });
        case "get_queue_stats":
          return Promise.resolve({ running: 0, queued: 0, scheduled: 0 });
        default:
          return Promise.resolve(null);
      }
    });
  });

  it("renders streaming response with tier and model metadata", async () => {
    // Track listen registrations
    const listeners: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(
      (eventName: string, callback: (event: { payload: unknown }) => void) => {
        listeners[eventName] = callback;
        return Promise.resolve(() => {});
      },
    );

    // chat_stream resolves immediately (fires events async)
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "chat_stream") {
        // Simulate streaming after a microtask
        setTimeout(() => {
          if (listeners["chat-internal-envelope"]) {
            listeners["chat-internal-envelope"]({
              payload: { kind: "request", correlation_id: "corr-stream", session_id: "s-stream" },
            });
            listeners["chat-internal-envelope"]({
              payload: {
                kind: "token",
                correlation_id: "corr-stream",
                session_id: "s-stream",
                token: "Hello ",
              },
            });
            listeners["chat-internal-envelope"]({
              payload: {
                kind: "token",
                correlation_id: "corr-stream",
                session_id: "s-stream",
                token: "world!",
              },
            });
            listeners["chat-internal-envelope"]({
              payload: {
                kind: "done",
                correlation_id: "corr-stream",
                session_id: "s-stream",
                done: {
                  reply: "Hello world!",
                  provider: "openai",
                  tier: "standard",
                  model_used: "gpt-4.1",
                  tool_calls_made: [],
                  complexity_score: 50,
                },
              },
            });
          }
        }, 10);
        return Promise.resolve();
      }
      // Fallback for init calls
      switch (cmd) {
        case "get_router_status":
          return Promise.resolve(
            JSON.stringify({
              id_provider: "candle_stub",
              id_url: null,
              ego_configured: true,
              ego_provider: "openai",
              superego_configured: false,
              routing_mode: "tier_based",
            }),
          );
        case "list_missing_skill_secrets":
          return Promise.resolve([]);
        case "detect_ollama":
          return Promise.resolve(
            JSON.stringify({ managed: false, running: false, port: 11434, model_ready: false }),
          );
        case "probe_local_llm":
          return Promise.resolve(false);
        case "get_stored_providers":
          return Promise.resolve([]);
        case "get_cli_server_status":
          return Promise.resolve(JSON.stringify({ running: false }));
        case "get_memory_disclosure_settings":
          return Promise.resolve(JSON.stringify({ enabled: true }));
        case "get_active_provider":
          return Promise.resolve("openai");
        case "get_ego_model":
          return Promise.resolve(null);
        case "get_model_registry":
          return Promise.resolve({ models: [] });
        case "get_queue_stats":
          return Promise.resolve({ running: 0, queued: 0, scheduled: 0 });
        default:
          return Promise.resolve(null);
      }
    });

    const user = userEvent.setup();
    renderChat();

    // Wait for initialization to complete
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: /message input/i })).toBeInTheDocument();
    });

    // Type and send a message
    const input = screen.getByRole("textbox", { name: /message input/i });
    await user.type(input, "hello");
    await user.keyboard("{Enter}");

    // Wait for the streamed response to appear
    await waitFor(
      () => {
        expect(screen.getByText(/Hello world!/)).toBeInTheDocument();
      },
      { timeout: 3000 },
    );

    // Verify tier and model metadata rendered (use getAllByText since the
    // model dropdown may also contain matching entries from FALLBACK_MODELS)
    await waitFor(() => {
      expect(screen.getByText(/Standard/)).toBeInTheDocument();
      expect(screen.getAllByText(/gpt-4\.1/).length).toBeGreaterThanOrEqual(1);
    });
  });

  it("renders error state when chat-error fires", async () => {
    const listeners: Record<string, (event: { payload: unknown }) => void> = {};
    mockListen.mockImplementation(
      (eventName: string, callback: (event: { payload: unknown }) => void) => {
        listeners[eventName] = callback;
        return Promise.resolve(() => {});
      },
    );

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "chat_stream") {
        setTimeout(() => {
          if (listeners["chat-internal-envelope"]) {
            listeners["chat-internal-envelope"]({
              payload: { kind: "request", correlation_id: "corr-error", session_id: "s-error" },
            });
            listeners["chat-internal-envelope"]({
              payload: {
                kind: "error",
                correlation_id: "corr-error",
                session_id: "s-error",
                error: "Provider unavailable",
              },
            });
          }
        }, 10);
        return Promise.resolve();
      }
      switch (cmd) {
        case "get_router_status":
          return Promise.resolve(
            JSON.stringify({
              id_provider: "candle_stub",
              id_url: null,
              ego_configured: true,
              ego_provider: "openai",
              superego_configured: false,
              routing_mode: "tier_based",
            }),
          );
        case "list_missing_skill_secrets":
          return Promise.resolve([]);
        case "detect_ollama":
          return Promise.resolve(
            JSON.stringify({ managed: false, running: false, port: 11434, model_ready: false }),
          );
        case "probe_local_llm":
          return Promise.resolve(false);
        case "get_stored_providers":
          return Promise.resolve([]);
        case "get_cli_server_status":
          return Promise.resolve(JSON.stringify({ running: false }));
        case "get_memory_disclosure_settings":
          return Promise.resolve(JSON.stringify({ enabled: true }));
        case "get_active_provider":
          return Promise.resolve("openai");
        case "get_ego_model":
          return Promise.resolve(null);
        case "get_model_registry":
          return Promise.resolve({ models: [] });
        case "get_queue_stats":
          return Promise.resolve({ running: 0, queued: 0, scheduled: 0 });
        default:
          return Promise.resolve(null);
      }
    });

    const user = userEvent.setup();
    renderChat();

    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: /message input/i })).toBeInTheDocument();
    });

    const input = screen.getByRole("textbox", { name: /message input/i });
    await user.type(input, "test error");
    await user.keyboard("{Enter}");

    await waitFor(
      () => {
        expect(screen.getByText(/Provider unavailable/)).toBeInTheDocument();
      },
      { timeout: 3000 },
    );
  });
});
