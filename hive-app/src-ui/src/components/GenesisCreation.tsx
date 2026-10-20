import { useEffect, useRef, useState } from "react";
import Presence from "../../../../entity-runtime-app/src-ui/src/presence/Presence";
import { type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { createOrResumeGenesis, genesisAction, resolveGenesisConnection, type GenesisSession } from "../lib/genesisClient";
import { assertGenesisDraft, clearGenesisDraft, GENESIS_DRAFT_KEY, readGenesisDraft, saveGenesisDraft, type GenesisDraft } from "../lib/genesisDraft";
import { readCreationDraft } from "../lib/creationDraft";
import { withCreationLock } from "../lib/presenceCreation";
import { loadPresence, savePresence, type SavedPresence } from "../lib/presenceApi";
import FirstSignal from "./FirstSignal";
import GenesisWorkspace from "./GenesisWorkspace";

interface Props { preview: boolean; existing?: SavedPresence; browse?: boolean; onClose: () => void; onSaved: (value: SavedPresence) => void; onReturn: (id: string, hiveUrl?: string) => void; onPersisted: () => void; onBusyChange: (busy: boolean) => void }
export default function GenesisCreation(props: Props) {
  const [initial] = useState(() => {
    if (props.preview || props.existing) return { legacy: true, draft: null, error: "" };
    try { return { legacy: !!readCreationDraft(), draft: readGenesisDraft(), error: "" }; }
    catch (error) { return { legacy: false, draft: null, error: String(error) }; }
  });
  const [draft, setDraft] = useState(initial.draft);
  const [session, setSession] = useState<GenesisSession | null>(null);
  const [revealed, setRevealed] = useState<SavedPresence | null>(null);
  const [error, setError] = useState(initial.error);
  const [stage, setStage] = useState("");
  const [paused, setPaused] = useState(false);
  const flight = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  const changed = useRef(false);
  useEffect(() => {
    mounted.current = true;
    if (initial.legacy) return () => { mounted.current = false; };
    const stop = (event: StorageEvent) => {
      if (event.storageArea && event.storageArea !== localStorage) return;
      if (event.key !== GENESIS_DRAFT_KEY && event.key !== null) return;
      changed.current = true; flight.current?.abort(); flight.current = null; setStage(""); props.onBusyChange(false);
      setError("This creation changed in another tab. Return to Hive and reopen the saved creation. An accepted request may still finish.");
    };
    window.addEventListener("storage", stop);
    return () => { mounted.current = false; flight.current?.abort(); window.removeEventListener("storage", stop); };
  }, [props.onBusyChange, initial.legacy]);
  const active = (control: AbortController) => mounted.current && flight.current === control && !control.signal.aborted && !changed.current;
  const run = async (work: (control: AbortController) => Promise<void>) => {
    if (flight.current || changed.current) return;
    const control = new AbortController(); flight.current = control; setError(""); setStage("Checking your saved creation"); props.onBusyChange(true);
    try { await withCreationLock(async () => { if (active(control)) await work(control); }); }
    catch (reason) { if (active(control)) setError(`${reason instanceof Error ? reason.message : reason} Your creation record is kept. Resume uses the same key.`); }
    finally { if (flight.current === control) { flight.current = null; if (mounted.current) { setStage(""); props.onBusyChange(false); } } }
  };
  const finish = async (current: GenesisSession, record: GenesisDraft | null, control: AbortController) => {
    const id = current.definition.entity_id;
    if (current.state !== "ready" || !id || !current.connection) throw new Error("Hive has not confirmed this Entity is ready.");
    if (!active(control)) return;
    if (record) assertGenesisDraft(record);
    setStage("Saving the familiar face");
    const presentation = record?.profile ?? await loadPresence(id, false, control.signal, current.connection.hiveUrl);
    const profile = { ...presentation, name: current.definition.name, purpose: current.definition.purpose };
    if (!active(control)) return;
    await savePresence(id, profile, false, { signal: control.signal, hiveUrl: current.connection.hiveUrl });
    if (!active(control)) return;
    if (record) clearGenesisDraft(record);
    setRevealed({ id, profile, hiveUrl: current.connection.hiveUrl }); setSession(null); props.onPersisted();
  };
  const resume = (record: GenesisDraft, explore: boolean, control: AbortController) => (async () => {
    assertGenesisDraft(record);
    setStage("Restoring this spark in Hive");
    let current = await createOrResumeGenesis(record.key, record.seed, { connection: record.connection, signal: control.signal });
    if (!active(control)) return;
    if (current.id !== record.key) throw new Error("Hive returned a different creation. The saved key is retained.");
    if (current.state !== "ready" && explore) { setSession(current); return; }
    if (current.state !== "ready") {
      setStage("Awakening your Entity · saving identity");
      current = await genesisAction(current, "activate", {}, control.signal);
    }
    await finish(current, record, control);
  })();
  const begin = (profile: EntityProfile, explore: boolean) => void run(async control => {
    if (readCreationDraft()) throw new Error("An older presentation-only creation needs to be resumed first.");
    const { connection } = await resolveGenesisConnection();
    if (!active(control)) return;
    const record: GenesisDraft = { version: 1, key: crypto.randomUUID(), profile, connection, seed: { mode: "quick", name: profile.name, purpose: profile.purpose } };
    saveGenesisDraft(record); setDraft(record);
    await resume(record, explore, control);
  });
  const stop = () => { flight.current?.abort(); flight.current = null; setStage(""); props.onBusyChange(false); setError("Waiting stopped. Hive may still finish the accepted step. Resume checks the same saved creation; no new Entity is requested automatically."); };
  if (initial.legacy) return <FirstSignal {...props} />;
  if (revealed) return <main className="signal-reveal"><span className="signal-eyebrow">FIRST SIGNAL / IDENTITY SAVED</span><div className="reveal-portrait"><Presence identity={revealed.profile.identity} reducedMotion={paused} label={`${revealed.profile.name}, procedural portrait`} /></div><div className="reveal-copy"><h1>Meet {revealed.profile.name}.</h1><p>A familiar presence. A new beginning.</p><p className="signal-fine">Your approved identity and visual preferences are saved in Hive. Your conversation is ready when you are.</p><button className="signal-primary" onClick={() => props.onSaved(revealed)}>Start a conversation</button><button className="signal-link" onClick={props.onClose}>Back to Your Entities</button><label className="signal-check"><input type="checkbox" checked={paused} onChange={event => setPaused(event.target.checked)} />Pause portrait</label></div></main>;
  if (session || props.browse) return <><GenesisWorkspace initialSession={session ?? undefined} resumeOnly={!session} onClose={props.onClose} onBusyChange={props.onBusyChange} onCreated={current => run(control => finish(current, draft?.key === current.id ? draft : null, control))} onMeet={current => { if (draft?.key === current.id) void run(control => finish(current, draft, control)); else if (current.definition.entity_id) props.onReturn(current.definition.entity_id, current.connection?.hiveUrl); }} />{error && <p className="signal-error" role="alert">{error}</p>}</>;
  if (draft || initial.error || stage) return <main className="signal-builder"><section className="signal-creation-stage"><p className="signal-eyebrow">FIRST SIGNAL / ONE SAVED SPARK</p><Presence identity={draft?.profile.identity ?? "aether"} state={stage ? "thinking" : error ? "interrupted" : "idle"} label={stage || "Saved creation awaiting your next step"} /></section><section className="signal-editor"><div className="signal-editor-content"><h1>{draft?.profile.name || "Your creation"}</h1><p>{draft?.profile.purpose}</p><p role="status">{stage || "Your place is kept. Continue when you are ready."}</p>{error && <p role="alert" className="signal-error">{error}</p>}{draft && <p className="signal-fine">The same saved creation is used after a retry or reopen. Clearing browser data removes this local record; saved Genesis sessions remain in Hive.</p>}<div className="genesis-actions">{stage ? <button className="signal-secondary" onClick={stop}>Stop waiting</button> : <>{draft && !changed.current && <><button className="signal-primary" onClick={() => void run(control => resume(draft, false, control))}>Resume awakening</button><button className="signal-secondary" onClick={() => void run(control => resume(draft, true, control))}>Explore saved character</button></>}<button className="signal-link" onClick={props.onClose}>Back to Hive</button></>}</div></div></section></main>;
  return <><FirstSignal {...props} onCanonicalCreate={begin} />{error && <p className="signal-error" role="alert">{error}</p>}</>;
}
