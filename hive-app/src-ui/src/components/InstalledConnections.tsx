import { useEffect, useState } from "react";
import { detectCliProviders, type CliDiscovery } from "../lib/daemonClient";
import { MODEL_LABELS } from "../lib/modelReadiness";

const TOOLS = ["claude-cli", "codex-cli", "grok-cli", "gemini-cli"];
const FRIENDLY_LABELS: Record<string, string> = { "claude-cli": "Claude", "codex-cli": "ChatGPT", "grok-cli": "Grok", "gemini-cli": "Gemini" };
const SIGN_IN: Record<string, string> = {
  "claude-cli": "In Claude Code, run claude auth login, then check again here.",
  "codex-cli": "In a terminal, run codex login and finish sign-in with your own account, then check again here.",
  "grok-cli": "In a terminal, run grok login and finish sign-in with your own account, then check again here.",
};

export default function InstalledConnections({ busy, onSelect, compact = false }: { busy: boolean; onSelect: (provider: string) => void; compact?: boolean }) {
  const [discovery, setDiscovery] = useState<CliDiscovery | null>(null);
  const [error, setError] = useState<string>();
  const [checking, setChecking] = useState(true);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    setChecking(true);
    setError(undefined);
    setDiscovery(null);
    void detectCliProviders().then(result => {
      if (active) setDiscovery(result);
    }).catch(reason => {
      if (active) setError(reason instanceof Error ? reason.message : "Installed tools could not be checked.");
    }).finally(() => { if (active) setChecking(false); });
    return () => { active = false; };
  }, [revision]);

  return <section aria-label="Installed AI tools" className="flex flex-col gap-3">
    <div className="flex items-center justify-between gap-3">
      <h3 className="font-medium text-theme-text-bright">Accounts on this computer</h3>
      <button type="button" disabled={checking || busy} onClick={() => setRevision(value => value + 1)} className="text-sm text-theme-primary disabled:opacity-40">Check again</button>
    </div>
    <p className="text-xs text-theme-text-dim">Use a supported connection tool with your own account. Messages go to that provider and use your account's allowance. A signed-in chat app alone does not connect a model.</p>
    {checking && <p role="status" className="text-sm text-theme-text-dim">Looking for installed tools. No model messages are sent.</p>}
    {error && <p role="alert" className="text-sm text-theme-danger">{error} Local and API connections are still available.</p>}
    {discovery?.unavailable_reason && <p role="status" className="text-sm text-theme-text-dim">{discovery.unavailable_reason}</p>}
    {discovery && <details open={!compact} className="flex flex-col gap-3"><summary className="cursor-pointer text-sm text-theme-primary">{discovery.providers.filter(tool => tool.on_path).length} connection tools found · View options</summary>{TOOLS.map(provider => {
      const tool = discovery.providers.find(item => item.provider === provider);
      const supported = provider !== "gemini-cli";
      // Older daemons only report a boolean: false is inconclusive there.
      const authentication = tool?.authentication ?? (tool?.is_authenticated ? true : null);
      const presenceOnly = discovery.presence_only === true;
      const allowed = supported && !presenceOnly && !discovery.unavailable_reason && tool?.on_path && tool.is_official;
      const state = !tool?.on_path ? "Not found in a supported location"
        : !supported ? "Installed · not supported in Abigail yet"
        : presenceOnly ? "Installed · account not checked"
        : !tool.is_official ? "Installed · version not recognized"
        : authentication === false ? "Not signed in"
        : authentication === true ? "Available · signed in"
        : "Installed · sign-in unknown";
      return <div key={provider} className="rounded-theme-md border border-theme-border bg-theme-surface px-4 py-3">
        <h4 className="font-medium text-theme-text-bright">{FRIENDLY_LABELS[provider]}</h4>
        <p className="text-xs text-theme-text-dim">Via {MODEL_LABELS[provider]}</p>
        <p className="text-sm text-theme-text-dim">{state}</p>
        {!supported ? <p className="mt-1 text-xs text-theme-text-dim">Choose Google (Gemini) under Paste an API key.</p>
          : !tool?.on_path ? <p className="mt-1 text-xs text-theme-text-dim">Install the official {MODEL_LABELS[provider]} CLI to use this route, then check again. API and local connections also work.</p>
          : !presenceOnly && !tool.is_official ? <p className="mt-1 text-xs text-theme-text-dim">Check that this is the official, supported CLI version, then retry discovery.</p>
          : allowed && authentication !== true && <p className="mt-1 text-xs text-theme-text-dim">{authentication === null && "You can check the existing connection. If sign-in is needed: "}{SIGN_IN[provider]}</p>}
        {allowed && authentication !== false && <button type="button" disabled={busy} onClick={() => onSelect(provider)} className="mt-2 text-sm text-theme-primary disabled:opacity-40">Use {FRIENDLY_LABELS[provider]}</button>}
      </div>;
    })}</details>}
  </section>;
}
