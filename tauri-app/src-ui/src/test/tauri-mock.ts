import { vi } from "vitest";

// Mock @tauri-apps/api/core
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

// Mock @tauri-apps/api/event
// The listen mock captures callbacks and auto-fires a { done: true } event
// for "chat-token" listeners so streaming-based tests complete promptly.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockImplementation((eventName: string, callback: (event: { payload: unknown }) => void) => {
    if (eventName === "chat-token") {
      // Simulate the backend emitting { done: true } after a microtask delay.
      Promise.resolve().then(() => {
        callback({ payload: { done: true } });
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
