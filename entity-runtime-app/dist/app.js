const invoke =
  window.__TAURI__?.core?.invoke?.bind(window.__TAURI__.core) ??
  window.__TAURI_INTERNALS__?.invoke;
const listen = window.__TAURI__?.event?.listen;

const state = {
  connection: null,
  runtimeStatus: null,
  sessionStatus: null,
  outboxStatus: null,
  acknowledgements: null,
  transcript: [],
  activeAssistantIndex: null,
  streaming: false,
  topicEvents: [],
  topicEventSource: null,
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
  busTopic: document.querySelector("#bus-topic"),
  busPill: document.querySelector("#bus-pill"),
  topicResults: document.querySelector("#topic-results"),
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
  if (!elements.busTopic.value.trim()) {
    elements.busTopic.value = `chat-${elements.sessionId.value.trim()}`;
  }
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function sanitizeUrl(rawUrl, allowDataImage = false) {
  if (!rawUrl) {
    return null;
  }
  const trimmed = rawUrl.trim();
  if (allowDataImage && /^data:image\/(png|jpeg|jpg|webp|gif);base64,[a-z0-9+/=]+$/i.test(trimmed)) {
    if (trimmed.length <= 2_000_000) {
      return trimmed;
    }
    return null;
  }
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed;
  }
  return null;
}

function markdownToHtml(markdown) {
  const source = markdown || "";
  const codeBlocks = [];
  let html = escapeHtml(source).replace(/```([\s\S]*?)```/g, (_, code) => {
    const token = `@@CODE_BLOCK_${codeBlocks.length}@@`;
    codeBlocks.push(`<pre><code>${code.trimEnd()}</code></pre>`);
    return token;
  });

  html = html.replace(/`([^`]+)`/g, "<code>$1</code>");
  html = html.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, url) => {
    const safeUrl = sanitizeUrl(url, true);
    if (!safeUrl) {
      return `<em>Image omitted (unsupported or unsafe URL): ${escapeHtml(url)}</em>`;
    }
    return `<img src="${safeUrl}" alt="${alt}" loading="lazy" />`;
  });
  html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, url) => {
    const safeUrl = sanitizeUrl(url, false);
    if (!safeUrl) {
      return escapeHtml(text);
    }
    return `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${text}</a>`;
  });

  const paragraphs = html
    .split(/\n{2,}/)
    .map((chunk) => chunk.trim())
    .filter((chunk) => chunk.length > 0)
    .map((chunk) => {
      const withBreaks = chunk.replace(/\n/g, "<br />");
      return `<p>${withBreaks}</p>`;
    });

  html = paragraphs.join("");
  codeBlocks.forEach((block, index) => {
    html = html.replaceAll(`@@CODE_BLOCK_${index}@@`, block);
  });
  return html || "<p></p>";
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
      <div class="bubble-body">${markdownToHtml(message.text)}</div>
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
    await refreshTopicResults();
  } catch (error) {
    setPill(elements.runtimePill, "Error", "warn");
    setPill(elements.ackPill, "Error", "warn");
    renderJson(elements.sessionStatus, { error: String(error) });
    renderJson(elements.runtimeStatus, { error: String(error) });
    renderJson(elements.acksStatus, { error: String(error) });
    renderJson(elements.topicResults, { error: String(error) });
  }
}

function currentTopic() {
  const topic = elements.busTopic.value.trim();
  if (topic) {
    return topic;
  }
  ensureSessionId();
  return `chat-${elements.sessionId.value.trim()}`;
}

async function refreshTopicResults() {
  try {
    const topic = currentTopic();
    const runtimeUrl = state.connection?.runtime_url ?? (await tauriInvoke("get_runtime_connection_info")).runtime_url;
    const response = await fetch(
      `${runtimeUrl}/v1/topics/${encodeURIComponent(topic)}/results?limit=20`,
      { method: "GET" },
    );
    const json = await response.json();
    renderJson(elements.topicResults, {
      latest_events: state.topicEvents.slice(-8),
      topic_results: json,
    });
    setPill(elements.busPill, "Ready", "ok");
  } catch (error) {
    setPill(elements.busPill, "Topic Error", "warn");
    renderJson(elements.topicResults, { error: String(error), latest_events: state.topicEvents.slice(-8) });
  }
}

function stopTopicWatch() {
  if (state.topicEventSource) {
    state.topicEventSource.close();
    state.topicEventSource = null;
    setPill(elements.busPill, "Watch Stopped", "idle");
  }
}

async function watchTopic() {
  stopTopicWatch();
  const topic = currentTopic();
  const runtimeUrl = state.connection?.runtime_url ?? (await tauriInvoke("get_runtime_connection_info")).runtime_url;
  const url = `${runtimeUrl}/v1/topics/${encodeURIComponent(topic)}/watch`;
  const stream = new EventSource(url);
  state.topicEventSource = stream;
  setPill(elements.busPill, "Watching", "warn");

  stream.addEventListener("job_event", (event) => {
    try {
      const payload = JSON.parse(event.data);
      state.topicEvents.push(payload);
      if (state.topicEvents.length > 50) {
        state.topicEvents = state.topicEvents.slice(-50);
      }
      renderJson(elements.topicResults, {
        latest_events: state.topicEvents.slice(-8),
      });
      setPill(elements.busPill, "Live", "ok");
    } catch (error) {
      renderJson(elements.topicResults, { error: String(error) });
    }
  });
  stream.addEventListener("error", () => {
    setPill(elements.busPill, "Watch Error", "warn");
  });
}

async function sendChat() {
  const message = elements.chatMessage.value.trim();
  if (!message) {
    elements.chatMessage.focus();
    return;
  }

  ensureSessionId();
  setPill(elements.chatPill, "Streaming", "warn");
  state.transcript.push({ role: "user", text: message });
  state.transcript.push({ role: "assistant", text: "" });
  state.activeAssistantIndex = state.transcript.length - 1;
  state.streaming = true;
  elements.chatMessage.value = "";
  renderTranscript();

  try {
    await tauriInvoke("send_chat_stream", {
      message,
      sessionId: elements.sessionId.value.trim(),
    });
    elements.busTopic.value = `chat-${elements.sessionId.value.trim()}`;
  } catch (error) {
    if (state.activeAssistantIndex != null) {
      state.transcript[state.activeAssistantIndex].text = `Error: ${String(error)}`;
    }
    state.streaming = false;
    state.activeAssistantIndex = null;
    setPill(elements.chatPill, "Failed", "warn");
    renderTranscript();
  }
}

async function cancelChat() {
  try {
    const cancelled = await tauriInvoke("cancel_chat_stream");
    if (cancelled) {
      setPill(elements.chatPill, "Cancelled", "warn");
    }
  } catch (error) {
    setPill(elements.chatPill, "Cancel Failed", "warn");
    state.transcript.push({ role: "assistant", text: `Error: ${String(error)}` });
    renderTranscript();
  }
}

async function bindRuntimeStreamEvents() {
  if (!listen) {
    return;
  }
  await listen("runtime-chat-envelope", async (event) => {
    const payload = event.payload || {};
    if (payload.type === "Token") {
      if (state.activeAssistantIndex == null) {
        state.transcript.push({ role: "assistant", text: "" });
        state.activeAssistantIndex = state.transcript.length - 1;
      }
      state.transcript[state.activeAssistantIndex].text += payload.token || "";
      renderTranscript();
      return;
    }
    if (payload.type === "Done") {
      if (state.activeAssistantIndex == null) {
        state.transcript.push({ role: "assistant", text: payload.reply || "" });
      } else if (payload.reply && !state.transcript[state.activeAssistantIndex].text) {
        state.transcript[state.activeAssistantIndex].text = payload.reply;
      }
      state.streaming = false;
      state.activeAssistantIndex = null;
      setPill(elements.chatPill, "Delivered", "ok");
      renderTranscript();
      await refreshRuntime();
      await refreshTopicResults();
      return;
    }
    if (payload.type === "Error") {
      if (state.activeAssistantIndex == null) {
        state.transcript.push({ role: "assistant", text: `Error: ${payload.error || "Unknown error"}` });
      } else {
        state.transcript[state.activeAssistantIndex].text = `Error: ${payload.error || "Unknown error"}`;
      }
      state.streaming = false;
      state.activeAssistantIndex = null;
      setPill(elements.chatPill, "Failed", "warn");
      renderTranscript();
    }
  });
}

document.querySelector("#refresh-runtime").addEventListener("click", refreshRuntime);
document.querySelector("#send-chat").addEventListener("click", sendChat);
document.querySelector("#cancel-chat").addEventListener("click", cancelChat);
document.querySelector("#refresh-topic").addEventListener("click", refreshTopicResults);
document.querySelector("#watch-topic").addEventListener("click", watchTopic);
document.querySelector("#stop-topic-watch").addEventListener("click", stopTopicWatch);
elements.chatMessage.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
    event.preventDefault();
    void sendChat();
  }
});

ensureSessionId();
renderTranscript();
void bindRuntimeStreamEvents();
void refreshRuntime();
