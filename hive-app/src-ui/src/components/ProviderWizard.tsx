import { useState } from "react";
import { SetupButton, SetupDialog, SetupField } from "./SetupControls";
import {
  activateSetupModel,
  discoverModels,
  type ProviderModel,
  type SetupStatus,
} from "../lib/daemonClient";

export default function ProviderWizard({
  onClose,
  onComplete,
}: {
  onClose: () => void;
  onComplete: (status: SetupStatus) => void;
}) {
  const [provider, setProvider] = useState("anthropic");
  const [key, setKey] = useState("");
  const [model, setModel] = useState("");
  const [models, setModels] = useState<ProviderModel[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const discover = async () => {
    setBusy(true);
    setError(null);
    try {
      setModels(await discoverModels(provider, key.trim()));
    } catch {
      setError(
        "Could not list models. Check your connection and API key, or enter a model ID from your provider.",
      );
    } finally {
      setBusy(false);
    }
  };
  const activate = async () => {
    setBusy(true);
    setError(null);
    try {
      const status = await activateSetupModel(provider, model.trim(), key.trim());
      setKey("");
      setDone(true);
      onComplete(status);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <SetupDialog onClose={onClose} busy={busy}>
      <div className="mb-4 flex items-center justify-between">
        <h2 id="connect-title" className="text-lg font-semibold">
          Connect a model
        </h2>
        <SetupButton
          type="button"
          disabled={busy}
          onClick={onClose}
          aria-label="Close"
        >
          ✕
        </SetupButton>
      </div>
      {done ? (
        <>
          <p role="status">
            Connected. I’m still Abigail, and our conversation continues right
            here.
          </p>
          <SetupButton onClick={onClose} variant="primary" className="mt-4">
            Continue with Abigail
          </SetupButton>
        </>
      ) : (
        <div className="space-y-4">
          <p className="text-sm text-theme-text-dim">
            Use an API account from your provider. A chat subscription may not
            include API access. This form keeps your key out of our conversation
            and saves it encrypted only after a successful test.
          </p>
          <label className="block text-sm">
            Provider
            <select
              disabled={busy}
              value={provider}
              onChange={(e) => {
                setProvider(e.target.value);
                setModels([]);
                setModel("");
                setKey("");
                setError(null);
              }}
              className="mt-1 w-full rounded-theme-md bg-theme-input-bg p-2"
            >
              <option value="anthropic">Anthropic (Claude)</option>
              <option value="openai">OpenAI</option>
            </select>
          </label>
          <SetupField
            label="API key"
            type="password"
            autoComplete="off"
            spellCheck={false}
            disabled={busy}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            className="mt-1 w-full rounded-theme-md border border-theme-border bg-theme-input-bg p-2"
          />
          <SetupButton
            type="button"
            onClick={() => void discover()}
            disabled={busy || !key.trim()}
            className="text-sm text-theme-primary disabled:opacity-40"
          >
            {busy ? "Checking…" : "List available models"}
          </SetupButton>
          {models.length > 0 && (
            <label className="block text-sm">
              Available models
              <select
                disabled={busy}
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="mt-1 w-full rounded-theme-md bg-theme-input-bg p-2"
              >
                <option value="">Choose a model…</option>
                {models.map((m) => (
                  <option key={m.model_id} value={m.model_id}>
                    {m.display_name ?? m.model_id}
                  </option>
                ))}
              </select>
            </label>
          )}
          <SetupField
            label="Model ID"
            disabled={busy}
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="Enter or select your model"
            className="mt-1 w-full rounded-theme-md border border-theme-border bg-theme-input-bg p-2"
          />
          <p className="text-sm text-theme-text-dim">
            Connecting sends a small test request that may incur an API charge.
            Subsequent replies send recent conversation context to this
            provider. You can return to the local model at any time.
          </p>
          <SetupButton
            type="button"
            disabled={busy || !key.trim() || !model.trim()}
            onClick={() => void activate()}
            variant="primary"
          >
            {busy ? "Validating connection…" : "Test and connect"}
          </SetupButton>
        </div>
      )}
      {error && (
        <p role="alert" className="mt-3 text-sm text-theme-danger">
          {error}
        </p>
      )}
    </SetupDialog>
  );
}
