import { useCallback, useEffect, useRef, useState } from "react";
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
  const mounted = useRef(true);

  const refreshStatus = useCallback(async () => {
    try {
      const next = await getStatus();
      if (mounted.current) setStatus(next);
    } catch {
      // Keep the prior snapshot on a transient failure.
    }
  }, []);

  const checkReadiness = useCallback(async () => {
    setReadiness("pending");
    const deadline = Date.now() + READY_CEILING_MS;
    while (mounted.current && Date.now() < deadline) {
      if (await hiveHealth()) {
        try {
          const next = await getStatus();
          if (!mounted.current) return;
          setStatus(next);
          setReadiness("ok");
          return;
        } catch {
          // A live HTTP listener is not ready until its status can be read.
        }
      }
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    if (mounted.current) setReadiness("failed");
  }, []);

  // Reveal the window with the splash already painted, then begin readiness.
  useEffect(() => {
    mounted.current = true;
    void showAppWindow();
    if (!preview) void checkReadiness();
    return () => { mounted.current = false; };
  }, [checkReadiness, preview]);

  // The coordinator starts in the background; keep its live connection visible
  // when it becomes ready or restarts on a different port.
  useEffect(() => {
    if (preview || phase !== "ready") return;
    const timer = setInterval(() => void refreshStatus(), 5_000);
    return () => clearInterval(timer);
  }, [phase, preview, refreshStatus]);

  const closeWizard = useCallback(() => setWizardOpen(false), []);

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
      <LoaderScreen
        error
        message="The Hive isn't responding yet."
        onRetry={() => {
          setPhase("loading");
          void checkReadiness();
        }}
      />
    );
  }

  return <>
    <PresenceHome preview={preview} status={status} onRefresh={() => { if (!preview) void refreshStatus(); }} onProviders={() => setWizardOpen(true)} />
    {wizardOpen && <ProviderWizard onClose={closeWizard} onComplete={refreshStatus} />}
  </>;
}
