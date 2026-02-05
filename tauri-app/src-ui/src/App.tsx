import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { ThemeProvider, useTheme } from "./contexts/ThemeContext";
import BootSequence from "./components/BootSequence";
import ChatInterface from "./components/ChatInterface";
import PersonaToggle from "./components/PersonaToggle";
import IdentityPanel from "./components/IdentityPanel";

type AppState = "loading" | "boot" | "startup_check" | "startup_failed" | "chat";

interface StartupCheckResult {
  heartbeat_ok: boolean;
  verification_ok: boolean;
  error: string | null;
}

function AppInner() {
  const [appState, setAppState] = useState<AppState>("loading");
  const [startupError, setStartupError] = useState<string | null>(null);
  const { mode, setMode, refreshAgentName } = useTheme();

  useEffect(() => {
    (async () => {
      try {
        const complete = await invoke<boolean>("get_birth_complete");
        if (complete) {
          // Already born: run startup checks before showing chat
          setAppState("startup_check");
          await refreshAgentName();
          await runStartupChecks();
        } else {
          // First run: show boot sequence (handles its own LLM setup)
          setAppState("boot");
        }
      } catch {
        setAppState("boot");
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

      // All checks passed — switch to ego mode for chat
      setMode("ego");
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

  if (appState === "loading") {
    return (
      <div className="min-h-screen bg-black text-theme-text font-mono flex items-center justify-center">
        Loading...
      </div>
    );
  }

  if (appState === "boot") {
    return <BootSequence onComplete={onBirthComplete} />;
  }

  if (appState === "startup_check") {
    return (
      <div className="min-h-screen bg-black text-theme-text font-mono flex flex-col items-center justify-center">
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
