import { useEffect, useState } from "react";
import { chooseRoutingModels, getRoutingChoices, type ModelConnection, type RoutingChoices as Choices } from "../lib/daemonClient";
import { MODEL_LABELS } from "../lib/modelReadiness";

const key = (connection: ModelConnection) => JSON.stringify([connection.provider, connection.model, connection.local_url]);
const label = (connection: ModelConnection) => `${MODEL_LABELS[connection.provider] ?? connection.provider} · ${connection.model || "provider default"}`;

export default function RoutingChoices({ onSaved, busy, onBusyChange }: { onSaved: () => void; busy: boolean; onBusyChange?: (busy: boolean) => void }) {
  const [choices, setChoices] = useState<Choices | null>(null);
  const [allowed, setAllowed] = useState<ModelConnection[]>([]);
  const [revision, setRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  useEffect(() => {
    let active = true;
    setError(""); setChoices(null);
    void getRoutingChoices().then(result => { if (active) { setChoices(result); setAllowed(result.policy.allowed); } }).catch(reason => { if (active) setError(reason instanceof Error ? reason.message : "Model choices are unavailable."); });
    return () => { active = false; };
  }, [revision]);
  const toggle = (connection: ModelConnection) => {
    setNotice("");
    setAllowed(current => current.some(route => key(route) === key(connection)) ? current.filter(route => key(route) !== key(connection)) : [...current, connection]);
  };
  const save = async () => {
    if (!choices || saving) return;
    setSaving(true); onBusyChange?.(true); setError(""); setNotice("");
    try {
      const next = await chooseRoutingModels(choices.policy, allowed);
      setChoices(next); setAllowed(next.policy.allowed);
      setNotice(next.policy.allowed.length ? "Saved. Abigail can use only these models." : "Saved. Conversations will wait until you allow a model.");
      onSaved();
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not save model choices."); }
    finally { setSaving(false); onBusyChange?.(false); }
  };
  if (!choices && !error) return <p role="status" className="text-xs text-theme-text-dim">Reading your model choices…</p>;
  if (choices?.policy.connections.length === 0) return null;
  return <section aria-label="Allowed models" className="rounded-theme-md border border-theme-border p-3 flex flex-col gap-2 text-theme-text">
    <h3 className="font-medium text-theme-text-bright">Models Abigail may use</h3>
    <p className="text-xs text-theme-text-dim">Choose up to three. Abigail tries your first choice, then another checked model if it is unavailable. Cloud connections use their account's allowance or API billing; local connections stay on your computer.</p>
    {choices?.policy.connections.map(connection => {
      const selected = allowed.some(route => key(route) === key(connection));
      const ready = choices.readiness.find(item => key(item.connection) === key(connection))?.ready === true;
      return <div key={key(connection)} className="text-sm">
        <label className="flex gap-2 items-start"><input type="checkbox" checked={selected} disabled={busy || saving || (!selected && (allowed.length >= 3 || !ready))} onChange={() => toggle(connection)} />{label(connection)}</label>
        {!ready && <p className="text-xs text-theme-text-dim">Unavailable now. Reconnect below; this choice will not add another provider.</p>}
        {selected && <button type="button" disabled={busy || saving || key(allowed[0]) === key(connection)} onClick={() => setAllowed(current => [connection, ...current.filter(route => key(route) !== key(connection))])} className="text-xs text-theme-primary disabled:opacity-50">{key(allowed[0]) === key(connection) ? "First choice" : "Try first"}</button>}
      </div>;
    })}
    {error && <p role="alert" className="text-xs text-theme-danger">{error}</p>}
    {notice && <p role="status" className="text-xs text-theme-text">{notice}</p>}
    <div className="flex justify-between"><button type="button" disabled={saving || busy} onClick={() => setRevision(value => value + 1)} className="text-xs text-theme-primary">Refresh choices</button><button type="button" disabled={!choices || saving || busy} onClick={() => void save()} className="text-sm text-theme-primary">{saving ? "Saving…" : "Save allowed models"}</button></div>
  </section>;
}
