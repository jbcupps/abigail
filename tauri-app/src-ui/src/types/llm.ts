/** Shared types for Ollama, LLM provider, and CLI detection interfaces. */

export interface OllamaDetection {
  status: "running" | "installed" | "not_found";
  path: string | null;
}

export interface RecommendedModel {
  name: string;
  label: string;
  size_bytes: number;
  description: string;
  recommended: boolean;
}

export interface InstalledModel {
  name: string;
  size: number;
  modified_at: string;
}

export interface OllamaInstallProgress {
  step: string;
  written?: number;
  total?: number;
  message: string;
}

export interface OllamaModelProgress {
  model: string;
  completed?: number;
  total?: number;
  status: string;
}

export interface CliDetection {
  provider_name: string;
  binary: string;
  on_path: boolean;
  is_official: boolean;
  is_authenticated: boolean;
  version: string | null;
  auth_hint: string | null;
}

/** Format a byte count to human-readable string (e.g. "2.1 GB"). */
export function formatBytes(value: number | undefined): string {
  if (!value || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}
