import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useState, useEffect } from "react";
import { isBrowserHarnessRuntime } from "../runtimeMode";

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

interface CliDetection {
  provider_name: string;
  binary: string;
  on_path: boolean;
  is_official: boolean;
  is_authenticated: boolean;
  version: string | null;
  auth_hint: string | null;
}

interface LlmSetupPanelProps {
  onConnected: (url: string) => void;
  onSkip?: () => void;
  showSkip?: boolean;
}

export default function LlmSetupPanel({ onConnected, onSkip, showSkip = false }: LlmSetupPanelProps) {
  const [mode, setMode] = useState<"ollama" | "lmstudio" | "cli">("ollama");
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
  const [showProceedAnyway, setShowProceedAnyway] = useState(false);
  const [installProgress, setInstallProgress] = useState<OllamaInstallProgress | null>(null);
  const [modelProgress, setModelProgress] = useState<OllamaModelProgress | null>(null);
  const [cliDetections, setCliDetections] = useState<CliDetection[]>([]);
  const [cliProbing, setCliProbing] = useState(false);
  const [activatingCli, setActivatingCli] = useState(false);
  const [initialProbeComplete, setInitialProbeComplete] = useState(false);
  const [autoConnectedBundled, setAutoConnectedBundled] = useState(false);

  // Which sources have something available
  const ollamaAvailable = ollama !== null && ollama.status !== "not_found";
  const lmStudioAvailable = detected.some(d => d.reachable);
  const cliAvailable = cliDetections.some(d => d.on_path);

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

  // Probe all three sources on mount, then auto-select the best available tab
  useEffect(() => {
    let cancelled = false;
    const probeAll = async () => {
      setProbing(true);
      setCliProbing(true);

      const [ollamaResult, lmResult, cliResult] = await Promise.allSettled([
        Promise.all([
          invoke<OllamaDetection>("detect_ollama"),
          invoke<RecommendedModel[]>("list_recommended_models"),
        ]),
        invoke<ProbeResult>("probe_local_llm"),
        invoke<CliDetection[]>("detect_cli_providers_full"),
      ]);

      if (cancelled) return;

      // Apply ollama results
      if (ollamaResult.status === "fulfilled") {
        const [detection, models] = ollamaResult.value;
        setOllama(detection);
        setRecommendedModels(models);
        const defaultModel = models.find((m) => m.recommended)?.name ?? models[0]?.name ?? "";
        setSelectedModel(defaultModel);
      }

      // Apply LM Studio results
      if (lmResult.status === "fulfilled") {
        setDetected(lmResult.value.detected);
        if (lmResult.value.detected.length > 0) {
          setManualUrl(lmResult.value.detected[0].url);
        } else {
          setManualUrl("http://localhost:1234");
        }
      }

      // Apply CLI results
      if (cliResult.status === "fulfilled") {
        setCliDetections(cliResult.value);
      }

      setProbing(false);
      setCliProbing(false);

      // Auto-select the best available tab
      const hasOllama = ollamaResult.status === "fulfilled" && ollamaResult.value[0].status !== "not_found";
      const hasLm = lmResult.status === "fulfilled" && lmResult.value.detected.some(d => d.reachable);
      const hasCli = cliResult.status === "fulfilled" && cliResult.value.some(d => d.on_path);

      const available: ("ollama" | "lmstudio" | "cli")[] = [];
      if (hasOllama) available.push("ollama");
      if (hasLm) available.push("lmstudio");
      if (hasCli) available.push("cli");

      if (available.length === 1) {
        setMode(available[0]);
      } else if (available.length > 0) {
        setMode(available[0]);
      }
      // If nothing detected, stay on "ollama" (the default)

      setInitialProbeComplete(true);
    };

    probeAll();
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (isBrowserHarnessRuntime()) return;
    if (!initialProbeComplete) return;
    if (autoConnectedBundled) return;
    if (mode !== "ollama") return;
    if (connecting || pullingModel || installing) return;
    if (ollama?.status !== "running") return;

    // Bundled/runtime Ollama is already up: skip unnecessary interactive steps.
    setAutoConnectedBundled(true);
    connectTo("http://localhost:11434", true);
  }, [
    initialProbeComplete,
    autoConnectedBundled,
    mode,
    connecting,
    pullingModel,
    installing,
    ollama?.status,
  ]);

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
      if (result.detected.length > 0 && !manualUrl) {
        setManualUrl(result.detected[0].url);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setProbing(false);
    }
  };

  const probeCli = async () => {
    setCliProbing(true);
    setError("");
    try {
      const results = await invoke<CliDetection[]>("detect_cli_providers_full");
      setCliDetections(results);
    } catch (e) {
      setError(String(e));
    } finally {
      setCliProbing(false);
    }
  };

  const activateCliProvider = async (providerName: string) => {
    setActivatingCli(true);
    setError("");
    try {
      await invoke("use_stored_provider", { provider: providerName });
      onConnected("cli:" + providerName);
    } catch (e) {
      setError(String(e));
    } finally {
      setActivatingCli(false);
    }
  };

  const connectTo = async (url: string, skipHealthCheck = false) => {
    setConnecting(true);
    setError("");
    setShowProceedAnyway(false);
    try {
      const ok = await invoke<boolean>("set_local_llm_during_birth", { url, skipHealthCheck });
      if (ok) {
        onConnected(url);
      } else {
        setError("Could not connect. Is the server running?");
        setShowProceedAnyway(true);
      }
    } catch (e) {
      setError(String(e));
      setShowProceedAnyway(true);
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
      await connectTo("http://localhost:11434", false);
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
        {([
          { key: "ollama" as const, label: "Ollama (guided)", available: ollamaAvailable },
          { key: "lmstudio" as const, label: "LM Studio / Custom URL", available: lmStudioAvailable },
          { key: "cli" as const, label: "CLI Quick-Start", available: cliAvailable },
        ]).map(({ key, label, available }) => (
          <button
            key={key}
            className={`px-3 py-2 rounded border text-sm transition-all ${
              mode === key
                ? "border-theme-primary text-theme-text"
                : initialProbeComplete && available
                  ? "border-green-600 text-theme-text hover:border-theme-primary bg-green-950/10"
                  : "border-theme-border-dim text-theme-text-dim hover:border-theme-primary"
            }`}
            onClick={() => setMode(key)}
            disabled={installing || pullingModel || connecting || activatingCli}
          >
            {label}
            {initialProbeComplete && available && mode !== key && (
              <span className="ml-1.5 inline-block w-1.5 h-1.5 rounded-full bg-green-500" />
            )}
          </button>
        ))}
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

          {ollama.status !== "running" && (
            <div className="pt-4 border-t border-theme-border-dim">
              <p className="text-theme-text-dim text-xs mb-2">Alternative: wake up in the cloud first</p>
              <button
                className="text-xs text-theme-primary hover:underline flex items-center gap-1"
                onClick={onSkip}
              >
                I have a Cloud API Key (OpenAI/Anthropic) &rsaquo;
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

      {mode === "cli" && cliProbing && (
        <div className="mb-4">
          <p className="text-theme-text">Detecting CLI tools...</p>
          <div className="animate-pulse mt-1">...</div>
        </div>
      )}

      {mode === "cli" && !cliProbing && (
        <div className="mb-6 space-y-4">
          {cliDetections.filter(d => d.on_path).length === 0 ? (
            <div className="border border-yellow-700 bg-yellow-900/20 p-4 rounded">
              <p className="text-yellow-500 text-sm">No CLI tools detected on PATH.</p>
              <p className="text-yellow-400/80 text-xs mt-2">
                Install one of the supported CLI tools below, then click Re-scan.
              </p>
            </div>
          ) : (
            <div className="space-y-2">
              {cliDetections.filter(d => d.on_path).map((d) => {
                const ready = d.is_official && d.is_authenticated;
                return (
                  <div
                    key={d.provider_name}
                    className={`px-4 py-3 border rounded ${
                      ready
                        ? "border-green-600 bg-green-950/20"
                        : "border-theme-border-dim bg-theme-bg-inset"
                    }`}
                  >
                    <div className="flex items-center justify-between">
                      <div>
                        <span className="text-theme-text-bright font-bold text-sm uppercase">
                          {d.provider_name.replace("-cli", "")}
                        </span>
                        {d.version && (
                          <span className="text-theme-text-dim text-xs ml-2">{d.version}</span>
                        )}
                      </div>
                      <div className="flex items-center gap-2">
                        {d.is_official ? (
                          <span className="text-green-500 text-[10px] uppercase">Official</span>
                        ) : (
                          <span className="text-yellow-500 text-[10px] uppercase">Unverified</span>
                        )}
                        {d.is_authenticated ? (
                          <span className="text-green-500 text-[10px] uppercase">Authed</span>
                        ) : (
                          <span className="text-yellow-500 text-[10px] uppercase">Not Authed</span>
                        )}
                      </div>
                    </div>
                    {ready ? (
                      <button
                        className="mt-2 px-4 py-1.5 border border-green-600 text-green-500 rounded text-xs hover:bg-green-950/40 disabled:opacity-50"
                        onClick={() => activateCliProvider(d.provider_name)}
                        disabled={activatingCli}
                      >
                        {activatingCli ? "Activating..." : "Activate as Primary"}
                      </button>
                    ) : (
                      <p className="text-yellow-400/80 text-xs mt-2">
                        {d.auth_hint || "CLI tool needs authentication."}
                      </p>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          <button
            className="text-xs text-theme-text-dim hover:text-theme-primary underline"
            onClick={probeCli}
            disabled={cliProbing}
          >
            Re-scan
          </button>
        </div>
      )}

      {error && (
        <div className="mt-2">
          <p className="text-red-400 text-sm">{error}</p>
          {showProceedAnyway && (
            <button
              className="mt-2 text-xs text-theme-text-dim hover:text-theme-primary underline"
              onClick={() => connectTo(manualUrl.startsWith("http") ? manualUrl : `http://${manualUrl}`, true)}
            >
              The server is running, proceed anyway
            </button>
          )}
        </div>
      )}

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
