import { useCallback, useEffect, useState } from "react";
import {
  detectCliProviders,
  discoverModels,
  setHiveDefault,
  storeSecret,
  type CliProviderDetection,
  type ProviderModel,
} from "../lib/daemonClient";

interface ProviderWizardProps {
  onClose: () => void;
  onComplete: () => void;
}

const PROVIDER_LABELS: Record<string, string> = {
  anthropic: "Anthropic (Claude)",
  openai: "OpenAI",
  google: "Google (Gemini)",
  xai: "xAI (Grok)",
  perplexity: "Perplexity",
};

const PROVIDER_OPTIONS = ["anthropic", "openai", "google", "xai", "perplexity"];

// Guess the provider from an API key's prefix.
function guessProvider(key: string): string | null {
  const k = key.trim();
  if (k.startsWith("sk-ant")) return "anthropic";
  if (k.startsWith("sk-")) return "openai";
  if (k.startsWith("AIza")) return "google";
  if (k.startsWith("xai-")) return "xai";
  if (k.startsWith("pplx-")) return "perplexity";
  return null;
}

function cliLabel(provider: string): string {
  const base = provider.replace(/-cli$/, "");
  return base.charAt(0).toUpperCase() + base.slice(1);
}

type Step = "choose" | "key" | "model" | "done";

// Guided "connect a model" flow for the Hive. A family head either uses an AI
// tool already installed and signed in, or pastes an API key — the wizard then
// finds the available models and saves the choice as the home's default, which
// every Entity inherits. Adding a model lives in the Hive, never in entity chat.
export default function ProviderWizard({ onClose, onComplete }: ProviderWizardProps) {
  const [step, setStep] = useState<Step>("choose");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [cliProviders, setCliProviders] = useState<CliProviderDetection[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [provider, setProvider] = useState<string>("");
  const [models, setModels] = useState<ProviderModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>("");

  useEffect(() => {
    detectCliProviders()
      .then((list) => setCliProviders(list.filter((p) => p.on_path && p.is_authenticated)))
      .catch(() => setCliProviders([]));
  }, []);

  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));

  const useCli = useCallback(async (cliProvider: string) => {
    setBusy(true);
    setError(null);
    try {
      await setHiveDefault(cliProvider);
      setStep("done");
      onComplete();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [onComplete]);

  const guessed = guessProvider(apiKey);
  const effectiveProvider = provider || guessed || "";

  const submitKey = useCallback(async () => {
    const key = apiKey.trim();
    const p = effectiveProvider;
    if (!key || !p || busy) return;
    setBusy(true);
    setError(null);
    try {
      await storeSecret(p, key);
      const found = await discoverModels(p, key);
      setModels(found);
      setSelectedModel(found[0]?.id ?? "");
      setProvider(p);
      setStep("model");
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [apiKey, effectiveProvider, busy]);

  const saveModel = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await setHiveDefault(provider, selectedModel || undefined);
      setStep("done");
      onComplete();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }, [busy, provider, selectedModel, onComplete]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-theme-overlay p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-6 shadow-theme-dropdown"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-theme-text-bright">Connect a model</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-theme-text-dim hover:text-theme-text"
            aria-label="Close"
          >
            ✕
          </button>
        </div>

        {step === "choose" && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-theme-text-dim">
              A local model already works. Connect a cloud model for more capability —
              your data still stays on this computer.
            </p>
            {cliProviders.map((cli) => (
              <button
                key={cli.provider}
                type="button"
                disabled={busy}
                onClick={() => void useCli(cli.provider)}
                className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3 text-left hover:border-theme-primary disabled:opacity-40"
              >
                <div className="font-medium text-theme-text-bright">
                  Use {cliLabel(cli.provider)} (already installed)
                </div>
                <div className="text-xs text-theme-text-dim">Signed in and ready — one click.</div>
              </button>
            ))}
            <button
              type="button"
              onClick={() => setStep("key")}
              className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3 text-left hover:border-theme-primary"
            >
              <div className="font-medium text-theme-text-bright">Paste an API key</div>
              <div className="text-xs text-theme-text-dim">
                Anthropic, OpenAI, Google, xAI, or Perplexity.
              </div>
            </button>
          </div>
        )}

        {step === "key" && (
          <div className="flex flex-col gap-3">
            <label className="text-sm text-theme-text-dim">
              Paste your provider API key
              <input
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-…"
                type="password"
                autoFocus
                className="mt-1 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
              />
            </label>
            <label className="text-sm text-theme-text-dim">
              Provider
              <select
                value={effectiveProvider}
                onChange={(e) => setProvider(e.target.value)}
                className="mt-1 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
              >
                <option value="">Select…</option>
                {PROVIDER_OPTIONS.map((p) => (
                  <option key={p} value={p}>
                    {PROVIDER_LABELS[p]}
                  </option>
                ))}
              </select>
            </label>
            <div className="flex justify-between">
              <button
                type="button"
                onClick={() => setStep("choose")}
                className="text-sm text-theme-text-dim hover:text-theme-text"
              >
                Back
              </button>
              <button
                type="button"
                disabled={!apiKey.trim() || !effectiveProvider || busy}
                onClick={() => void submitKey()}
                className="rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-40"
              >
                {busy ? "Checking…" : "Continue"}
              </button>
            </div>
          </div>
        )}

        {step === "model" && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-theme-text-dim">
              Choose a model for {PROVIDER_LABELS[provider] ?? provider}.
            </p>
            <select
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              className="w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
            >
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.display_name ?? m.id}
                </option>
              ))}
            </select>
            <div className="flex justify-end">
              <button
                type="button"
                disabled={busy}
                onClick={() => void saveModel()}
                className="rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-40"
              >
                {busy ? "Saving…" : "Save"}
              </button>
            </div>
          </div>
        )}

        {step === "done" && (
          <div className="flex flex-col gap-4 text-center">
            <p className="text-theme-success">All set — your Entities can use this model now.</p>
            <button
              type="button"
              onClick={onClose}
              className="self-center rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white"
            >
              Done
            </button>
          </div>
        )}

        {error && <p className="mt-3 text-xs text-theme-danger">{error}</p>}
      </div>
    </div>
  );
}
