#!/usr/bin/env node
/**
 * SYNTHETIC DEVELOPMENT FIXTURE — no model, provider, microphone, or file access.
 *
 * Run: node scripts/presence-fixture.mjs [port] [creation-delay-ms]
 * Defaults: 1531 and 0. A delay holds the response after memory-only creation,
 * allowing cancellation/uncertain-result recovery to be inspected.
 * Hive UI: ?hiveUrl=http://127.0.0.1:1531
 * Runtime UI: ?entityUrl=http://127.0.0.1:1531/runtime/<created-id>
 *
 * All identities/profiles/creation keys/transcripts live only in memory until
 * this process stops. Use
 * invented test data only. A normal message produces labeled synthetic SSE;
 * exactly `slow` produces a long stream for Stop checks; exactly `error` fails
 * both SSE and JSON fallback so the actual gateway reaches its error state.
 * Keyed creation replay and chat history permit same-process recovery/reopen
 * checks. This validates HTTP transport/UI behavior, not durable daemon storage,
 * model integration, signed birth documents, or restart persistence.
 */
import { createServer } from "node:http";
import { randomUUID } from "node:crypto";

const port = Number(process.argv[2] ?? 1531);
const creationDelayMs = Number(process.argv[3] ?? 0);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error("Usage: node scripts/presence-fixture.mjs [port: 1-65535]");
  process.exit(1);
}
if (!Number.isInteger(creationDelayMs) || creationDelayMs < 0 || creationDelayMs > 60_000) {
  console.error("Creation delay must be an integer from 0 to 60000 ms.");
  process.exit(1);
}
const baseUrl = `http://127.0.0.1:${port}`;
const maxBodyBytes = 32 * 1024;
const entities = new Map();
const profiles = new Map();
const creations = new Map();
const genesisSessions = new Map();
const conversations = new Map();
const streams = new Set();
const pendingCreations = new Set();
const fixtureError = "Synthetic transport fixture: requested error. No model was called.";

function json(response, status, data, error) {
  response.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  response.end(JSON.stringify(error ? { ok: false, error } : { ok: true, data }));
}

function fail(response, status, message) {
  json(response, status, undefined, message);
}

async function body(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > maxBodyBytes) throw Object.assign(new Error("Fixture request exceeds 32 KiB."), { status: 413 });
    chunks.push(chunk);
  }
  try {
    const value = JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error();
    return value;
  } catch {
    throw Object.assign(new Error("Expected a JSON object."), { status: 400 });
  }
}

function validName(name) {
  return typeof name === "string" && name.trim() && name.trim() === name
    && [...name].length <= 80 && !/[\u0000-\u001f\u007f-\u009f]/u.test(name);
}

function validCreationKey(key) {
  return typeof key === "string"
    && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(key);
}

function appendTurn(entityId, sessionId, role, content) {
  let conversation = conversations.get(entityId);
  if (!conversation) {
    conversation = { latest: sessionId, sessions: new Map() };
    conversations.set(entityId, conversation);
  }
  conversation.latest = sessionId;
  const messages = conversation.sessions.get(sessionId) || [];
  messages.push({ role, content });
  conversation.sessions.set(sessionId, messages);
}

function validProfile(value) {
  const keys = ["name", "purpose", "temperament", "boundaries", "identity"];
  return Object.keys(value).every(key => keys.includes(key))
    && validName(value.name)
    && ["purpose", "boundaries"].every(key => typeof value[key] === "string"
      && [...value[key]].length <= 2000
      && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/u.test(value[key]))
    && ["gentle", "direct", "curious"].includes(value.temperament)
    && ["aether", "ember", "lumen"].includes(value.identity);
}

function streamReply(response, entityId, sessionId, slow) {
  const reply = slow
    ? "Synthetic transport fixture: this deliberately slow text stream allows you to test Stop. "
      + "This is simulated transport output, with no inference or audio. ".repeat(12)
      + "No model was called."
    : "Synthetic transport fixture: your text connection is working. This response tests the presence interface and streaming transport. No model was called.";
  const words = reply.match(/\S+\s*/gu) || [];
  const result = { reply, provider: "synthetic-fixture", model_used: "none", session_id: sessionId };
  response.writeHead(200, {
    "Content-Type": "text/event-stream; charset=utf-8",
    "Cache-Control": "no-cache, no-transform",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });
  response.flushHeaders();
  response.write(": Synthetic transport fixture. No model was called.\n\n");
  let index = 0;
  let timer;
  const stream = { entityId, sessionId, stop: () => { cleanup(); response.end(); } };
  const cleanup = () => { clearTimeout(timer); streams.delete(stream); };
  response.on("close", cleanup);
  response.on("error", cleanup);
  streams.add(stream);
  const next = () => {
    if (response.destroyed || response.writableEnded) return cleanup();
    if (index === words.length) {
      // Keep only the completed assistant turn. Stop/error never saves an
      // invented completion, while the submitted user turn remains available.
      appendTurn(entityId, sessionId, "assistant", reply);
      response.write(`event: done\ndata: ${JSON.stringify(result)}\n\n`);
      cleanup();
      response.end();
      return;
    }
    response.write(`event: token\ndata: ${words[index++]}\n\n`);
    timer = setTimeout(next, slow ? 500 : 60);
  };
  // A visible request-wait phase, not a claim about model reasoning.
  timer = setTimeout(next, 450);
}

async function route(request, response) {
  const origin = request.headers.origin;
  if (origin) {
    let allowed = false;
    try {
      const url = new URL(origin);
      allowed = ["http:", "https:"].includes(url.protocol)
        && ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
    } catch { /* Reject non-HTTP and opaque origins. */ }
    if (!allowed) return fail(response, 403, "Fixture accepts only loopback browser origins.");
    response.setHeader("Access-Control-Allow-Origin", origin);
    response.setHeader("Vary", "Origin");
  }
  response.setHeader("X-Abigail-Fixture", "synthetic-no-model");
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("X-Content-Type-Options", "nosniff");
  if (request.method === "OPTIONS") {
    response.writeHead(204, {
      "Access-Control-Allow-Methods": "GET, POST, PUT, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type, Accept, Authorization",
      "Access-Control-Max-Age": "600",
    });
    return response.end();
  }
  if (Number(request.headers["content-length"] || 0) > maxBodyBytes) {
    request.resume();
    return fail(response, 413, "Fixture request exceeds 32 KiB.");
  }

  const requestUrl = new URL(request.url, baseUrl);
  let path = requestUrl.pathname;
  const runtimeMatch = path.match(/^\/runtime\/([^/]+)(\/.*)$/u);
  const entityId = runtimeMatch?.[1];
  if (runtimeMatch) {
    if (!entities.has(entityId)) return fail(response, 404, "Unknown fixture Entity.");
    path = runtimeMatch[2];
  }
  if (request.method === "GET" && path === "/health") {
    response.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
    return response.end("ok — synthetic transport fixture; no model connected");
  }
  // Deliberately synthetic Genesis transport. The literal fixture marker is
  // public test data, never a generated credential or a real Hive capability.
  // Start the dev UI with VITE_GENESIS_TOKEN=synthetic-genesis-fixture.
  if (path.startsWith("/v1/genesis/")) {
    if (request.headers.authorization !== "Bearer synthetic-genesis-fixture") return fail(response, 401, "Use the public synthetic Genesis fixture marker for this fixture.");
    if (request.method === "GET" && path === "/v1/genesis/media/capabilities") return json(response, 200, { portrait_available: false, video_available: false, portrait_model: "none", video_model: "none", portrait_reservation_usd: 0, video_reservation_usd: 0, daily_budget_usd: 0, daily_job_limit: 0, video_seconds: [], notice: "Synthetic fixture: no media providers or generation." });
    if (request.method === "GET" && path === "/v1/genesis/media/activity") return json(response, 200, []);
    if (path === "/v1/genesis/sessions") {
      if (request.method === "GET") return json(response, 200, [...genesisSessions.values()]);
      if (request.method !== "POST") return fail(response, 405, "Unsupported fixture operation.");
      const input = await body(request), seed = input.seed;
      if (!validCreationKey(input.creation_key) || !validName(seed?.name) || !seed?.purpose?.trim() || seed.mode !== "quick") return fail(response, 400, "Fixture requires the reviewed First Signal name, purpose and creation key.");
      const old = genesisSessions.get(input.creation_key);
      if (old) return JSON.stringify(old.creation_seed) === JSON.stringify(seed) ? json(response, 200, old) : fail(response, 409, "Original input differs. Resume the same draft.");
      const time = new Date().toISOString();
      const value = { id: input.creation_key, owner_id: "synthetic-household", mode: "quick", state: "draft", revision: 0, created_at: time, updated_at: time, composer_draft: "", creation_seed: seed,
        messages: [{ id: "synthetic-guide", role: "assistant", content: "Synthetic fixture: your chosen name and purpose are saved in memory. No model was called.", created_at: time }], proposals: [], conflicts: [], sources: [], revisions: [], asset_ids: [], job_ids: [], choices: [],
        definition: { schema_version: 1, owner_id: "synthetic-household", name: seed.name, purpose: seed.purpose, status: "draft", identity: "", personality: { description: "", fears: [], aspirations: [], communication_style: "", response_patterns: [] }, values: [], boundaries: [], appearance: "", visual_style: "", voice_profile: "", lore: [], relationships: [], fictional_memories: [], capabilities: [], permissions: {}, runtime_configuration: {}, created_at: time, updated_at: time, revision: 0 } };
      genesisSessions.set(value.id, value);
      if (creationDelayMs) await new Promise(resolve => setTimeout(resolve, creationDelayMs));
      return json(response, 200, value);
    }
    const match = path.match(/^\/v1\/genesis\/sessions\/([^/]+)(?:\/(actions|media))?$/u);
    const value = match && genesisSessions.get(match[1]);
    if (!value) return fail(response, 404, "Unknown synthetic creation.");
    if (!match[2] && request.method === "GET") return json(response, 200, value);
    if (match[2] === "media" && request.method === "GET") return json(response, 200, { assets: [], jobs: [] });
    if (match[2] !== "actions" || request.method !== "POST") return fail(response, 400, "This fixture does not call models, generate media, or interpret documents.");
    const input = await body(request);
    if (input.expected_revision !== value.revision) return fail(response, 409, "This synthetic draft changed. Reload it.");
    if (input.action === "activate") {
      if (!value.definition.entity_id) {
        const id = randomUUID(); value.definition.entity_id = id;
        entities.set(id, { id, name: value.definition.name, birth_complete: true, is_hive: false, immortal: false });
        profiles.set(id, { name: value.definition.name, purpose: "", temperament: "gentle", boundaries: "", identity: "aether" });
        appendTurn(id, "genesis-awakening", "assistant", `Synthetic awakening for ${value.definition.name}. No signed birth or model inference occurred.`);
      }
      value.state = "ready"; value.definition.status = "ready";
    } else if (input.action === "save_draft") value.composer_draft = input.text;
    else if (input.action === "review") value.state = "review";
    else if (input.action === "update" && Array.isArray(input.updates) && input.updates.every(update => ["name", "purpose"].includes(update.field) && typeof update.value === "string")) {
      for (const update of input.updates) value.definition[update.field] = update.value;
    } else return fail(response, 400, "This bounded synthetic fixture supports awakening, name/purpose review and draft save only.");
    value.revision++; value.updated_at = new Date().toISOString();
    return json(response, 200, value);
  }
  if (request.method === "GET" && path === "/v1/status") {
    return json(response, 200, entityId ? {
      entity_id: entityId, name: entities.get(entityId).name,
      fixture: true, provider: "synthetic-fixture", model: null,
    } : {
      entity_count: entities.size, entities: [...entities.values()],
      // "ready" means this explicit synthetic transport can accept test text.
      // It must not be interpreted as proof of any provider connection.
      ready_state: "ready", any_provider_configured: false,
      setup_complete: true, helper: { running: false, local_url: null },
      fixture: true, description: "Synthetic transport fixture. No model or provider connected.",
    });
  }
  if (entityId && request.method === "GET" && path === "/v1/session/status") {
    return json(response, 200, {
      fixture: true,
      lease: { entity_id: entityId, hive_url: baseUrl, runtime_id: `fixture-${entityId}`, lease_id: "synthetic-fixture" },
    });
  }
  const creationMatch = !entityId && path.match(/^\/v1\/entity-creations\/([^/]+)$/u);
  if (creationMatch && request.method === "GET") {
    const key = creationMatch[1];
    if (!validCreationKey(key)) return fail(response, 400, "creation_key must be a canonical UUID.");
    const creation = creations.get(key);
    return json(response, 200, creation
      ? { creation_key: key, state: "complete", ...creation }
      : { creation_key: key, state: "absent" });
  }
  if (!entityId && path === "/v1/entities") {
    if (request.method === "GET") return json(response, 200, [...entities.values()]);
    if (request.method === "POST") {
      const input = await body(request);
      if (!validName(input.name)) return fail(response, 400, "Name must be trimmed and contain 1-80 characters without control characters.");
      const keyed = Object.hasOwn(input, "creation_key");
      if (keyed && !validCreationKey(input.creation_key)) return fail(response, 400, "creation_key must be a canonical UUID.");
      const previous = keyed && creations.get(input.creation_key);
      if (previous) {
        if (previous.name !== input.name) return fail(response, 409, "This creation key belongs to a different Entity name.");
        return json(response, 201, { id: previous.id, directory: previous.directory, creation_key: input.creation_key });
      }
      const id = randomUUID();
      const directory = `synthetic:memory-only/${id}`;
      entities.set(id, { id, name: input.name, birth_complete: false, birth_date: null, is_hive: false, immortal: false });
      profiles.set(id, { name: input.name, purpose: "", temperament: "gentle", boundaries: "", identity: "aether" });
      if (keyed) creations.set(input.creation_key, { name: input.name, id, directory });
      if (creationDelayMs) await new Promise(resolve => {
        const timer = setTimeout(() => { pendingCreations.delete(timer); resolve(); }, creationDelayMs);
        pendingCreations.add(timer);
      });
      if (response.destroyed || response.writableEnded) return;
      return json(response, 201, { id, directory, ...(keyed ? { creation_key: input.creation_key } : {}) });
    }
  }
  const entityMatch = !entityId && path.match(/^\/v1\/entities\/([^/]+)(?:\/(presence|open|birth))?$/u);
  if (entityMatch) {
    const [, id, action] = entityMatch;
    if (!entities.has(id)) return fail(response, 404, "Unknown fixture Entity.");
    if (!action && request.method === "GET") return json(response, 200, entities.get(id));
    if (action === "birth" && request.method === "POST") {
      const input = await body(request);
      if (input.path !== "quickstart" || !Array.isArray(input.choices) || input.choices.length
        || typeof input.purpose !== "string") {
        return fail(response, 400, "Fixture supports only quickstart with empty choices and a text purpose.");
      }
      const entity = entities.get(id);
      entity.birth_complete = true;
      entity.birth_date ??= new Date().toISOString();
      return json(response, 200, {
        fixture: true, birth_complete: true,
        description: "Synthetic setup state only. No signed birth documents were created.",
      });
    }
    if (action === "open" && request.method === "POST") {
      if (!entities.get(id).birth_complete) return fail(response, 409, "Complete synthetic quickstart setup before opening.");
      return json(response, 200, { local_url: `${baseUrl}/runtime/${id}` });
    }
    if (action === "presence" && request.method === "GET") return json(response, 200, profiles.get(id));
    if (action === "presence" && request.method === "PUT") {
      const profile = await body(request);
      if (!validProfile(profile)) return fail(response, 400, "Invalid presence profile.");
      profiles.set(id, profile);
      return json(response, 200, profile);
    }
  }
  if (entityId && request.method === "GET" && path === "/v1/chat/history") {
    const conversation = conversations.get(entityId);
    const sessionId = requestUrl.searchParams.get("session_id")?.trim() || conversation?.latest || null;
    const requestedLimit = Number(requestUrl.searchParams.get("limit") ?? 200);
    if (!Number.isInteger(requestedLimit) || requestedLimit < 0) return fail(response, 400, "History limit must be a nonnegative integer.");
    const limit = Math.min(500, Math.max(1, requestedLimit));
    const messages = conversation?.sessions.get(sessionId) || [];
    return json(response, 200, { session_id: sessionId, messages: messages.slice(-limit) });
  }
  if (entityId && request.method === "POST" && path === "/v1/chat/cancel") {
    const input = await body(request);
    let stopped = 0;
    for (const stream of streams) {
      if (stream.entityId === entityId && stream.sessionId === input.session_id) {
        stream.stop();
        stopped += 1;
      }
    }
    return json(response, 200, { stopped, fixture: true });
  }
  if (entityId && request.method === "POST" && ["/v1/chat", "/v1/chat/stream"].includes(path)) {
    const input = await body(request);
    if (typeof input.message !== "string" || !input.message.trim()) return fail(response, 400, "A test message is required.");
    if (input.session_id !== undefined && (typeof input.session_id !== "string" || !input.session_id.trim())) {
      return fail(response, 400, "A supplied session_id must be nonempty text.");
    }
    const sessionId = input.session_id ?? randomUUID();
    appendTurn(entityId, sessionId, "user", input.message);
    const mode = input.message.trim().toLowerCase();
    if (mode === "error") {
      if (path === "/v1/chat") return fail(response, 503, fixtureError);
      response.writeHead(200, { "Content-Type": "text/event-stream; charset=utf-8" });
      return response.end(`event: error\ndata: ${fixtureError}\n\n`);
    }
    if (path === "/v1/chat/stream") return streamReply(response, entityId, sessionId, mode === "slow");
    const reply = "Synthetic transport fixture: JSON fallback received your test request. No model was called.";
    appendTurn(entityId, sessionId, "assistant", reply);
    return json(response, 200, {
      reply,
      provider: "synthetic-fixture", model_used: "none", session_id: sessionId,
    });
  }
  return fail(response, 404, "Unsupported synthetic fixture route. No action was performed.");
}

const server = createServer((request, response) => {
  void route(request, response).catch(error => {
    if (response.headersSent || response.destroyed) return response.destroy();
    fail(response, Number.isInteger(error.status) ? error.status : 500,
      error.status ? error.message : "Synthetic fixture request failed.");
  });
});
server.requestTimeout = 15_000;
server.headersTimeout = 10_000;
server.on("error", error => {
  console.error(`Synthetic fixture could not listen: ${error.code || "server error"}`);
  process.exitCode = 1;
});
server.listen(port, "127.0.0.1", () => {
  console.log(`SYNTHETIC DEVELOPMENT FIXTURE: ${baseUrl}`);
  console.log("No model/provider/microphone/files. Identities, keys, and transcripts are memory-only. Stop with Ctrl+C.");
  if (creationDelayMs) console.log(`Creation responses delayed ${creationDelayMs} ms after memory-only registration.`);
});
const shutdown = () => {
  for (const timer of pendingCreations) clearTimeout(timer);
  pendingCreations.clear();
  for (const stream of streams) stream.stop();
  server.close();
  server.closeAllConnections();
};
process.once("SIGINT", shutdown);
process.once("SIGTERM", shutdown);
