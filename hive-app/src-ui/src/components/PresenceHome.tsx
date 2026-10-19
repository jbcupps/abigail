import { useEffect, useRef, useState } from "react";
import Presence from "../../../../entity-runtime-app/src-ui/src/presence/Presence";
import RuntimeChat from "../../../../entity-runtime-app/src-ui/src/components/ChatPanel";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { loadPresence, openPresence, previewEntities, type SavedPresence } from "../lib/presenceApi";
import type { EntityInfo, HiveStatus } from "../lib/daemonClient";
import FirstSignal from "./FirstSignal";

interface Props { preview: boolean; status: HiveStatus | null; onRefresh: () => void; onProviders: () => void }
export default function PresenceHome({ preview, status, onRefresh, onProviders }: Props) {
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<SavedPresence>();
  const [active, setActive] = useState<(SavedPresence & { url: string })>();
  const [profiles, setProfiles] = useState<Record<string, EntityProfile>>({});
  const [previewRows, setPreviewRows] = useState<EntityInfo[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string>();
  const [revision, setRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const openGeneration = useRef(0);
  const returnHome = () => { openGeneration.current++; setBusy(undefined); setCreating(false); setEditing(undefined); setActive(undefined); };
  useEffect(() => () => { openGeneration.current++; }, []);
  const entities = preview ? previewRows : (status?.entities ?? []).filter(entity => !entity.is_hive);
  useEffect(() => {
    if (!preview) return;
    try { setPreviewRows(previewEntities()); } catch (err) { setError(String(err)); }
  }, [preview, revision]);
  useEffect(() => {
    let cancelled = false;
    const rows = preview ? previewRows : (status?.entities ?? []).filter(entity => !entity.is_hive);
    void Promise.all(rows.map(async row => {
      try { return [row.id, await loadPresence(row.id, preview)] as const; }
      catch { return [row.id, defaultProfile(row.name)] as const; }
    })).then(values => { if (!cancelled) setProfiles(Object.fromEntries(values)); });
    return () => { cancelled = true; };
  }, [status, preview, previewRows, revision]);
  const open = async (id: string, intent: "open" | "edit", known?: EntityProfile) => {
    if (busy) return;
    const generation = ++openGeneration.current;
    setBusy(id); setError("");
    try {
      const profile = known ?? await loadPresence(id, preview);
      if (generation !== openGeneration.current) return;
      if (intent === "edit") { setEditing({ id, profile }); setCreating(true); }
      else {
        const url = preview ? "" : await openPresence(id);
        if (generation === openGeneration.current) setActive({ id, profile, url });
      }
    } catch (err) { if (generation === openGeneration.current) setError(err instanceof Error ? err.message : String(err)); }
    finally { if (generation === openGeneration.current) setBusy(undefined); }
  };
  const saved = (value: SavedPresence) => {
    setCreating(false); setEditing(undefined); setRevision(current => current + 1); onRefresh();
    void open(value.id, "open", value.profile);
  };
  return <div className="first-signal-shell">
    <header className="signal-nav"><a href={preview ? "?presencePreview=1" : "?"} onClick={event => { event.preventDefault(); if (!saving) returnHome(); }} aria-disabled={saving} className="signal-wordmark" aria-label="Abigail Hive home"><span aria-hidden="true">✳</span> ABIGAIL <small>HIVE</small></a><nav aria-label="Main navigation"><button disabled={saving} className="signal-nav-active" onClick={returnHome}>Your Entities</button>{!preview && <button disabled={saving} onClick={onProviders}>Model connections</button>}{!preview && status?.helper?.running && status.helper.local_url && <button disabled={saving} onClick={() => { openGeneration.current++; setBusy(undefined); setCreating(false); setActive({ id: "hive-helper", profile: defaultProfile("Abigail Hive"), url: status.helper!.local_url! }); }}>Ask Abigail</button>}</nav><span className="signal-nav-status"><i />{preview ? "DESIGN PREVIEW" : "HIVE CONNECTED"}</span></header>
    {preview && <div className="signal-preview-banner">LOCAL DESIGN PREVIEW <span>Identity drafts stay in this browser. No model, microphone, or provider is connected.</span><a href="?" aria-disabled={saving} onClick={event => { if (saving) event.preventDefault(); }}>Return to live Hive</a></div>}
    {creating ? <FirstSignal preview={preview} existing={editing} onBusyChange={setSaving} onClose={() => { setCreating(false); setEditing(undefined); setRevision(value => value + 1); onRefresh(); }} onSaved={saved} /> : active ? <RuntimeChat key={active.id} baseUrl={active.url} profile={active.profile} demo={preview} onBack={returnHome} /> : <main className="signal-home">
      <div className="signal-home-heading"><div><p className="signal-eyebrow">YOUR PERSONAL CONSTELLATION</p><h1>A space for<br /><em>good company.</em></h1><p>Distinct identities. Familiar presences.<br />A little more possibility in every conversation.</p></div><div className="signal-home-orbit"><Presence identity="aether" compact label="Decorative procedural portrait" /></div></div>
      <section className="signal-entities"><div className="signal-section-title"><h2>Your Entities <span>{entities.length}</span></h2><button className="signal-link" onClick={() => { setRevision(value => value + 1); onRefresh(); }}>Refresh ↻</button></div>
        <div className="signal-entity-grid"><button className="signal-create-card" onClick={() => { returnHome(); setCreating(true); }}><span className="signal-create-glyph">✧</span><strong>A new presence</strong><p>Start with an intention.<br />See who takes shape.</p><span className="signal-card-action">Create an Entity <span aria-hidden="true">↗</span></span></button>
          {entities.map(entity => <article className="signal-entity-card" key={entity.id}><Presence identity={profiles[entity.id]?.identity ?? "aether"} compact label={`${entity.name} procedural portrait`} /><div className="signal-card-copy"><span className="signal-eyebrow">{preview ? "SAVED PREVIEW" : "ENTITY"} / {profiles[entity.id]?.identity ?? "AETHER"}</span><h3>{profiles[entity.id]?.name || entity.name}</h3><p>{profiles[entity.id]?.purpose || "A conversation waiting to happen."}</p><div><button className="signal-secondary" disabled={!!busy} onClick={() => void open(entity.id, "open")}>{busy === entity.id ? "Opening…" : "Open conversation ↗"}</button><button className="signal-link" disabled={!!busy} onClick={() => void open(entity.id, "edit")}>Edit identity</button></div></div></article>)}
        </div>
      </section>
      <footer className="signal-home-footer"><span>✧ Your Hive stays here while you explore.</span><span>APPEARANCE IS PERSONAL. PERMISSIONS ARE SEPARATE.</span></footer>
    </main>}
    {error && <div className="signal-global-error" role="alert"><strong>Could not complete that step.</strong><p>{error}</p><button className="signal-link" onClick={() => setError("")}>Dismiss</button></div>}
  </div>;
}
