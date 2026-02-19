import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useState, useEffect } from "react";

interface DetectedLlm {
  name: string;
  url: string;
  reachable: boolean;
}

interface ProbeResult {
  detected: DetectedLlm[];
}

interface OllamaDetection {
  status: "running" | "installed" | "not_found";
  path: string | null;
}

interface RecommendedModel {
  name: string;
  label: string;
  size_bytes: number;
  description: string;
  recommended: boolean;
}

interface OllamaInstallProgress {
  step: string;
  written?: number;
  total?: number;
  message: string;
}

interface OllamaModelProgress {
  model: string;
  completed?: number;
  total?: number;
  status: string;
}

interface LlmSetupPanelProps {
  onConnected: (url: string) => void;
  onSkip?: () => void;
  showSkip?: boolean;
}

export default function LlmSetupPanel({ onConnected, onSkip, showSkip = false }: LlmSetupPanelProps) {
  const [mode, setMode] = useState<"ollama" | "lmstudio">("ollama");
  const [probing, setProbing] = useState(false);
  const [detected, setDetected] = useState<DetectedLlm[]>([]);
  const [ollama, setOllama] = useState<OllamaDetection | null>(null);
  const [recommendedModels, setRecommendedModels] = useState<RecommendedModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>("");

  const [manualUrl, setManualUrl] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [pullingModel, setPullingModel] = useState(false);
  const [error, setError] = useState("");
  const [installProgress, setInstallProgress] = useState<OllamaInstallProgress | null>(null);
  const [modelProgress, setModelProgress] = useState<OllamaModelProgress | null>(null);

  useEffect(() => {
    let unlistenInstall: (() => void) | null = null;
    let unlistenModel: (() => void) | null = null;
    let cancelled = false;

    listen<OllamaInstallProgress>("ollama-install-progress", (event) => {
      if (!cancelled) setInstallProgress(event.payload);
    }).then((fn) => {
      unlistenInstall = fn;
    }).catch(() => {
      // Ignore listener setup failures; command will still return errors.
    });

    listen<OllamaModelProgress>("ollama-model-progress", (event) => {
      if (!cancelled) setModelProgress(event.payload);
    }).then((fn) => {
      unlistenModel = fn;
    }).catch(() => {
      // Ignore listener setup failures; command will still return errors.
    });

    return () => {
      cancelled = true;
      if (unlistenInstall) unlistenInstall();
      if (unlistenModel) unlistenModel();
    };
  }, []);

  useEffect(() => {
    if (mode === "ollama") {
      probeOllama();
    } else {
      if (!manualUrl) setManualUrl("http://localhost:1234");
      probeLmStudio();
    }
  }, [mode]);

  const formatBytes = (value?: number) => {
    if (!value || value <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB"];
    let size = value;
    let unit = 0;
    while (size >= 1024 && unit < units.length - 1) {
      size /= 1024;
      unit += 1;
    }
    return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
  };

  const probeOllama = async () => {
    setProbing(true);
    setError("");
    try {
      const [detection, models] = await Promise.all([
        invoke<OllamaDetection>("detect_ollama"),
        invoke<RecommendedModel[]>("list_recommended_models"),
      ]);
      setOllama(detection);
      setRecommendedModels(models);
      const defaultModel = models.find((m) => m.recommended)?.name ?? models[0]?.name ?? "";
      setSelectedModel(defaultModel);
    } catch (e) {
      setError(String(e));
    } finally {
      setProbing(false);
    }
  };

  const probeLmStudio = async () => {
    setProbing(true);
    setError("");
    try {
      const result = await invoke<ProbeResult>("probe_local_llm");
      setDetected(result.detected);
    } catch (e) {
      setError(String(e));
    } finally {
      setProbing(false);
    }
  };

  const connectTo = async (url: string) => {
    setConnecting(true);
    setError("");
    try {
      const ok = await invoke<boolean>("set_local_llm_during_birth", { url });
      if (ok) {
        onConnected(url);
      } else {
        setError("Could not connect. Is the server running?");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setConnecting(false);
    }
  };

  const installOllama = async () => {
    setInstalling(true);
    setInstallProgress({ step: "starting", message: "Starting installer..." });
    setError("");
    try {
      await invoke("install_ollama");
      await probeOllama();
    } catch (e) {
      setError(String(e));
    } finally {
      setInstalling(false);
    }
  };

  const pullAndConnect = async () => {
    if (!selectedModel) {
      setError("Choose a model first.");
      return;
    }
    setPullingModel(true);
    setModelProgress({ model: selectedModel, status: "starting" });
    setError("");
    try {
      await invoke("pull_ollama_model", { model: selectedModel });
      await connectTo("http://localhost:11434");
    } catch (e) {
      setError(String(e));
    } finally {
      setPullingModel(false);
    }
  };

  const continueWithoutModel = async () => {
    setError("");
    try {
      await invoke("advance_to_connectivity");
      onConnected("candle_stub");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleManualConnect = () => {
    const url = manualUrl.trim();
    if (!url) {
      setError("URL is required");
      return;
    }
    // Ensure it starts with http
    const fullUrl = url.startsWith("http") ? url : `http://${url}`;
    connectTo(fullUrl);
  };

  const reachable = detected.filter(d => d.reachable);
  const installPercent = installProgress?.written && installProgress?.total
    ? Math.min(100, Math.round((installProgress.written / installProgress.total) * 100))
    : undefined;
  const modelPercent = modelProgress?.completed && modelProgress?.total
    ? Math.min(100, Math.round((modelProgress.completed / modelProgress.total) * 100))
    : undefined;

  return (
    <div className="p-6">
      <h2 className="text-theme-primary-dim text-lg mb-4">Ignition: Connect Your Local Mind</h2>
      <p className="text-theme-text-dim text-sm mb-4">
        Abigail needs a local LLM for private, offline-first reasoning.
      </p>

      <div className="flex gap-2 mb-6">
        <button
          className={`px-3 py-2 rounded border text-sm ${mode === "ollama"
            ? "border-theme-primary text-theme-text"
            : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"}`}
          onClick={() => setMode("ollama")}
          disabled={installing || pullingModel || connecting}
        >
          Ollama (guided)
        </button>
        <button
          className={`px-3 py-2 rounded border text-sm ${mode === "lmstudio"
            ? "border-theme-primary text-theme-text"
            : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"}`}
          onClick={() => setMode("lmstudio")}
          disabled={installing || pullingModel || connecting}
        >
          LM Studio / Custom URL
        </button>
      </div>

      {mode === "ollama" && probing && (
        <div className="mb-4">
          <p className="text-theme-text">Checking Ollama status...</p>
          <div className="animate-pulse mt-1">...</div>
        </div>
      )}

      {mode === "ollama" && !probing && ollama && (
        <div className="mb-6 space-y-4">
          <div className="border border-theme-border-dim rounded p-4 bg-theme-surface">
            {ollama.status === "running" && (
              <p className="text-green-500 text-sm">Ollama is running and ready at `localhost:11434`.</p>
            )}
            {ollama.status === "installed" && (
              <p className="text-yellow-500 text-sm">Ollama is installed. Continue to choose a model and connect.</p>
            )}
            {ollama.status === "not_found" && (
              <p className="text-yellow-500 text-sm">Ollama is not installed yet.</p>
            )}
          </div>

          {ollama.status === "not_found" && (
            <div className="space-y-3">
              <button
                disabled={installing || pullingModel || connecting}
                className="px-4 py-2 border border-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50"
                onClick={installOllama}
              >
                {installing ? "Installing Ollama..." : "Install Ollama"}
              </button>
              <a
                className="block text-theme-text-dim text-xs hover:text-theme-primary-dim"
                href="https://ollama.com/download"
                target="_blank"
                rel="noreferrer"
              >
                Manual install (open ollama.com)
              </a>
            </div>
          )}

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

          {ollama.status !== "not_found" && recommendedModels.length > 0 && (
            <div>
              <p className="text-theme-primary-dim mb-2">Choose a local model</p>
              <div className="space-y-2">
                {recommendedModels.map((model) => (
                  <button
                    key={model.name}
                    className={`block w-full text-left px-4 py-3 border rounded ${selectedModel === model.name
                      ? "border-theme-primary bg-theme-primary-glow"
                      : "border-theme-border-dim hover:border-theme-primary"}`}
                    onClick={() => setSelectedModel(model.name)}
                    disabled={pullingModel || connecting}
                  >
                    <div className="flex items-center justify-between">
                      <span className="text-theme-text-bright font-bold">
                        {model.label} - {model.name}
                      </span>
                      <span className="text-theme-text-dim text-xs">{formatBytes(model.size_bytes)}</span>
                    </div>
                    <p className="text-theme-text-dim text-xs mt-1">{model.description}</p>
                  </button>
                ))}
              </div>
            </div>
          )}

          {ollama.status !== "not_found" && (
            <div className="flex gap-2">
              <button
                className="px-4 py-2 border border-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50"
                disabled={pullingModel || connecting}
                onClick={pullAndConnect}
              >
                {pullingModel ? "Downloading model..." : "Download model and continue"}
              </button>
              <button
                className="px-4 py-2 border border-theme-border-dim rounded hover:border-theme-primary text-theme-text-dim disabled:opacity-50"
                disabled={pullingModel || connecting}
                onClick={continueWithoutModel}
              >
                Continue without model
              </button>
            </div>
          )}

          {(pullingModel || modelProgress) && (
            <div className="border border-theme-border-dim rounded p-3">
              <p className="text-sm text-theme-text">
                {modelProgress?.status ? `Model status: ${modelProgress.status}` : "Pulling model..."}
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
        </div>
      )}

      {mode === "lmstudio" && probing && (
        <div className="mb-4">
          <p className="text-theme-text">Scanning for local LLM servers...</p>
          <div className="animate-pulse mt-1">...</div>
        </div>
      )}

      {mode === "lmstudio" && !probing && reachable.length > 0 && (
        <div className="mb-6">
          <p className="text-theme-primary-dim mb-2">Detected:</p>
          <div className="space-y-2">
            {reachable.map((llm, i) => (
              <button
                key={i}
                disabled={connecting}
                className="block w-full text-left px-4 py-3 border border-theme-primary rounded hover:bg-theme-primary-glow disabled:opacity-50"
                onClick={() => connectTo(llm.url)}
              >
                <span className="text-theme-text-bright font-bold">{llm.name}</span>
                <span className="text-theme-text-dim text-sm ml-2">{llm.url}</span>
                <span className="text-theme-text text-xs ml-2">[online]</span>
              </button>
            ))}
          </div>
        </div>
      )}

      {mode === "lmstudio" && !probing && reachable.length === 0 && (
        <div className="mb-6">
          <div className="border border-yellow-700 bg-yellow-900/20 p-4 rounded mb-4">
            <p className="text-yellow-500 text-sm">
              No local LLM server detected on default ports.
            </p>
            <p className="text-yellow-400/80 text-xs mt-2">
              In LM Studio: load a model, then click <strong>Start Server</strong> (default port 1234).
              Enter the URL below or click Re-scan after starting.
            </p>
          </div>
        </div>
      )}

      {mode === "lmstudio" && !probing && (
        <div className="mb-4">
          <p className="text-theme-text text-sm mb-2">Server URL:</p>
          <div className="flex gap-2">
            <input
              type="text"
              className="flex-1 bg-black border border-theme-primary text-theme-text px-3 py-2 rounded text-sm"
              placeholder="http://localhost:1234"
              value={manualUrl}
              onChange={(e) => setManualUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleManualConnect()}
              autoFocus
            />
            <button
              disabled={connecting}
              className="border border-theme-primary px-4 py-2 rounded hover:bg-theme-primary-glow text-sm disabled:opacity-50"
              onClick={handleManualConnect}
            >
              {connecting ? "..." : "Connect"}
            </button>
            <button
              disabled={probing || connecting}
              className="border border-theme-border-dim px-3 py-2 rounded hover:border-theme-primary text-theme-text-dim text-sm disabled:opacity-50"
              onClick={() => probeLmStudio()}
            >
              Re-scan
            </button>
          </div>
        </div>
      )}

      {error && <p className="text-red-400 text-sm mt-2">{error}</p>}

      {showSkip && onSkip && (
        <div className="mt-8 pt-4 border-t border-theme-border-dim">
          <button
            className="text-theme-primary-faint text-xs hover:text-theme-text"
            onClick={onSkip}
          >
            Skip interactive setup (use default birth)
          </button>
        </div>
      )}
    </div>
  );
}
