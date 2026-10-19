import { useCallback, useEffect, useState } from "react";
import SplashScreen from "./components/SplashScreen";
import LoaderScreen from "./components/LoaderScreen";
import PresenceHome from "./components/PresenceHome";
import ProviderWizard from "./components/ProviderWizard";
import { hiveHealth, getStatus, type HiveStatus } from "./lib/daemonClient";
import { showAppWindow } from "./lib/window";

type Phase = "splash" | "loading" | "ready" | "error";
type Readiness = "pending" | "ok" | "failed";

const READY_CEILING_MS = 20_000;

export default function App() {
  const preview = new URLSearchParams(window.location.search).get("presencePreview") === "1";
  const [phase, setPhase] = useState<Phase>(preview ? "ready" : "splash");
  const [readiness, setReadiness] = useState<Readiness>("pending");
  const [status, setStatus] = useState<HiveStatus | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await getStatus());
    } catch {
      // Keep the prior snapshot on a transient failure.
    }
  }, []);

  const checkReadiness = useCallback(async () => {
    setReadiness("pending");
    const deadline = Date.now() + READY_CEILING_MS;
    while (Date.now() < deadline) {
      if (await hiveHealth()) {
        await refreshStatus();
        setReadiness("ok");
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    setReadiness("failed");
  }, [refreshStatus]);

  // Reveal the window with the splash already painted, then begin readiness.
  useEffect(() => {
    void showAppWindow();
    if (!preview) void checkReadiness();
  }, [checkReadiness, preview]);

  useEffect(() => {
    if (phase !== "loading") return;
    if (readiness === "ok") setPhase("ready");
    else if (readiness === "failed") setPhase("error");
  }, [phase, readiness]);

  const onSplashComplete = useCallback(() => {
    if (readiness === "ok") setPhase("ready");
    else if (readiness === "failed") setPhase("error");
    else setPhase("loading");
  }, [readiness]);

  if (phase === "splash") return <SplashScreen onComplete={onSplashComplete} />;
  if (phase === "loading") return <LoaderScreen message="Starting the Hive…" />;
  if (phase === "error") {
    return (
      <div><LoaderScreen
        error
        message="The Hive isn't responding yet."
        onRetry={() => {
          setPhase("loading");
          void checkReadiness();
        }}
      /><a href="?presencePreview=1" style={{ position: "fixed", bottom: 30, left: 30, color: "#c6d6fa", zIndex: 10000 }}>Explore the local design preview</a></div>
    );
  }

  return <>
    <PresenceHome preview={preview} status={status} onRefresh={() => { if (!preview) void refreshStatus(); }} onProviders={() => setWizardOpen(true)} />
    {wizardOpen && <ProviderWizard onClose={() => setWizardOpen(false)} onComplete={() => void refreshStatus()} />}
  </>;
}
