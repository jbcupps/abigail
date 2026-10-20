import { useEffect, useRef, useState } from "react";
import Presence from "../../../../entity-runtime-app/src-ui/src/presence/Presence";
import { defaultProfile, extractProfile, identities, profileMarkdown, temperaments, type EntityProfile } from "../../../../entity-runtime-app/src-ui/src/presence/profile";
import { createPresenceEntity, loadPresence, savePresence, type SavedPresence } from "../lib/presenceApi";
import { CreationError, listCreationCandidates, lookupCreationAttempt, withCreationLock, type CreationAttempt, type CreationCandidate, type CreationLease, type CreationLookup } from "../lib/presenceCreation";
import { CREATION_DRAFT_EVENT, CREATION_DRAFT_KEY, readCreationDraft, migrateCreationDraft, writeCreationDraft, confirmCreationDraft, clearCreationDraft, type CreationDraft } from "../lib/creationDraft";

interface Props { preview: boolean; existing?: SavedPresence; onClose: () => void; onSaved: (value: SavedPresence) => void; onPersisted?: () => void; onBusyChange?: (busy: boolean) => void; onCanonicalCreate?: (profile: EntityProfile, explore: boolean) => void }
export default function FirstSignal({ preview, existing, onClose, onSaved, onPersisted, onBusyChange, onCanonicalCreate }: Props) {
  const [restored] = useState(() => {
    try { return { draft: !preview && !existing ? readCreationDraft() : null, error: "" }; }
    catch { return { draft: null, error: "The unfinished creation record could not be read. Creation is paused to avoid duplicates. Export your brief and check Your Entities." }; }
  });
  const [profile, setProfile] = useState<EntityProfile>(existing?.profile ?? restored.draft?.profile ?? defaultProfile());
  const canonical = !!onCanonicalCreate && !preview && !existing && !restored.draft && !restored.error;
  const [step, setStep] = useState(restored.draft ? 2 : 0);
  const [proposal, setProposal] = useState<Partial<EntityProfile> | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(restored.error);
  const [storageError, setStorageError] = useState("");
  const [storageChanged, setStorageChanged] = useState(false);
  const storageChangedRef = useRef(false);
  const [draft, setDraft] = useState<CreationDraft | null>(restored.draft);
  const draftRef = useRef(draft);
  const [candidates, setCandidates] = useState<CreationCandidate[] | null>(null);
  const [lookupResult, setLookupResult] = useState<CreationLookup | null>(null);
  const [outcome, setOutcome] = useState<"idle" | "interrupted" | "error">("idle");
  const [reviewed, setReviewed] = useState<SavedPresence | null>(null);
  const [operationLabel, setOperationLabel] = useState("");
  const [revealed, setRevealed] = useState<SavedPresence | null>(null);
  const [paused, setPaused] = useState(false);
  const createdId = useRef(existing?.id ?? restored.draft?.entityId);
  const operation = useRef<{ controller: AbortController; generation: number; kind: string } | null>(null);
  const generation = useRef(0);
  const importGeneration = useRef(0);
  const editWork = useRef<Promise<void>>(Promise.resolve());
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
    if (preview || existing || restored.draft?.version !== 1) return;
    editWork.current = migrateCreationDraft().then(next => {
      if (mounted.current && next?.attempt.attemptId === draftRef.current?.attempt.attemptId) remember(next);
    }).catch(err => { if (mounted.current) setStorageError(`The older creation could not be moved into browser storage. Its session record is retained. ${err instanceof Error ? err.message : err}`); });
  }, [preview, existing, restored.draft]);
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
    const changedElsewhere = (event: StorageEvent) => {
      if (preview || existing || (event.key !== CREATION_DRAFT_KEY && event.key !== null)
        || (event.storageArea && event.storageArea !== localStorage)) return;
      storageChangedRef.current = true; setStorageChanged(true);
      generation.current++; operation.current?.controller.abort(); operation.current = null;
      setBusy(false); setOperationLabel(""); onBusyChange?.(false); setOutcome("interrupted");
      setLookupResult(null); setReviewed(null);
      setError("This creation changed in another tab. Waiting has stopped; an accepted server request may still finish. Reload the saved creation before continuing. Your visible edits can still be exported.");
    };
    window.addEventListener(CREATION_DRAFT_EVENT, syncConfirmation);
    window.addEventListener("storage", changedElsewhere);
    return () => { window.removeEventListener(CREATION_DRAFT_EVENT, syncConfirmation); window.removeEventListener("storage", changedElsewhere); };
  }, [preview, existing, onBusyChange]);
  const reloadDraft = () => {
    try {
      const next = readCreationDraft();
      if (!next) throw new Error("The saved record was removed. Check Your Entities before starting a separate creation; this old operation will not be resumed.");
      remember(next); setProfile(next.profile); createdId.current = next.entityId;
      setStep(2); setLookupResult(null); setCandidates(null); setReviewed(null);
      storageChangedRef.current = false; setStorageChanged(false); setError(""); setStorageError(""); setOutcome("idle");
    } catch (err) { setStorageError(err instanceof Error ? err.message : String(err)); }
  };
  const changeProfile = (next: EntityProfile) => {
    if (operation.current) return;
    setProfile(next);
    const current = draftRef.current;
    if (current && !storageChangedRef.current) {
      editWork.current = editWork.current.then(async () => {
        if (!mounted.current || storageChangedRef.current || draftRef.current?.attempt.attemptId !== current.attempt.attemptId) return;
        try { remember(await writeCreationDraft({ ...draftRef.current, profile: next })); setStorageError(""); }
        catch { setStorageError("This browser could not keep your latest edits, possibly because another tab is saving. Export the brief before leaving, or reload the saved creation. The original record is retained."); }
      });
    }
  };
  const update = <K extends keyof EntityProfile>(key: K, value: EntityProfile[K]) => changeProfile({ ...profile, [key]: value });
  const begin = (kind: string) => {
    if (operation.current || storageChangedRef.current) return null;
    const next = { controller: new AbortController(), generation: ++generation.current, kind };
    operation.current = next;
    setBusy(true); setOperationLabel(kind); onBusyChange?.(true); setError(""); setOutcome("idle");
    return next;
  };
  const owns = (current: NonNullable<typeof operation.current>) => mounted.current && generation.current === current.generation;
  const active = (current: NonNullable<typeof operation.current>) => owns(current) && operation.current === current && !current.controller.signal.aborted;
  const runLocked = async (current: NonNullable<typeof operation.current>, work: (lease?: CreationLease) => Promise<void>) => {
    await editWork.current;
    const run = async (lease?: CreationLease) => {
      if (!active(current)) return;
      if (!preview && !existing) {
        const saved = readCreationDraft();
        if (saved?.attempt.attemptId !== draftRef.current?.attempt.attemptId || saved?.revision !== draftRef.current?.revision) {
          storageChangedRef.current = true; setStorageChanged(true);
          throw new Error("The saved creation changed before this operation acquired its lock. Reload it before continuing.");
        }
      }
      await work(lease);
    };
    if (preview) await run(); else await withCreationLock(run);
  };
  const finish = (current: NonNullable<typeof operation.current>) => {
    if (operation.current !== current) return;
    operation.current = null;
    if (mounted.current) { setBusy(false); setOperationLabel(""); onBusyChange?.(false); }
  };
  const stopWaiting = () => {
    const current = operation.current;
    if (!current) return;
    generation.current++;
    current.controller.abort();
    finish(current);
    setOutcome("interrupted");
    setError(current.kind === "Checking Hive" || current.kind === "Reading saved brief"
      ? "Stopped checking. Your brief and creation record are still here."
      : createdId.current || draftRef.current?.entityId || current.kind === "Saving brief"
        ? "Stopped waiting for the brief save. It may already have been saved; retrying uses the same Entity."
        : draftRef.current
          ? "Stopped waiting. Hive may still create the Entity. Check this saved attempt; no automatic retry will be sent."
          : "Stopped before a creation request was sent. Your brief is still here.");
  };
  const recordConfirmation = async (confirmedId: string, attempt?: CreationAttempt, lease?: CreationLease) => {
    if (!attempt || draftRef.current?.attempt.attemptId !== attempt.attemptId) return;
    try {
      const next = await confirmCreationDraft(attempt.attemptId, confirmedId, lease);
      if (!next) return;
      // A late response records evidence only. It cannot save or reveal, and
      // cannot restore stale UI after a cross-tab change or component unmount.
      if (mounted.current && !storageChangedRef.current) { remember(next); createdId.current = confirmedId; }
    } catch {
      if (mounted.current) setStorageError(`Hive confirmed Entity ${confirmedId}, but its record could not be updated. Keep this ID and export your brief; check the same creation key again.`);
    }
  };
  const save = async (selected?: SavedPresence) => {
    if (canonical) { if (profile.name.trim() && profile.purpose.trim()) onCanonicalCreate?.({ ...profile, name: profile.name.trim(), purpose: profile.purpose.trim() }, false); return; }
    if (!profile.name.trim() || restored.error || lookupResult?.state === "unavailable" || (!selected && draftRef.current && !createdId.current)) return;
    const current = begin(selected?.id || createdId.current ? "Saving brief" : "Checking creation support");
    if (!current) return;
    try {
      await runLocked(current, async lease => {
      const clean = { ...profile, name: profile.name.trim() };
      let id = selected?.id ?? createdId.current;
      if (!id) {
        current.kind = "Creating Entity"; setOperationLabel(current.kind);
        id = await createPresenceEntity(clean.name, preview, {
          signal: current.controller.signal, lease,
          onDispatched: async attempt => {
            if (!active(current)) throw new Error("Creation was stopped before dispatch.");
            const pending: CreationDraft = { version: attempt.creationKey ? 2 : 1, profile: clean, attempt };
            remember(await writeCreationDraft(pending, lease));
          },
          onConfirmed: (id, attempt) => recordConfirmation(id, attempt, lease),
        });
        createdId.current = id;
      }
      if (!active(current)) return;
      current.kind = "Saving brief"; setOperationLabel(current.kind);
      const pending = draftRef.current;
      if (pending) {
        const next = { ...pending, profile: clean, ...(selected ? { recoveryEntityId: id } : {}) };
        remember(await writeCreationDraft(next, lease));
      }
      const savedJournal = draftRef.current;
      const hiveUrl = pending?.attempt.hiveUrl ?? existing?.hiveUrl;
      await savePresence(id, clean, preview, { signal: current.controller.signal, hiveUrl });
      if (!active(current)) return;
      if (savedJournal) {
        const latest = readCreationDraft();
        if (latest?.attempt.attemptId !== savedJournal.attempt.attemptId || latest?.revision !== savedJournal.revision) {
          storageChangedRef.current = true; setStorageChanged(true);
          throw new Error("The brief save returned after this creation changed in another tab. Review the latest saved creation; it has not been cleared or revealed.");
        }
      }
      setProfile(clean);
      setRevealed({ id, profile: clean, ...(hiveUrl ? { hiveUrl } : {}) });
      // A user-selected candidate is not proof of the original POST outcome.
      // Keep that attempt and both IDs unless Hive confirmed this exact target.
      if (draftRef.current?.entityId === id) {
        try { await clearCreationDraft(draftRef.current.attempt.attemptId, draftRef.current.revision, lease); remember(null); }
        catch { setStorageError("Your brief was saved, but the unfinished record could not be cleared. It will reopen safely in this browser."); }
      }
      onPersisted?.();
      });
    } catch (err) {
      if (!owns(current)) return;
      setOutcome("error");
      if (err instanceof CreationError && err.outcome === "not-sent") {
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
      await runLocked(current, async lease => {
      if (pending.attempt.creationKey) {
        const found = await lookupCreationAttempt(pending.attempt, current.controller.signal);
        if (!active(current)) return;
        setLookupResult(found);
        if (found.state === "complete") await recordConfirmation(found.id, pending.attempt, lease);
        if (found.state === "unavailable") setOutcome("error");
      } else {
        const found = await listCreationCandidates(pending.attempt, current.controller.signal);
        if (active(current)) setCandidates(found);
      }
      });
    } catch (err) { if (active(current)) { setOutcome("error"); setError(err instanceof Error ? err.message : String(err)); } }
    finally { finish(current); }
  };
  const retryCreation = async () => {
    const pending = draftRef.current;
    if (!pending?.attempt.creationKey || pending.entityId) return;
    const current = begin("Retrying the same creation");
    if (!current) return;
    try {
      await runLocked(current, async lease => {
      await createPresenceEntity(pending.attempt.name, false, {
        attempt: pending.attempt, signal: current.controller.signal, lease,
        onDispatched: () => {
          if (!active(current)) throw new Error("Creation was stopped before retry.");
          const saved = readCreationDraft();
          if (!saved || saved.attempt.attemptId !== pending.attempt.attemptId || saved.revision !== draftRef.current?.revision) throw new Error("The saved creation changed. Reload it before retrying.");
        },
        onConfirmed: (id, attempt) => recordConfirmation(id, attempt, lease),
      });
      if (active(current)) { setLookupResult(null); setOutcome("idle"); }
      // Explicit retry confirms identity only. Saving the edited brief is a
      // separate action, including when GET already reports complete.
      });
    } catch (err) { if (active(current)) { setOutcome("error"); setError(err instanceof Error ? err.message : String(err)); } }
    finally { finish(current); }
  };
  const reviewCandidate = async (id: string) => {
    const pending = draftRef.current;
    if (!pending || pending.attempt.creationKey) return;
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
  const creationState = busy ? "thinking" : outcome;
  const creationLabel = busy ? operationLabel
    : outcome === "interrupted" ? "Waiting stopped; server outcome may still need checking"
    : outcome === "error" ? "Creation needs attention"
    : createdId.current ? "Entity confirmed; review and save its brief"
    : draft ? "Saved creation awaiting confirmation"
    : "Identity preview; no creation request active";
  if (revealed) return <main className="signal-reveal">
    <span className="signal-eyebrow">FIRST SIGNAL / IDENTITY SAVED</span>
    <div className={`reveal-portrait${paused ? " reveal-still" : ""}`}><Presence identity={profile.identity} reducedMotion={paused} label={`${profile.name}, procedural portrait`} /></div>
    <div className="reveal-copy"><h1 ref={heading} tabIndex={-1}>Meet {profile.name}.</h1><p>A familiar presence. A new beginning.</p><p className="signal-fine">{preview ? "Saved in this browser's design preview. No runtime entity was created." : "Entity and visual brief saved locally by Hive. Conversation opens through the Hive where this brief was saved."}</p>
      {draft && <p className="signal-error" role="status">This brief was saved to Entity {revealed.id}. {draft.entityId === revealed.id ? "Its unfinished record is still kept in this browser." : `The original creation ${draft.entityId ? `confirmed a different Entity: ${draft.entityId}` : "is still unconfirmed"}. Its record and your brief remain in this browser; no repeat creation request will be sent.`}</p>}
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
      <Presence identity={profile.identity} state={creationState} reducedMotion={paused} label={creationLabel} />
      <p className="signal-operation-status signal-fine" role="status">{creationLabel}. Visual activity reflects this creation operation, not model reasoning.</p>
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
          <div className="signal-scope"><span aria-hidden="true">◇</span><p>{canonical ? "Awaken approves this name and purpose as part of their identity. Portrait, response style examples, and notes here remain presentation preferences. Explore further to review more identity details. Permissions stay with Hive." : "This brief is saved as design metadata. Purpose, temperament, and boundaries do not yet change model instructions or tool permissions. Your existing Hive settings still apply."}</p></div>
          {canonical && <><p className="signal-fine">A name and purpose are enough to begin. No model call is needed to awaken.</p><button className="signal-secondary" disabled={!profile.name.trim() || !profile.purpose.trim()} onClick={() => onCanonicalCreate?.({ ...profile, name: profile.name.trim(), purpose: profile.purpose.trim() }, true)}>Explore this character first</button></>}
          <button className="signal-link" disabled={busy} onClick={exportBrief}>↓ Export Markdown brief</button>
        </>}
        {draft && <section className="signal-proposal" aria-label="Creation recovery">
          <h3>{draft.entityId ? "Hive confirmed an Entity" : "Creation outcome not confirmed"}</h3>
          <p>{draft.entityId ? `Confirmed Entity: ${draft.entityId}. Saving this brief will reuse that Entity.` : "Hive may still finish the original request. Stopping or timing out does not undo creation. No automatic retry will be sent."}</p>
          <p className="signal-fine">{draft.attempt.creationKey ? "Unfinished brief kept in this browser, including after closing the tab." : "Older unfinished brief retained. If browser storage reports an error, export it before closing this tab."} Clearing site data removes this record; export a copy before doing so. Hive: {draft.attempt.hiveUrl}</p>
          {draft.attempt.creationKey ? <>
            <p className="signal-fine">Creation key: {draft.attempt.creationKey}. Original creation name: {draft.attempt.name}. Editing this brief never changes that request name.</p>
            <button className="signal-secondary" disabled={busy || storageChanged} onClick={() => void checkResults()}>Check this creation</button>
            {!draft.entityId && lookupResult?.state !== "unavailable" && <button className="signal-secondary" disabled={busy || storageChanged} onClick={() => void retryCreation()}>Retry the same creation safely</button>}
            {lookupResult && <p role="status">{lookupResult.state === "absent" ? "Hive has no record of this exact key yet. A retry will use the same key and original name."
              : lookupResult.state === "reserved" ? "Hive reserved this exact creation. A retry resumes the same reservation and cannot create a second Entity for this key."
              : lookupResult.state === "complete" ? `Exact creation confirmed: ${lookupResult.id}. Review the brief, then save it when ready.`
              : `The Entity for this creation is unavailable (${lookupResult.id}). This key and brief are retained. No replacement or new key will be created automatically.`}</p>}
          </> : <>
          <p>This older request has no creation key and cannot be replayed safely. Matching names are suggestions, never confirmation.</p>
          {draft.recoveryEntityId && <p>Previously selected existing Entity: {draft.recoveryEntityId}.{draft.entityId && draft.entityId !== draft.recoveryEntityId ? " This differs from the original request's confirmed Entity. Neither will be changed automatically." : ""}</p>}
          <button className="signal-secondary" disabled={busy || storageChanged} onClick={() => void checkResults()}>Check Hive for results</button>
          {draft.recoveryEntityId && <button className="signal-link" disabled={busy || storageChanged} onClick={() => void reviewCandidate(draft.recoveryEntityId!)}>Review selected Entity</button>}
          {candidates && <div role="status"><p>{candidates.length ? "Possible new matches. A matching name does not prove which request created an Entity." : "No new matching Entity found yet. This does not prove nothing was created; you can check again."}</p></div>}
          {candidates?.map(candidate => <p key={candidate.id}>{candidate.name} <small>{candidate.id}</small> <button className="signal-link" disabled={busy || storageChanged} onClick={() => void reviewCandidate(candidate.id)}>Review Entity {candidate.id}</button></p>)}
          {reviewed && <div className="signal-scope"><div><h4>Review existing Entity</h4><p>{reviewed.profile.name} ({reviewed.id})</p><p>Saved purpose: {reviewed.profile.purpose || "None"}</p><p>Saved appearance: {reviewed.profile.identity}; temperament: {reviewed.profile.temperament}</p><p>Saved boundaries: {reviewed.profile.boundaries || "None"}</p><p>Saving replaces this Entity's visual brief with the brief above. It does not confirm the original creation request or change model instructions.</p><button className="signal-secondary" disabled={busy || storageChanged || !profile.name.trim()} onClick={() => void save(reviewed)}>Save this brief to this Entity</button></div></div>}
          </>}
        </section>}
        {storageChanged && <div className="signal-proposal"><p>Another tab changed the saved creation. Export visible edits before reloading if you want to keep them.</p><button className="signal-secondary" onClick={reloadDraft}>Reload saved creation</button><button className="signal-link" onClick={exportBrief}>Export visible brief</button></div>}
        {busy && <p role="status">{operationLabel}. You can stop waiting; an accepted server request may still finish.</p>}
        {storageError && <p role="alert" className="signal-error">{storageError}</p>}
        {error && <p ref={errorHeading} tabIndex={-1} role="alert" className="signal-error">{error}</p>}
      </div>
      <footer className="signal-editor-footer"><span className="signal-fine">{step === 0 ? "01 / A name and an intention" : step === 1 ? "02 / Appearance and character" : "03 / Ready when you are"}</span><div>{busy && <button className="signal-secondary" onClick={stopWaiting}>Stop waiting</button>}{step > 0 && <button className="signal-link" disabled={busy} onClick={() => setStep(step - 1)}>Back</button>}{step < 2 ? <button className="signal-primary" disabled={busy || !profile.name.trim()} onClick={() => setStep(step + 1)}>Continue <span aria-hidden="true">→</span></button> : <button className="signal-primary" disabled={busy || storageChanged || lookupResult?.state === "unavailable" || !profile.name.trim() || (canonical && !profile.purpose.trim()) || !!restored.error || (!!draft && !createdId.current)} onClick={() => void save()}>{busy ? "Saving…" : existing ? "Save identity" : preview ? "Save preview identity" : canonical ? "Awaken Entity" : createdId.current ? "Save brief to confirmed Entity" : "Create Entity"} <span aria-hidden="true">✧</span></button>}</div></footer>
    </section>
  </main>;
}
