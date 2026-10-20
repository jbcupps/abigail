import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { resolveHiveUrl } from "./connection";
import { listEntities } from "./daemonClient";
import { openPresence, savePresence } from "./presenceApi";
import { createPresenceEntity, lookupCreationAttempt, type CreationAttempt } from "./presenceCreation";
import { defaultProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), fetch: vi.fn() }));
const HIVE = "http://127.0.0.1:44141";
const KEY = "11111111-1111-4111-8111-111111111111";
const ENTITY = "22222222-2222-4222-8222-222222222222";
const owner = { channel: "beta", canonical_data_root: "C:\\synthetic-preview", profile_id: KEY, instance_id: ENTITY, hive_instance_id: ENTITY };
const attempt: CreationAttempt = { attemptId: KEY, creationKey: KEY, hiveUrl: HIVE, name: "Nova", baselineIds: [] };
const envelope = (data: unknown) => new Response(JSON.stringify({ ok: true, data }), { status: 200 });

beforeEach(() => {
  window.isTauri = true;
  // The shared module resolves Runtime's Tauri package; exercise its real
  // invoke adapter through the same native boundary used by either shell.
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: mocks.invoke } });
  window.history.replaceState({}, "", "/");
  mocks.invoke.mockReset().mockResolvedValue({ hive_url: HIVE, process_identity: owner });
  mocks.fetch.mockReset();
  vi.stubGlobal("fetch", mocks.fetch);
  vi.stubGlobal("navigator", { locks: {
    request: async (_name: string, _options: unknown, run: (lock: object) => unknown) => run({}),
  } });
});
afterEach(() => { delete window.isTauri; Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.unstubAllGlobals(); vi.unstubAllEnvs(); });

it("rejects native resolution errors instead of using browser or default ports", async () => {
  window.history.replaceState({}, "", "/?hiveUrl=http://127.0.0.1:49999");
  vi.stubEnv("VITE_HIVE_DAEMON_URL", "http://127.0.0.1:48888");
  mocks.invoke.mockRejectedValue(new Error("Wrong live instance"));
  await expect(resolveHiveUrl()).rejects.toThrow(/Wrong live instance/);
  expect(mocks.fetch).not.toHaveBeenCalled();
});

it("adds verified ownership to ordinary Hive API requests", async () => {
  mocks.fetch.mockResolvedValue(envelope([]));
  await listEntities();
  const [url, init] = mocks.fetch.mock.calls[0];
  expect(url).toBe(`${HIVE}/v1/entities`);
  const headers = new Headers(init.headers);
  expect(headers.get("x-abigail-profile-id")).toBe(KEY);
  expect(headers.get("x-abigail-process-instance-id")).toBe(ENTITY);
  expect(init.redirect).toBe("error");
});

it("verifies the retained Hive before exact lookup and preserves the key on refusal", async () => {
  mocks.invoke.mockRejectedValue(new Error("Stale endpoint"));
  await expect(lookupCreationAttempt(attempt)).rejects.toThrow(/saved Hive endpoint is stale/);
  expect(mocks.invoke).toHaveBeenCalledWith("get_hive_connection_info", { hiveUrl: HIVE }, undefined);
  expect(mocks.fetch).not.toHaveBeenCalled();
  expect(attempt.creationKey).toBe(KEY);
});

it("does not dispatch or create when native verification fails", async () => {
  mocks.invoke.mockRejectedValue(new Error("Stale endpoint"));
  const onDispatched = vi.fn();
  await expect(createPresenceEntity("Nova", false, { attempt, onDispatched })).rejects.toMatchObject({ outcome: "not-sent" });
  expect(onDispatched).not.toHaveBeenCalled();
  expect(mocks.fetch).not.toHaveBeenCalled();
});

it("rechecks native ownership after lookup before journaling and POST", async () => {
  mocks.invoke.mockResolvedValueOnce({ hive_url: HIVE, process_identity: owner })
    .mockRejectedValueOnce(new Error("Instance changed after lookup"));
  mocks.fetch.mockResolvedValue(envelope({ creation_key: KEY, state: "absent" }));
  const onDispatched = vi.fn();
  await expect(createPresenceEntity("Nova", false, { attempt, onDispatched })).rejects.toMatchObject({ outcome: "not-sent" });
  expect(onDispatched).not.toHaveBeenCalled();
  expect(mocks.fetch).toHaveBeenCalledTimes(1);
  expect(mocks.fetch.mock.calls[0][0]).toContain("/v1/entity-creations/");
  expect(mocks.fetch.mock.calls[0][1].redirect).toBe("error");
});

it("labels keyed creation without changing its original key or name", async () => {
  mocks.fetch.mockResolvedValueOnce(envelope({ creation_key: KEY, state: "absent" }))
    .mockResolvedValueOnce(envelope({ creation_key: KEY, id: ENTITY, directory: `identities/${ENTITY}` }));
  const onDispatched = vi.fn();
  await expect(createPresenceEntity("Edited label", false, { attempt, onDispatched })).resolves.toBe(ENTITY);
  const [, init] = mocks.fetch.mock.calls[1];
  expect(JSON.parse(init.body)).toEqual({ name: "Nova", creation_key: KEY });
  expect(new Headers(init.headers).get("x-abigail-process-instance-id")).toBe(ENTITY);
  expect(init.redirect).toBe("error");
  expect(onDispatched).toHaveBeenCalledOnce();
});

it("refuses redirects for native visual profile saves", async () => {
  const profile = defaultProfile("Nova");
  mocks.fetch.mockResolvedValue(envelope(profile));
  await savePresence(ENTITY, profile, false, { hiveUrl: HIVE });
  expect(mocks.fetch.mock.calls[0][1].redirect).toBe("error");
});

it("refuses profile saves against a stale journal endpoint", async () => {
  mocks.invoke.mockRejectedValue(new Error("Stale endpoint"));
  await expect(savePresence(ENTITY, defaultProfile("Nova"), false, { hiveUrl: HIVE })).rejects.toThrow(/saved Hive endpoint is stale/);
  expect(mocks.fetch).not.toHaveBeenCalled();
});

it("opens the native inline runtime through the shell's pinning command", async () => {
  mocks.invoke.mockResolvedValue({ runtime_url: "http://127.0.0.1:44142", hive_url: HIVE, process_identity: owner, hive_process_identity: owner });
  await expect(openPresence(ENTITY, undefined, HIVE)).resolves.toBe("http://127.0.0.1:44142");
  expect(mocks.invoke).toHaveBeenCalledWith("open_entity_connection", { entityId: ENTITY, hiveUrl: HIVE }, undefined);
  expect(mocks.fetch).not.toHaveBeenCalled();
});
