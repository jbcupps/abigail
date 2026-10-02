import { useCallback, useEffect, useRef, useState } from "react";
import {
  connectLocalModel,
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

type Step = "choose" | "local" | "key" | "model" | "done";

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
  const [localUrl, setLocalUrl] = useState("http://127.0.0.1:11434");
  const [connectionLabel, setConnectionLabel] = useState("");
  const [checkingCli, setCheckingCli] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const inFlight = useRef(false);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !inFlight.current) onClose();
      if (event.key !== "Tab") return;
      const controls = dialogRef.current?.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]',
      );
      if (!controls?.length) return;
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!dialogRef.current?.contains(document.activeElement)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && (document.activeElement === first || document.activeElement === dialogRef.current)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", keydown);
    return () => {
      document.removeEventListener("keydown", keydown);
      previous?.focus();
    };
  }, [onClose]);

  useEffect(() => {
    let active = true;
    detectCliProviders()
      .then((list) => {
        if (active) setCliProviders(list.filter((p) =>
          ["claude-cli", "codex-cli", "grok-cli"].includes(p.provider)
            && p.on_path && p.is_official
            && (p.provider !== "claude-cli" || p.is_authenticated),
        ));
      })
      .catch(() => { if (active) setCliProviders([]); });
    return () => { active = false; };
  }, []);

  const fail = useCallback((e: unknown) => setError(
    e instanceof TypeError
      ? "The Hive could not be reached. Retry in a moment."
      : e instanceof Error ? e.message : "The connection could not be saved. Try again.",
  ), []);

  const useCli = useCallback(async (cliProvider: string) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setCheckingCli(cliProvider);
    setBusy(true);
    setError(null);
    try {
      await setHiveDefault(cliProvider);
      setConnectionLabel(`${cliLabel(cliProvider)} is connected. Messages are processed through that connection. Memory stays on this computer.`);
      setStep("done");
      onComplete();
    } catch (e) {
      fail(e);
    } finally {
      inFlight.current = false;
      setCheckingCli(null);
      setBusy(false);
    }
  }, [onComplete, fail]);

  const guessed = guessProvider(apiKey);
  const effectiveProvider = provider || guessed || "";

  const submitKey = useCallback(async () => {
    const key = apiKey.trim();
    const p = effectiveProvider;
    if (!key || !p || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      const found = await discoverModels(p, key);
      if (found.length === 0) throw new Error("No models were found. Check the key and your provider account, then try again.");
      await storeSecret(p, key);
      setApiKey("");
      setModels(found);
      setSelectedModel(found[0]?.id ?? "");
      setProvider(p);
      setStep("model");
    } catch (e) {
      fail(e);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }, [apiKey, effectiveProvider, fail]);

  const saveLocal = useCallback(async () => {
    if (!localUrl.trim() || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      const connected = await connectLocalModel(localUrl);
      setConnectionLabel(`Local model ${connected.model} is connected. Messages are processed by your local server.`);
      setStep("done");
      onComplete();
    } catch (e) {
      fail(e);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }, [localUrl, onComplete, fail]);

  const saveModel = useCallback(async () => {
    if (!selectedModel || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await setHiveDefault(provider, selectedModel || undefined);
      setConnectionLabel(`${PROVIDER_LABELS[provider] ?? provider} is connected. Messages sent with this model are processed by that provider. Memory stays on this computer.`);
      setStep("done");
      onComplete();
    } catch (e) {
      fail(e);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }, [provider, selectedModel, onComplete, fail]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-theme-overlay p-4"
      onClick={() => { if (!busy) onClose(); }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="connect-model-title"
        tabIndex={-1}
        className="max-h-[calc(100dvh-2rem)] w-full max-w-md overflow-y-auto rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-6 shadow-theme-dropdown"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 id="connect-model-title" className="text-lg font-semibold text-theme-text-bright">Connect a model</h2>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="text-theme-text-dim hover:text-theme-text"
            aria-label="Close"
          >
            ✕
          </button>
        </div>

        {step === "choose" && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-theme-text-dim">
              Connect a model to start chatting. Local models process messages on your computer;
              cloud models send messages to the selected provider. Memory stays here.
            </p>
            <button
              type="button"
              disabled={busy}
              onClick={() => { setError(null); setStep("local"); }}
              className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3 text-left hover:border-theme-primary"
            >
              <div className="font-medium text-theme-text-bright">Use a local model</div>
              <div className="text-xs text-theme-text-dim">Connect a running Ollama or LM Studio server.</div>
            </button>
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
                <div className="text-xs text-theme-text-dim">
                  {cli.is_authenticated
                    ? "Abigail checks your existing connection before saving."
                    : cli.auth_hint || "Check your existing account connection. No API key is needed."}
                </div>
              </button>
            ))}
            <button
              type="button"
              disabled={busy}
              onClick={() => { setError(null); setStep("key"); }}
              className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3 text-left hover:border-theme-primary"
            >
              <div className="font-medium text-theme-text-bright">Paste an API key</div>
              <div className="text-xs text-theme-text-dim">
                Anthropic, OpenAI, Google, xAI, or Perplexity.
              </div>
            </button>
            {checkingCli && <p role="status" className="text-sm text-theme-text-dim">Checking your {cliLabel(checkingCli)} connection. This can take up to a minute.</p>}
          </div>
        )}

        {step === "local" && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-theme-text-dim">
              Start Ollama or LM Studio and load a model first. Abigail checks the server before connecting.
            </p>
            <label className="text-sm text-theme-text-dim">
              Local server address
              <input
                type="url"
                value={localUrl}
                onChange={(e) => setLocalUrl(e.target.value)}
                disabled={busy}
                className="mt-1 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
              />
            </label>
            <div className="flex gap-3 text-xs text-theme-primary">
              <button type="button" disabled={busy} onClick={() => setLocalUrl("http://127.0.0.1:11434")}>Ollama</button>
              <button type="button" disabled={busy} onClick={() => setLocalUrl("http://127.0.0.1:1234/v1")}>LM Studio</button>
            </div>
            <div className="flex justify-between">
              <button type="button" disabled={busy} onClick={() => { setError(null); setStep("choose"); }} className="text-sm text-theme-text-dim">Back</button>
              <button type="button" disabled={busy || !localUrl.trim()} onClick={() => void saveLocal()} className="rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white disabled:opacity-40">
                {busy ? "Checking…" : "Connect"}
              </button>
            </div>
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
                disabled={busy}
                autoFocus
                className="mt-1 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-sm text-theme-text outline-none focus:border-theme-primary"
              />
            </label>
            <label className="text-sm text-theme-text-dim">
              Provider
              <select
                value={effectiveProvider}
                onChange={(e) => setProvider(e.target.value)}
                disabled={busy}
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
                disabled={busy}
                onClick={() => { setError(null); setStep("choose"); }}
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
              aria-label="Model"
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              disabled={busy}
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
                disabled={busy || !selectedModel}
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
            <p className="text-theme-success">{connectionLabel}</p>
            <p className="text-sm text-theme-text-dim">You can start chatting now.</p>
            <button
              type="button"
              onClick={onClose}
              className="self-center rounded-theme-md bg-theme-primary px-4 py-2 text-sm font-medium text-white"
            >
              Done
            </button>
          </div>
        )}

        {error && <p role="alert" className="mt-3 text-xs text-theme-danger">{error}</p>}
      </div>
    </div>
  );
}
