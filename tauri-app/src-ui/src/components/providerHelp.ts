import type { TooltipLink } from "./HelpTooltip";

export interface HelpInfo {
  title: string;
  description: string;
  links: TooltipLink[];
}

export interface ApiProviderHelp extends HelpInfo {
  id: string;
  label: string;
}

export const API_PROVIDER_HELP: ApiProviderHelp[] = [
  {
    id: "openai",
    label: "OpenAI",
    title: "OpenAI API",
    description:
      "Use an OpenAI API key to route chat and tool use through GPT models hosted by OpenAI.",
    links: [
      {
        href: "https://platform.openai.com/docs/overview",
        label: "OpenAI API docs",
      },
    ],
  },
  {
    id: "anthropic",
    label: "Anthropic",
    title: "Anthropic API",
    description:
      "Use an Anthropic API key to route the entity through Claude models over the Anthropic API.",
    links: [
      {
        href: "https://docs.anthropic.com/",
        label: "Anthropic docs",
      },
    ],
  },
  {
    id: "google",
    label: "Google (Gemini)",
    title: "Gemini API",
    description:
      "Use Google AI Studio or Gemini API credentials to route the entity through Gemini-hosted models.",
    links: [
      {
        href: "https://ai.google.dev/gemini-api/docs",
        label: "Gemini API docs",
      },
    ],
  },
  {
    id: "xai",
    label: "X.AI (Grok)",
    title: "xAI API",
    description:
      "Use an xAI API key to route the entity through Grok-hosted models and compatible APIs.",
    links: [
      {
        href: "https://docs.x.ai/",
        label: "xAI docs",
      },
    ],
  },
];

const CLI_PROVIDER_HELP: Record<string, HelpInfo> = {
  "claude-cli": {
    title: "Claude CLI",
    description:
      "Use Claude Code as a local orchestrator when you want desktop-style operation through the Anthropic CLI.",
    links: [
      {
        href: "https://docs.anthropic.com/en/docs/claude-code/overview",
        label: "Claude Code docs",
      },
    ],
  },
  "gemini-cli": {
    title: "Gemini CLI",
    description:
      "Use the Gemini CLI when you want a local command-line orchestrator backed by Google Gemini.",
    links: [
      {
        href: "https://github.com/google-gemini/gemini-cli",
        label: "Gemini CLI project",
      },
    ],
  },
  "codex-cli": {
    title: "Codex CLI",
    description:
      "Use the OpenAI CLI toolchain when you want a local orchestrator backed by OpenAI-hosted models.",
    links: [
      {
        href: "https://platform.openai.com/docs/overview",
        label: "OpenAI docs",
      },
    ],
  },
  "grok-cli": {
    title: "Grok CLI",
    description:
      "Use the xAI CLI path when you want local orchestration backed by Grok and xAI-hosted models.",
    links: [
      {
        href: "https://docs.x.ai/",
        label: "xAI docs",
      },
    ],
  },
};

export const OLLAMA_HELP: HelpInfo = {
  title: "Ollama",
  description:
    "Ollama runs local models on this machine. Use it when you want offline or local-first inference instead of a hosted API provider.",
  links: [
    {
      href: "https://ollama.com/download",
      label: "Download Ollama",
    },
    {
      href: "https://ollama.com/library",
      label: "Model library",
    },
  ],
};

export function getCliProviderHelp(providerName: string): HelpInfo | null {
  return CLI_PROVIDER_HELP[providerName] ?? null;
}
