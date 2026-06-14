const storageKey = "abigail-split-stack-harness";

const elements = {
  hiveUrl: document.querySelector("#hive-url"),
  runtimeUrl: document.querySelector("#runtime-url"),
  entityId: document.querySelector("#entity-id"),
  sessionId: document.querySelector("#session-id"),
  hiveStatus: document.querySelector("#hive-status"),
  runtimeStatus: document.querySelector("#runtime-status"),
  sessionStatus: document.querySelector("#session-status"),
  outboxStatus: document.querySelector("#outbox-status"),
  skillAcks: document.querySelector("#skill-acks"),
  chatResponse: document.querySelector("#chat-response"),
  entityList: document.querySelector("#entity-list"),
  chatMessage: document.querySelector("#chat-message"),
  hiveHealthPill: document.querySelector("#hive-health-pill"),
  runtimeHealthPill: document.querySelector("#runtime-health-pill"),
  chatPill: document.querySelector("#chat-pill"),
  ackPill: document.querySelector("#ack-pill"),
};

function setJson(target, value) {
  target.textContent = JSON.stringify(value, null, 2);
}

function setPill(target, label, variant = "muted") {
  target.textContent = label;
  target.className = `pill pill-${variant}`;
}

function sessionSnapshot() {
  return {
    hiveUrl: elements.hiveUrl.value.trim(),
    runtimeUrl: elements.runtimeUrl.value.trim(),
    entityId: elements.entityId.value.trim(),
    sessionId: elements.sessionId.value.trim(),
  };
}

function persist() {
  localStorage.setItem(storageKey, JSON.stringify(sessionSnapshot()));
}

function loadSaved() {
  try {
    return JSON.parse(localStorage.getItem(storageKey) || "{}");
  } catch {
    return {};
  }
}

async function fetchJson(url, options = undefined) {
  const response = await fetch(url, options);
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch {
    return { raw: text, status: response.status };
  }
}

async function probeHealth(baseUrl) {
  const response = await fetch(`${baseUrl}/health`);
  return response.ok;
}

async function refreshHive() {
  const hiveUrl = elements.hiveUrl.value.trim();
  setPill(elements.hiveHealthPill, "Checking", "warn");
  try {
    const healthy = await probeHealth(hiveUrl);
    setPill(elements.hiveHealthPill, healthy ? "Healthy" : "Unreachable", healthy ? "ok" : "warn");

    const status = await fetchJson(`${hiveUrl}/v1/status`);
    const entities = await fetchJson(`${hiveUrl}/v1/entities`);
    setJson(elements.hiveStatus, { status, entities });
    renderEntities(entities?.data || []);
  } catch (error) {
    renderEntities([]);
    setPill(elements.hiveHealthPill, "Error", "warn");
    setJson(elements.hiveStatus, { error: String(error) });
  }
}

function renderEntities(entities) {
  elements.entityList.innerHTML = "";
  for (const entity of entities) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = `${entity.name} (${entity.id})`;
    button.addEventListener("click", () => {
      elements.entityId.value = entity.id;
      persist();
    });
    item.appendChild(button);
    elements.entityList.appendChild(item);
  }
}

async function refreshRuntime() {
  const runtimeUrl = elements.runtimeUrl.value.trim();
  setPill(elements.runtimeHealthPill, "Checking", "warn");
  try {
    const healthy = await probeHealth(runtimeUrl);
    setPill(elements.runtimeHealthPill, healthy ? "Healthy" : "Unreachable", healthy ? "ok" : "warn");

    const status = await fetchJson(`${runtimeUrl}/v1/status`);
    const session = await fetchJson(`${runtimeUrl}/v1/session/status`);
    const outbox = await fetchJson(`${runtimeUrl}/v1/outbox/status`);
    const acknowledgements = await fetchJson(`${runtimeUrl}/v1/skills/acks`);

    setJson(elements.runtimeStatus, status);
    setJson(elements.sessionStatus, session);
    setJson(elements.outboxStatus, outbox);
    setJson(elements.skillAcks, acknowledgements);
    setPill(elements.ackPill, acknowledgements?.ok ? "Loaded" : "Unavailable", acknowledgements?.ok ? "ok" : "warn");

    const inferredEntityId = session?.data?.lease?.entity_id;
    if (inferredEntityId && !elements.entityId.value.trim()) {
      elements.entityId.value = inferredEntityId;
    }
  } catch (error) {
    setPill(elements.runtimeHealthPill, "Error", "warn");
    setPill(elements.ackPill, "Error", "warn");
    setJson(elements.runtimeStatus, { error: String(error) });
    setJson(elements.sessionStatus, { error: String(error) });
    setJson(elements.outboxStatus, { error: String(error) });
    setJson(elements.skillAcks, { error: String(error) });
  }
}

async function sendChat() {
  const runtimeUrl = elements.runtimeUrl.value.trim();
  const message = elements.chatMessage.value.trim();
  if (!message) {
    setPill(elements.chatPill, "Message required", "warn");
    return;
  }

  const sessionId = elements.sessionId.value.trim() || `browser-harness-${Date.now()}`;
  elements.sessionId.value = sessionId;
  persist();
  setPill(elements.chatPill, "Sending", "warn");

  try {
    const response = await fetchJson(`${runtimeUrl}/v1/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        message,
        session_id: sessionId,
      }),
    });

    setJson(elements.chatResponse, response);
    setPill(elements.chatPill, response?.ok ? "Delivered" : "Failed", response?.ok ? "ok" : "warn");
  } catch (error) {
    setJson(elements.chatResponse, { error: String(error) });
    setPill(elements.chatPill, "Error", "warn");
  }
}

async function refreshAll() {
  persist();
  await Promise.allSettled([refreshHive(), refreshRuntime()]);
}

async function bootstrap() {
  const config = await fetchJson("./config.json");
  const saved = loadSaved();

  elements.hiveUrl.value = saved.hiveUrl || config.hiveUrl || "http://127.0.0.1:43141";
  elements.runtimeUrl.value = saved.runtimeUrl || config.runtimeUrl || "http://127.0.0.1:43142";
  elements.entityId.value = saved.entityId || "";
  elements.sessionId.value = saved.sessionId || `browser-harness-${Date.now()}`;

  document.querySelector("#refresh-all").addEventListener("click", refreshAll);
  document.querySelector("#load-entities").addEventListener("click", refreshHive);
  document.querySelector("#load-runtime").addEventListener("click", refreshRuntime);
  document.querySelector("#send-chat").addEventListener("click", sendChat);

  for (const input of [elements.hiveUrl, elements.runtimeUrl, elements.entityId, elements.sessionId]) {
    input.addEventListener("change", persist);
  }

  await refreshAll();
}

bootstrap().catch((error) => {
  setJson(elements.hiveStatus, { error: String(error) });
  setPill(elements.hiveHealthPill, "Error", "warn");
});
