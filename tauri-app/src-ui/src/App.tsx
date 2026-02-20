import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { ThemeProvider, useTheme } from "./contexts/ThemeContext";
import BootSequence from "./components/BootSequence";
import ChatInterface from "./components/ChatInterface";
import PersonaToggle from "./components/PersonaToggle";
import IdentityConflictPanel, { IdentitySummary } from "./components/IdentityConflictPanel";
import ManagementScreen from "./components/ManagementScreen";
import SplashScreen from "./components/SplashScreen";
import ForgeDrawer from "./components/ForgeDrawer";

type AppState =
  | "splash"
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

function assertNeverState(state: never): never {
  throw new Error(`Unhandled AppState: ${state}`);
}

function AppInner() {
  const [appState, setAppState] = useState<AppState>("splash");
  const [startupError, setStartupError] = useState<string | null>(null);
  const [existingIdentity, setExistingIdentity] = useState<IdentitySummary | null>(null);
  const [forgeOpen, setForgeOpen] = useState(false);
  const { setMode, refreshAgentName } = useTheme();

  const initializeApp = async () => {
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
    setForgeOpen(false);
    try {
      await invoke("disconnect_agent");
    } catch (e) {
      console.warn("[App] disconnect_agent failed; continuing to management:", e);
    }
    setMode("neutral");
    setAppState("management");
  };

  switch (appState) {
    case "splash":
      return <SplashScreen onComplete={handleSplashComplete} />;
    case "loading":
      return (
        <div className="min-h-screen bg-theme-bg text-theme-text-dim font-mono flex items-center justify-center">
          <div className="animate-pulse">Loading...</div>
        </div>
      );
    case "management":
      return (
        <ManagementScreen
          onAgentSelected={handleAgentSelected}
          onCreateAgent={handleCreateAgent}
        />
      );
    case "identity_conflict":
      if (!existingIdentity) {
        console.warn("[App] identity_conflict without identity; returning to management");
        return (
          <ManagementScreen
            onAgentSelected={handleAgentSelected}
            onCreateAgent={handleCreateAgent}
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
          <PersonaToggle
            onToggle={() => setForgeOpen((prev) => !prev)}
            forgeOpen={forgeOpen}
          />
          <ForgeDrawer
            open={forgeOpen}
            onClose={() => setForgeOpen(false)}
            onDisconnect={handleDisconnect}
          />
          <div className="flex-1 min-h-0">
            <ChatInterface target="EGO" />
          </div>
        </div>
      );
    default:
      return assertNeverState(appState);
  }
}

function App() {
  return (
    <ThemeProvider initialMode="neutral">
      <AppInner />
    </ThemeProvider>
  );
}

export default App;
