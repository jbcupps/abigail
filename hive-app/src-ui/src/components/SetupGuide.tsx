import { useState, type MouseEvent } from "react";
import { detectRuntimeMode } from "../runtimeMode";
import type { SetupStatus } from "../lib/daemonClient";
import { SetupButton } from "./SetupControls";

// Application-owned guidance: generated prose never changes these destinations,
// credential instructions, next action, or the displayed connection status.
export default function SetupGuide({
  setup,
  onConnect,
}: {
  setup: SetupStatus | null;
  onConnect: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const openProvider = (
    event: MouseEvent<HTMLAnchorElement>,
    provider: string,
  ) => {
    if (detectRuntimeMode() !== "native") return;
    event.preventDefault();
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("open_provider_console", { provider }))
      .catch(() =>
        setError(
          "Could not open your browser. Visit the provider address shown here.",
        ),
      );
  };
  const local = !setup || setup.active_provider === "local";
  return (
    <section
      aria-labelledby="setup-guide-title"
      className="rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-5"
    >
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2
            id="setup-guide-title"
            className="font-semibold text-theme-text-bright"
          >
            Your setup steps
          </h2>
          <p className="mt-1 text-sm text-theme-text-dim">
            Local chat works offline without an account. Connecting a cloud
            model is optional.
          </p>
        </div>
        <p role="status" className="text-sm text-theme-text">
          {local
            ? "Current connection: local model"
            : `Connected: ${setup.active_provider} · ${setup.active_model}`}
        </p>
      </div>
      <ol className="grid gap-4 text-sm sm:grid-cols-3 lg:grid-cols-1">
        <li>
          <h3 className="font-medium">1. Create an API account</h3>
          <p className="mt-1 text-theme-text-dim">
            An API account is a developer account on the provider’s website.
            Create it there, not in Abigail. API billing may be separate from
            your chat subscription.
          </p>
          <div className="mt-2 flex flex-col gap-1">
            <a
              className="text-theme-primary-dim underline"
              href="https://platform.claude.com/"
              target="_blank"
              rel="noreferrer"
              onClick={(e) => openProvider(e, "anthropic")}
            >
              Anthropic · platform.claude.com
            </a>
            <a
              className="text-theme-primary-dim underline"
              href="https://platform.openai.com/"
              target="_blank"
              rel="noreferrer"
              onClick={(e) => openProvider(e, "openai")}
            >
              OpenAI · platform.openai.com
            </a>
          </div>
        </li>
        <li>
          <h3 className="font-medium">2. Create an API key</h3>
          <p className="mt-1 text-theme-text-dim">
            An API key is a secret generated inside that account. It is
            different from the account itself. Enter it only in Abigail’s
            Connect a model form, never in chat.
          </p>
        </li>
        <li>
          <h3 className="font-medium">3. Test your chosen model</h3>
          <p className="mt-1 text-theme-text-dim">
            Enter the key, choose a model, then select Test and connect. A real
            reply must succeed before the encrypted connection is saved. Cloud
            replies send recent conversation context to your provider.
          </p>
          <SetupButton variant="primary" className="mt-3" onClick={onConnect}>
            {local ? "Connect a model" : "Change model"}
          </SetupButton>
        </li>
      </ol>
      <p className="mt-4 text-xs text-theme-text-dim">
        Follow these setup steps and connection status for account and key
        instructions. The small local model’s explanations may be inaccurate.
      </p>
      {error && (
        <p role="alert" className="mt-2 text-sm text-theme-danger">
          {error}
        </p>
      )}
    </section>
  );
}
