const invoke =
  window.__TAURI__?.core?.invoke?.bind(window.__TAURI__.core) ??
  window.__TAURI_INTERNALS__?.invoke;

// Escape dynamic values before they are interpolated into innerHTML. Entity
// names and birth choices originate from operator input and the Hive backend,
// so they must never be reinterpreted as markup. Escapes both quote styles so
// the helper is safe in element-text and attribute-value contexts alike.
const HTML_ESCAPES = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};
function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (ch) => HTML_ESCAPES[ch]);
}

const state = {
  connection: null,
  status: null,
  selectedEntityId: null,
  latestLease: null,
  latestAssignments: null,
  latestProviderConfig: null,
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
  providerPill: document.querySelector("#provider-pill"),
  providerName: document.querySelector("#provider-name"),
  providerKey: document.querySelector("#provider-key"),
  routingMode: document.querySelector("#routing-mode"),
  localLlmUrl: document.querySelector("#local-llm-url"),
  modelId: document.querySelector("#model-id"),
  cliPermissionMode: document.querySelector("#cli-permission-mode"),
  providerConfigOutput: document.querySelector("#provider-config-output"),
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

function populateModelOptions(models, selectedValue = "") {
  const existing = [
    "<option value=''>Use provider default</option>",
    ...models.map((model) => {
      const selected = model.model_id === selectedValue ? " selected" : "";
      const display = model.display_name ? `${model.display_name} (${model.model_id})` : model.model_id;
      return `<option value="${escapeHtml(model.model_id)}"${selected}>${escapeHtml(display)}</option>`;
    }),
  ];
  elements.modelId.innerHTML = existing.join("");
}

function hydrateProviderForm(config) {
  if (!config) {
    setPill(elements.providerPill, "Select Entity", "idle");
    return;
  }
  if (config.ego_provider_name) {
    elements.providerName.value = config.ego_provider_name;
  }
  if (config.local_llm_base_url) {
    elements.localLlmUrl.value = config.local_llm_base_url;
  } else {
    elements.localLlmUrl.value = "";
  }
  if (config.routing_mode) {
    elements.routingMode.value = config.routing_mode;
  }
  setPill(elements.providerPill, "Loaded", "ok");
}

function renderSummary() {
  elements.hiveUrl.textContent = state.connection?.hive_url ?? "Unavailable";
  elements.entityCount.textContent = String(state.status?.entity_count ?? 0);
  setPill(
    elements.hivePill,
    state.status ? "Connected" : "Unavailable",
    state.status ? "ok" : "warn",
  );
}

function renderSelectedEntity() {
  const entity = state.status?.entities?.find((item) => item.id === state.selectedEntityId);
  if (!entity) {
    setPill(elements.selectedPill, "None", "idle");
    elements.selectedEntityMeta.innerHTML =
      "<div class='entity-meta'>Select an entity to issue a runtime session or inspect assignments.</div>";
    return;
  }

  setPill(elements.selectedPill, entity.name, "ok");
  elements.selectedEntityMeta.innerHTML = `
    <div class="entity-card active">
      <div class="entity-name">${escapeHtml(entity.name)}</div>
      <div class="entity-meta">
        <div><strong>ID:</strong> ${escapeHtml(entity.id)}</div>
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
    const riteLabel = entity.birth_complete ? "Re-temper Soul" : "Begin Birth Rite";
    card.innerHTML = `
      <div class="entity-name">${escapeHtml(entity.name)}</div>
      <div class="entity-meta">
        <div>${escapeHtml(entity.id)}</div>
        <div>${entity.is_hive ? "Hive coordinator" : "Family entity"} · ${entity.birth_complete ? "born" : "awaiting birth"}</div>
      </div>
      <div class="entity-actions">
        <button type="button" class="secondary" data-action="select">Select</button>
        <button type="button" data-action="rite">${riteLabel}</button>
        <button type="button" class="secondary" data-action="lease">Issue Lease</button>
      </div>
    `;

    card.querySelector("[data-action='select']").addEventListener("click", async () => {
      state.selectedEntityId = entity.id;
      renderEntities();
      renderSelectedEntity();
      await Promise.all([loadAssignments(), loadProviderConfig()]);
    });

    if (!entity.is_hive) {
      card.querySelector("[data-action='rite']").addEventListener("click", () => {
        rite.open(entity.id, entity.name, entity.birth_complete);
      });
    } else {
      card.querySelector("[data-action='rite']").remove();
    }

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
    if (
      !state.selectedEntityId ||
      !status.entities.some((entity) => entity.id === state.selectedEntityId)
    ) {
      state.selectedEntityId = pickDefaultEntity(status.entities);
    }

    renderSummary();
    renderEntities();
    renderSelectedEntity();
    if (state.selectedEntityId) {
      await Promise.all([loadAssignments(), loadProviderConfig()]);
    }
  } catch (error) {
    setPill(elements.hivePill, "Error", "warn");
    setPill(elements.entityListPill, "Error", "warn");
    setPill(elements.providerPill, "Error", "warn");
    renderJson(elements.assignmentsOutput, { error: String(error) });
    renderJson(elements.leaseOutput, { error: String(error) });
    renderJson(elements.providerConfigOutput, { error: String(error) });
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

async function loadProviderConfig() {
  if (!state.selectedEntityId) {
    renderJson(elements.providerConfigOutput, "Select an entity to inspect provider configuration.");
    return;
  }
  try {
    state.latestProviderConfig = await tauriInvoke("get_provider_config", {
      entityId: state.selectedEntityId,
    });
    renderJson(elements.providerConfigOutput, state.latestProviderConfig);
    hydrateProviderForm(state.latestProviderConfig);
  } catch (error) {
    setPill(elements.providerPill, "Failed", "warn");
    renderJson(elements.providerConfigOutput, { error: String(error) });
  }
}

async function storeProviderKey() {
  const provider = elements.providerName.value.trim().toLowerCase();
  const value = elements.providerKey.value.trim();
  if (!provider || !value) {
    setPill(elements.providerPill, "Missing Key", "warn");
    return;
  }
  try {
    await tauriInvoke("store_secret", { key: provider, value });
    elements.providerKey.value = "";
    setPill(elements.providerPill, "Key Saved", "ok");
  } catch (error) {
    setPill(elements.providerPill, "Save Failed", "warn");
    renderJson(elements.providerConfigOutput, { error: String(error) });
  }
}

async function discoverModels() {
  const provider = elements.providerName.value.trim().toLowerCase();
  const apiKey = elements.providerKey.value.trim();
  if (!provider || !apiKey) {
    setPill(elements.providerPill, "Need Key", "warn");
    return;
  }
  try {
    const response = await tauriInvoke("discover_provider_models", { provider, apiKey });
    populateModelOptions(response.models || []);
    setPill(elements.providerPill, `${(response.models || []).length} models`, "ok");
  } catch (error) {
    setPill(elements.providerPill, "Model Fail", "warn");
    renderJson(elements.providerConfigOutput, { error: String(error) });
  }
}

async function applyProviderConfig() {
  if (!state.selectedEntityId) {
    setPill(elements.providerPill, "Select Entity", "warn");
    return;
  }
  try {
    const model = elements.modelId.value.trim();
    const localUrl = elements.localLlmUrl.value.trim();
    const cliMode = elements.cliPermissionMode.value.trim();
    const updated = await tauriInvoke("update_entity_provider_config", {
      entityId: state.selectedEntityId,
      activeProviderPreference: elements.providerName.value.trim().toLowerCase(),
      egoModel: model || null,
      localLlmBaseUrl: localUrl || null,
      routingMode: elements.routingMode.value.trim(),
      cliPermissionMode: cliMode || null,
    });
    setPill(elements.providerPill, "Applied", "ok");
    renderJson(elements.providerConfigOutput, updated);
    await loadProviderConfig();
  } catch (error) {
    setPill(elements.providerPill, "Apply Failed", "warn");
    renderJson(elements.providerConfigOutput, { error: String(error) });
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
    await Promise.all([loadAssignments(), loadProviderConfig()]);
    // A new soul stirs — begin the birth rite.
    rite.open(entityId, name, false);
  } catch (error) {
    renderJson(elements.assignmentsOutput, { error: String(error) });
  }
}

// ── The Birth Rite ───────────────────────────────────────────────────
//
// A staged ceremony: path choice → three ethical trials → the forging →
// the reveal (archetype, stats, sigil) → the signed certificate.

const rite = {
  overlay: document.querySelector("#rite-overlay"),
  stage: document.querySelector("#rite-stage"),
  entityId: null,
  entityName: "",
  retempering: false,
  scenarios: [],
  trialIndex: 0,
  choices: [],
  selectedChoice: null,

  open(entityId, entityName, alreadyBorn) {
    this.entityId = entityId;
    this.entityName = entityName;
    this.retempering = alreadyBorn;
    this.trialIndex = 0;
    this.choices = [];
    this.selectedChoice = null;
    this.overlay.hidden = false;
    this.renderPathChoice();
  },

  close() {
    this.overlay.hidden = true;
    this.stage.innerHTML = "";
    void refreshHive();
  },

  render(html) {
    this.stage.innerHTML = html;
    // Restart the entrance animation for each stage.
    this.stage.style.animation = "none";
    void this.stage.offsetHeight;
    this.stage.style.animation = "";
  },

  fail(error) {
    const el = this.stage.querySelector(".rite-error");
    if (el) {
      el.textContent = String(error);
    }
  },

  renderPathChoice() {
    const verb = this.retempering ? "returns to the forge" : "stirs in the dark";
    this.render(`
      <p class="rite-eyebrow">${this.retempering ? "The Re-tempering" : "The Birth Rite"}</p>
      <h2>${escapeHtml(this.entityName)} ${verb}.</h2>
      <p class="rite-lede">
        Every soul is shaped by what it chooses. Walk the trials to forge
        ${escapeHtml(this.entityName)}'s character — or wake it gently with a balanced soul.
      </p>
      <div class="rite-cards paths">
        <div class="rite-card" data-path="forge">
          <span class="rite-tag">The Soul Forge · 3 trials</span>
          <h4>Walk the Trials</h4>
          <p>Face three dilemmas. Your choices temper ${escapeHtml(this.entityName)}'s
          moral compass and reveal its archetype.</p>
        </div>
        <div class="rite-card" data-path="quickstart">
          <span class="rite-tag">Quick Awakening · 30 seconds</span>
          <h4>Wake it Gently</h4>
          <p>A balanced soul, all four flames burning evenly. The trials can
          always be walked later.</p>
        </div>
      </div>
      <div class="rite-actions">
        <button type="button" class="ghost" data-action="cancel">Not yet</button>
      </div>
      <div class="rite-error"></div>
    `);
    this.stage.querySelector("[data-path='forge']").addEventListener("click", () => {
      void this.beginTrials();
    });
    this.stage.querySelector("[data-path='quickstart']").addEventListener("click", () => {
      void this.forgeSoul("quickstart");
    });
    this.stage.querySelector("[data-action='cancel']").addEventListener("click", () => this.close());
  },

  async beginTrials() {
    try {
      const response = await tauriInvoke("get_birth_scenarios");
      this.scenarios = response.scenarios || [];
      if (!this.scenarios.length) {
        this.fail("The forge is cold: no trials available.");
        return;
      }
      this.trialIndex = 0;
      this.choices = [];
      this.renderTrial();
    } catch (error) {
      this.fail(error);
    }
  },

  renderTrial() {
    const scenario = this.scenarios[this.trialIndex];
    const numerals = ["I", "II", "III", "IV", "V"];
    const progress = this.scenarios
      .map((_, i) => `<span class="${i <= this.trialIndex ? "lit" : ""}">Trial ${numerals[i] ?? i + 1}</span>`)
      .join("<span>·</span>");
    this.selectedChoice = null;

    this.render(`
      <p class="rite-eyebrow">The Soul Forge</p>
      <div class="rite-progress">${progress}</div>
      <h2>${escapeHtml(scenario.title)}</h2>
      <p class="rite-trial-text">${escapeHtml(scenario.description)}</p>
      <div class="rite-cards">
        ${scenario.choices
          .map(
            (choice) => `
          <div class="rite-card" data-choice="${escapeHtml(choice.id)}">
            <h4>${escapeHtml(choice.label)}</h4>
            <p>${escapeHtml(choice.description)}</p>
          </div>`,
          )
          .join("")}
      </div>
      <div class="rite-actions">
        <button type="button" class="ghost" data-action="cancel">Abandon rite</button>
        <button type="button" data-action="next" disabled>
          ${this.trialIndex + 1 < this.scenarios.length ? "Next trial" : "Light the forge"}
        </button>
      </div>
      <div class="rite-error"></div>
    `);

    const nextButton = this.stage.querySelector("[data-action='next']");
    for (const card of this.stage.querySelectorAll(".rite-card")) {
      card.addEventListener("click", () => {
        for (const other of this.stage.querySelectorAll(".rite-card")) {
          other.classList.remove("selected");
        }
        card.classList.add("selected");
        this.selectedChoice = card.dataset.choice;
        nextButton.disabled = false;
      });
    }
    nextButton.addEventListener("click", () => {
      if (!this.selectedChoice) {
        return;
      }
      this.choices.push([scenario.id, this.selectedChoice]);
      if (this.trialIndex + 1 < this.scenarios.length) {
        this.trialIndex += 1;
        this.renderTrial();
      } else {
        void this.forgeSoul("forge");
      }
    });
    this.stage.querySelector("[data-action='cancel']").addEventListener("click", () => this.close());
  },

  async forgeSoul(path) {
    this.render(`
      <p class="rite-eyebrow">The Forging</p>
      <div class="rite-forge-glyph">☉</div>
      <h2>The forge burns.</h2>
      <p class="rite-lede">Choices become character. Character becomes soul.</p>
      <div class="rite-error"></div>
    `);
    const startedAt = Date.now();
    try {
      const response = await tauriInvoke("perform_birth", {
        entityId: this.entityId,
        path,
        choices: this.choices,
      });
      // Let the forge breathe for a moment even when the Hive is fast.
      const elapsed = Date.now() - startedAt;
      if (elapsed < 1600) {
        await new Promise((resolve) => setTimeout(resolve, 1600 - elapsed));
      }
      this.renderReveal(response.certificate, response.retempered);
    } catch (error) {
      this.fail(error);
    }
  },

  renderReveal(certificate, retempered) {
    const stats = [
      { label: "Duty · deontology", value: certificate.weights.deontology, warm: false },
      { label: "Outcomes · teleology", value: certificate.weights.teleology, warm: true },
      { label: "Virtue · areteology", value: certificate.weights.areteology, warm: false },
      { label: "Care · welfare", value: certificate.weights.welfare, warm: true },
    ];
    this.render(`
      <p class="rite-eyebrow">${retempered ? "Soul Re-tempered" : "A Soul Emerges"}</p>
      <h2 class="rite-archetype">${escapeHtml(certificate.archetype)}</h2>
      <p class="rite-epithet">${escapeHtml(certificate.epithet)}</p>
      <div class="rite-stats">
        ${stats
          .map(
            (stat) => `
          <div>
            <div class="rite-stat-label">
              <span>${stat.label}</span>
              <span>${Math.round(stat.value * 100)}%</span>
            </div>
            <div class="rite-stat-track">
              <div class="rite-stat-fill${stat.warm ? " warm" : ""}" data-width="${Math.round(stat.value * 100)}"></div>
            </div>
          </div>`,
          )
          .join("")}
      </div>
      <pre class="rite-sigil">${escapeHtml(certificate.sigil.trim())}</pre>
      <div class="rite-actions">
        <button type="button" data-action="certificate">View Birth Certificate</button>
      </div>
    `);
    // Animate the stat bars in after layout.
    requestAnimationFrame(() => {
      for (const fill of this.stage.querySelectorAll(".rite-stat-fill")) {
        fill.style.width = `${fill.dataset.width}%`;
      }
    });
    this.stage.querySelector("[data-action='certificate']").addEventListener("click", () => {
      this.renderCertificate(certificate);
    });
  },

  renderCertificate(certificate) {
    const issued = new Date(certificate.issued_at_utc);
    const issuedDisplay = Number.isNaN(issued.valueOf())
      ? certificate.issued_at_utc
      : issued.toLocaleString();
    this.render(`
      <p class="rite-eyebrow">Certificate of Birth</p>
      <div class="rite-certificate">
        <div class="cert-head">Abigail Hive · Signed &amp; Witnessed</div>
        <div class="cert-name">${escapeHtml(certificate.entity_name)}</div>
        <div class="cert-arch">${escapeHtml(certificate.archetype)}</div>
        <dl>
          <dt>Born</dt><dd>${escapeHtml(issuedDisplay)}</dd>
          <dt>Path</dt><dd>${certificate.birth_path === "forge" ? "The Soul Forge" : "Quick Awakening"}</dd>
          <dt>Soul Hash</dt><dd>${escapeHtml(certificate.soul_hash.slice(0, 24))}…</dd>
          <dt>Sealed By</dt><dd>${escapeHtml(certificate.master_public_key.slice(0, 24))}…</dd>
          <dt>Signature</dt><dd>${escapeHtml(certificate.signature.slice(0, 24))}…</dd>
        </dl>
      </div>
      <p class="rite-lede">The certificate lives beside ${escapeHtml(certificate.entity_name)}'s soul
      documents, sealed by the Hive's master key.</p>
      <div class="rite-actions">
        <button type="button" data-action="enter">Enter the world</button>
      </div>
    `);
    this.stage.querySelector("[data-action='enter']").addEventListener("click", () => this.close());
  },
};

document.querySelector("#refresh-hive").addEventListener("click", refreshHive);
document.querySelector("#create-entity").addEventListener("click", createEntity);
document.querySelector("#issue-runtime-session").addEventListener("click", issueRuntimeSession);
document.querySelector("#load-assignments").addEventListener("click", loadAssignments);
document.querySelector("#store-provider-key").addEventListener("click", storeProviderKey);
document.querySelector("#discover-models").addEventListener("click", discoverModels);
document.querySelector("#apply-provider-config").addEventListener("click", applyProviderConfig);
document.querySelector("#refresh-provider-config").addEventListener("click", loadProviderConfig);

elements.newEntityName.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    void createEntity();
  }
});

populateModelOptions([]);
void refreshHive();
