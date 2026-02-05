import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import BootSequence from "./components/BootSequence";
import ChatInterface from "./components/ChatInterface";

type AppState = "loading" | "boot" | "startup_check" | "startup_failed" | "chat";

interface StartupCheckResult {
  heartbeat_ok: boolean;
  verification_ok: boolean;
  error: string | null;
}

function App() {
  const [appState, setAppState] = useState<AppState>("loading");
  const [startupError, setStartupError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const complete = await invoke<boolean>("get_birth_complete");
        if (complete) {
          // Already born: run startup checks before showing chat
          setAppState("startup_check");
          await runStartupChecks();
        } else {
          // First run: show boot sequence
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

      // All checks passed
      setAppState("chat");
    } catch (e) {
      setStartupError(String(e));
      setAppState("startup_failed");
    }
  };

  const onBirthComplete = () => {
    // After birth, go directly to chat (startup checks already ran during boot)
    setAppState("chat");
  };

  const handleRetry = () => {
    setStartupError(null);
    setAppState("startup_check");
    runStartupChecks();
  };

  const handleContinueAnyway = () => {
    // Allow user to continue despite startup check failure (dev mode)
    setAppState("chat");
  };

  if (appState === "loading") {
    return (
      <div className="min-h-screen bg-black text-green-500 font-mono flex items-center justify-center">
        Loading...
      </div>
    );
  }

  if (appState === "boot") {
    return <BootSequence onComplete={onBirthComplete} />;
  }

  if (appState === "startup_check") {
    return (
      <div className="min-h-screen bg-black text-green-500 font-mono flex flex-col items-center justify-center">
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
      <div className="min-h-screen bg-black text-green-500 font-mono flex flex-col items-center justify-center p-6">
        <pre className="text-sm mb-4">
          AO STARTUP
          ============
        </pre>
        <p className="text-red-400 mb-4">{startupError}</p>
        <div className="flex gap-4">
          <button
            className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20"
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

  return <ChatInterface />;
}

export default App;
