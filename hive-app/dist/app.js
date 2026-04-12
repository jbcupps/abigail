const invoke =
  window.__TAURI__?.core?.invoke?.bind(window.__TAURI__.core) ??
  window.__TAURI_INTERNALS__?.invoke;

const state = {
  connection: null,
  status: null,
  selectedEntityId: null,
  latestLease: null,
  latestAssignments: null,
};

const elements = {
  hiveUrl: document.querySelector("#hive-url"),
  entityCount: document.querySelector("#entity-count"),
  hivePill: document.querySelector("#hive-pill"),
  entityListPill: document.querySelector("#entity-list-pill"),
  entityList: document.querySelector("#entity-list"),
  newEntityName: document.querySelector("#new-entity-name"),
  selectedPill: document.querySelector("#selected-pill"),
  selectedEntityMeta: document.querySelector("#selected-entity-meta"),
  leaseOutput: document.querySelector("#lease-output"),
  assignmentsOutput: document.querySelector("#assignments-output"),
};

function setPill(element, label, variant = "idle") {
  element.textContent = label;
  element.className = `pill pill-${variant}`;
}

function renderJson(element, value) {
  element.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

async function tauriInvoke(command, args = {}) {
  if (!invoke) {
    throw new Error("Tauri invoke API is unavailable in this window.");
  }

  return invoke(command, args);
}

function pickDefaultEntity(entities) {
  return entities.find((entity) => !entity.is_hive)?.id ?? entities[0]?.id ?? null;
}

function renderSummary() {
  elements.hiveUrl.textContent = state.connection?.hive_url ?? "Unavailable";
  elements.entityCount.textContent = String(state.status?.entity_count ?? 0);
  setPill(elements.hivePill, state.status ? "Connected" : "Unavailable", state.status ? "ok" : "warn");
}

function renderSelectedEntity() {
  const entity = state.status?.entities?.find((item) => item.id === state.selectedEntityId);
  if (!entity) {
    setPill(elements.selectedPill, "None", "idle");
    elements.selectedEntityMeta.innerHTML = "<div class='entity-meta'>Select an entity to issue a runtime session or inspect assignments.</div>";
    return;
  }

  setPill(elements.selectedPill, entity.name, "ok");
  elements.selectedEntityMeta.innerHTML = `
    <div class="entity-card active">
      <div class="entity-name">${entity.name}</div>
      <div class="entity-meta">
        <div><strong>ID:</strong> ${entity.id}</div>
        <div><strong>Birth Complete:</strong> ${entity.birth_complete ? "Yes" : "No"}</div>
        <div><strong>Role:</strong> ${entity.is_hive ? "Hive" : "Entity"}</div>
      </div>
    </div>
  `;
}

function renderEntities() {
  const entities = state.status?.entities ?? [];
  elements.entityList.innerHTML = "";

  if (!entities.length) {
    setPill(elements.entityListPill, "Empty", "warn");
    elements.entityList.innerHTML = "<div class='entity-meta'>No entities returned from Hive.</div>";
    return;
  }

  setPill(elements.entityListPill, `${entities.length} loaded`, "ok");

  for (const entity of entities) {
    const card = document.createElement("article");
    card.className = `entity-card${entity.id === state.selectedEntityId ? " active" : ""}`;
    card.innerHTML = `
      <div class="entity-name">${entity.name}</div>
      <div class="entity-meta">
        <div>${entity.id}</div>
        <div>${entity.is_hive ? "Hive coordinator" : "Family entity"} · ${entity.birth_complete ? "born" : "in setup"}</div>
      </div>
      <div class="entity-actions">
        <button type="button" class="secondary" data-action="select">Select</button>
        <button type="button" data-action="lease">Issue Lease</button>
      </div>
    `;

    card.querySelector("[data-action='select']").addEventListener("click", async () => {
      state.selectedEntityId = entity.id;
      renderEntities();
      renderSelectedEntity();
      await loadAssignments();
    });

    card.querySelector("[data-action='lease']").addEventListener("click", async () => {
      state.selectedEntityId = entity.id;
      renderEntities();
      renderSelectedEntity();
      await issueRuntimeSession();
    });

    elements.entityList.appendChild(card);
  }
}

async function refreshHive() {
  try {
    const [connection, status] = await Promise.all([
      tauriInvoke("get_hive_connection_info"),
      tauriInvoke("get_hive_status"),
    ]);

    state.connection = connection;
    state.status = status;
    if (!state.selectedEntityId || !status.entities.some((entity) => entity.id === state.selectedEntityId)) {
      state.selectedEntityId = pickDefaultEntity(status.entities);
    }

    renderSummary();
    renderEntities();
    renderSelectedEntity();
    if (state.selectedEntityId) {
      await loadAssignments();
    }
  } catch (error) {
    setPill(elements.hivePill, "Error", "warn");
    setPill(elements.entityListPill, "Error", "warn");
    renderJson(elements.assignmentsOutput, { error: String(error) });
    renderJson(elements.leaseOutput, { error: String(error) });
  }
}

async function loadAssignments() {
  if (!state.selectedEntityId) {
    renderJson(elements.assignmentsOutput, "Select an entity to inspect assignments.");
    return;
  }

  try {
    state.latestAssignments = await tauriInvoke("list_assignments", { entityId: state.selectedEntityId });
    renderJson(elements.assignmentsOutput, state.latestAssignments);
  } catch (error) {
    renderJson(elements.assignmentsOutput, { error: String(error) });
  }
}

async function issueRuntimeSession() {
  if (!state.selectedEntityId) {
    renderJson(elements.leaseOutput, "Select an entity before issuing a runtime session.");
    return;
  }

  try {
    state.latestLease = await tauriInvoke("issue_runtime_session", { entityId: state.selectedEntityId });
    renderJson(elements.leaseOutput, state.latestLease);
  } catch (error) {
    renderJson(elements.leaseOutput, { error: String(error) });
  }
}

async function createEntity() {
  const name = elements.newEntityName.value.trim();
  if (!name) {
    elements.newEntityName.focus();
    return;
  }

  try {
    const entityId = await tauriInvoke("create_entity", { name });
    elements.newEntityName.value = "";
    await refreshHive();
    state.selectedEntityId = entityId;
    renderEntities();
    renderSelectedEntity();
    await loadAssignments();
  } catch (error) {
    renderJson(elements.assignmentsOutput, { error: String(error) });
  }
}

document.querySelector("#refresh-hive").addEventListener("click", refreshHive);
document.querySelector("#create-entity").addEventListener("click", createEntity);
document.querySelector("#issue-runtime-session").addEventListener("click", issueRuntimeSession);
document.querySelector("#load-assignments").addEventListener("click", loadAssignments);
elements.newEntityName.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    void createEntity();
  }
});

void refreshHive();
