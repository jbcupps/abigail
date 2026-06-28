import { invoke } from "@tauri-apps/api/core";
import { type FormEvent, type MouseEvent, useEffect, useRef, useState } from "react";
import ConfirmationModal from "./ConfirmationModal";
import OllamaDrawer from "./OllamaDrawer";
import ProviderDrawer from "./ProviderDrawer";
import ThemeDrawer from "./ThemeDrawer";

interface OllamaStatusInfo {
  managed: boolean;
  running: boolean;
  port: number;
  model_ready: boolean;
}

interface OllamaModelTag {
  name: string;
  size: number;
  modified_at: string;
}

interface SoulIdentityInfo {
  id: string;
  name: string;
  directory: string;
  birth_complete: boolean;
  birth_date: string | null;
  is_hive: boolean;
  immortal: boolean;
  primary_color?: string | null;
  avatar_url?: string | null;
}

interface BackupInfo {
  directory_name: string;
  directory_path: string;
  agent_name: string;
  backup_type: string;
  created_at: string;
  birth_complete: boolean;
  birth_date: string | null;
  has_memories: boolean;
  has_signatures: boolean;
}

type ConfirmAction =
  | { type: "delete"; soul: SoulIdentityInfo }
  | { type: "archive"; soul: SoulIdentityInfo }
  | { type: "delete_backup"; backup: BackupInfo };

interface SoulRegistryProps {
  onSoulSelected: (soulId: string) => void;
  onNewSoul: (soulId: string) => void;
}

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

const buttonVariants: Record<ButtonVariant, string> = {
  primary:
    "bg-theme-primary text-white hover:bg-theme-primary-muted disabled:hover:bg-theme-primary",
  secondary:
    "border border-theme-border bg-theme-bg-elevated text-theme-text hover:border-theme-primary hover:bg-theme-hover",
  ghost:
    "text-theme-text-dim hover:bg-theme-hover hover:text-theme-text",
  danger:
    "border border-theme-danger text-theme-danger hover:bg-theme-danger-dim",
};

function Button({
  children,
  className = "",
  disabled,
  variant = "secondary",
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  return (
    <button
      type="button"
      disabled={disabled}
      className={[
        "inline-flex min-h-9 items-center justify-center gap-2 rounded-theme-md px-3 py-2",
        "text-sm font-medium transition-colors duration-theme-fast",
        "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-theme-focus-ring",
        "disabled:cursor-not-allowed disabled:opacity-45",
        buttonVariants[variant],
        className,
      ].join(" ")}
      {...props}
    >
      {children}
    </button>
  );
}

function StatusPill({
  children,
  tone = "neutral",
}: {
  children: React.ReactNode;
  tone?: "neutral" | "success" | "warning" | "danger" | "primary";
}) {
  const tones = {
    neutral: "border-theme-border text-theme-text-dim bg-theme-surface-dim",
    success: "border-theme-success text-theme-success bg-theme-success-dim",
    warning: "border-theme-warning text-theme-warning bg-theme-warning-dim",
    danger: "border-theme-danger text-theme-danger bg-theme-danger-dim",
    primary: "border-theme-primary text-theme-primary bg-theme-primary-glow",
  };

  return (
    <span
      className={[
        "inline-flex min-h-6 items-center rounded-theme-sm border px-2 py-0.5",
        "text-[11px] font-semibold leading-4",
        tones[tone],
      ].join(" ")}
    >
      {children}
    </span>
  );
}

function safeErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim()) return error.message;
  const message = String(error || "").trim();
  return message || "Something went wrong. Try again.";
}

function formatBytes(bytes: number) {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  return `${bytes} B`;
}

function formatDate(dateStr: string | null) {
  if (!dateStr) return "Not recorded";
  try {
    return new Date(dateStr).toLocaleString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch {
    return dateStr;
  }
}

function modelMatchesActive(modelName: string, activeModel: string) {
  return modelName === activeModel || modelName.startsWith(activeModel + ":");
}

function HiveAgentPanel() {
  const [status, setStatus] = useState<OllamaStatusInfo | null>(null);
  const [models, setModels] = useState<OllamaModelTag[]>([]);
  const [activeModel, setActiveModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("http://localhost:11434");
  const [expanded, setExpanded] = useState(false);
  const [pullModel, setPullModel] = useState("");
  const [pulling, setPulling] = useState(false);
  const [pullStatus, setPullStatus] = useState("");

  const refreshStatus = async () => {
    try {
      const ollamaStatus = await invoke<OllamaStatusInfo>("get_ollama_status");
      setStatus(ollamaStatus);

      const config = await invoke<{ local_llm_base_url?: string; bundled_model?: string }>(
        "get_config_snapshot"
      );
      const computedBaseUrl = config.local_llm_base_url || `http://127.0.0.1:${ollamaStatus.port}`;
      setBaseUrl(computedBaseUrl);
      setActiveModel(config.bundled_model || "llama3.2:3b");

      if (ollamaStatus.running) {
        try {
          const response = await fetch(`${computedBaseUrl}/api/tags`);
          if (response.ok) {
            const data = await response.json();
            setModels(
              (data.models || []).map((model: Record<string, unknown>) => ({
                name: String(model.name || ""),
                size: Number(model.size || 0),
                modified_at: String(model.modified_at || ""),
              }))
            );
          }
        } catch {
          setModels([]);
        }
      }
    } catch {
      setStatus(null);
    }
  };

  useEffect(() => {
    void refreshStatus();
  }, []);

  const handlePullModel = async (event?: FormEvent) => {
    event?.preventDefault();
    const modelName = pullModel.trim();
    if (!modelName || pulling) return;

    setPulling(true);
    setPullStatus("Downloading model...");
    try {
      await invoke("pull_ollama_model", { model: modelName, baseUrl });
      setPullStatus("Model downloaded.");
      setPullModel("");
      await refreshStatus();
    } catch (error) {
      setPullStatus(`Model download failed. ${safeErrorMessage(error)}`);
    } finally {
      setPulling(false);
    }
  };

  const handleSwitchModel = async (modelName: string) => {
    try {
      await invoke("set_bundled_model", { modelName });
      setActiveModel(modelName);
    } catch (error) {
      setPullStatus(`Could not switch model. ${safeErrorMessage(error)}`);
    }
  };

  if (!status) {
    return (
      <section className="rounded-theme-lg border border-theme-border bg-theme-surface p-5">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold text-theme-text-bright">Hive model</h2>
            <p className="mt-1 text-sm text-theme-text-dim">
              Abigail can still use cloud providers while local model status is unavailable.
            </p>
          </div>
          <StatusPill tone="warning">Checking</StatusPill>
        </div>
      </section>
    );
  }

  const statusTone = status.running && status.model_ready ? "success" : status.running ? "warning" : "danger";
  const statusText = status.running && status.model_ready ? "Online" : status.running ? "Loading" : "Offline";

  return (
    <section className="rounded-theme-lg border border-theme-border bg-theme-surface p-5">
      <div className="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-lg font-semibold text-theme-text-bright">Hive model</h2>
            <StatusPill tone={statusTone}>{statusText}</StatusPill>
          </div>
          <p className="mt-1 text-sm text-theme-text-dim">
            Local model for the Hive control plane.
          </p>
          <p className="mt-2 font-mono text-xs text-theme-text">
            {activeModel || "No model selected"}
          </p>
        </div>
        <Button variant="secondary" onClick={() => setExpanded((value) => !value)}>
          {expanded ? "Hide model details" : "Manage local model"}
        </Button>
      </div>

      {expanded && (
        <div className="mt-5 grid gap-5 border-t border-theme-border-dim pt-5 lg:grid-cols-[1fr_320px]">
          <div>
            <h3 className="text-xs font-semibold uppercase tracking-wide text-theme-text-dim">
              Installed models
            </h3>
            {models.length === 0 ? (
              <p className="mt-3 text-sm text-theme-text-dim">No local models found.</p>
            ) : (
              <ul className="mt-3 grid gap-2">
                {models.map((model) => {
                  const active = modelMatchesActive(model.name, activeModel);
                  return (
                    <li
                      key={model.name}
                      className="flex flex-col gap-3 rounded-theme-md border border-theme-border-dim bg-theme-bg-inset p-3 sm:flex-row sm:items-center sm:justify-between"
                    >
                      <div className="min-w-0">
                        <div className="truncate font-mono text-sm text-theme-text-bright">
                          {model.name}
                        </div>
                        <div className="mt-1 text-xs text-theme-text-dim">
                          {formatBytes(model.size)}
                        </div>
                      </div>
                      {active ? (
                        <StatusPill tone="success">Active</StatusPill>
                      ) : (
                        <Button
                          variant="ghost"
                          className="self-start sm:self-auto"
                          onClick={() => void handleSwitchModel(model.name)}
                        >
                          Use model
                        </Button>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <form className="rounded-theme-md border border-theme-border-dim bg-theme-bg-inset p-4" onSubmit={handlePullModel}>
            <label htmlFor="pull-model" className="text-xs font-semibold text-theme-text">
              Download model
            </label>
            <p className="mt-1 text-xs text-theme-text-dim">
              Use an Ollama model name such as llama3.2:3b.
            </p>
            <input
              id="pull-model"
              type="text"
              value={pullModel}
              onChange={(event) => setPullModel(event.target.value)}
              placeholder="llama3.2:3b"
              className="mt-3 min-h-10 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 font-mono text-sm text-theme-text placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none"
              disabled={pulling}
            />
            <Button
              type="submit"
              variant="primary"
              className="mt-3 w-full"
              disabled={pulling || !pullModel.trim()}
              aria-busy={pulling}
            >
              {pulling ? "Downloading..." : "Download model"}
            </Button>
            {pullStatus && (
              <p
                className={[
                  "mt-3 text-xs",
                  pullStatus.includes("failed") || pullStatus.startsWith("Could not")
                    ? "text-theme-danger"
                    : "text-theme-text-dim",
                ].join(" ")}
                role={pullStatus.includes("failed") ? "alert" : "status"}
              >
                {pullStatus}
              </p>
            )}
          </form>
        </div>
      )}
    </section>
  );
}

export default function SoulRegistry({
  onSoulSelected,
  onNewSoul,
}: SoulRegistryProps) {
  const [souls, setSouls] = useState<SoulIdentityInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [birthing, setBirthing] = useState(false);
  const [newSoulName, setNewSoulName] = useState("");
  const [migrating, setMigrating] = useState(false);
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(null);
  const [confirmLoading, setConfirmLoading] = useState(false);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [showRecoverPanel, setShowRecoverPanel] = useState(false);
  const [restoring, setRestoring] = useState<string | null>(null);
  const [ollamaDrawerOpen, setOllamaDrawerOpen] = useState(false);
  const [providerDrawerOpen, setProviderDrawerOpen] = useState(false);
  const [themeDrawerOpen, setThemeDrawerOpen] = useState(false);
  const mountedRef = useRef(true);
  const birthInFlightRef = useRef(false);

  const fetchSouls = async () => {
    try {
      if (mountedRef.current) setLoading(true);
      const identities = await invoke<SoulIdentityInfo[]>("get_identities");
      if (!mountedRef.current) return;
      setSouls(identities);
      setError(null);
    } catch (fetchError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(fetchError));
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  };

  const fetchBackups = async () => {
    try {
      const list = await invoke<BackupInfo[]>("list_backups");
      if (!mountedRef.current) return;
      setBackups(list);
    } catch (backupError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(backupError));
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    void fetchSouls();
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (showRecoverPanel) void fetchBackups();
  }, [showRecoverPanel]);

  const handleBirthSoul = async (event?: FormEvent) => {
    event?.preventDefault();
    const name = newSoulName.trim();
    if (!name || birthInFlightRef.current) return;

    birthInFlightRef.current = true;
    setBirthing(true);
    setError(null);
    try {
      const uuid = await invoke<string>("create_agent", { name });
      if (!mountedRef.current) return;
      setNewSoulName("");
      await invoke("load_agent", { agentId: uuid });
      if (!mountedRef.current) return;
      onNewSoul(uuid);
    } catch (birthError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(birthError));
    } finally {
      if (mountedRef.current) setBirthing(false);
      birthInFlightRef.current = false;
    }
  };

  const handleWakeSoul = async (soulId: string) => {
    try {
      await invoke("load_agent", { agentId: soulId });
      if (!mountedRef.current) return;
      onSoulSelected(soulId);
    } catch (wakeError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(wakeError));
    }
  };

  const handleBackupSoul = async (event: MouseEvent, soul: SoulIdentityInfo) => {
    event.stopPropagation();
    try {
      await invoke<string>("backup_agent_identity", { agentId: soul.id });
      if (!mountedRef.current) return;
      setError(null);
    } catch (backupError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(backupError));
    }
  };

  const handleConfirm = async () => {
    if (!confirmAction) return;
    setConfirmLoading(true);
    try {
      if (confirmAction.type === "delete") {
        await invoke("delete_agent_identity", { agentId: confirmAction.soul.id });
        await fetchSouls();
      } else if (confirmAction.type === "archive") {
        await invoke("archive_agent_identity", { agentId: confirmAction.soul.id });
        await fetchSouls();
      } else {
        await invoke("delete_backup", { backupDirName: confirmAction.backup.directory_name });
        await fetchBackups();
      }
    } catch (confirmError) {
      setError(safeErrorMessage(confirmError));
    } finally {
      setConfirmLoading(false);
      setConfirmAction(null);
    }
  };

  const handleRestoreBackup = async (backup: BackupInfo) => {
    setRestoring(backup.directory_name);
    try {
      await invoke<string>("restore_from_backup", { backupDirName: backup.directory_name });
      if (!mountedRef.current) return;
      await fetchSouls();
      await fetchBackups();
    } catch (restoreError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(restoreError));
    } finally {
      if (mountedRef.current) setRestoring(null);
    }
  };

  const handleMigrateLegacy = async () => {
    setMigrating(true);
    setError(null);
    try {
      const uuid = await invoke<string | null>("migrate_legacy_identity");
      if (uuid) await fetchSouls();
    } catch (migrationError) {
      if (!mountedRef.current) return;
      setError(safeErrorMessage(migrationError));
    } finally {
      if (mountedRef.current) setMigrating(false);
    }
  };

  const toggleRecoverPanel = () => setShowRecoverPanel((value) => !value);

  const confirmModal = confirmAction && (
    <ConfirmationModal
      title={
        confirmAction.type === "delete"
          ? `Delete "${confirmAction.soul.name}"?`
          : confirmAction.type === "archive"
            ? `Archive "${confirmAction.soul.name}"?`
            : `Delete backup "${confirmAction.backup.agent_name}"?`
      }
      message={
        confirmAction.type === "delete"
          ? "This permanently removes local data, memories, keys, and documents. This cannot be undone."
          : confirmAction.type === "archive"
            ? "This Entity will move to backups and leave the active list. You can restore it later."
            : "This backup will be permanently deleted. This cannot be undone."
      }
      detail={confirmAction.type === "delete" ? "Use Archive if you want to preserve the data." : undefined}
      confirmLabel={
        confirmAction.type === "delete" || confirmAction.type === "delete_backup"
          ? "Delete permanently"
          : "Archive"
      }
      variant={
        confirmAction.type === "delete" || confirmAction.type === "delete_backup"
          ? "danger"
          : "warning"
      }
      onConfirm={handleConfirm}
      onCancel={() => setConfirmAction(null)}
      loading={confirmLoading}
    />
  );

  const drawers = (
    <>
      {ollamaDrawerOpen && <OllamaDrawer onClose={() => setOllamaDrawerOpen(false)} />}
      {providerDrawerOpen && <ProviderDrawer onClose={() => setProviderDrawerOpen(false)} />}
      {themeDrawerOpen && <ThemeDrawer onClose={() => setThemeDrawerOpen(false)} />}
    </>
  );

  if (loading) {
    return (
      <main className="min-h-screen bg-theme-bg px-6 py-8 text-theme-text font-primary">
        <div className="mx-auto grid min-h-[70vh] max-w-5xl place-items-center">
          <div className="w-full max-w-md rounded-theme-lg border border-theme-border bg-theme-surface p-6">
            <div className="h-4 w-32 animate-pulse rounded bg-theme-surface-bright" />
            <div className="mt-4 h-16 animate-pulse rounded-theme-md bg-theme-surface-dim" />
            <p className="mt-4 text-sm text-theme-text-dim" role="status">
              Loading Abigail Hive...
            </p>
          </div>
        </div>
      </main>
    );
  }

  const hiveSoul = souls.find((soul) => soul.is_hive);
  const familyEntities = souls.filter((soul) => !soul.is_hive);

  return (
    <main className="min-h-screen bg-theme-bg px-4 py-6 text-theme-text font-primary sm:px-6 lg:px-8">
      {confirmModal}
      {drawers}

      <div className="mx-auto flex w-full max-w-7xl flex-col gap-6">
        <header className="flex flex-col gap-4 rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-5 shadow-theme-elevated lg:flex-row lg:items-start lg:justify-between">
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="text-2xl font-semibold leading-8 text-theme-text-bright">
                Abigail Hive
              </h1>
              <StatusPill tone="primary">Private coordinator</StatusPill>
            </div>
            <p className="mt-2 max-w-2xl text-sm leading-[22px] text-theme-text-dim">
              Create Entities your family can talk to, connect models, and keep local memory under
              Hive control.
            </p>
          </div>

          <nav className="flex flex-wrap gap-2" aria-label="Hive tools">
            <Button variant="secondary" onClick={() => setOllamaDrawerOpen(true)}>
              Manage Ollama
            </Button>
            <Button variant="secondary" onClick={() => setProviderDrawerOpen(true)}>
              Connect models
            </Button>
            <Button variant="secondary" onClick={() => setThemeDrawerOpen(true)}>
              Theme
            </Button>
          </nav>
        </header>

        {error && (
          <div
            className="flex flex-col gap-3 rounded-theme-lg border border-theme-danger bg-theme-danger-dim p-4 text-sm text-theme-danger sm:flex-row sm:items-center sm:justify-between"
            role="alert"
          >
            <span>{error}</span>
            <Button variant="ghost" className="self-start text-theme-danger sm:self-auto" onClick={() => setError(null)}>
              Dismiss
            </Button>
          </div>
        )}

        <section className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">
          <HiveAgentPanel />

          <form
            className="rounded-theme-lg border border-theme-border bg-theme-surface p-5"
            onSubmit={handleBirthSoul}
          >
            <h2 className="text-lg font-semibold text-theme-text-bright">Create Entity</h2>
            <p className="mt-1 text-sm text-theme-text-dim">
              Use the name your family will say naturally.
            </p>
            <label htmlFor="new-entity-name" className="mt-4 block text-xs font-semibold text-theme-text">
              Entity name
            </label>
            <input
              id="new-entity-name"
              type="text"
              value={newSoulName}
              onChange={(event) => setNewSoulName(event.target.value)}
              placeholder="Ada"
              className="mt-2 min-h-10 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none"
              disabled={birthing}
              autoFocus={souls.length === 0}
            />
            <Button
              type="submit"
              variant="primary"
              className="mt-4 w-full"
              disabled={birthing || !newSoulName.trim()}
              aria-busy={birthing}
            >
              {birthing ? "Creating..." : "Create Entity"}
            </Button>
            <div className="mt-4 flex flex-wrap gap-2">
              <Button variant="ghost" onClick={toggleRecoverPanel}>
                {showRecoverPanel ? "Hide backups" : "Restore backup"}
              </Button>
              <Button variant="ghost" onClick={() => void handleMigrateLegacy()} disabled={migrating}>
                {migrating ? "Checking..." : "Recall legacy identity"}
              </Button>
            </div>
          </form>
        </section>

        <section className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">
          <div>
            <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
              <div>
                <h2 className="text-xs font-semibold uppercase tracking-wide text-theme-text-dim">
                  Entities ({familyEntities.length})
                </h2>
                <p className="mt-1 text-sm text-theme-text-dim">
                  Open the Entity your family wants to talk to.
                </p>
              </div>
              <Button variant="ghost" onClick={() => void fetchSouls()}>
                Refresh
              </Button>
            </div>

            {familyEntities.length === 0 ? (
              <div className="rounded-theme-lg border border-dashed border-theme-border bg-theme-surface-dim p-8 text-center">
                <h3 className="text-lg font-semibold text-theme-text-bright">No Entities yet</h3>
                <p className="mx-auto mt-2 max-w-md text-sm text-theme-text-dim">
                  Create the first Entity your family will talk to.
                </p>
              </div>
            ) : (
              <ul
                className="grid gap-4"
                style={{ gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))" }}
              >
                {familyEntities.map((soul) => (
                  <li
                    key={soul.id}
                    className="relative overflow-hidden rounded-theme-lg border border-theme-border bg-theme-surface p-5"
                  >
                    <div
                      className="absolute inset-y-0 left-0 w-1 bg-theme-primary"
                      style={soul.primary_color ? { backgroundColor: soul.primary_color } : undefined}
                    />
                    <div className="flex items-start gap-4">
                      <div
                        className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-full border border-theme-border bg-theme-bg-inset text-lg font-semibold text-theme-primary"
                        style={
                          soul.primary_color
                            ? { borderColor: soul.primary_color, color: soul.primary_color }
                            : undefined
                        }
                      >
                        {soul.avatar_url ? (
                          <img src={soul.avatar_url} alt="" className="size-full object-cover" />
                        ) : (
                          soul.name.substring(0, 1).toUpperCase()
                        )}
                      </div>

                      <div className="min-w-0 flex-1">
                        <h3 className="truncate text-lg font-semibold text-theme-text-bright">
                          {soul.name}
                        </h3>
                        <div className="mt-2 flex flex-wrap gap-2">
                          <StatusPill tone={soul.birth_complete ? "success" : "warning"}>
                            {soul.birth_complete ? "Ready" : "New"}
                          </StatusPill>
                          {soul.immortal && <StatusPill tone="primary">Hive protected</StatusPill>}
                        </div>
                        <p className="mt-3 font-mono text-xs text-theme-text-dim">
                          ID: {soul.id.substring(0, 8)}
                        </p>
                        {soul.birth_date && (
                          <p className="mt-1 text-xs text-theme-text-dim">
                            Created {formatDate(soul.birth_date)}
                          </p>
                        )}
                      </div>
                    </div>

                    <div className="mt-5 flex flex-wrap gap-2 border-t border-theme-border-dim pt-4">
                      <Button variant="primary" onClick={() => void handleWakeSoul(soul.id)}>
                        Open
                      </Button>
                      <Button variant="secondary" onClick={(event) => void handleBackupSoul(event, soul)}>
                        Back up
                      </Button>
                      <Button
                        variant="ghost"
                        onClick={() => setConfirmAction({ type: "archive", soul })}
                        disabled={soul.immortal}
                      >
                        Archive
                      </Button>
                      <Button
                        variant="danger"
                        onClick={() => setConfirmAction({ type: "delete", soul })}
                        disabled={soul.immortal}
                      >
                        Delete
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <aside className="rounded-theme-lg border border-theme-border bg-theme-surface p-5">
            <h2 className="text-lg font-semibold text-theme-text-bright">Hive status</h2>
            {hiveSoul ? (
              <div className="mt-4 grid gap-3 text-sm">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-theme-text-dim">Coordinator</span>
                  <span className="font-medium text-theme-text-bright">{hiveSoul.name}</span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span className="text-theme-text-dim">Protection</span>
                  <StatusPill tone={hiveSoul.immortal ? "primary" : "warning"}>
                    {hiveSoul.immortal ? "Immortal" : "Standard"}
                  </StatusPill>
                </div>
                <div>
                  <span className="text-theme-text-dim">Hive ID</span>
                  <p className="mt-1 break-all font-mono text-xs text-theme-text">
                    {hiveSoul.id}
                  </p>
                </div>
              </div>
            ) : (
              <p className="mt-3 text-sm text-theme-text-dim">
                The Hive identity will appear here after initialization.
              </p>
            )}
          </aside>
        </section>

        {showRecoverPanel && (
          <section className="rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-5">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div>
                <h2 className="text-lg font-semibold text-theme-text-bright">Backups</h2>
                <p className="mt-1 text-sm text-theme-text-dim">
                  Restore an archived Entity or remove old backups.
                </p>
              </div>
              <Button variant="ghost" onClick={toggleRecoverPanel}>Close</Button>
            </div>

            {backups.length === 0 ? (
              <p className="mt-5 rounded-theme-md border border-theme-border-dim bg-theme-bg-inset p-4 text-sm text-theme-text-dim">
                No backups found.
              </p>
            ) : (
              <ul className="mt-5 grid gap-3">
                {backups.map((backup) => (
                  <li
                    key={backup.directory_name}
                    className="flex flex-col gap-3 rounded-theme-md border border-theme-border-dim bg-theme-bg-inset p-4 md:flex-row md:items-center md:justify-between"
                  >
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="truncate text-sm font-semibold text-theme-text-bright">
                          {backup.agent_name}
                        </h3>
                        <StatusPill tone={backup.backup_type === "archive" ? "warning" : "success"}>
                          {backup.backup_type === "archive" ? "Archived" : "Backup"}
                        </StatusPill>
                      </div>
                      <p className="mt-1 text-xs text-theme-text-dim">
                        Created {formatDate(backup.created_at)}
                      </p>
                      <p className="mt-1 text-xs text-theme-text-dim">
                        {backup.has_memories ? "Includes memories" : "No memories recorded"}
                        {" / "}
                        {backup.has_signatures ? "Signed" : "Unsigned"}
                      </p>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="secondary"
                        onClick={() => void handleRestoreBackup(backup)}
                        disabled={restoring === backup.directory_name}
                      >
                        {restoring === backup.directory_name ? "Restoring..." : "Restore"}
                      </Button>
                      <Button
                        variant="danger"
                        onClick={() => setConfirmAction({ type: "delete_backup", backup })}
                      >
                        Delete
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
      </div>
    </main>
  );
}
