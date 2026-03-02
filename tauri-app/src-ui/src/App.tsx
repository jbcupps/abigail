import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { ThemeProvider, useTheme } from "./contexts/ThemeContext";
import SoulRegistry from "./components/SoulRegistry";
import BootSequence from "./components/BootSequence";
import ChatInterface from "./components/ChatInterface";
import type { ChatSessionSnapshot } from "./components/ChatInterface";
import PersonaToggle from "./components/PersonaToggle";
import IdentityConflictPanel, { IdentitySummary } from "./components/IdentityConflictPanel";
import SplashScreen from "./components/SplashScreen";
import AbnormalBrainScreen from "./components/AbnormalBrainScreen";
import SanctumDrawer from "./components/SanctumDrawer";
import UpdateNotification from "./components/UpdateNotification";
import HarnessDebugPanel from "./components/HarnessDebugPanel";
import {
  detectRuntimeMode,
  isBrowserHarnessRuntime,
  isHarnessDebugEnabled,
  setHarnessDebugEnabled,
} from "./runtimeMode";

type AppState =
  | "splash"
  | "loading"
  | "model_loading"
  | "management"
  | "identity_conflict"
  | "boot"
  | "startup_check"
  | "startup_failed"
  | "chat";

interface StartupCheckResult {
  heartbeat_ok: boolean;
  verification_ok: boolean;
  error: string | null;
}

function assertNeverState(state: never): never {
  throw new Error(`Unhandled AppState: ${state}`);
}

function AppInner() {
  const [appState, setAppState] = useState<AppState>("splash");
  const [startupError, setStartupError] = useState<string | null>(null);
  const [existingIdentity, setExistingIdentity] = useState<IdentitySummary | null>(null);
  const [forgeOpen, setForgeOpen] = useState(false);
  const [activeSoulId, setActiveSoulId] = useState<string | null>(null);
  const [suspendedSessions, setSuspendedSessions] = useState<Record<string, ChatSessionSnapshot>>({});
  const [ollamaProgress, setOllamaProgress] = useState(0);
  const [ollamaStatus, setOllamaStatus] = useState("");
  const [isFirstPull, setIsFirstPull] = useState(true);
  const { setMode, refreshAgentName } = useTheme();

  const initializeApp = async () => {
    try {
      // Always enter Registry selection first for explicit mentor choice.
      const activeAgent = await invoke<string | null>("get_active_agent");
      if (activeAgent) {
        await invoke("suspend_agent");
      }

      // Start managed Ollama (bundled Hive agent LLM).
      // Await the result BEFORE entering model_loading so AbnormalBrainScreen
      // mounts with correct initial values for isFirstPull / progress / status.
      let needsPull = false;
      let ollamaFailed = false;
      try {
        needsPull = await invoke<boolean>("start_managed_ollama");
      } catch (e) {
        console.warn("[App] start_managed_ollama failed; continuing without bundled Ollama:", e);
        ollamaFailed = true;
      }

      if (ollamaFailed) {
        // No Ollama — show instant text + 100% bar, let onReady transition
        setIsFirstPull(false);
        setOllamaProgress(100);
        setOllamaStatus("No local LLM found — continuing with cloud providers");
        setAppState("model_loading");
        return;
      }

      if (needsPull) {
        // Model is being downloaded — typewriter + progress bar via events
        setIsFirstPull(true);
        setOllamaProgress(0);
        setOllamaStatus("Downloading model...");
        setAppState("model_loading");
        return;
      }

      // Model exists — show instant text + 100% bar, fire warmup in background
      setIsFirstPull(false);
      setOllamaProgress(100);
      setOllamaStatus("Hive agent ready");
      setAppState("model_loading");
      // Non-blocking warmup — model will load on first real request if this fails
      invoke("warmup_ollama_model").catch(() => {});
    } catch (e) {
      console.error("[App] initializeApp failed; falling back to management screen:", e);
      // Fallback to management screen on error
      setAppState("management");
    }
  };

  const handleSplashComplete = () => {
    setAppState("loading");
    initializeApp();
  };

  // Continue to management screen after model loading completes
  const continueAfterModelReady = async () => {
    try {
      const identities = await invoke<unknown[]>("get_identities");
      if (identities.length === 0) {
        const identity = await invoke<IdentitySummary | null>("check_existing_identity");
        if (identity) {
          setExistingIdentity(identity);
          setAppState("identity_conflict");
          return;
        }
      }
      setAppState("management");
    } catch (e) {
      console.error("[App] continueAfterModelReady failed:", e);
      setAppState("management");
    }
  };

  // Listen for Ollama lifecycle and model progress events
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    listen<Record<string, unknown> | string>("ollama-lifecycle", (event) => {
      const payload = event.payload;
      if (typeof payload === "object" && payload !== null && "pulling_model" in payload) {
        // PullingModel { progress_pct } — serde serializes to {"pulling_model": {"progress_pct": N}}
        const inner = (payload as { pulling_model: { progress_pct?: number } }).pulling_model;
        setOllamaProgress(inner?.progress_pct ?? 0);
      } else if (payload === "model_ready") {
        setOllamaProgress(100);
        // onReady in AbnormalBrainScreen will handle transition after brief pause
      } else if (typeof payload === "object" && payload !== null && "error" in payload) {
        console.warn("[App] Ollama lifecycle error:", payload);
        // Skip to management on error
        setAppState("management");
      }
    }).then((fn) => unlisteners.push(fn));

    listen<{ model: string; status: string; completed?: number; total?: number }>(
      "ollama-model-progress",
      (event) => {
        const { status, completed, total } = event.payload;
        setOllamaStatus(status || "Downloading...");
        if (completed != null && total != null && total > 0) {
          setOllamaProgress((completed / total) * 100);
        }
      }
    ).then((fn) => unlisteners.push(fn));

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  useEffect(() => {
    // If we somehow start in loading (e.g. no splash needed), initialize immediately
    if (appState === "loading") {
      initializeApp();
    }
  }, []);

  const runStartupChecks = async () => {
    try {
      const result = await invoke<StartupCheckResult>("run_startup_checks");

      if (!result.heartbeat_ok) {
        setStartupError(result.error || "LLM heartbeat failed. Is the local LLM server running?");
        setAppState("startup_failed");
        return;
      }

      if (!result.verification_ok && result.error) {
        setStartupError(result.error);
        setAppState("startup_failed");
        return;
      }

      // All checks passed — go to chat (ego mode already set)
      setAppState("chat");
    } catch (e) {
      setStartupError(String(e));
      setAppState("startup_failed");
    }
  };

  const onBirthComplete = async () => {
    // After birth, switch to ego and go to chat
    setMode("ego");
    await refreshAgentName();
    setAppState("chat");
  };

  const handleRetry = () => {
    setStartupError(null);
    setAppState("startup_check");
    runStartupChecks();
  };

  const handleContinueAnyway = () => {
    setMode("ego");
    setAppState("chat");
  };

  // Handlers for identity conflict screen (legacy migration path)
  const handleIdentityResume = async () => {
    // Migrate legacy identity to Hive format, then load it
    try {
      const uuid = await invoke<string | null>("migrate_legacy_identity");
      if (uuid) {
        await invoke("load_agent", { agentId: uuid });
        setActiveSoulId(uuid);
        setExistingIdentity(null);
        setMode("ego");
        setAppState("startup_check");
        await refreshAgentName();
        await runStartupChecks();
      } else {
        // No legacy identity found, go to management
        setExistingIdentity(null);
        setAppState("management");
      }
    } catch (e) {
      console.error("[App] handleIdentityResume failed; returning to management:", e);
      setExistingIdentity(null);
      setAppState("management");
    }
  };

  const handleIdentityArchive = () => {
    // Identity has been archived, go to management screen
    setExistingIdentity(null);
    setAppState("management");
  };

  const handleIdentityWipe = () => {
    // Identity has been wiped, go to management screen
    setExistingIdentity(null);
    setAppState("management");
  };

  // Handler for agent selection from management screen
  const handleAgentSelected = async (_agentId: string) => {
    setActiveSoulId(_agentId);
    const complete = await invoke<boolean>("get_birth_complete");
    if (complete) {
      setMode("ego");
      setAppState("startup_check");
      await refreshAgentName();
      await runStartupChecks();
    } else {
      // Agent exists but not born yet, go to boot
      setAppState("boot");
    }
  };

  // Handler for creating a new agent from management screen
  const handleCreateAgent = (agentId?: string) => {
    if (agentId) setActiveSoulId(agentId);
    // Agent was just created and loaded, go to boot sequence
    setAppState("boot");
  };

  // Handler for disconnecting from agent (back to management)
  const handleDisconnect = async () => {
    setForgeOpen(false);
    try {
      await invoke("suspend_agent");
    } catch (e) {
      console.warn("[App] suspend_agent failed; continuing to management:", e);
    }
    setMode("neutral");
    setAppState("management");
  };

  const handleSessionSnapshot = useCallback(
    (snapshot: ChatSessionSnapshot) => {
      if (!activeSoulId) return;
      setSuspendedSessions((prev) => ({ ...prev, [activeSoulId]: snapshot }));
    },
    [activeSoulId]
  );

  switch (appState) {
    case "splash":
      return <SplashScreen onComplete={handleSplashComplete} />;
    case "loading":
      return (
        <div className="min-h-screen bg-theme-bg text-theme-text-dim font-mono flex items-center justify-center">
          <div className="animate-pulse">Loading...</div>
        </div>
      );
    case "model_loading":
      return (
        <AbnormalBrainScreen
          isFirstPull={isFirstPull}
          progress={ollamaProgress}
          statusText={ollamaStatus}
          onReady={continueAfterModelReady}
          onSkip={continueAfterModelReady}
        />
      );
    case "management":
      return (
        <SoulRegistry
          onSoulSelected={handleAgentSelected}
          onNewSoul={handleCreateAgent}
        />
      );
    case "identity_conflict":
      if (!existingIdentity) {
        console.warn("[App] identity_conflict without identity; returning to registry");
        return (
          <SoulRegistry
            onSoulSelected={handleAgentSelected}
            onNewSoul={handleCreateAgent}
          />
        );
      }
      return (
        <IdentityConflictPanel
          identity={existingIdentity}
          onResume={handleIdentityResume}
          onArchive={handleIdentityArchive}
          onWipe={handleIdentityWipe}
        />
      );
    case "boot":
      return <BootSequence onComplete={onBirthComplete} />;
    case "startup_check":
      return (
        <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col items-center justify-center">
          <div className="bg-theme-bg-elevated border border-theme-border-dim rounded-lg p-6 text-center">
            <pre className="text-theme-primary text-sm mb-4">
              ABIGAIL STARTUP
              ============
            </pre>
            <p className="text-theme-text-dim">Running startup checks...</p>
            <div className="animate-pulse mt-2 text-theme-text-dim">...</div>
          </div>
        </div>
      );
    case "startup_failed":
      return (
        <div className="min-h-screen bg-theme-bg text-theme-text font-mono flex flex-col items-center justify-center p-6">
          <div className="bg-theme-bg-elevated border border-theme-border-dim rounded-lg p-6 text-center">
            <pre className="text-theme-primary text-sm mb-4">
              ABIGAIL STARTUP
              ============
            </pre>
            <p className="text-theme-danger mb-4">{startupError}</p>
          </div>
          <div className="flex gap-4 mt-4">
            <button
              className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow"
              onClick={handleRetry}
            >
              Retry
            </button>
            <button
              className="border border-yellow-500 text-yellow-500 px-4 py-2 rounded hover:bg-yellow-500/20"
              onClick={handleContinueAnyway}
            >
              Continue anyway
            </button>
          </div>
        </div>
      );
    case "chat":
      return (
        <div className="h-screen flex flex-col">
          <UpdateNotification />
          <PersonaToggle
            onToggle={() => setForgeOpen((prev) => !prev)}
            forgeOpen={forgeOpen}
          />
          <SanctumDrawer
            open={forgeOpen}
            onClose={() => setForgeOpen(false)}
            onDisconnect={handleDisconnect}
          />
          <div className="flex-1 min-h-0">
            <ChatInterface
              initialSession={activeSoulId ? suspendedSessions[activeSoulId] ?? null : null}
              onSessionSnapshot={handleSessionSnapshot}
            />
          </div>
        </div>
      );
    default:
      return assertNeverState(appState);
  }
}

function App() {
  const runtimeMode = detectRuntimeMode();
  const showRuntimeBadge = runtimeMode === "browser-harness" || isHarnessDebugEnabled();
  const showHarnessDebugPanel = isBrowserHarnessRuntime() && isHarnessDebugEnabled();

  return (
    <ThemeProvider initialMode="neutral">
      {showHarnessDebugPanel && <HarnessDebugPanel />}
      {showRuntimeBadge && (
        <button
          className="fixed bottom-2 right-2 z-[9999] text-[10px] px-2 py-1 rounded border border-theme-border-dim bg-theme-bg-elevated text-theme-text-dim hover:text-theme-text"
          onClick={() => {
            if (isBrowserHarnessRuntime()) {
              setHarnessDebugEnabled(!isHarnessDebugEnabled());
              window.location.reload();
            }
          }}
          title={
            isBrowserHarnessRuntime()
              ? "Click to toggle harness debug panel"
              : "Native runtime mode"
          }
        >
          runtime: {runtimeMode}
        </button>
      )}
      <AppInner />
    </ThemeProvider>
  );
}

export default App;
