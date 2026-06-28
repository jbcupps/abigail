import { useCallback, useEffect, useState } from "react";
import SplashScreen from "./components/SplashScreen";
import LoaderScreen from "./components/LoaderScreen";
import CreateEntityCard from "./components/CreateEntityCard";
import ProviderWizard from "./components/ProviderWizard";
import ChatPanel from "./components/ChatPanel";
import { hiveHealth, getStatus, type HiveStatus } from "./lib/daemonClient";
import { showAppWindow } from "./lib/window";
import { openEntity } from "./lib/entityWindow";

type Phase = "splash" | "loading" | "ready" | "error";
type Readiness = "pending" | "ok" | "failed";

const READY_CEILING_MS = 20_000;

export default function App() {
  const [phase, setPhase] = useState<Phase>("splash");
  const [readiness, setReadiness] = useState<Readiness>("pending");
  const [status, setStatus] = useState<HiveStatus | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);

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
    void checkReadiness();
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

  const entities = status?.entities ?? [];
  const helperUrl = status?.helper?.running ? (status.helper.local_url ?? null) : null;
  const needsProvider = status?.ready_state === "needs_provider";

  return (
    <div className="theme-modern flex h-screen bg-theme-bg text-theme-text font-primary">
      <div className="flex-1 overflow-y-auto p-8">
        <header className="mb-6 flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold text-theme-text-bright">Abigail Hive</h1>
            <p className="text-theme-text-dim text-sm mt-1">
              Your family's private AI coordinator.
            </p>
          </div>
          <button
            type="button"
            onClick={() => setWizardOpen(true)}
            className="shrink-0 rounded-theme-md border border-theme-border px-3 py-2 text-sm text-theme-text hover:border-theme-primary"
          >
            Connect a model
          </button>
        </header>

        {needsProvider && (
          <div className="mb-6 rounded-theme-lg border border-theme-border bg-theme-warning-dim p-4">
            <p className="text-sm text-theme-text">
              Abigail is running on a local model. Connect a cloud model for more capability —
              your data still stays on this computer.
            </p>
            <button
              type="button"
              onClick={() => setWizardOpen(true)}
              className="mt-2 rounded-theme-md bg-theme-primary px-3 py-2 text-sm font-medium text-white"
            >
              Connect a model
            </button>
          </div>
        )}

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
            style={{ gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))" }}
          >
            <li>
              <CreateEntityCard onCreated={() => void refreshStatus()} />
            </li>
            {entities.map((entity) => (
              <li
                key={entity.id}
                className="flex flex-col rounded-theme-lg border border-theme-border bg-theme-surface p-4"
              >
                <div className="font-medium text-theme-text-bright">{entity.name}</div>
                <div className="text-xs text-theme-text-dim mt-1">
                  {entity.is_hive
                    ? "Hive coordinator"
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
          {openError && <p className="mt-3 text-xs text-theme-danger">{openError}</p>}
        </section>
      </div>

      {helperUrl && (
        <aside className="flex w-[360px] flex-col border-l border-theme-border bg-theme-bg-elevated">
          <div className="border-b border-theme-border px-4 py-3 text-sm font-medium text-theme-text-bright">
            Ask Abigail
          </div>
          <div className="min-h-0 flex-1">
            <ChatPanel
              baseUrl={helperUrl}
              greeting="Hi! I can help you set up Abigail, create Entities, or connect a model. What would you like to do?"
            />
          </div>
        </aside>
      )}

      {wizardOpen && (
        <ProviderWizard onClose={() => setWizardOpen(false)} onComplete={() => void refreshStatus()} />
      )}
    </div>
  );
}
