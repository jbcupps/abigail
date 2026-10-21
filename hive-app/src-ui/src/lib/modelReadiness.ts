import type { HiveStatus } from "./daemonClient";

export const MODEL_LABELS: Record<string, string> = {
  "claude-cli": "Claude Code",
  "codex-cli": "Codex (ChatGPT)",
  "grok-cli": "Grok Build",
  "gemini-cli": "Gemini CLI",
  anthropic: "Anthropic (Claude)",
  openai: "OpenAI",
  google: "Google (Gemini)",
  xai: "xAI (Grok)",
  perplexity: "Perplexity",
  local: "Local server",
};

export function modelSelectionLabel(status?: HiveStatus | null): string {
  if (!status) return "Model status unavailable";
  if (status.fixture === true) return "Synthetic replies · no model connected";
  if (status.ready_state === "needs_provider" || !status.any_provider_configured) return "No model connected";
  const selection = status.model_connection;
  if (!selection?.provider) return "Model configured · selection details unavailable";
  const provider = MODEL_LABELS[selection.provider] ?? selection.provider;
  return `Model selected: ${provider} · ${selection.model || (selection.provider === "local" ? "server-selected model" : "provider default")}`;
}

export class ModelRequiredError extends Error {
  constructor(readonly hiveUrl: string) {
    super("Connect a model in this Hive before opening a conversation. Your saved identity is still here.");
    this.name = "ModelRequiredError";
  }
}
