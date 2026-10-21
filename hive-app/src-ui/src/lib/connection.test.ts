import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

async function nativeConnection(hiveUrl: string) {
  vi.resetModules();
  window.isTauri = true;
  invoke.mockImplementation(async (command: string) => (command === "get_hive_connection_info" ? { hive_url: hiveUrl } : "control-token"));
  const { resolveHiveConnection } = await import("./connection");
  return resolveHiveConnection();
}

afterEach(() => { invoke.mockReset(); delete window.isTauri; });

describe("Hive connection", () => {
  it.each(["https://evil.example", "http://localhost.evil.example", "http://user:password@127.0.0.1", "http://127.0.0.1?token=x", "http://127.0.0.1/forward", "http://127.0.0.1#fragment"])("rejects unsafe connection %s before obtaining the token", async (url) => {
    await expect(nativeConnection(url)).rejects.toThrow("local Hive connection");
    expect(invoke).not.toHaveBeenCalledWith("get_hive_control_token");
  });
  it("pairs the local Hive origin with the token from the native shell", async () => {
    await expect(nativeConnection("http://127.0.0.1:43141/")).resolves.toEqual({ base: "http://127.0.0.1:43141", token: "control-token" });
    expect(invoke).toHaveBeenCalledWith("get_hive_control_token");
  });
});
