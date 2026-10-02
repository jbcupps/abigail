import { useCallback, useEffect, useRef, useState } from "react";
import SplashScreen from "./components/SplashScreen";
import LoaderScreen from "./components/LoaderScreen";
import CreateEntityCard from "./components/CreateEntityCard";
import ProviderWizard from "./components/ProviderWizard";
import ChatPanel from "./components/ChatPanel";
import { completeEntitySetup, hiveHealth, getStatus, type EntityInfo, type HiveStatus } from "./lib/daemonClient";
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
  const [openingId, setOpeningId] = useState<string | null>(null);
  const openingRef = useRef(false);
  const mounted = useRef(true);

  const handleOpen = useCallback(async (entity: EntityInfo) => {
    if (openingRef.current) return;
    openingRef.current = true;
    setOpeningId(entity.id);
    setOpenError(null);
    try {
      if (!entity.birth_complete) await completeEntitySetup(entity.id);
      await openEntity(entity.id);
      const next = await getStatus();
      if (mounted.current) setStatus(next);
    } catch (err) {
      if (mounted.current) setOpenError(err instanceof Error ? err.message : "The Entity could not be opened. Try again.");
    } finally {
      openingRef.current = false;
      if (mounted.current) setOpeningId(null);
    }
  }, []);

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
    void checkReadiness();
    return () => { mounted.current = false; };
  }, [checkReadiness]);

  // The coordinator starts in the background; keep its live connection visible
  // when it becomes ready or restarts on a different port.
  useEffect(() => {
    if (phase !== "ready") return;
    const timer = setInterval(() => void refreshStatus(), 5_000);
    return () => clearInterval(timer);
  }, [phase, refreshStatus]);

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

  const entities = status?.entities ?? [];
  const familyEntities = entities.filter((entity) => !entity.is_hive);
  const helperUrl = status?.helper?.running ? (status.helper.local_url ?? null) : null;
  const needsProvider = status?.ready_state === "needs_provider";

  return (
    <div className="theme-modern flex h-screen bg-theme-bg text-theme-text font-primary">
      <div className="min-w-0 flex-1 overflow-y-auto p-8">
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
              Connect a local or cloud model before opening an Entity. Abigail stores memory on this computer;
              cloud models process messages with the provider you choose.
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
              Entities ({familyEntities.length})
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
            {familyEntities.map((entity) => (
              <li
                key={entity.id}
                className="flex flex-col rounded-theme-lg border border-theme-border bg-theme-surface p-4"
              >
                <div className="font-medium text-theme-text-bright">{entity.name}</div>
                <div className="text-xs text-theme-text-dim mt-1">
                  {entity.birth_complete
                    ? "Ready"
                    : "Setup pending"}
                </div>
                <button
                  type="button"
                  onClick={() => void handleOpen(entity)}
                  disabled={needsProvider || openingId !== null}
                  className="mt-3 self-start rounded-theme-md bg-theme-primary px-3 py-1.5 text-xs font-medium text-white disabled:opacity-40"
                >
                  {openingId === entity.id ? "Opening…" : `Open ${entity.name}`}
                </button>
              </li>
            ))}
          </ul>
          {openError && <p role="alert" className="mt-3 text-xs text-theme-danger">{openError}</p>}
        </section>
      </div>

      {helperUrl && !needsProvider && (
        <aside className="flex w-[360px] flex-col border-l border-theme-border bg-theme-bg-elevated">
          <div className="border-b border-theme-border px-4 py-3 text-sm font-medium text-theme-text-bright">
            Ask Abigail
          </div>
          <div className="min-h-0 flex-1">
            <ChatPanel
              baseUrl={helperUrl}
              showHeader={false}
              greeting="Hi! I can help you set up Abigail, create Entities, or connect a model. What would you like to do?"
            />
          </div>
        </aside>
      )}

      {wizardOpen && (
        <ProviderWizard onClose={closeWizard} onComplete={refreshStatus} />
      )}
    </div>
  );
}
