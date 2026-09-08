import { useCallback, useEffect, useRef, useState } from "react";
import SplashScreen from "./components/SplashScreen";
import LoaderScreen from "./components/LoaderScreen";
import CreateEntityCard from "./components/CreateEntityCard";
import ProviderWizard from "./components/ProviderWizard";
import SetupChat from "./components/SetupChat";
import SetupGuide from "./components/SetupGuide";
import {
  getSetupStatus,
  retrySetup,
  cancelSetup,
  getStatus,
  type SetupStatus,
  type HiveStatus,
} from "./lib/daemonClient";
import { showAppWindow } from "./lib/window";
import { openEntity } from "./lib/entityWindow";

type Phase = "splash" | "loading" | "ready" | "error";
type Readiness = "pending" | "ok" | "failed";

export default function App() {
  const readinessRun = useRef(0);
  const wizardOpener = useRef<HTMLElement | null>(null);
  const [phase, setPhase] = useState<Phase>("splash");
  const [readiness, setReadiness] = useState<Readiness>("pending");
  const [status, setStatus] = useState<HiveStatus | null>(null);
  const [setup, setSetup] = useState<SetupStatus | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);

  const openWizard = () => {
    wizardOpener.current = document.activeElement as HTMLElement | null;
    setWizardOpen(true);
  };
  const closeWizard = () => {
    setWizardOpen(false);
    requestAnimationFrame(() => wizardOpener.current?.focus());
  };

  const handleOpen = useCallback(async (entityId: string) => {
    setOpenError(null);
    try {
      await openEntity(entityId);
    } catch (err) {
      setOpenError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await getStatus());
    } catch {
      // Keep the prior snapshot on a transient failure.
    }
  }, []);

  const checkReadiness = useCallback(async () => {
    const run = ++readinessRun.current;
    setReadiness("pending");
    const deadline = Date.now() + 240_000;
    while (Date.now() < deadline && readinessRun.current === run) {
      try {
        const current = await getSetupStatus();
        if (readinessRun.current !== run) return;
        setSetup(current);
        if (current.phase === "ready") {
          await refreshStatus();
          setReadiness("ok");
          return;
        }
        if (["error", "cancelled"].includes(current.phase)) {
          setReadiness("failed");
          return;
        }
      } catch {
        /* The native shell may still be starting its authenticated daemon. */
      }
      await new Promise((resolve) => setTimeout(resolve, 750));
    }
    if (readinessRun.current !== run) return;
    setSetup(
      (prior) =>
        prior ?? {
          phase: "error",
          message:
            "Abigail could not start. Try again; if this continues, reinstall the full app.",
          model: "",
          active_provider: "local",
          active_model: "",
        },
    );
    setReadiness("failed");
  }, [refreshStatus]);

  // Reveal the window with the splash already painted, then begin readiness.
  useEffect(() => {
    void showAppWindow();
    void checkReadiness();
    return () => {
      readinessRun.current++;
    };
  }, [checkReadiness]);

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
  if (phase === "loading")
    return (
      <LoaderScreen
        message={setup?.message ?? "Starting Abigail..."}
        onCancel={() => void cancelSetup()}
      />
    );
  if (phase === "error") {
    return (
      <LoaderScreen
        error
        message={setup?.message ?? "Abigail is not responding yet."}
        onRetry={() => {
          setPhase("loading");
          void retrySetup()
            .catch(() => undefined)
            .then(checkReadiness);
        }}
      />
    );
  }

  const entities = (status?.entities ?? []).filter((entity) => !entity.is_hive);

  return (
    <div className="theme-modern flex h-screen bg-theme-bg text-theme-text font-primary">
      <div
        inert={wizardOpen}
        className={`flex-1 p-8 ${wizardOpen ? "overflow-hidden" : "overflow-y-auto"}`}
      >
        <header className="mb-6 flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold text-theme-text-bright">
              Abigail
            </h1>
            <p className="text-theme-text-dim text-sm mt-1">
              Your local guide to getting started.
            </p>
          </div>
          <button
            type="button"
            onClick={openWizard}
            className="shrink-0 rounded-theme-md border border-theme-border px-3 py-2 text-sm text-theme-text hover:border-theme-primary"
          >
            Connect a model
          </button>
        </header>

        <div className="mb-8 grid gap-6 lg:grid-cols-[minmax(0,1fr)_360px]">
          <div className="h-[68vh] min-h-[440px] overflow-hidden rounded-theme-lg border border-theme-border">
            <SetupChat
              onConnect={openWizard}
              setup={setup}
              onStatus={setSetup}
            />
          </div>
          <SetupGuide setup={setup} onConnect={openWizard} />
        </div>
        <section>
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-xs uppercase tracking-wide text-theme-text-dim">
              Entities ({entities.length})
            </h2>
            <button
              type="button"
              className="text-xs text-theme-primary hover:underline"
              onClick={() => void refreshStatus()}
            >
              Refresh
            </button>
          </div>
          <ul
            className="grid gap-3"
            style={{
              gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
            }}
          >
            <li>
              <CreateEntityCard onCreated={() => void refreshStatus()} />
            </li>
            {entities.map((entity) => (
              <li
                key={entity.id}
                className="flex flex-col rounded-theme-lg border border-theme-border bg-theme-surface p-4"
              >
                <div className="font-medium text-theme-text-bright">
                  {entity.name}
                </div>
                <div className="text-xs text-theme-text-dim mt-1">
                  {entity.is_hive
                    ? "Abigail"
                    : entity.birth_complete
                      ? "Ready"
                      : "New"}
                </div>
                {!entity.is_hive && (
                  <button
                    type="button"
                    onClick={() => void handleOpen(entity.id)}
                    className="mt-3 self-start rounded-theme-md bg-theme-primary px-3 py-1.5 text-xs font-medium text-white"
                  >
                    Open
                  </button>
                )}
              </li>
            ))}
          </ul>
          {openError && (
            <p className="mt-3 text-xs text-theme-danger">{openError}</p>
          )}
        </section>
      </div>

      {wizardOpen && (
        <ProviderWizard
          onClose={closeWizard}
          onComplete={(status) => {
            setSetup(status);
            void refreshStatus();
          }}
        />
      )}
    </div>
  );
}
