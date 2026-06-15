import { useCallback, useEffect, useState } from "react";
import SplashScreen from "./components/SplashScreen";
import LoaderScreen from "./components/LoaderScreen";
import ChatPanel from "./components/ChatPanel";
import { entityHealth } from "./lib/daemonClient";
import { resolveEntityUrl } from "./lib/connection";
import { showAppWindow } from "./lib/window";

type Phase = "splash" | "loading" | "ready" | "error";
type Readiness = "pending" | "ok" | "failed";

const READY_CEILING_MS = 20_000;

export default function App() {
  const [phase, setPhase] = useState<Phase>("splash");
  const [readiness, setReadiness] = useState<Readiness>("pending");
  const [baseUrl, setBaseUrl] = useState<string>("");

  const checkReadiness = useCallback(async () => {
    setReadiness("pending");
    setBaseUrl(await resolveEntityUrl());
    const deadline = Date.now() + READY_CEILING_MS;
    while (Date.now() < deadline) {
      if (await entityHealth()) {
        setReadiness("ok");
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    setReadiness("failed");
  }, []);

  // Reveal the window with the splash already painted, then begin readiness.
  useEffect(() => {
    void showAppWindow();
    void checkReadiness();
  }, [checkReadiness]);

  // Once past the splash, follow readiness into chat or the retry screen.
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
  if (phase === "loading") return <LoaderScreen message="Waking up your Entity…" />;
  if (phase === "error") {
    return (
      <LoaderScreen
        error
        message="Your Entity isn't responding yet."
        onRetry={() => {
          setPhase("loading");
          void checkReadiness();
        }}
      />
    );
  }

  return (
    <div className="h-screen">
      <ChatPanel baseUrl={baseUrl} />
    </div>
  );
}
