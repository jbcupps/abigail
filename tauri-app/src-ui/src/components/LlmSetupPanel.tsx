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

  useEffect(() => {
    probe();
  }, []);

  const probe = async () => {
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
      <h2 className="text-green-400 text-lg mb-4">Ignition: Connect Your Local Mind</h2>
      <p className="text-green-600 text-sm mb-6">
        AO needs a local LLM to think independently. This is your Id — it runs on your machine,
        keeps your data private, and never leaves your network.
      </p>

      {probing && (
        <div className="mb-4">
          <p className="text-green-500">Scanning for local LLM servers...</p>
          <div className="animate-pulse mt-1">...</div>
        </div>
      )}

      {!probing && reachable.length > 0 && (
        <div className="mb-6">
          <p className="text-green-400 mb-2">Detected:</p>
          <div className="space-y-2">
            {reachable.map((llm, i) => (
              <button
                key={i}
                disabled={connecting}
                className="block w-full text-left px-4 py-3 border border-green-500 rounded hover:bg-green-500/20 disabled:opacity-50"
                onClick={() => connectTo(llm.url)}
              >
                <span className="text-green-300 font-bold">{llm.name}</span>
                <span className="text-green-600 text-sm ml-2">{llm.url}</span>
                <span className="text-green-500 text-xs ml-2">[online]</span>
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
            className="text-green-600 text-sm hover:text-green-400"
            onClick={probe}
          >
            Re-scan
          </button>
        </div>
      )}

      {!showManual ? (
        <button
          className="text-green-600 text-xs hover:text-green-400 mb-4"
          onClick={() => setShowManual(true)}
        >
          Enter URL manually
        </button>
      ) : (
        <div className="mb-4">
          <p className="text-green-500 text-sm mb-2">Custom LLM URL:</p>
          <div className="flex gap-2">
            <input
              type="text"
              className="flex-1 bg-black border border-green-500 text-green-500 px-3 py-2 rounded text-sm"
              placeholder="http://localhost:1234"
              value={manualUrl}
              onChange={(e) => setManualUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleManualConnect()}
              autoFocus
            />
            <button
              disabled={connecting}
              className="border border-green-500 px-4 py-2 rounded hover:bg-green-500/20 text-sm disabled:opacity-50"
              onClick={handleManualConnect}
            >
              {connecting ? "..." : "Connect"}
            </button>
          </div>
        </div>
      )}

      {error && <p className="text-red-400 text-sm mt-2">{error}</p>}

      {showSkip && onSkip && (
        <div className="mt-8 pt-4 border-t border-green-900">
          <button
            className="text-green-700 text-xs hover:text-green-500"
            onClick={onSkip}
          >
            Skip interactive setup (use default birth)
          </button>
        </div>
      )}
    </div>
  );
}
