import { afterEach, describe, expect, it, vi } from "vitest";
import { hiveHealth, listEntities, storeSecret } from "./daemonClient";

vi.mock("./connection", () => ({
  resolveHiveUrl: async () => "http://127.0.0.1:43141",
  resolveHiveConnection: async () => ({ base: "http://127.0.0.1:43141", token: "control-token" }),
}));
const json = (data: unknown) => new Response(JSON.stringify({ ok: true, data }), { headers: { "Content-Type": "application/json" } });

afterEach(() => vi.unstubAllGlobals());

describe("Hive daemon requests", () => {
  it("present the control token everywhere except the public health check", async () => {
    const fetcher = vi.fn(async (url: string) => (url.endsWith("/health") ? new Response("ok") : json(url.endsWith("/v1/secrets") ? "stored" : [])));
    vi.stubGlobal("fetch", fetcher);
    await expect(hiveHealth()).resolves.toBe(true);
    await expect(listEntities()).resolves.toEqual([]);
    await storeSecret("openai", "sk-test");
    const [health, entities, secret] = fetcher.mock.calls as unknown as [string, RequestInit][];
    expect(health[1].headers).toBeUndefined();
    expect(entities[1].headers).toEqual({ Accept: "application/json", Authorization: "Bearer control-token" });
    expect(secret[1].headers).toMatchObject({ "Content-Type": "application/json", Authorization: "Bearer control-token" });
  });
});
