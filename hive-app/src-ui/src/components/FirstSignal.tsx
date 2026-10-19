import { useEffect, useRef, useState } from "react";
import Presence from "../../../../entity-runtime-app/src-ui/src/presence/Presence";
import { defaultProfile, extractProfile, identities, profileMarkdown, temperaments, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { createPresenceEntity, savePresence, type SavedPresence } from "../lib/presenceApi";

interface Props { preview: boolean; existing?: SavedPresence; onClose: () => void; onSaved: (value: SavedPresence) => void; onPersisted?: () => void; onBusyChange?: (busy: boolean) => void }
export default function FirstSignal({ preview, existing, onClose, onSaved, onPersisted, onBusyChange }: Props) {
  const [profile, setProfile] = useState<EntityProfile>(existing?.profile ?? defaultProfile());
  const [step, setStep] = useState(0);
  const [proposal, setProposal] = useState<Partial<EntityProfile> | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [revealed, setRevealed] = useState<SavedPresence | null>(null);
  const [paused, setPaused] = useState(false);
  const createdId = useRef(existing?.id);
  const inFlight = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus(); }, [step, revealed]);
  const update = <K extends keyof EntityProfile>(key: K, value: EntityProfile[K]) => setProfile(previous => ({ ...previous, [key]: value }));
  const save = async () => {
    if (inFlight.current || !profile.name.trim()) return;
    inFlight.current = true; setBusy(true); onBusyChange?.(true); setError("");
    try {
      const clean = { ...profile, name: profile.name.trim() };
      const id = createdId.current ?? await createPresenceEntity(clean.name, preview);
      createdId.current = id;
      await savePresence(id, clean, preview);
      setProfile(clean);
      setRevealed({ id, profile: clean });
      onPersisted?.();
    } catch (err) { setError(`${createdId.current && !existing && !preview ? "Entity created; saving its brief is not confirmed. Retry will reuse this entity. " : ""}${err instanceof Error ? err.message : err}`); }
    finally { setBusy(false); onBusyChange?.(false); inFlight.current = false; }
  };
  const importFile = async (file?: File) => {
    if (!file) return;
    setError(""); setProposal(null);
    try {
      if (!/\.(md|markdown|txt)$/i.test(file.name) || file.size > 32768) throw new Error("Choose a .md, .markdown, or .txt file up to 32 KB.");
      setProposal(extractProfile(await file.text()));
    } catch (err) { setError(err instanceof Error ? err.message : String(err)); }
  };
  const exportBrief = () => {
    const url = URL.createObjectURL(new Blob([profileMarkdown(profile)], { type: "text/markdown" }));
    const anchor = document.createElement("a"); anchor.href = url; anchor.download = "entity-brief.md"; anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  if (revealed) return <main className="signal-reveal">
    <span className="signal-eyebrow">FIRST SIGNAL / IDENTITY SAVED</span>
    <div className={`reveal-portrait${paused ? " reveal-still" : ""}`}><Presence identity={profile.identity} reducedMotion={paused} label={`${profile.name}, procedural portrait`} /></div>
    <div className="reveal-copy"><h1 ref={heading} tabIndex={-1}>Meet {profile.name}.</h1><p>A familiar presence. A new beginning.</p><p className="signal-fine">{preview ? "Saved in this browser's design preview. No runtime entity was created." : "Entity and visual brief saved locally by Hive. Conversation uses your configured runtime connection."}</p>
      <button className="signal-primary" onClick={() => onSaved(revealed)}>Continue to conversation <span aria-hidden="true">↗</span></button>
      <button className="signal-link" onClick={() => onSaved(revealed)}>Skip reveal</button>
      <label className="signal-check"><input type="checkbox" checked={paused} onChange={e => setPaused(e.target.checked)} /> Pause portrait</label>
    </div>
  </main>;
  return <main className="signal-builder">
    <section className="signal-creation-stage">
      <div className="signal-stage-top"><span className="signal-eyebrow">FIRST SIGNAL / 0{step + 1}</span><span className="signal-pill">{preview ? "DESIGN PREVIEW" : "LOCAL CREATION"}</span></div>
      <div className="signal-stage-title"><p>A little less interface.</p><h1>A little more<br /><em>presence.</em></h1></div>
      <Presence identity={profile.identity} reducedMotion={paused} label="Procedural identity preview, not a live runtime signal" />
      <div className="signal-stage-bottom"><span><i /> PROCEDURAL PORTRAIT <small>Original motion study · audio off</small></span><label className="signal-check"><input type="checkbox" checked={paused} onChange={e => setPaused(e.target.checked)} /> Pause motion</label></div>
    </section>
    <section className="signal-editor">
      <div className="signal-editor-top"><button className="signal-link" disabled={busy} onClick={onClose}>← Back to Hive</button><span className="signal-fine">{existing ? "EDIT IDENTITY" : "CREATE AN ENTITY"}</span></div>
      <ol className="signal-steps" aria-label="Creation steps">{["Intention", "Character", "Review"].map((title, i) => <li key={title} aria-current={step === i ? "step" : undefined}><span>0{i + 1}</span>{title}</li>)}</ol>
      <div className="signal-editor-content">
        {step === 0 && <><p className="signal-eyebrow">LET'S START WITH YOU</p><h2 ref={heading} tabIndex={-1}>What brings you here?</h2><p className="signal-description">Give your Entity a name and a place in your day. You can change this brief later.</p>
          <label className="signal-field">What should we call them?<input autoFocus value={profile.name} maxLength={64} placeholder="e.g. Lyra" onChange={e => update("name", e.target.value)} /></label>
          <label className="signal-field">What would you like to explore together?<textarea rows={3} maxLength={2000} value={profile.purpose} placeholder="A thoughtful partner for ideas, everyday plans, and the occasional impossible question." onChange={e => update("purpose", e.target.value)} /></label>
          <div className="signal-import"><span>Already have a vision?<small>Import Markdown · stays on this device until you save</small></span><label className="signal-secondary signal-file">Import brief<input type="file" accept=".md,.markdown,.txt" aria-label="Import Markdown brief" onChange={e => { void importFile(e.target.files?.[0]); e.target.value = ""; }} /></label></div>
          {proposal && <div className="signal-proposal"><h3>Review proposed settings</h3><dl>{Object.entries(proposal).map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl><p>Only these fields will be applied. Other content is ignored. This grants no permissions.</p><button className="signal-secondary" onClick={() => { setProfile(current => ({ ...current, ...proposal })); setProposal(null); }}>Apply proposed settings</button><button className="signal-link" onClick={() => setProposal(null)}>Discard</button></div>}
        </>}
        {step === 1 && <><p className="signal-eyebrow">A FORM THAT FEELS FAMILIAR</p><h2 ref={heading} tabIndex={-1}>Shape the signal.</h2><p className="signal-description">Choose a visual identity, then a conversational intention. Appearance never changes permissions.</p>
          <fieldset className="signal-choice-group"><legend>Visual identity</legend><div className="signal-identities">{identities.map(item => <button className={`signal-identity identity-${item.id}`} aria-pressed={profile.identity === item.id} key={item.id} onClick={() => update("identity", item.id)}><span className="signal-orb" /><strong>{item.name}</strong><small>{item.description}</small></button>)}</div></fieldset>
          <fieldset className="signal-choice-group"><legend>Which response feels right?</legend><p className="signal-fine">Written examples · not generated responses</p>{temperaments.map(item => <label className="signal-temperament" key={item.id}><input type="radio" name="temperament" checked={profile.temperament === item.id} onChange={() => update("temperament", item.id)} /><span><strong>{item.name}</strong><span>“{item.sample}”</span></span></label>)}</fieldset>
        </>}
        {step === 2 && <><p className="signal-eyebrow">YOUR IDENTITY BRIEF</p><h2 ref={heading} tabIndex={-1}>Make it yours.</h2><p className="signal-description">Review what you're saving. A visual identity and intention, ready to return to.</p>
          <dl className="signal-summary"><div><dt>Name</dt><dd>{profile.name}</dd></div><div><dt>Purpose</dt><dd>{profile.purpose || "Still to be discovered."}</dd></div><div><dt>Presence</dt><dd>{profile.identity} / {profile.temperament}</dd></div></dl>
          <label className="signal-field">What should they keep in mind?<textarea disabled={busy} rows={3} maxLength={2000} value={profile.boundaries} onChange={e => update("boundaries", e.target.value)} /></label>
          <div className="signal-scope"><span aria-hidden="true">◇</span><p>This brief is saved as design metadata. Purpose, temperament, and boundaries do not yet change model instructions or tool permissions. Your existing Hive settings still apply.</p></div>
          <button className="signal-link" disabled={busy} onClick={exportBrief}>↓ Export Markdown brief</button>
        </>}
        {error && <p role="alert" className="signal-error">{error}</p>}
      </div>
      <footer className="signal-editor-footer"><span className="signal-fine">{step === 0 ? "01 / A name and an intention" : step === 1 ? "02 / Appearance and character" : "03 / Ready when you are"}</span><div>{step > 0 && <button className="signal-link" disabled={busy} onClick={() => setStep(step - 1)}>Back</button>}{step < 2 ? <button className="signal-primary" disabled={!profile.name.trim()} onClick={() => setStep(step + 1)}>Continue <span aria-hidden="true">→</span></button> : <button className="signal-primary" disabled={busy || !profile.name.trim()} onClick={() => void save()}>{busy ? "Saving…" : existing ? "Save identity" : preview ? "Save preview identity" : "Create Entity"} <span aria-hidden="true">✧</span></button>}</div></footer>
    </section>
  </main>;
}
