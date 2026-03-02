import { vi } from "vitest";

// Mock @tauri-apps/api/core
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

// Mock @tauri-apps/api/event
// The listen mock captures callbacks and auto-fires a terminal internal chat
// envelope so streaming-based tests complete promptly.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockImplementation((eventName: string, callback: (event: { payload: unknown }) => void) => {
    if (eventName === "chat-internal-envelope") {
      // Simulate the backend emitting a terminal done envelope after a microtask delay.
      Promise.resolve().then(() => {
        callback({
          payload: {
            kind: "done",
            correlation_id: "test-correlation",
            session_id: "test-session",
            done: { reply: "mock reply" },
          },
        });
      });
    }
    return Promise.resolve(() => {});
  }),
  emit: vi.fn().mockResolvedValue(undefined),
}));

// Mock @tauri-apps/plugin-dialog
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
  message: vi.fn().mockResolvedValue(undefined),
  ask: vi.fn().mockResolvedValue(false),
  confirm: vi.fn().mockResolvedValue(false),
}));
