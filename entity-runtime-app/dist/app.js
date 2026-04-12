const invoke =
  window.__TAURI__?.core?.invoke?.bind(window.__TAURI__.core) ??
  window.__TAURI_INTERNALS__?.invoke;

const state = {
  connection: null,
  runtimeStatus: null,
  sessionStatus: null,
  outboxStatus: null,
  acknowledgements: null,
  transcript: [],
};

const elements = {
  runtimeUrl: document.querySelector("#runtime-url"),
  entityName: document.querySelector("#entity-name"),
  runtimePill: document.querySelector("#runtime-pill"),
  outboxCount: document.querySelector("#outbox-count"),
  sessionId: document.querySelector("#session-id"),
  chatMessage: document.querySelector("#chat-message"),
  chatPill: document.querySelector("#chat-pill"),
  sessionStatus: document.querySelector("#session-status"),
  runtimeStatus: document.querySelector("#runtime-status"),
  acksStatus: document.querySelector("#acks-status"),
  ackPill: document.querySelector("#ack-pill"),
  transcript: document.querySelector("#transcript"),
};

function setPill(element, label, variant = "idle") {
  element.textContent = label;
  element.className = `pill pill-${variant}`;
}

function renderJson(element, value) {
  element.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

function ensureSessionId() {
  if (!elements.sessionId.value.trim()) {
    elements.sessionId.value = `runtime-${Date.now()}`;
  }
}

async function tauriInvoke(command, args = {}) {
  if (!invoke) {
    throw new Error("Tauri invoke API is unavailable in this window.");
  }

  return invoke(command, args);
}

function renderTranscript() {
  if (!state.transcript.length) {
    elements.transcript.innerHTML = `
      <div class="bubble assistant">
        <span class="bubble-role">Assistant</span>
        Runtime transcript will appear here as you chat.
      </div>
    `;
    return;
  }

  elements.transcript.innerHTML = "";
  for (const message of state.transcript) {
    const item = document.createElement("div");
    item.className = `bubble ${message.role}`;
    item.innerHTML = `
      <span class="bubble-role">${message.role}</span>
      <div>${message.text}</div>
    `;
    elements.transcript.appendChild(item);
  }
}

function renderSummary() {
  elements.runtimeUrl.textContent = state.connection?.runtime_url ?? "Unavailable";
  elements.entityName.textContent =
    state.sessionStatus?.lease?.entity_name ??
    state.runtimeStatus?.name ??
    state.runtimeStatus?.entity_id ??
    "Unknown";
  elements.outboxCount.textContent = String(state.outboxStatus?.queued_records ?? 0);
  setPill(
    elements.runtimePill,
    state.sessionStatus?.connected_to_hive ? "Connected" : "Degraded",
    state.sessionStatus?.connected_to_hive ? "ok" : "warn",
  );
  setPill(
    elements.ackPill,
    state.acknowledgements?.acknowledgements?.length
      ? `${state.acknowledgements.acknowledgements.length} ack(s)`
      : "No acks",
    state.acknowledgements?.acknowledgements?.length ? "ok" : "idle",
  );
}

async function refreshRuntime() {
  try {
    ensureSessionId();
    const [connection, runtimeStatus, sessionStatus, outboxStatus, acknowledgements] =
      await Promise.all([
        tauriInvoke("get_runtime_connection_info"),
        tauriInvoke("get_runtime_status"),
        tauriInvoke("get_session_status"),
        tauriInvoke("get_outbox_status"),
        tauriInvoke("list_skill_acks"),
      ]);

    state.connection = connection;
    state.runtimeStatus = runtimeStatus;
    state.sessionStatus = sessionStatus;
    state.outboxStatus = outboxStatus;
    state.acknowledgements = acknowledgements;

    renderSummary();
    renderJson(elements.sessionStatus, sessionStatus);
    renderJson(elements.runtimeStatus, runtimeStatus);
    renderJson(elements.acksStatus, acknowledgements);
  } catch (error) {
    setPill(elements.runtimePill, "Error", "warn");
    setPill(elements.ackPill, "Error", "warn");
    renderJson(elements.sessionStatus, { error: String(error) });
    renderJson(elements.runtimeStatus, { error: String(error) });
    renderJson(elements.acksStatus, { error: String(error) });
  }
}

async function sendChat() {
  const message = elements.chatMessage.value.trim();
  if (!message) {
    elements.chatMessage.focus();
    return;
  }

  ensureSessionId();
  setPill(elements.chatPill, "Sending", "warn");
  state.transcript.push({ role: "user", text: message });
  renderTranscript();

  try {
    const response = await tauriInvoke("send_chat", {
      message,
      sessionId: elements.sessionId.value.trim(),
    });
    state.transcript.push({ role: "assistant", text: response.reply ?? JSON.stringify(response, null, 2) });
    elements.chatMessage.value = "";
    setPill(elements.chatPill, "Delivered", "ok");
    await refreshRuntime();
  } catch (error) {
    state.transcript.push({ role: "assistant", text: `Error: ${String(error)}` });
    setPill(elements.chatPill, "Failed", "warn");
  }

  renderTranscript();
}

document.querySelector("#refresh-runtime").addEventListener("click", refreshRuntime);
document.querySelector("#send-chat").addEventListener("click", sendChat);
elements.chatMessage.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
    event.preventDefault();
    void sendChat();
  }
});

ensureSessionId();
renderTranscript();
void refreshRuntime();
