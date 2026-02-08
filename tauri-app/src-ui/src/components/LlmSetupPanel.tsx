import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect } from "react";

interface DetectedLlm {
  name: string;
  url: string;
  reachable: boolean;
}

interface ProbeResult {
  detected: DetectedLlm[];
}

interface LlmSetupPanelProps {
  onConnected: (url: string) => void;
  onSkip?: () => void;
  showSkip?: boolean;
}

export default function LlmSetupPanel({ onConnected, onSkip, showSkip = false }: LlmSetupPanelProps) {
  const [probing, setProbing] = useState(true);
  const [detected, setDetected] = useState<DetectedLlm[]>([]);
  const [manualUrl, setManualUrl] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [retryCount, setRetryCount] = useState(0);
  const MAX_RETRIES = 3;

  useEffect(() => {
    probe();
  }, []);

  const probe = async (isRetry = false) => {
    setProbing(true);
    if (!isRetry) {
      setError("");
      setRetryCount(0);
    }
    try {
      const result = await invoke<ProbeResult>("probe_local_llm");
      setDetected(result.detected);
      setRetryCount(0);
    } catch (e) {
      const errorMsg = String(e);
      // Auto-retry on network errors
      if (retryCount < MAX_RETRIES) {
        setError(`Scanning failed. Retrying... (${retryCount + 1}/${MAX_RETRIES})`);
        setRetryCount(prev => prev + 1);
        setTimeout(() => probe(true), 1500 * (retryCount + 1));
        return;
      }
      setError(errorMsg);
    } finally {
      if (retryCount >= MAX_RETRIES || !error?.includes("Retrying")) {
        setProbing(false);
      }
    }
  };

  const connectTo = async (url: string, retries = 0) => {
    setConnecting(true);
    setError("");
    try {
      const ok = await invoke<boolean>("set_local_llm_during_birth", { url });
      if (ok) {
        onConnected(url);
      } else {
        // Retry if server might be starting up
        if (retries < MAX_RETRIES) {
          setError(`Connection failed. Retrying... (${retries + 1}/${MAX_RETRIES})`);
          setTimeout(() => connectTo(url, retries + 1), 1500);
          return;
        }
        setError("Could not connect. Is the server running?");
      }
    } catch (e) {
      const errorMsg = String(e);
      // Retry on network errors
      if (retries < MAX_RETRIES && (
        errorMsg.toLowerCase().includes("connection") ||
        errorMsg.toLowerCase().includes("timeout") ||
        errorMsg.toLowerCase().includes("network")
      )) {
        setError(`Connection error. Retrying... (${retries + 1}/${MAX_RETRIES})`);
        setTimeout(() => connectTo(url, retries + 1), 1500);
        return;
      }
      setError(errorMsg);
    } finally {
      if (!error?.includes("Retrying")) {
        setConnecting(false);
      }
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

  return (
    <div className="p-6">
      <h2 className="text-theme-primary-dim text-lg mb-4">Ignition: Connect Your Local Mind</h2>
      <p className="text-theme-text-dim text-sm mb-6">
        Abigail needs a local LLM to think independently. This is your Id — it runs on your machine,
        keeps your data private, and never leaves your network.
      </p>

      {probing && (
        <div className="mb-4">
          <p className="text-theme-text">Scanning for local LLM servers...</p>
          <div className="animate-pulse mt-1">...</div>
        </div>
      )}

      {!probing && reachable.length > 0 && (
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

      {!probing && reachable.length === 0 && (
        <div className="mb-6">
          <div className="border border-yellow-700 bg-yellow-900/20 p-4 rounded mb-4">
            <p className="text-yellow-500 text-sm">
              No local LLM detected. Please start one of:
            </p>
            <ul className="text-yellow-400 text-sm mt-2 space-y-1">
              <li>- <strong>Ollama</strong>: Install from ollama.com, then run <code>ollama serve</code></li>
              <li>- <strong>LM Studio</strong>: Install from lmstudio.ai, load a model, start the server</li>
            </ul>
          </div>
          <button
            className="text-theme-text-dim text-sm hover:text-theme-primary-dim"
            onClick={() => probe()}
          >
            Re-scan
          </button>
        </div>
      )}

      {!showManual ? (
        <button
          className="text-theme-text-dim text-xs hover:text-theme-primary-dim mb-4"
          onClick={() => setShowManual(true)}
        >
          Enter URL manually
        </button>
      ) : (
        <div className="mb-4">
          <p className="text-theme-text text-sm mb-2">Custom LLM URL:</p>
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
