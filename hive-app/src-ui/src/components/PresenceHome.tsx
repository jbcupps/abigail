import { useEffect, useRef, useState } from "react";
import Presence from "../../../../entity-runtime-app/src-ui/src/presence/Presence";
import RuntimeChat from "../../../../entity-runtime-app/src-ui/src/components/ChatPanel";
import { defaultProfile, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { loadPresence, openPresence, previewEntities, type SavedPresence } from "../lib/presenceApi";
import type { EntityInfo, HiveStatus } from "../lib/daemonClient";
import FirstSignal from "./FirstSignal";
import { readCreationDraft } from "../lib/creationDraft";

interface Props { preview: boolean; status: HiveStatus | null; onRefresh: () => void; onProviders: () => void }
export default function PresenceHome({ preview, status, onRefresh, onProviders }: Props) {
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<SavedPresence>();
  const [active, setActive] = useState<(SavedPresence & { url: string; notice?: string })>();
  const [profiles, setProfiles] = useState<Record<string, EntityProfile>>({});
  const [unavailableProfiles, setUnavailableProfiles] = useState<Record<string, boolean>>({});
  const [previewRows, setPreviewRows] = useState<EntityInfo[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string>();
  const [revision, setRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const [unfinishedCreation, setUnfinishedCreation] = useState(false);
  const openGeneration = useRef(0);
  const pendingOpen = useRef<AbortController | null>(null);
  const returnHome = () => { openGeneration.current++; pendingOpen.current?.abort(); setBusy(undefined); setCreating(false); setEditing(undefined); setActive(undefined); };
  useEffect(() => () => { openGeneration.current++; pendingOpen.current?.abort(); }, []);
  useEffect(() => {
    if (preview) { setUnfinishedCreation(false); return; }
    try { setUnfinishedCreation(readCreationDraft() !== null); }
    catch { setUnfinishedCreation(true); }
  }, [preview, creating]);
  const entities = preview ? previewRows : (status?.entities ?? []).filter(entity => !entity.is_hive);
  useEffect(() => {
    if (!preview) return;
    try { setPreviewRows(previewEntities()); } catch (err) { setError(String(err)); }
  }, [preview, revision]);
  useEffect(() => {
    const controller = new AbortController();
    const rows = preview ? previewRows : (status?.entities ?? []).filter(entity => !entity.is_hive);
    // Restore each card independently: one unavailable profile must not hold
    // every other identity behind it.
    for (const row of rows) {
      void loadPresence(row.id, preview, controller.signal).then(profile => {
        if (!controller.signal.aborted) {
          setProfiles(current => ({ ...current, [row.id]: profile }));
          setUnavailableProfiles(current => ({ ...current, [row.id]: false }));
        }
      }).catch(() => {
        if (!controller.signal.aborted) {
          setProfiles(current => ({ ...current, [row.id]: defaultProfile(row.name) }));
          setUnavailableProfiles(current => ({ ...current, [row.id]: true }));
        }
      });
    }
    return () => controller.abort();
  }, [status, preview, previewRows, revision]);
  const open = async (id: string, intent: "open" | "edit", known?: EntityProfile, hiveUrl?: string) => {
    if (busy) return;
    const generation = ++openGeneration.current;
    pendingOpen.current?.abort();
    const controller = new AbortController();
    pendingOpen.current = controller;
    const isCurrent = () => generation === openGeneration.current && !controller.signal.aborted;
    setBusy(id); setError("");
    try {
      if (intent === "edit" || preview) {
        // Editing requires the saved brief; a failed read must never offer a
        // default form that could overwrite an existing identity.
        const profile = known ?? await loadPresence(id, preview, controller.signal, hiveUrl);
        if (!isCurrent()) return;
        if (intent === "edit") { setEditing({ id, profile, hiveUrl }); setCreating(true); }
        else setActive({ id, profile, hiveUrl, url: "" });
      } else {
        const fallback = defaultProfile(entities.find(entity => entity.id === id)?.name || known?.name || "Entity");
        const restoration = known ? Promise.resolve({ profile: known, notice: undefined }) : loadPresence(id, false, controller.signal, hiveUrl)
          .then(profile => ({ profile, notice: undefined }))
          .catch(() => ({ profile: fallback, notice: "Saved appearance unavailable. Using the default appearance; your saved brief has not been changed." }));
        const url = await openPresence(id, controller.signal, hiveUrl);
        if (!isCurrent()) return;
        setActive({ id, profile: known ?? fallback, hiveUrl, url, notice: known ? undefined : "Loading saved appearance…" });
        void restoration.then(restored => {
          if (isCurrent()) setActive(current => current?.id === id ? { ...current, ...restored } : current);
        });
      }
    } catch (err) {
      if (isCurrent()) setError(err instanceof Error ? err.message : String(err));
      controller.abort();
    }
    finally { if (generation === openGeneration.current) setBusy(undefined); }
  };
  const persisted = () => { setRevision(current => current + 1); onRefresh(); };
  const saved = (value: SavedPresence) => {
    setCreating(false); setEditing(undefined);
    void open(value.id, "open", value.profile, value.hiveUrl);
  };
  return <div className="first-signal-shell">
    <header className="signal-nav"><a href={preview ? "?presencePreview=1" : "?"} onClick={event => { event.preventDefault(); if (!saving) returnHome(); }} aria-disabled={saving} className="signal-wordmark" aria-label="Abigail Hive home"><span aria-hidden="true">✳</span> ABIGAIL <small>HIVE</small></a><nav aria-label="Main navigation"><button disabled={saving} className="signal-nav-active" onClick={returnHome}>Your Entities</button>{!preview && <button disabled={saving} onClick={onProviders}>Model connections</button>}{!preview && status?.helper?.running && status.helper.local_url && <button disabled={saving} onClick={() => { returnHome(); setActive({ id: "hive-helper", profile: defaultProfile("Abigail Hive"), url: status.helper!.local_url! }); }}>Ask Abigail</button>}</nav><span className="signal-nav-status"><i />{preview ? "DESIGN PREVIEW" : "HIVE CONNECTED"}</span></header>
    {preview && <div className="signal-preview-banner">LOCAL DESIGN PREVIEW <span>Identity drafts stay in this browser. No model, microphone, or provider is connected.</span><a href="?" aria-disabled={saving} onClick={event => { if (saving) event.preventDefault(); }}>Return to live Hive</a></div>}
    {creating ? <FirstSignal preview={preview} existing={editing} onBusyChange={setSaving} onClose={() => { setCreating(false); setEditing(undefined); setRevision(value => value + 1); onRefresh(); }} onPersisted={persisted} onSaved={saved} /> : active ? <RuntimeChat key={active.id} baseUrl={active.url} profile={active.profile} profileNotice={active.notice} demo={preview} onBack={returnHome} /> : <main className="signal-home">
      <div className="signal-home-heading"><div><p className="signal-eyebrow">YOUR PERSONAL CONSTELLATION</p><h1>A space for<br /><em>good company.</em></h1><p>Distinct identities. Familiar presences.<br />A little more possibility in every conversation.</p></div><div className="signal-home-orbit"><Presence identity="aether" compact label="Decorative procedural portrait" /></div></div>
      <section className="signal-entities"><div className="signal-section-title"><h2>Your Entities <span>{entities.length}</span></h2><button className="signal-link" onClick={() => { setRevision(value => value + 1); onRefresh(); }}>Refresh ↻</button></div>
        <div className="signal-entity-grid"><button className="signal-create-card" onClick={() => { returnHome(); setCreating(true); }}><span className="signal-create-glyph">✧</span><strong>{unfinishedCreation ? "An unfinished presence" : "A new presence"}</strong><p>{unfinishedCreation ? "Your brief and creation record are kept in this tab." : <>Start with an intention.<br />See who takes shape.</>}</p><span className="signal-card-action">{unfinishedCreation ? "Resume creation" : "Create an Entity"} <span aria-hidden="true">↗</span></span></button>
          {entities.map(entity => <article className="signal-entity-card" key={entity.id}><Presence identity={profiles[entity.id]?.identity ?? "aether"} compact label={`${entity.name} procedural portrait`} /><div className="signal-card-copy"><span className="signal-eyebrow">{preview ? "SAVED PREVIEW" : "ENTITY"} / {profiles[entity.id]?.identity ?? "AETHER"}</span><h3>{profiles[entity.id]?.name || entity.name}</h3>{unavailableProfiles[entity.id] && <p className="signal-fine">Saved appearance unavailable. Default shown.</p>}<p>{profiles[entity.id]?.purpose || "A conversation waiting to happen."}</p><div><button className="signal-secondary" disabled={!!busy} onClick={() => void open(entity.id, "open")}>{busy === entity.id ? "Opening…" : "Open conversation ↗"}</button><button className="signal-link" disabled={!!busy} onClick={() => void open(entity.id, "edit")}>Edit identity</button></div></div></article>)}
        </div>
      </section>
      <footer className="signal-home-footer"><span>✧ Your Hive stays here while you explore.</span><span>APPEARANCE IS PERSONAL. PERMISSIONS ARE SEPARATE.</span></footer>
    </main>}
    {error && <div className="signal-global-error" role="alert"><strong>Could not complete that step.</strong><p>{error}</p><button className="signal-link" onClick={() => setError("")}>Dismiss</button></div>}
  </div>;
}
