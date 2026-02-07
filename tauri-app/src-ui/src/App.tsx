import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { ThemeProvider, useTheme } from "./contexts/ThemeContext";
import BootSequence from "./components/BootSequence";
import ChatInterface from "./components/ChatInterface";
import PersonaToggle from "./components/PersonaToggle";
import IdentityPanel from "./components/IdentityPanel";
import IdentityConflictPanel, { IdentitySummary } from "./components/IdentityConflictPanel";
import ManagementScreen from "./components/ManagementScreen";

type AppState =
  | "loading"
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

function AppInner() {
  const [appState, setAppState] = useState<AppState>("loading");
  const [startupError, setStartupError] = useState<string | null>(null);
  const [existingIdentity, setExistingIdentity] = useState<IdentitySummary | null>(null);
  const { mode, setMode, refreshAgentName } = useTheme();

  useEffect(() => {
    (async () => {
      try {
        // Check if an agent is already active (e.g. resumed session)
        const activeAgent = await invoke<string | null>("get_active_agent");
        if (activeAgent) {
          // Agent already loaded, go to startup checks
          const complete = await invoke<boolean>("get_birth_complete");
          if (complete) {
            setMode("ego");
            setAppState("startup_check");
            await refreshAgentName();
            await runStartupChecks();
          } else {
            setAppState("boot");
          }
          return;
        }

        // Check if Hive has any agents
        const identities = await invoke<unknown[]>("get_identities");

        if (identities.length === 0) {
          // Check for legacy single-identity installation
          const identity = await invoke<IdentitySummary | null>("check_existing_identity");
          if (identity) {
            // Show identity conflict/migration screen
            setExistingIdentity(identity);
            setAppState("identity_conflict");
            return;
          }
        }

        // Default: show the management screen (identity selector)
        setAppState("management");
      } catch {
        // Fallback to management screen on error
        setAppState("management");
      }
    })();
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
    } catch {
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
  const handleCreateAgent = () => {
    // Agent was just created and loaded, go to boot sequence
    setAppState("boot");
  };

  // Handler for disconnecting from agent (back to management)
  const handleDisconnect = async () => {
    try {
      await invoke("disconnect_agent");
    } catch {
      // Ignore errors during disconnect
    }
    setAppState("management");
  };

  if (appState === "loading") {
    return (
      <div className="min-h-screen bg-black text-gray-500 font-mono flex items-center justify-center">
        Loading...
      </div>
    );
  }

  if (appState === "management") {
    return (
      <ManagementScreen
        onAgentSelected={handleAgentSelected}
        onCreateAgent={handleCreateAgent}
      />
    );
  }

  if (appState === "identity_conflict" && existingIdentity) {
    return (
      <IdentityConflictPanel
        identity={existingIdentity}
        onResume={handleIdentityResume}
        onArchive={handleIdentityArchive}
        onWipe={handleIdentityWipe}
      />
    );
  }

  if (appState === "boot") {
    return <BootSequence onComplete={onBirthComplete} />;
  }

  if (appState === "startup_check") {
    return (
      <div className="min-h-screen bg-black text-gray-400 font-mono flex flex-col items-center justify-center">
        <pre className="text-sm mb-4">
          AO STARTUP
          ============
        </pre>
        <p>Running startup checks...</p>
        <div className="animate-pulse mt-2">...</div>
      </div>
    );
  }

  if (appState === "startup_failed") {
    return (
      <div className="min-h-screen bg-black text-theme-text font-mono flex flex-col items-center justify-center p-6">
        <pre className="text-sm mb-4">
          AO STARTUP
          ============
        </pre>
        <p className="text-red-400 mb-4">{startupError}</p>
        <div className="flex gap-4">
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
  }

  // ── CHAT STATE ──
  return (
    <>
      <PersonaToggle />
      <div className={mode === "ego" ? "" : "hidden"}>
        <ChatInterface target="EGO" />
      </div>
      {mode === "id" && <IdentityPanel />}
      {/* Disconnect button */}
      <button
        className="fixed top-2 right-2 text-gray-600 hover:text-gray-400 text-xs font-mono z-50"
        onClick={handleDisconnect}
        title="Return to identity selector"
      >
        [disconnect]
      </button>
    </>
  );
}

function App() {
  return (
    <ThemeProvider initialMode="id">
      <AppInner />
    </ThemeProvider>
  );
}

export default App;
