import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import HelpTooltip from "./HelpTooltip";
import { OLLAMA_HELP } from "./providerHelp";
import type {
  OllamaDetection,
  RecommendedModel,
  InstalledModel,
  OllamaInstallProgress,
  OllamaModelProgress,
} from "../types/llm";
import { formatBytes } from "../types/llm";

interface OllamaDrawerProps {
  onClose: () => void;
}

export default function OllamaDrawer({ onClose }: OllamaDrawerProps) {
  const [ollama, setOllama] = useState<OllamaDetection | null>(null);
  const [installedModels, setInstalledModels] = useState<InstalledModel[]>([]);
  const [recommendedModels, setRecommendedModels] = useState<RecommendedModel[]>([]);
  const [activeModel, setActiveModel] = useState("");
  const [installing, setInstalling] = useState(false);
  const [installProgress, setInstallProgress] = useState<OllamaInstallProgress | null>(null);
  const [pullingModel, setPullingModel] = useState("");
  const [modelProgress, setModelProgress] = useState<OllamaModelProgress | null>(null);
  const [customModel, setCustomModel] = useState("");
  const [error, setError] = useState("");
  const [starting, setStarting] = useState(false);

  const pulling = pullingModel !== "";

  // Event listeners for progress
  useEffect(() => {
    let unlistenInstall: (() => void) | null = null;
    let unlistenModel: (() => void) | null = null;
    let cancelled = false;

    (async () => {
      try {
        const fn = await listen<OllamaInstallProgress>("ollama-install-progress", (event) => {
          if (!cancelled) setInstallProgress(event.payload);
        });
        if (cancelled) { fn(); } else { unlistenInstall = fn; }
      } catch { /* listener setup failed */ }

      try {
        const fn = await listen<OllamaModelProgress>("ollama-model-progress", (event) => {
          if (!cancelled) setModelProgress(event.payload);
        });
        if (cancelled) { fn(); } else { unlistenModel = fn; }
      } catch { /* listener setup failed */ }
    })();

    return () => {
      cancelled = true;
      if (unlistenInstall) unlistenInstall();
      if (unlistenModel) unlistenModel();
    };
  }, []);

  // Fetch status on mount (component is conditionally rendered)
  useEffect(() => {
    refreshAll();
  }, []);

  const refreshAll = async () => {
    setError("");
    try {
      const [detection, recommended, config] = await Promise.all([
        invoke<OllamaDetection>("detect_ollama"),
        invoke<RecommendedModel[]>("list_recommended_models"),
        invoke<{ bundled_model?: string }>("get_config_snapshot"),
      ]);
      setOllama(detection);
      setRecommendedModels(recommended);
      setActiveModel(config.bundled_model || "");

      if (detection.status === "running") {
        try {
          const models = await invoke<InstalledModel[]>("list_ollama_models", {});
          setInstalledModels(models);
        } catch {
          setInstalledModels([]);
        }
      } else {
        setInstalledModels([]);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const handleInstall = async () => {
    setInstalling(true);
    setInstallProgress({ step: "starting", message: "Starting installer..." });
    setError("");
    try {
      await invoke("install_ollama");
      await refreshAll();
    } catch (e) {
      setError(String(e));
    } finally {
      setInstalling(false);
      setInstallProgress(null);
    }
  };

  const handleStart = async () => {
    setStarting(true);
    setError("");
    try {
      await invoke("start_managed_ollama");
      await refreshAll();
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  };

  const handleUseModel = async (modelName: string) => {
    try {
      await invoke("set_bundled_model", { modelName });
      setActiveModel(modelName);
    } catch (e) {
      setError(String(e));
    }
  };

  const handlePullModel = async (modelName: string) => {
    setPullingModel(modelName);
    setModelProgress({ model: modelName, status: "starting" });
    setError("");
    try {
      await invoke("pull_ollama_model", { model: modelName });
      await refreshAll();
    } catch (e) {
      setError(String(e));
    } finally {
      setPullingModel("");
      setModelProgress(null);
    }
  };

  const handlePullCustom = async () => {
    const name = customModel.trim();
    if (!name) return;
    try {
      await handlePullModel(name);
      setCustomModel("");
    } catch {
      // Keep the input value on failure so the user can retry
    }
  };

  const isModelActive = (name: string) =>
    name === activeModel || name.startsWith(activeModel + ":");

  const installedNames = installedModels.map((m) => m.name);
  const uninstalledRecommended = recommendedModels.filter(
    (m) => !installedNames.some((n) => n === m.name || n.startsWith(m.name + ":"))
  );

  const installPercent =
    installProgress?.written && installProgress?.total
      ? Math.min(100, Math.round((installProgress.written / installProgress.total) * 100))
      : undefined;
  const modelPercent =
    modelProgress?.completed && modelProgress?.total
      ? Math.min(100, Math.round((modelProgress.completed / modelProgress.total) * 100))
      : undefined;

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-theme-overlay z-40 transition-opacity"
        onClick={onClose}
        data-testid="ollama-drawer-backdrop"
      />

      {/* Drawer */}
      <div
        className="fixed top-0 left-0 h-full w-[420px] max-w-[90vw] bg-theme-bg border-r border-theme-border z-50 flex flex-col"
        data-testid="ollama-drawer"
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-theme-border shrink-0">
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-theme-primary-dim text-lg font-bold font-mono tracking-widest uppercase">
                Ollama
              </h1>
              <HelpTooltip
                label="Ollama help"
                title={OLLAMA_HELP.title}
                description={OLLAMA_HELP.description}
                links={OLLAMA_HELP.links}
                align="start"
              />
            </div>
            <p className="text-theme-text-dim text-[10px] uppercase tracking-tighter">
              Local Model Management
            </p>
          </div>
          <button
            className="text-theme-text-dim hover:text-theme-text text-xl px-2"
            onClick={onClose}
            aria-label="Close drawer"
          >
            &times;
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-6">
          {/* Status banner */}
          {ollama && (
            <div className="flex items-center gap-3 px-4 py-3 border border-theme-border-dim rounded-lg bg-theme-bg-elevated">
              <div
                className={`w-2.5 h-2.5 rounded-full ${
                  ollama.status === "running"
                    ? "bg-theme-success shadow-[0_0_6px_rgba(34,197,94,0.5)]"
                    : ollama.status === "installed"
                      ? "bg-theme-warning"
                      : "bg-theme-danger"
                }`}
              />
              <div className="flex-1">
                <span className="text-theme-text-bright text-sm font-bold uppercase">
                  {ollama.status === "running" ? "Running" : ollama.status === "installed" ? "Installed" : "Not Found"}
                </span>
                {ollama.path && (
                  <p className="text-theme-text-dim text-[10px] font-mono truncate">{ollama.path}</p>
                )}
              </div>
            </div>
          )}

          {/* Install section */}
          {ollama?.status === "not_found" && (
            <div className="space-y-3">
              <h3 className="text-[10px] uppercase tracking-widest text-theme-text-dim font-bold">
                Install Ollama
              </h3>
              <button
                className="w-full px-4 py-2 border border-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50 text-sm"
                onClick={handleInstall}
                disabled={installing}
              >
                {installing ? "Installing..." : "Install Ollama"}
              </button>
              {(installing || installProgress) && (
                <div className="border border-theme-border-dim rounded p-3">
                  <p className="text-sm text-theme-text">{installProgress?.message ?? "Working..."}</p>
                  {typeof installPercent === "number" && (
                    <div className="mt-2">
                      <div className="h-2 bg-theme-border-dim rounded">
                        <div className="h-2 bg-theme-primary rounded" style={{ width: `${installPercent}%` }} />
                      </div>
                      <p className="text-xs text-theme-text-dim mt-1">
                        {installPercent}% ({formatBytes(installProgress?.written)} / {formatBytes(installProgress?.total)})
                      </p>
                    </div>
                  )}
                </div>
              )}
              <a
                className="block text-theme-text-dim text-xs hover:text-theme-primary-dim"
                href="https://ollama.com/download"
                target="_blank"
                rel="noreferrer"
              >
                Or download manually from ollama.com
              </a>
            </div>
          )}

          {/* Start section */}
          {ollama?.status === "installed" && (
            <div className="space-y-3">
              <h3 className="text-[10px] uppercase tracking-widest text-theme-text-dim font-bold">
                Start Ollama
              </h3>
              <button
                className="w-full px-4 py-2 border border-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50 text-sm"
                onClick={handleStart}
                disabled={starting}
              >
                {starting ? "Starting..." : "Start Ollama"}
              </button>
            </div>
          )}

          {/* Installed models */}
          {ollama?.status === "running" && (
            <div>
              <h3 className="text-[10px] uppercase tracking-widest text-theme-text-dim font-bold mb-2">
                Installed Models
              </h3>
              {installedModels.length === 0 ? (
                <p className="text-theme-text-dim text-xs">No models found.</p>
              ) : (
                <div className="space-y-1">
                  {installedModels.map((m) => (
                    <div
                      key={m.name}
                      className={`flex items-center justify-between px-3 py-2 rounded border text-xs ${
                        isModelActive(m.name)
                          ? "border-theme-success bg-theme-success-dim text-theme-success"
                          : "border-theme-border-dim text-theme-text-dim"
                      }`}
                    >
                      <div className="flex items-center gap-2">
                        <span className="font-mono">{m.name}</span>
                        <span className="text-[10px] text-theme-text-dim">{formatBytes(m.size)}</span>
                      </div>
                      {isModelActive(m.name) ? (
                        <span className="text-[10px] text-theme-success">Active</span>
                      ) : (
                        <button
                          className="text-[10px] px-2 py-0.5 border border-theme-border-dim rounded hover:border-theme-primary hover:text-theme-primary"
                          onClick={() => handleUseModel(m.name)}
                        >
                          Use
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Recommended models */}
          {uninstalledRecommended.length > 0 && (
            <div>
              <h3 className="text-[10px] uppercase tracking-widest text-theme-text-dim font-bold mb-2">
                Recommended Models
              </h3>
              <div className="space-y-2">
                {uninstalledRecommended.map((m) => (
                  <div
                    key={m.name}
                    className="border border-theme-border-dim rounded-lg p-3 bg-theme-bg-elevated"
                  >
                    <div className="flex items-center justify-between mb-1">
                      <div className="flex items-center gap-2">
                        <span className="text-theme-text-bright text-xs font-bold">{m.label}</span>
                        {m.recommended && (
                          <span className="text-[9px] text-theme-success uppercase border border-theme-success px-1 py-0.5 rounded bg-theme-success-dim">
                            Recommended
                          </span>
                        )}
                      </div>
                      <span className="text-[10px] text-theme-text-dim">{formatBytes(m.size_bytes)}</span>
                    </div>
                    <p className="text-theme-text-dim text-[10px] mb-2">{m.description}</p>
                    <div className="flex items-center gap-2">
                      <span className="text-theme-text-dim text-[10px] font-mono flex-1">{m.name}</span>
                      <button
                        className="text-[10px] px-3 py-1 border border-theme-primary-faint text-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50"
                        onClick={() => handlePullModel(m.name)}
                        disabled={pulling}
                      >
                        {pullingModel === m.name ? "Pulling..." : "Pull"}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Pull progress */}
          {(pulling || modelProgress) && (
            <div className="border border-theme-border-dim rounded p-3">
              <p className="text-sm text-theme-text">
                {modelProgress?.status ? `Model: ${modelProgress.status}` : "Pulling model..."}
              </p>
              {typeof modelPercent === "number" && (
                <div className="mt-2">
                  <div className="h-2 bg-theme-border-dim rounded">
                    <div className="h-2 bg-theme-primary rounded" style={{ width: `${modelPercent}%` }} />
                  </div>
                  <p className="text-xs text-theme-text-dim mt-1">
                    {modelPercent}% ({formatBytes(modelProgress?.completed)} / {formatBytes(modelProgress?.total)})
                  </p>
                </div>
              )}
            </div>
          )}

          {/* Custom model pull */}
          <div>
            <h3 className="text-[10px] uppercase tracking-widest text-theme-text-dim font-bold mb-2">
              Download Custom Model
            </h3>
            <div className="flex gap-2">
              <input
                type="text"
                value={customModel}
                onChange={(e) => setCustomModel(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handlePullCustom(); }}
                placeholder="e.g. llama3.2:3b, mistral, phi3..."
                className="flex-1 bg-theme-bg border border-theme-border-dim rounded px-3 py-1.5 text-xs text-theme-text placeholder:text-theme-text-dim focus:border-theme-primary focus:outline-none font-mono"
                disabled={pulling}
              />
              <button
                className="border border-theme-primary-faint text-theme-primary px-4 py-1.5 rounded text-xs hover:bg-theme-primary-glow disabled:opacity-50"
                onClick={handlePullCustom}
                disabled={pulling || !customModel.trim()}
              >
                Pull
              </button>
            </div>
          </div>

          {/* Error */}
          {error && (
            <div className="p-3 border border-theme-danger rounded bg-theme-danger-dim text-theme-danger text-sm">
              {error}
              <button
                className="ml-2 text-theme-danger underline text-xs"
                onClick={() => setError("")}
              >
                dismiss
              </button>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
