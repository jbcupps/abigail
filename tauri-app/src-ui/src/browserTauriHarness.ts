type EventCallback = (event: { event: string; id: number; payload: unknown }) => void;

interface HarnessOptions {
  force?: boolean;
  resetState?: boolean;
  strict?: boolean;
  trace?: boolean;
  seed?: number;
}

interface HarnessState {
  identities: Array<{
    id: string;
    name: string;
    directory: string;
    birth_complete: boolean;
    birth_date: string | null;
    primary_color?: string | null;
    avatar_url?: string | null;
  }>;
  activeAgentId: string | null;
  birthComplete: boolean;
  birthStage: "Darkness" | "KeyPresentation" | "Connectivity" | "Crystallization" | "Life";
  providers: Set<string>;
  activeProviderPreference: string | null;
  memoryDisclosureEnabled: boolean;
  localLlmUrl: string | null;
  cliServer: { running: boolean; port?: number; token?: string };
  genesisTurns: number;
  agenticRuns: Array<{
    task_id: string;
    status: string;
    current_turn: number;
    config: { goal: string; max_turns: number; require_confirmation: boolean };
    events: Array<Record<string, unknown>>;
    created_at: string;
    completed_at: string | null;
    attribution: {
      origin: string;
      entity_id: string | null;
      session_id: string | null;
      correlation_id: string | null;
    };
  }>;
}

type HarnessFaultMode = "none" | "chat_timeout" | "chat_error" | "provider_validation_error";

interface HarnessTraceEntry {
  at: string;
  type: "invoke" | "event" | "fault";
  name: string;
  detail?: Record<string, unknown>;
}

type HarnessWindow = Window & {
  __TAURI_INTERNALS__?: {
    invoke: (cmd: string, args?: Record<string, unknown>, options?: unknown) => Promise<unknown>;
    transformCallback: (callback: (...args: unknown[]) => unknown, once?: boolean) => number;
    unregisterCallback: (id: number) => void;
    convertFileSrc: (filePath: string, protocol?: string) => string;
  };
  __TAURI_EVENT_PLUGIN_INTERNALS__?: {
    unregisterListener: (event: string, eventId: number) => void;
  };
  isTauri?: boolean;
  __ABIGAIL_BROWSER_HARNESS__?: { installed: boolean };
};

const callbackRegistry = new Map<number, { callback: EventCallback; once: boolean }>();
const eventListeners = new Map<string, Map<number, number>>();
let callbackCounter = 1;
let eventListenerCounter = 1;
let traceLog: HarnessTraceEntry[] = [];
let faultMode: HarnessFaultMode = "none";
let agentSeq = 1;
let chatCorrelationSeq = 1;
let activeChatCorrelationId: string | null = null;
const providerValidationResults = new Map<string, string>();
let activeChatTimer: ReturnType<typeof setTimeout> | null = null;
const harnessConfig = {
  strict: false,
  trace: true,
  seed: 1337,
};

const defaultState = (): HarnessState => ({
  identities: [],
  activeAgentId: null,
  birthComplete: false,
  birthStage: "Darkness",
  providers: new Set<string>(),
  activeProviderPreference: null,
  memoryDisclosureEnabled: true,
  localLlmUrl: null,
  cliServer: { running: false },
  genesisTurns: 0,
  agenticRuns: [],
});

let state: HarnessState = defaultState();

function trace(type: HarnessTraceEntry["type"], name: string, detail?: Record<string, unknown>): void {
  if (!harnessConfig.trace) return;
  traceLog.push({
    at: new Date().toISOString(),
    type,
    name,
    detail,
  });
  if (traceLog.length > 300) {
    traceLog = traceLog.slice(traceLog.length - 300);
  }
}

function linkedProvider(provider: string): string | null {
  const mapping: Record<string, string> = {
    openai: "codex-cli",
    anthropic: "claude-cli",
    google: "gemini-cli",
    xai: "grok-cli",
    "codex-cli": "openai",
    "claude-cli": "anthropic",
    "gemini-cli": "google",
    "grok-cli": "xai",
  };
  return mapping[provider] ?? null;
}

function preferredProvider(): string {
  if (state.activeProviderPreference && state.providers.has(state.activeProviderPreference)) {
    return state.activeProviderPreference;
  }
  for (const candidate of ["openai", "google", "xai", "anthropic"]) {
    if (state.providers.has(candidate)) return candidate;
  }
  return "local";
}

function emitEvent(event: string, payload: unknown): void {
  trace("event", event, { payload: typeof payload === "object" ? payload as Record<string, unknown> : undefined });
  const listeners = eventListeners.get(event);
  if (!listeners) return;
  for (const [eventId, callbackId] of listeners.entries()) {
    const entry = callbackRegistry.get(callbackId);
    if (!entry) continue;
    entry.callback({ event, id: eventId, payload });
    if (entry.once) {
      callbackRegistry.delete(callbackId);
      listeners.delete(eventId);
    }
  }
}

function emitChatEnvelope(kind: "request" | "token" | "done" | "error", payload: Record<string, unknown> = {}): void {
  const correlationId = String(payload.correlation_id ?? activeChatCorrelationId ?? `corr-${chatCorrelationSeq++}`);
  activeChatCorrelationId = correlationId;
  emitEvent("chat-internal-envelope", {
    kind,
    correlation_id: correlationId,
    session_id: String(payload.session_id ?? "harness-session"),
    ...payload,
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function handleInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  trace("invoke", cmd, args);
  switch (cmd) {
    case "harness_debug_snapshot":
      return getHarnessDebugSnapshot();
    case "harness_debug_get_traces":
      return traceLog;
    case "harness_debug_set_fault": {
      const nextMode = String(args.mode ?? "none") as HarnessFaultMode;
      faultMode = nextMode;
      trace("fault", "set_fault", { mode: faultMode });
      return { ok: true, mode: faultMode };
    }
    case "harness_debug_reset":
      resetHarnessState();
      return getHarnessDebugSnapshot();
    case "harness_debug_config":
      if (typeof args.strict === "boolean") harnessConfig.strict = args.strict;
      if (typeof args.trace === "boolean") harnessConfig.trace = args.trace;
      if (typeof args.seed === "number") harnessConfig.seed = args.seed;
      return { ...harnessConfig };
    case "harness_debug_set_provider_validation": {
      const provider = String(args.provider ?? "").trim().toLowerCase();
      const error = String(args.error ?? "").trim();
      if (!provider) {
        return { ok: false, error: "provider is required" };
      }
      if (!error) {
        providerValidationResults.delete(provider);
      } else {
        providerValidationResults.set(provider, error);
      }
      trace("fault", "set_provider_validation", { provider, error: error || null });
      return { ok: true, provider, error: error || null };
    }

    // Event plugin
    case "plugin:event|listen": {
      const event = String(args.event ?? "");
      const callbackId = Number(args.handler ?? 0);
      const eventId = eventListenerCounter++;
      const listeners = eventListeners.get(event) ?? new Map<number, number>();
      listeners.set(eventId, callbackId);
      eventListeners.set(event, listeners);
      return eventId;
    }
    case "plugin:event|unlisten": {
      const event = String(args.event ?? "");
      const eventId = Number(args.eventId ?? 0);
      const listeners = eventListeners.get(event);
      listeners?.delete(eventId);
      return null;
    }
    case "plugin:event|emit": {
      emitEvent(String(args.event ?? ""), args.payload);
      return null;
    }
    case "plugin:event|emit_to":
      emitEvent(String(args.event ?? ""), args.payload);
      return null;

    // App + identity bootstrap
    case "get_active_agent":
      return state.activeAgentId;
    case "get_identities":
      return state.identities;
    case "check_existing_identity":
      return null;
    case "create_agent": {
      const name = String(args.name ?? "Abigail");
      const id = `agent-${agentSeq++}`;
      state.identities.push({
        id,
        name,
        directory: `E:/Agents/abigail/.hive/${id}`,
        birth_complete: false,
        birth_date: null,
        primary_color: "#00c2a8",
        avatar_url: null,
      });
      state.activeAgentId = id;
      return id;
    }
    case "load_agent": {
      const agentId = String(args.agentId ?? "");
      state.activeAgentId = agentId || state.activeAgentId;
      return null;
    }
    case "suspend_agent":
      state.activeAgentId = null;
      return null;
    case "archive_agent_identity": {
      const agentId = String(args.agentId ?? "");
      if (!agentId) throw new Error("agentId is required");
      if (state.activeAgentId === agentId) {
        throw new Error("Cannot archive active agent. Suspend first.");
      }
      state.identities = state.identities.filter((identity) => identity.id !== agentId);
      return true;
    }
    case "delete_agent_identity": {
      const agentId = String(args.agentId ?? "");
      if (!agentId) throw new Error("agentId is required");
      if (state.activeAgentId === agentId) {
        throw new Error("Cannot delete active agent. Suspend first.");
      }
      state.identities = state.identities.filter((identity) => identity.id !== agentId);
      return true;
    }
    case "archive_identity": {
      if (!state.activeAgentId) throw new Error("No active agent to archive");
      throw new Error("Cannot archive active agent. Suspend first.");
    }
    case "wipe_identity": {
      if (!state.activeAgentId) throw new Error("No active agent to wipe");
      throw new Error("Cannot wipe active agent. Suspend first.");
    }
    case "get_birth_complete":
      return state.birthComplete;
    case "run_startup_checks":
      return {
        heartbeat_ok: true,
        verification_ok: true,
        integrity: {
          status: "ok",
          summary: "Harness integrity checks passed.",
          details: [],
        },
        allow_continue: false,
        error: null,
      };
    case "inspect_identity_integrity":
      return {
        status: "ok",
        summary: "Harness integrity checks passed.",
        details: [],
      };
    case "get_agent_name": {
      const active = state.identities.find((i) => i.id === state.activeAgentId);
      return active?.name ?? null;
    }
    case "get_entity_theme": {
      const active = state.identities.find((i) => i.id === state.activeAgentId);
      return { primary_color: active?.primary_color ?? null, avatar_url: active?.avatar_url ?? null };
    }

    // Birth sequence
    case "init_soul":
      return null;
    case "check_interrupted_birth":
      return { was_interrupted: false, stage: null };
    case "check_identity_status":
      return state.birthComplete ? "Complete" : "Clean";
    case "start_birth":
      state.birthStage = "Darkness";
      return null;
    case "generate_identity":
      state.birthStage = "KeyPresentation";
      return {
        private_key_base64: "ZTItdGVzdC1wcml2YXRlLWtleS1zYW1wbGU=",
        public_key_path: "E:/Agents/abigail/.hive/external_pubkey.bin",
        newly_generated: true,
      };
    case "save_recovery_key":
      return "E:/Agents/abigail/Recovery/RECOVERY_BUNDLE.abigail-recovery";
    case "save_recovery_key_plaintext":
      return "E:/Agents/abigail/Recovery/RECOVERY_KEY.txt";
    case "advance_past_darkness":
      return null;
    case "advance_to_connectivity":
      state.birthStage = "Connectivity";
      return null;
    case "advance_to_crystallization":
      state.birthStage = "Crystallization";
      return null;
    case "crystallize_soul":
      return `# Soul\nName: ${String(args.name ?? "Abigail")}\nPurpose: ${String(args.purpose ?? "")}`;
    case "complete_emergence":
      return null;
    case "sign_agent_with_hive":
      state.birthStage = "Life";
      state.birthComplete = true;
      if (state.activeAgentId) {
        state.identities = state.identities.map((identity) =>
          identity.id === state.activeAgentId
            ? { ...identity, birth_complete: true, birth_date: new Date().toISOString().slice(0, 10) }
            : identity
        );
      }
      return null;
    case "complete_birth":
      state.birthComplete = true;
      return null;
    case "reset_birth":
      state.birthComplete = false;
      state.birthStage = "Darkness";
      return null;

    // Ignition/local setup
    case "detect_ollama":
      return { status: "running", path: "C:/Program Files/Ollama/ollama.exe" };
    case "list_recommended_models":
      return [
        {
          name: "qwen2.5:3b",
          label: "Balanced",
          size_bytes: 1900000000,
          description: "Fast local model for development.",
          recommended: true,
        },
      ];
    case "probe_local_llm":
      return { detected: [{ name: "LM Studio", url: "http://localhost:1234", reachable: true }] };
    case "set_local_llm_during_birth":
      {
        const raw = String(args.url ?? "").trim();
        state.localLlmUrl = raw.length > 0 ? raw : null;
      }
      return true;
    case "set_local_llm_url":
      {
        const raw = String(args.url ?? "").trim();
        state.localLlmUrl = raw.length > 0 ? raw : null;
      }
      return true;

    // Provider/key setup
    case "get_stored_providers":
      return Array.from(state.providers);
    case "detect_cli_providers":
      return [];
    case "detect_cli_providers_full":
      return [];
    case "store_provider_key": {
      const provider = String(args.provider ?? "");
      if (!provider) return { success: false, provider: "", validated: false, error: "Provider missing" };
      const validationError = providerValidationResults.get(provider.toLowerCase());
      if (validationError) {
        trace("fault", "provider_validation_fixture_error", { provider, error: validationError });
        return { success: false, provider, validated: false, error: validationError };
      }
      if (faultMode === "provider_validation_error") {
        trace("fault", "provider_validation_error", { provider });
        return { success: false, provider, validated: false, error: "Synthetic provider validation failure" };
      }
      state.providers.add(provider);
      const linked = linkedProvider(provider);
      if (linked) state.providers.add(linked);
      if (!provider.endsWith("-cli")) {
        state.activeProviderPreference = provider;
      }
      return { success: true, provider, validated: Boolean(args.validate), error: null };
    }
    case "use_stored_provider":
      state.activeProviderPreference = String(args.provider ?? state.activeProviderPreference ?? "openai");
      return true;
    case "set_api_key":
      state.providers.add("openai");
      state.activeProviderPreference = "openai";
      return true;

    // Birth chat + genesis
    case "birth_chat":
      if (state.activeProviderPreference) {
        const runtimeValidationError = providerValidationResults.get(
          state.activeProviderPreference.toLowerCase()
        );
        if (runtimeValidationError) {
          throw new Error(`Ego verification failed: ${runtimeValidationError}`);
        }
      }
      return {
        message: "I received that. Add your provider keys using the command center above when ready.",
        stage: "Connectivity",
        actions: [],
      };
    case "genesis_chat":
      state.genesisTurns += 1;
      return {
        message:
          state.genesisTurns >= 2
            ? "Identity signals are converging. We can proceed to review."
            : "Tell me one more thing about the tone you want me to carry.",
        complete: state.genesisTurns >= 2,
      };
    case "extract_crystallization_identity":
      return {
        name: "Abigail",
        purpose: "Assist and execute practical tasks safely.",
        personality: "Clear, concise, and candid.",
        primary_color: "#00c2a8",
        avatar_url: "",
      };

    // Agentic runtime
    case "get_agentic_runtime_status":
      return {
        healthy: true,
        storage_path: "E:/Agents/abigail/data/agentic_runs.json",
        loaded_runs: state.agenticRuns.length,
        active_runs: state.agenticRuns.filter((run) => !["completed", "failed", "cancelled"].includes(run.status)).length,
        recovered_runs: 0,
      };
    case "start_agentic_run":
    case "start_entity_initiated_agentic_run": {
      const taskId = `task-${Date.now()}-${Math.floor(Math.random() * 999)}`;
      const goal = String(args.goal ?? "");
      const maxTurns = Number(args.maxTurns ?? args.max_turns ?? 10);
      const requireConfirmation = Boolean(args.requireConfirmation ?? args.require_confirmation ?? false);
      const origin = cmd === "start_entity_initiated_agentic_run" ? "entity_pipeline" : "gui";
      const run = {
        task_id: taskId,
        status: "running",
        current_turn: 1,
        config: { goal, max_turns: maxTurns, require_confirmation: requireConfirmation },
        events: [{ type: "run_started", task_id: taskId, goal }],
        created_at: new Date().toISOString(),
        completed_at: null,
        attribution: {
          origin,
          entity_id: args.entityId ? String(args.entityId) : null,
          session_id: args.sessionId ? String(args.sessionId) : null,
          correlation_id: args.correlationId ? String(args.correlationId) : null,
        },
      };
      state.agenticRuns.unshift(run);
      emitEvent("agentic-event", { type: "run_started", task_id: taskId, goal });
      return taskId;
    }
    case "get_agentic_run_status": {
      const taskId = String(args.taskId ?? args.task_id ?? "");
      const run = state.agenticRuns.find((r) => r.task_id === taskId);
      if (!run) throw new Error(`Run '${taskId}' not found`);
      return { run, attribution: run.attribution };
    }
    case "list_agentic_runs":
      return state.agenticRuns.map((run) => ({ run, attribution: run.attribution }));
    case "respond_to_mentor_ask":
    case "respond_agentic_mentor": {
      const taskId = String(args.taskId ?? args.task_id ?? "");
      const run = state.agenticRuns.find((r) => r.task_id === taskId);
      if (!run) throw new Error(`Run '${taskId}' not found`);
      run.status = "running";
      emitEvent("agentic-event", { type: "mentor_response_received", task_id: taskId });
      return null;
    }
    case "confirm_tool_execution":
    case "confirm_agentic_action": {
      const taskId = String(args.taskId ?? args.task_id ?? "");
      const run = state.agenticRuns.find((r) => r.task_id === taskId);
      if (!run) throw new Error(`Run '${taskId}' not found`);
      run.status = "running";
      emitEvent("agentic-event", {
        type: "mentor_confirmation_received",
        task_id: taskId,
        approved: Boolean(args.approved),
      });
      return null;
    }
    case "cancel_agentic_run": {
      const taskId = String(args.taskId ?? args.task_id ?? "");
      const run = state.agenticRuns.find((r) => r.task_id === taskId);
      if (!run) throw new Error(`Run '${taskId}' not found`);
      run.status = "cancelled";
      run.completed_at = new Date().toISOString();
      emitEvent("agentic-event", { type: "run_cancelled", task_id: taskId });
      return null;
    }

    // Orchestration backend
    case "get_orchestration_backend_status":
      return {
        healthy: true,
        jobs_loaded: 0,
        runtime_loaded_runs: state.agenticRuns.length,
        runtime_active_runs: state.agenticRuns.filter((run) => !["completed", "failed", "cancelled"].includes(run.status)).length,
      };
    case "list_orchestration_jobs":
      return [];
    case "set_orchestration_job_enabled":
      return null;
    case "delete_orchestration_job":
      return null;
    case "run_orchestration_job_now":
      return { run_id: `orch-${Date.now()}`, mode: "id_check", result_summary: "Harness orchestration run complete." };
    case "list_orchestration_job_logs":
      return [];

    // Chat screen status
    case "get_router_status": {
      const hasLocalHttp = typeof state.localLlmUrl === "string" && state.localLlmUrl.trim().length > 0;
      return {
        id_provider: hasLocalHttp ? "local_http" : "candle_stub",
        id_url: hasLocalHttp ? state.localLlmUrl : null,
        ego_configured: state.providers.size > 0,
        ego_provider: state.providers.size > 0 ? preferredProvider() : null,
        superego_configured: false,
        routing_mode: "tier_based",
        council_providers: 0,
      };
    }
    case "get_ollama_status":
      return { managed: true, running: true, port: 11434, model_ready: true };
    case "get_cli_server_status":
      return state.cliServer;
    case "start_cli_server":
      state.cliServer = { running: true, port: Number(args.port ?? 8080), token: "test-token" };
      return null;
    case "stop_cli_server":
      state.cliServer = { running: false };
      return null;
    case "get_log_level":
      return "info";
    case "set_log_level":
      return null;
    case "get_captured_logs":
      return { entries: [], next_index: 0 };
    case "clear_captured_logs":
      return null;
    case "export_logs":
      return "";
    case "save_logs_to_file":
      return null;
    case "list_missing_skill_secrets":
      return [];
    case "list_skills_vault_entries":
      return [];
    case "get_memory_disclosure_settings":
      return { enabled: state.memoryDisclosureEnabled };
    case "set_memory_disclosure_settings":
      state.memoryDisclosureEnabled = Boolean(args.enabled);
      return null;

    // Streaming chat — emits internal chat envelope events.
    case "cancel_chat_stream":
      if (activeChatTimer) {
        clearTimeout(activeChatTimer);
        activeChatTimer = null;
        emitChatEnvelope("error", { error: "Interrupted by user" });
        return true;
      }
      return false;
    case "chat_stream": {
      const correlationId = `corr-${chatCorrelationSeq++}`;
      activeChatCorrelationId = correlationId;
      emitChatEnvelope("request", { correlation_id: correlationId, session_id: "harness-session" });

      if (faultMode === "chat_timeout") {
        trace("fault", "chat_timeout");
        await sleep(1500);
        emitChatEnvelope("error", {
          correlation_id: correlationId,
          error: "Synthetic chat timeout injected by harness",
        });
        return null;
      }
      if (faultMode === "chat_error") {
        trace("fault", "chat_error");
        emitChatEnvelope("error", {
          correlation_id: correlationId,
          error: "Synthetic chat failure injected by harness",
        });
        return null;
      }

      const message = String(args.message ?? "").toLowerCase();
      const provider = preferredProvider();

      let response: Record<string, unknown>;

      if (message.includes("clipboard")) {
        response = {
          reply: "Clipboard skill result: read succeeded. Current clipboard text is 'sample clipboard value'.",
          provider,
          tool_calls_made: [{ skill_id: "builtin.clipboard", tool_name: "read_clipboard", success: true }],
          tier: "fast",
          model_used: "harness-model",
          complexity_score: 15,
        };
      } else if (message.includes("create a skill") || message.includes("author skill")) {
        response = {
          reply: "I've created the skill 'custom.greeter' with an author_skill tool call. The skill directory has been set up.",
          provider,
          tool_calls_made: [{ skill_id: "builtin.skill_factory", tool_name: "author_skill", success: true }],
          tier: "standard",
          model_used: "harness-model",
          complexity_score: 55,
        };
      } else {
        const replyText = `Harness reply via ${provider}: acknowledged "${String(args.message ?? "")}".`;
        response = { reply: replyText, provider, tool_calls_made: [] };
      }

      // Simulate async streaming: token then done.
      if (activeChatTimer) {
        clearTimeout(activeChatTimer);
        activeChatTimer = null;
      }
      activeChatTimer = setTimeout(() => {
        emitChatEnvelope("token", {
          correlation_id: correlationId,
          token: String(response.reply ?? ""),
        });
        emitChatEnvelope("done", {
          correlation_id: correlationId,
          done: response,
        });
        activeChatTimer = null;
        activeChatCorrelationId = null;
      }, 5);
      return null;
    }

    default:
      return null;
  }
}

function resetHarnessState(): void {
  state = defaultState();
  callbackRegistry.clear();
  eventListeners.clear();
  callbackCounter = 1;
  eventListenerCounter = 1;
  faultMode = "none";
  agentSeq = 1;
  if (activeChatTimer) {
    clearTimeout(activeChatTimer);
    activeChatTimer = null;
  }
  activeChatCorrelationId = null;
  chatCorrelationSeq = 1;
  providerValidationResults.clear();
  traceLog = [];
}

export function getHarnessDebugSnapshot(): {
  runtime: "browser-harness";
  config: { strict: boolean; trace: boolean; seed: number };
  faultMode: HarnessFaultMode;
  state: {
    activeAgentId: string | null;
    birthComplete: boolean;
    birthStage: HarnessState["birthStage"];
    providers: string[];
    activeProviderPreference: string | null;
    localLlmUrl: string | null;
    memoryDisclosureEnabled: boolean;
    listenerCount: number;
    providerValidationOverrides: Record<string, string>;
  };
} {
  let listenerCount = 0;
  for (const listeners of eventListeners.values()) {
    listenerCount += listeners.size;
  }
  return {
    runtime: "browser-harness",
    config: { ...harnessConfig },
    faultMode,
    state: {
      activeAgentId: state.activeAgentId,
      birthComplete: state.birthComplete,
      birthStage: state.birthStage,
      providers: Array.from(state.providers),
      activeProviderPreference: state.activeProviderPreference,
      localLlmUrl: state.localLlmUrl,
      memoryDisclosureEnabled: state.memoryDisclosureEnabled,
      listenerCount,
      providerValidationOverrides: Object.fromEntries(providerValidationResults.entries()),
    },
  };
}

export function installBrowserTauriHarness(options: HarnessOptions = {}): void {
  const { force = false, resetState = false, strict, trace: traceEnabled, seed } = options;
  const harnessWindow = window as HarnessWindow;
  const hasNative = Boolean(harnessWindow.__TAURI_INTERNALS__) || Boolean(harnessWindow.isTauri);

  if (hasNative && !force) return;
  if (harnessWindow.__ABIGAIL_BROWSER_HARNESS__?.installed && !force && !resetState) return;

  if (resetState) {
    resetHarnessState();
  }
  if (typeof strict === "boolean") harnessConfig.strict = strict;
  if (typeof traceEnabled === "boolean") harnessConfig.trace = traceEnabled;
  if (typeof seed === "number") harnessConfig.seed = seed;

  harnessWindow.__TAURI_INTERNALS__ = {
    invoke: (cmd, args, _options) => handleInvoke(cmd, args),
    transformCallback: (callback, once = false) => {
      const id = callbackCounter++;
      callbackRegistry.set(id, { callback: callback as EventCallback, once });
      return id;
    },
    unregisterCallback: (id) => {
      callbackRegistry.delete(id);
    },
    convertFileSrc: (filePath: string, protocol = "asset") =>
      `${protocol}://${filePath.replace(/\\/g, "/")}`,
  };

  harnessWindow.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (event: string, eventId: number) => {
      const listeners = eventListeners.get(event);
      listeners?.delete(eventId);
    },
  };

  harnessWindow.__ABIGAIL_BROWSER_HARNESS__ = { installed: true };
  trace("invoke", "harness_installed", { strict: harnessConfig.strict, trace: harnessConfig.trace, seed: harnessConfig.seed });
}
