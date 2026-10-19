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
 * All identities/profiles live only in memory until this process stops. Use
 * invented test data only. A normal message produces labeled synthetic SSE;
 * exactly `slow` produces a long stream for Stop checks; exactly `error` fails
 * both SSE and JSON fallback so the actual gateway reaches its error state.
 * This validates HTTP transport/UI behavior, not model or daemon integration.
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
  return typeof name === "string" && name.trim() && [...name].length <= 80 && !/[\u0000-\u001f\u007f]/u.test(name);
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
      "Access-Control-Allow-Headers": "Content-Type, Accept",
      "Access-Control-Max-Age": "600",
    });
    return response.end();
  }
  if (Number(request.headers["content-length"] || 0) > maxBodyBytes) {
    request.resume();
    return fail(response, 413, "Fixture request exceeds 32 KiB.");
  }

  let path = new URL(request.url, baseUrl).pathname;
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
  if (request.method === "GET" && path === "/v1/status") {
    return json(response, 200, entityId ? {
      entity_id: entityId, name: entities.get(entityId).name,
      fixture: true, provider: "synthetic-fixture", model: null,
    } : {
      entity_count: entities.size, entities: [...entities.values()],
      ready_state: "needs_provider", any_provider_configured: false,
      setup_complete: false, helper: { running: false, local_url: null },
      fixture: true, description: "Synthetic transport fixture. No model or provider connected.",
    });
  }
  if (entityId && request.method === "GET" && path === "/v1/session/status") {
    return json(response, 200, {
      fixture: true,
      lease: { entity_id: entityId, hive_url: baseUrl, runtime_id: `fixture-${entityId}`, lease_id: "synthetic-fixture" },
    });
  }
  if (!entityId && path === "/v1/entities") {
    if (request.method === "GET") return json(response, 200, [...entities.values()]);
    if (request.method === "POST") {
      const input = await body(request);
      if (!validName(input.name)) return fail(response, 400, "Name must contain 1-80 characters without control characters.");
      const id = randomUUID();
      entities.set(id, { id, name: input.name.trim(), birth_complete: false, birth_date: null, is_hive: false, immortal: false });
      profiles.set(id, { name: input.name.trim(), purpose: "", temperament: "gentle", boundaries: "", identity: "aether" });
      if (creationDelayMs) await new Promise(resolve => {
        const timer = setTimeout(() => { pendingCreations.delete(timer); resolve(); }, creationDelayMs);
        pendingCreations.add(timer);
      });
      if (response.destroyed || response.writableEnded) return;
      return json(response, 201, { id, directory: "synthetic:memory-only" });
    }
  }
  const entityMatch = !entityId && path.match(/^\/v1\/entities\/([^/]+)\/(presence|open)$/u);
  if (entityMatch) {
    const [, id, action] = entityMatch;
    if (!entities.has(id)) return fail(response, 404, "Unknown fixture Entity.");
    if (action === "open" && request.method === "POST") return json(response, 200, { local_url: `${baseUrl}/runtime/${id}` });
    if (action === "presence" && request.method === "GET") return json(response, 200, profiles.get(id));
    if (action === "presence" && request.method === "PUT") {
      const profile = await body(request);
      if (!validProfile(profile)) return fail(response, 400, "Invalid presence profile.");
      profiles.set(id, profile);
      return json(response, 200, profile);
    }
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
    const mode = input.message.trim().toLowerCase();
    if (mode === "error") {
      if (path === "/v1/chat") return fail(response, 503, fixtureError);
      response.writeHead(200, { "Content-Type": "text/event-stream; charset=utf-8" });
      return response.end(`event: error\ndata: ${fixtureError}\n\n`);
    }
    const sessionId = typeof input.session_id === "string" ? input.session_id : randomUUID();
    if (path === "/v1/chat/stream") return streamReply(response, entityId, sessionId, mode === "slow");
    return json(response, 200, {
      reply: "Synthetic transport fixture: JSON fallback received your test request. No model was called.",
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
  console.log("No model/provider/microphone/files. In-memory invented test data only. Stop with Ctrl+C.");
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
