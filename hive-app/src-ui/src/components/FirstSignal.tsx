import { useEffect, useRef, useState } from "react";
import Presence from "../../../../entity-runtime-app/src-ui/src/presence/Presence";
import { defaultProfile, extractProfile, identities, profileMarkdown, temperaments, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { createPresenceEntity, loadPresence, savePresence, type SavedPresence } from "../lib/presenceApi";
import { CreationError, listCreationCandidates, type CreationCandidate } from "../lib/presenceCreation";
import { CREATION_DRAFT_EVENT, readCreationDraft, writeCreationDraft, confirmCreationDraft, clearCreationDraft, type CreationDraft } from "../lib/creationDraft";

interface Props { preview: boolean; existing?: SavedPresence; onClose: () => void; onSaved: (value: SavedPresence) => void; onPersisted?: () => void; onBusyChange?: (busy: boolean) => void }
export default function FirstSignal({ preview, existing, onClose, onSaved, onPersisted, onBusyChange }: Props) {
  const [restored] = useState(() => {
    try { return { draft: !preview && !existing ? readCreationDraft() : null, error: "" }; }
    catch { return { draft: null, error: "The unfinished creation record could not be read. Creation is paused to avoid duplicates. Export your brief and check Your Entities." }; }
  });
  const [profile, setProfile] = useState<EntityProfile>(existing?.profile ?? restored.draft?.profile ?? defaultProfile());
  const [step, setStep] = useState(restored.draft ? 2 : 0);
  const [proposal, setProposal] = useState<Partial<EntityProfile> | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(restored.error);
  const [storageError, setStorageError] = useState("");
  const [draft, setDraft] = useState<CreationDraft | null>(restored.draft);
  const draftRef = useRef(draft);
  const [candidates, setCandidates] = useState<CreationCandidate[] | null>(null);
  const [reviewed, setReviewed] = useState<SavedPresence | null>(null);
  const [operationLabel, setOperationLabel] = useState("");
  const [revealed, setRevealed] = useState<SavedPresence | null>(null);
  const [paused, setPaused] = useState(false);
  const createdId = useRef(existing?.id ?? restored.draft?.entityId);
  const operation = useRef<{ controller: AbortController; generation: number; kind: string } | null>(null);
  const generation = useRef(0);
  const importGeneration = useRef(0);
  const mounted = useRef(true);
  const heading = useRef<HTMLHeadingElement>(null);
  const errorHeading = useRef<HTMLParagraphElement>(null);
  useEffect(() => { heading.current?.focus(); }, [step, revealed]);
  useEffect(() => { if (error) errorHeading.current?.focus(); }, [error]);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current++; operation.current?.controller.abort(); operation.current = null; };
  }, []);
  const remember = (next: CreationDraft | null) => { draftRef.current = next; if (mounted.current) setDraft(next); };
  useEffect(() => {
    const syncConfirmation = () => {
      try {
        const next = readCreationDraft();
        if (next && next.attempt.attemptId === draftRef.current?.attempt.attemptId) {
          remember(next);
          createdId.current = next.entityId;
        }
      } catch { /* Preserve the in-memory record; the next operation reports storage errors. */ }
    };
    window.addEventListener(CREATION_DRAFT_EVENT, syncConfirmation);
    return () => window.removeEventListener(CREATION_DRAFT_EVENT, syncConfirmation);
  }, []);
  const changeProfile = (next: EntityProfile) => {
    if (operation.current) return;
    setProfile(next);
    const current = draftRef.current;
    if (current) {
      remember({ ...current, profile: next });
      try { writeCreationDraft({ ...current, profile: next }); setStorageError(""); }
      catch { setStorageError("This tab could not keep your latest edits. Export the brief before leaving. The original creation record is retained."); }
    }
  };
  const update = <K extends keyof EntityProfile>(key: K, value: EntityProfile[K]) => changeProfile({ ...profile, [key]: value });
  const begin = (kind: string) => {
    if (operation.current) return null;
    const next = { controller: new AbortController(), generation: ++generation.current, kind };
    operation.current = next;
    setBusy(true); setOperationLabel(kind); onBusyChange?.(true); setError("");
    return next;
  };
  const owns = (current: NonNullable<typeof operation.current>) => mounted.current && generation.current === current.generation;
  const active = (current: NonNullable<typeof operation.current>) => owns(current) && operation.current === current && !current.controller.signal.aborted;
  const finish = (current: NonNullable<typeof operation.current>) => {
    if (operation.current !== current) return;
    operation.current = null;
    if (mounted.current) { setBusy(false); setOperationLabel(""); onBusyChange?.(false); }
  };
  const stopWaiting = () => {
    const current = operation.current;
    if (!current) return;
    current.controller.abort();
    finish(current);
    setError(current.kind === "Checking Hive" || current.kind === "Reading saved brief"
      ? "Stopped checking. Your brief and creation record are still here."
      : createdId.current || draftRef.current?.entityId || current.kind === "Saving brief"
        ? "Stopped waiting for the brief save. It may already have been saved; retrying uses the same Entity."
        : draftRef.current
          ? "Stopped waiting. Hive may still create the Entity. No repeat creation request will be sent."
          : "Stopped before a creation request was sent. Your brief is still here.");
  };
  const save = async (selected?: SavedPresence) => {
    if (!profile.name.trim() || restored.error || (!selected && draftRef.current && !createdId.current)) return;
    const current = begin("Saving brief");
    if (!current) return;
    try {
      const clean = { ...profile, name: profile.name.trim() };
      let id = selected?.id ?? createdId.current;
      if (!id) {
        current.kind = "Creating Entity"; setOperationLabel(current.kind);
        id = await createPresenceEntity(clean.name, preview, {
          signal: current.controller.signal,
          onDispatched: attempt => {
            const pending: CreationDraft = { version: 1, profile: clean, attempt };
            writeCreationDraft(pending);
            remember(pending);
          },
          onConfirmed: (confirmedId, attempt) => {
            if (!attempt || draftRef.current?.attempt.attemptId !== attempt.attemptId) return;
            let next: CreationDraft = { ...draftRef.current, entityId: confirmedId };
            try {
              const persisted = confirmCreationDraft(attempt.attemptId, confirmedId);
              if (!persisted) return;
              next = persisted;
            }
            catch {
              if (mounted.current) setStorageError(`Hive confirmed Entity ${confirmedId}, but this tab could not record the result. Keep this ID and export your brief before leaving.`);
            }
            remember(next);
            createdId.current = confirmedId;
            // A late response is evidence for recovery only. The canceled or
            // abandoned operation may not save the brief or trigger a reveal.
          },
        });
        createdId.current = id;
      }
      if (!active(current)) return;
      current.kind = "Saving brief"; setOperationLabel(current.kind);
      const pending = draftRef.current;
      if (pending) {
        const next = { ...pending, profile: clean, ...(selected ? { recoveryEntityId: id } : {}) };
        writeCreationDraft(next);
        remember(next);
      }
      const hiveUrl = pending?.attempt.hiveUrl ?? existing?.hiveUrl;
      await savePresence(id, clean, preview, { signal: current.controller.signal, hiveUrl });
      if (!active(current)) return;
      setProfile(clean);
      setRevealed({ id, profile: clean, ...(hiveUrl ? { hiveUrl } : {}) });
      // A user-selected candidate is not proof of the original POST outcome.
      // Keep that attempt and both IDs unless Hive confirmed this exact target.
      if (draftRef.current?.entityId === id) {
        try { clearCreationDraft(draftRef.current.attempt.attemptId); remember(null); }
        catch { setStorageError("Your brief was saved, but the unfinished record could not be cleared. It will reopen safely in this tab."); }
      }
      onPersisted?.();
    } catch (err) {
      if (!owns(current)) return;
      if (err instanceof CreationError && err.outcome === "not-sent") {
        if (draftRef.current && !draftRef.current.entityId) {
          try { clearCreationDraft(draftRef.current.attempt.attemptId); remember(null); }
          catch { setStorageError("The local attempt record could not be cleared. Keep this tab open and export your brief."); }
        }
        setError(`No creation request was sent. ${err.message}`);
      } else setError(`${createdId.current && !existing && !preview ? "Entity created; saving its brief is not confirmed. Retry will reuse this entity. " : ""}${err instanceof Error ? err.message : err}`);
    } finally { finish(current); }
  };
  const checkResults = async () => {
    const pending = draftRef.current;
    if (!pending) return;
    const current = begin("Checking Hive");
    if (!current) return;
    try {
      const found = await listCreationCandidates(pending.attempt, current.controller.signal);
      if (active(current)) setCandidates(found);
    } catch (err) { if (active(current)) setError(err instanceof Error ? err.message : String(err)); }
    finally { finish(current); }
  };
  const reviewCandidate = async (id: string) => {
    const pending = draftRef.current;
    if (!pending) return;
    const current = begin("Reading saved brief");
    if (!current) return;
    try {
      const saved = await loadPresence(id, false, current.controller.signal, pending.attempt.hiveUrl);
      if (active(current)) setReviewed({ id, profile: saved });
    } catch (err) { if (active(current)) setError(err instanceof Error ? err.message : String(err)); }
    finally { finish(current); }
  };
  const importFile = async (file?: File) => {
    if (!file || operation.current) return;
    const importId = ++importGeneration.current;
    const operationGeneration = generation.current;
    const canApply = () => mounted.current && importId === importGeneration.current && operationGeneration === generation.current && !operation.current;
    setError(""); setProposal(null);
    try {
      if (!/\.(md|markdown|txt)$/i.test(file.name) || file.size > 32768) throw new Error("Choose a .md, .markdown, or .txt file up to 32 KB.");
      const extracted = extractProfile(await file.text());
      if (canApply()) setProposal(extracted);
    } catch (err) { if (canApply()) setError(err instanceof Error ? err.message : String(err)); }
  };
  const exportBrief = () => {
    const url = URL.createObjectURL(new Blob([profileMarkdown(profile)], { type: "text/markdown" }));
    const anchor = document.createElement("a"); anchor.href = url; anchor.download = "entity-brief.md"; anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  if (revealed) return <main className="signal-reveal">
    <span className="signal-eyebrow">FIRST SIGNAL / IDENTITY SAVED</span>
    <div className={`reveal-portrait${paused ? " reveal-still" : ""}`}><Presence identity={profile.identity} reducedMotion={paused} label={`${profile.name}, procedural portrait`} /></div>
    <div className="reveal-copy"><h1 ref={heading} tabIndex={-1}>Meet {profile.name}.</h1><p>A familiar presence. A new beginning.</p><p className="signal-fine">{preview ? "Saved in this browser's design preview. No runtime entity was created." : "Entity and visual brief saved locally by Hive. Conversation opens through the Hive where this brief was saved."}</p>
      {draft && <p className="signal-error" role="status">This brief was saved to Entity {revealed.id}. {draft.entityId === revealed.id ? "Its unfinished record is still kept in this tab." : `The original creation ${draft.entityId ? `confirmed a different Entity: ${draft.entityId}` : "is still unconfirmed"}. Its record and your brief remain in this tab; no repeat creation request will be sent.`}</p>}
      {storageError && <p className="signal-error" role="alert">{storageError}</p>}
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
          <label className="signal-field">What should we call them?<input disabled={busy} autoFocus value={profile.name} maxLength={64} placeholder="e.g. Lyra" onChange={e => update("name", e.target.value)} /></label>
          <label className="signal-field">What would you like to explore together?<textarea disabled={busy} rows={3} maxLength={2000} value={profile.purpose} placeholder="A thoughtful partner for ideas, everyday plans, and the occasional impossible question." onChange={e => update("purpose", e.target.value)} /></label>
          <div className="signal-import"><span>Already have a vision?<small>Import Markdown · stays on this device until you save</small></span><label className="signal-secondary signal-file">Import brief<input disabled={busy} type="file" accept=".md,.markdown,.txt" aria-label="Import Markdown brief" onChange={e => { void importFile(e.target.files?.[0]); e.target.value = ""; }} /></label></div>
          {proposal && <div className="signal-proposal"><h3>Review proposed settings</h3><dl>{Object.entries(proposal).map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl><p>Only these fields will be applied. Other content is ignored. This grants no permissions.</p><button className="signal-secondary" disabled={busy} onClick={() => { changeProfile({ ...profile, ...proposal }); setProposal(null); }}>Apply proposed settings</button><button className="signal-link" disabled={busy} onClick={() => setProposal(null)}>Discard</button></div>}
        </>}
        {step === 1 && <><p className="signal-eyebrow">A FORM THAT FEELS FAMILIAR</p><h2 ref={heading} tabIndex={-1}>Shape the signal.</h2><p className="signal-description">Choose a visual identity, then a conversational intention. Appearance never changes permissions.</p>
          <fieldset className="signal-choice-group" disabled={busy}><legend>Visual identity</legend><div className="signal-identities">{identities.map(item => <button className={`signal-identity identity-${item.id}`} aria-pressed={profile.identity === item.id} key={item.id} onClick={() => update("identity", item.id)}><span className="signal-orb" /><strong>{item.name}</strong><small>{item.description}</small></button>)}</div></fieldset>
          <fieldset className="signal-choice-group" disabled={busy}><legend>Which response feels right?</legend><p className="signal-fine">Written examples · not generated responses</p>{temperaments.map(item => <label className="signal-temperament" key={item.id}><input type="radio" name="temperament" checked={profile.temperament === item.id} onChange={() => update("temperament", item.id)} /><span><strong>{item.name}</strong><span>“{item.sample}”</span></span></label>)}</fieldset>
        </>}
        {step === 2 && <><p className="signal-eyebrow">YOUR IDENTITY BRIEF</p><h2 ref={heading} tabIndex={-1}>Make it yours.</h2><p className="signal-description">Review what you're saving. A visual identity and intention, ready to return to.</p>
          <dl className="signal-summary"><div><dt>Name</dt><dd>{profile.name}</dd></div><div><dt>Purpose</dt><dd>{profile.purpose || "Still to be discovered."}</dd></div><div><dt>Presence</dt><dd>{profile.identity} / {profile.temperament}</dd></div></dl>
          <label className="signal-field">What should they keep in mind?<textarea disabled={busy} rows={3} maxLength={2000} value={profile.boundaries} onChange={e => update("boundaries", e.target.value)} /></label>
          <div className="signal-scope"><span aria-hidden="true">◇</span><p>This brief is saved as design metadata. Purpose, temperament, and boundaries do not yet change model instructions or tool permissions. Your existing Hive settings still apply.</p></div>
          <button className="signal-link" disabled={busy} onClick={exportBrief}>↓ Export Markdown brief</button>
        </>}
        {draft && <section className="signal-proposal" aria-label="Creation recovery">
          <h3>{draft.entityId ? "Hive confirmed an Entity" : "Creation outcome not confirmed"}</h3>
          <p>{draft.entityId ? `Confirmed Entity: ${draft.entityId}. Saving this brief will reuse that Entity.` : "Hive may still finish the original request. Stopping or timing out does not undo creation. No repeat creation request will be sent."}</p>
          <p className="signal-fine">Unfinished brief kept in this tab. Closing the tab or clearing its storage can lose this recovery record. Export a copy before leaving. Hive: {draft.attempt.hiveUrl}</p>
          {draft.recoveryEntityId && <p>Previously selected existing Entity: {draft.recoveryEntityId}.{draft.entityId && draft.entityId !== draft.recoveryEntityId ? " This differs from the original request's confirmed Entity. Neither will be changed automatically." : ""}</p>}
          <button className="signal-secondary" disabled={busy} onClick={() => void checkResults()}>Check Hive for results</button>
          {draft.recoveryEntityId && <button className="signal-link" disabled={busy} onClick={() => void reviewCandidate(draft.recoveryEntityId!)}>Review selected Entity</button>}
          {candidates && <div role="status"><p>{candidates.length ? "Possible new matches. A matching name does not prove which request created an Entity." : "No new matching Entity found yet. This does not prove nothing was created; you can check again."}</p></div>}
          {candidates?.map(candidate => <p key={candidate.id}>{candidate.name} <small>{candidate.id}</small> <button className="signal-link" disabled={busy} onClick={() => void reviewCandidate(candidate.id)}>Review Entity {candidate.id}</button></p>)}
          {reviewed && <div className="signal-scope"><div><h4>Review existing Entity</h4><p>{reviewed.profile.name} ({reviewed.id})</p><p>Saved purpose: {reviewed.profile.purpose || "None"}</p><p>Saved appearance: {reviewed.profile.identity}; temperament: {reviewed.profile.temperament}</p><p>Saved boundaries: {reviewed.profile.boundaries || "None"}</p><p>Saving replaces this Entity's visual brief with the brief above. It does not confirm the original creation request or change model instructions.</p><button className="signal-secondary" disabled={busy || !profile.name.trim()} onClick={() => void save(reviewed)}>Save this brief to this Entity</button></div></div>}
        </section>}
        {busy && <p role="status">{operationLabel}. You can stop waiting; an accepted server request may still finish.</p>}
        {storageError && <p role="alert" className="signal-error">{storageError}</p>}
        {error && <p ref={errorHeading} tabIndex={-1} role="alert" className="signal-error">{error}</p>}
      </div>
      <footer className="signal-editor-footer"><span className="signal-fine">{step === 0 ? "01 / A name and an intention" : step === 1 ? "02 / Appearance and character" : "03 / Ready when you are"}</span><div>{busy && <button className="signal-secondary" onClick={stopWaiting}>Stop waiting</button>}{step > 0 && <button className="signal-link" disabled={busy} onClick={() => setStep(step - 1)}>Back</button>}{step < 2 ? <button className="signal-primary" disabled={busy || !profile.name.trim()} onClick={() => setStep(step + 1)}>Continue <span aria-hidden="true">→</span></button> : <button className="signal-primary" disabled={busy || !profile.name.trim() || !!restored.error || (!!draft && !createdId.current)} onClick={() => void save()}>{busy ? "Saving…" : existing ? "Save identity" : preview ? "Save preview identity" : createdId.current ? "Save brief to confirmed Entity" : "Create Entity"} <span aria-hidden="true">✧</span></button>}</div></footer>
    </section>
  </main>;
}
