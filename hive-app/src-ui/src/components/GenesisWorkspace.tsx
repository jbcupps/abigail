import { useCallback, useEffect, useRef, useState } from "react";
import { deleteGenesisSession, displayGenesisValue, essenceFor, exportGenesisMarkdown, GENESIS_MODES, genesisAction, getGenesisSession, importGenesisMarkdown, listGenesisSessions, sendGenesisMessage, type FieldUpdate, type GenesisCreationSeed, type GenesisSession } from "../lib/genesisClient";
import { dispatchGenesisLaunch, readGenesisLaunchDraft } from "../lib/genesisLaunchDraft";
import { openEntity } from "../lib/entityWindow";
import GenesisDefinition from "./GenesisDefinition";
import GenesisFirstSignal from "./GenesisFirstSignal";
import GenesisStudio from "./GenesisStudio";
import GenesisVoice from "./GenesisVoice";
import GenesisChime from "./GenesisChime";
import GenesisRetention from "./GenesisRetention";
import GenesisQuest, { QuestRail } from "./GenesisQuest";
import GenesisCelebration from "./GenesisCelebration";
import type { BadgeView, QuestStageId } from "../lib/questClient";

type Tab = "conversation" | "quest" | "definition" | "sources" | "chronicle";
const TAB_LABELS: Record<Tab, string> = { conversation: "Discovery", quest: "Quest", definition: "Definition", sources: "Markdown", chronicle: "Chronicle" };
const DIMENSIONS = [["identity", "Identity"], ["personality", "Personality"], ["purpose", "Purpose"], ["memories_lore", "Memories & lore"], ["appearance", "Appearance"], ["voice_presence", "Voice & presence"]] as const;
const SPARKS = ["A compassionate guide for everyday life", "An ancient librarian who fears forgetting", "A curious companion for family discoveries"];

export default function GenesisWorkspace({ onClose, onCreated, resumeId }: { onClose: () => void; onCreated: () => void; resumeId?: string }) {
  const [sessions, setSessions] = useState<GenesisSession[]>([]);
  const [session, setSession] = useState<GenesisSession | null>(null);
  const [composer, setComposer] = useState("");
  const [tab, setTab] = useState<Tab>("conversation");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pendingEdits, setPendingEdits] = useState<FieldUpdate[]>([]);
  const [selectedProposals, setSelectedProposals] = useState<string[]>([]);
  const [replaceImportFacts, setReplaceImportFacts] = useState(false);
  const [confirmation, setConfirmation] = useState<string | null>(null);
  const [questStage, setQuestStage] = useState<QuestStageId>("reservation");
  const [celebration, setCelebration] = useState<BadgeView[] | null>(null);
  const flight = useRef(false);
  const mounted = useRef(true);
  const latest = useRef(session);
  latest.current = session;

  useEffect(() => {
    mounted.current = true;
    void listGenesisSessions().then((items) => { if (mounted.current) setSessions(items); }).catch((reason) => { if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason)); }).finally(() => { if (mounted.current) setLoading(false); });
    return () => { mounted.current = false; };
  }, []);

  const operation = async <T,>(work: () => Promise<T>): Promise<T | undefined> => {
    if (flight.current) return;
    flight.current = true; setBusy(true); setError(""); setNotice("");
    try { return await work(); }
    catch (reason) { if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason)); return undefined; }
    finally { flight.current = false; if (mounted.current) setBusy(false); }
  };
  const adopt = (next: GenesisSession) => { latest.current = next; if (mounted.current) setSession(next); return next; };
  const begin = (seed: GenesisCreationSeed) => operation(async () => {
    const creationKey = readGenesisLaunchDraft()?.creation_key ?? crypto.randomUUID();
    const next = await dispatchGenesisLaunch(seed, creationKey);
    adopt(next); setComposer(next.composer_draft || ""); setTab(next.quest ? "quest" : "conversation");
    setQuestStage(next.quest?.current ?? "reservation");
  });
  const resume = (id: string) => operation(async () => {
    const next = await getGenesisSession(id);
    adopt(next); setComposer(next.composer_draft || ""); setPendingEdits([]); setTab("conversation");
    setQuestStage(next.quest?.current ?? "reservation");
  });
  const change = async (action: string, args: Record<string, unknown> = {}) => operation(async () => {
    if (!latest.current) return;
    return adopt(await genesisAction(latest.current, action, args));
  });
  const saveUpdates = async (updates: FieldUpdate[]) => {
    if (!updates.length) return;
    const result = await change("update", { updates });
    if (!result) throw new Error("The definition has not been saved. Your edits are still here.");
    setPendingEdits((previous) => previous.filter((edit) => !updates.some((saved) => saved.field === edit.field)));
  };
  const refresh = useCallback(async (force = false) => {
    const current = latest.current;
    if (!current || (flight.current && !force)) return;
    const next = await getGenesisSession(current.id);
    if (mounted.current && latest.current?.id === next.id && latest.current.revision <= next.revision) {
      latest.current = next; setSession(next);
    }
  }, []);
  const resumed = useRef(false);
  useEffect(() => {
    if (!resumeId || resumed.current) return;
    resumed.current = true;
    void resume(resumeId).then(() => { if (latest.current?.quest) setTab("quest"); });
  }, [resumeId]);
  const saveAndExit = () => operation(async () => {
    let current = latest.current;
    if (!current) { onClose(); return; }
    if (pendingEdits.length) current = adopt(await genesisAction(current, "update", { updates: pendingEdits }));
    adopt(await genesisAction(current, "save_draft", { text: composer }));
    onClose();
  });
  const send = (text = composer) => operation(async () => {
    if (!latest.current || !text.trim()) return;
    const next = await sendGenesisMessage(latest.current, text.trim());
    adopt(next); setComposer("");
  });
  const approveSuggestions = () => operation(async () => {
    let current = latest.current;
    if (!current) return;
    const conflicts = new Set(current.conflicts.filter((conflict) => !conflict.resolved).map((conflict) => conflict.proposal_id));
    for (const proposal of current.proposals.filter((item) => item.status === "pending" && !conflicts.has(item.id))) {
      current = adopt(await genesisAction(current, "review_proposal", { proposal_id: proposal.id, accept: true, replace_confirmed: false }));
    }
    setNotice(conflicts.size ? "Accepted the suggestions without conflicts. Review conflicting choices individually." : "Your accepted suggestions are now part of the definition.");
  });
  const awaken = () => operation(async () => {
    let current = latest.current;
    if (!current) return;
    if (current.conflicts.some((conflict) => !conflict.resolved)) throw new Error("Resolve conflicting choices before awakening your Entity.");
    if (pendingEdits.length) current = adopt(await genesisAction(current, "update", { updates: pendingEdits }));
    if (!["review", "portrait_pending", "portrait_approved", "manifesting", "ready", "failed"].includes(current.state)) current = adopt(await genesisAction(current, "review"));
    const before = new Set(current.quest?.badges.filter((badge) => badge.earned_at).map((badge) => badge.badge));
    const firstAwakening = !current.awakening;
    const awakened = adopt(await genesisAction(current, "activate")); setPendingEdits([]); setTab("conversation"); onCreated();
    if (firstAwakening && awakened.awakening && awakened.quest && awakened.definition.entity_id) {
      setCelebration(awakened.quest.badges.filter((badge) => badge.earned_at && !before.has(badge.badge)));
    }
  });
  const voiceCommand = (command: string) => {
    if (command === "accept suggestions") void approveSuggestions();
    else if (command === "review entity") { setTab("definition"); void change("review"); }
    else if (command === "awaken entity") void awaken();
    else if (command === "skip question") void send("Please skip this question and suggest the next useful step. Do not invent confirmed facts.");
    else if (command === "save and exit") void saveAndExit();
  };
  const importFile = (file?: File) => operation(async () => {
    if (!file || !latest.current) return;
    if (!/\.(md|markdown)$/i.test(file.name)) throw new Error("Choose a .md or .markdown document.");
    if (file.size > 256 * 1024) throw new Error("Markdown documents can be at most 256 KiB. Split this document into smaller sources.");
    const next = await importGenesisMarkdown(latest.current, file.name, await file.text());
    adopt(next); setTab("sources"); setSelectedProposals([]); setReplaceImportFacts(false);
    setNotice(next.last_error
      ? "Your source is saved, but interpretation could not finish. Review any available suggestions, continue through conversation, or edit the definition directly. Your confirmed identity is preserved."
      : "Import preview saved. Select the proposed details you want to confirm; your current identity has not been overwritten.");
  });
  const exportMarkdown = () => operation(async () => {
    if (!latest.current) return;
    const markdown = await exportGenesisMarkdown(latest.current.id);
    const url = URL.createObjectURL(new Blob([markdown], { type: "text/markdown;charset=utf-8" }));
    const anchor = document.createElement("a"); anchor.href = url; anchor.download = `${latest.current.definition.name.replace(/[^a-zA-Z0-9_-]/g, "_") || "entity"}-genesis.md`;
    document.body.append(anchor); anchor.click(); anchor.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
    setNotice("Your Markdown export is ready.");
  });

  if (!session) return <section className="genesis genesis-launch" aria-label="Entity Genesis">
    <header className="genesis-header"><div><div className="genesis-eyebrow">Entity Genesis</div><h2>Every presence begins with a spark.</h2><p>Discover someone your family will come to know. Bring an idea, a story, or simply your curiosity.</p></div><button type="button" onClick={onClose}>Back to Hive</button></header>
    {error && <p role="alert" className="genesis-error">{error}</p>}
    <GenesisFirstSignal onBegin={async (seed) => { await begin(seed); }} />
    <h3>Return to a creation</h3>
    {loading ? <p role="status">Loading saved creations…</p> : sessions.length === 0 ? <p className="genesis-muted">Your saved sessions will appear here. Every confirmed choice is kept by Hive.</p> : <ul className="genesis-session-list">{sessions.map((item) => <li key={item.id}><div><strong>{item.definition.name || "An unnamed spark"}</strong><span>{item.state.replace(/_/g, " ")} · {new Date(item.updated_at).toLocaleString()}</span></div><button type="button" disabled={busy} onClick={() => void resume(item.id)}>Resume{item.definition.name ? ` ${item.definition.name}` : " creation"}</button><button type="button" disabled={busy} onClick={() => setConfirmation(`session:${item.id}`)}>{item.definition.entity_id ? "Delete Genesis record and media" : "Delete draft"}</button>{confirmation === `session:${item.id}` && <div className="genesis-confirm"><p>Permanently delete this creation session, sources, and generated media? An awakened Entity and its chat history remain available.</p><button type="button" onClick={() => void operation(async () => { await deleteGenesisSession(item.id); setSessions((previous) => previous.filter((draft) => draft.id !== item.id)); setConfirmation(null); })}>Confirm deletion</button><button type="button" onClick={() => setConfirmation(null)}>Keep creation</button></div>}</li>)}</ul>}
  </section>;

  const definition = session.definition;
  const essence = session.essence ?? essenceFor(definition);
  const pending = session.proposals.filter((proposal) => proposal.status === "pending");
  const unresolved = session.conflicts.filter((conflict) => !conflict.resolved);
  const creationReady = Boolean(definition.name.trim() && definition.purpose.trim()) && unresolved.length === 0;
  const latestAssistant = [...session.messages].reverse().find((message) => message.role !== "user");
  const latestResponse = latestAssistant && (!session.awakening || latestAssistant.created_at > definition.updated_at)
    ? latestAssistant.content
    : session.awakening || latestAssistant?.content || "What is the most important quality this Entity should possess?";
  const renderProposals = (sourceId?: string) => {
    const proposals = pending.filter((proposal) => sourceId ? proposal.source_ref === sourceId : !proposal.source_ref);
    return proposals.map((proposal) => {
      const conflict = unresolved.find((item) => item.proposal_id === proposal.id);
      return <article className={`genesis-proposal ${conflict ? "has-conflict" : ""}`} key={proposal.id}>
        <strong>{proposal.field.replace(/_/g, " ")}</strong><p className="genesis-value">{displayGenesisValue(proposal.value)}</p><p className="genesis-muted">{proposal.rationale}</p>
        {conflict && <p className="genesis-conflict">Your confirmed choice: {displayGenesisValue(conflict.confirmed_value)}. Choose explicitly before replacing it.</p>}
        {sourceId ? <label className="genesis-check"><input type="checkbox" checked={selectedProposals.includes(proposal.id)} onChange={(event) => setSelectedProposals((previous) => event.target.checked ? [...previous, proposal.id] : previous.filter((id) => id !== proposal.id))} />Include in import</label> : <div className="genesis-actions"><button type="button" disabled={busy} onClick={() => void change("review_proposal", { proposal_id: proposal.id, accept: true, replace_confirmed: Boolean(conflict) })}>{conflict ? "Replace confirmed choice" : "Accept suggestion"}</button><button type="button" disabled={busy} onClick={() => void change("review_proposal", { proposal_id: proposal.id, accept: false, replace_confirmed: false })}>{conflict ? "Keep confirmed choice" : "Reject suggestion"}</button></div>}
      </article>;
    });
  };

  return <section className="genesis" aria-label="Entity Genesis workspace">
    <header className="genesis-header"><div><div className="genesis-eyebrow">Entity Genesis · {session.state.replace(/_/g, " ")}</div><h2>{definition.name || "Your Entity is taking shape"}</h2><p>{session.state === "ready" ? "A beginning to return to. Your Entity is ready to meet you." : "Discover a presence. Shape a story. Begin a relationship."}</p></div><div className="genesis-actions"><button type="button" disabled={busy || session.revisions.length === 0 || pendingEdits.length > 0} onClick={() => void change("undo")}>Undo</button><button type="button" disabled={busy} onClick={() => void exportMarkdown()}>Export Markdown</button><button type="button" disabled={busy} onClick={() => void saveAndExit()}>Save and exit</button></div></header>
    <div className="genesis-status" role="status">{busy ? "Saving your next step…" : `Saved in Hive · revision ${session.revision}`}{pending.length > 0 ? ` · ${pending.length} suggestions awaiting your choice` : ""}{unresolved.length > 0 ? ` · ${unresolved.length} unresolved conflicts` : ""}</div>
    {session.last_error && <div className="genesis-error" role="alert"><strong>Your last step needs attention.</strong><p>{session.last_error}</p><p>Your saved source and confirmed choices are still available. Review the available suggestions, try again through Discovery, or edit Definition directly.</p></div>}
    {error && <div className="genesis-error" role="alert">{error}<button type="button" disabled={busy} onClick={() => void operation(async () => { await refresh(true); setNotice("Reloaded the saved session. Your composer is preserved."); })}>Reload saved session</button></div>}
    {notice && <p className="genesis-notice" role="status">{notice}</p>}
    {session.quest && <QuestRail quest={session.quest} onSelect={(stage) => { setQuestStage(stage); setTab("quest"); }} />}
    <div className="genesis-layout">
      <aside className="genesis-panel genesis-essence" aria-label="Essence constellation">
        <div className="genesis-eyebrow">Essence constellation</div><h3>A character, becoming clear</h3>
        <div className="genesis-stars" aria-hidden="true">{DIMENSIONS.map(([key], index) => <span key={key} style={{ "--star": index, opacity: 0.25 + essence[key] / 135 } as React.CSSProperties} />)}</div>
        {DIMENSIONS.map(([key, label]) => <div className="genesis-dimension" key={key}><div><span>{label}</span><span>{essence[key]}%</span></div><progress max={100} value={essence[key]} aria-label={`${label} defined`} /></div>)}
        <p className="genesis-muted">This shows defined characteristics, never your Entity’s value. Explore as much or as little as you like.</p>
        <div className="genesis-readiness"><strong>{creationReady ? "A coherent beginning is ready." : "The essentials"}</strong><p>{!definition.name.trim() ? "Choose a name. " : ""}{!definition.purpose.trim() ? "Give your Entity a purpose. " : ""}{unresolved.length > 0 ? `Resolve ${unresolved.length} conflicting ${unresolved.length === 1 ? "choice" : "choices"}. ` : ""}{creationReady ? "You can awaken your Entity now and keep refining later. Portraits and films are optional." : "Everything else can evolve together."}</p></div>
        <button type="button" className="genesis-primary" disabled={busy || !creationReady} onClick={() => void awaken()}>{definition.entity_id ? "Apply identity refinements" : "Awaken Entity"}</button>
        {definition.entity_id && session.awakening && <button type="button" disabled={busy} onClick={() => void operation(() => openEntity(definition.entity_id!))}>Meet {definition.name}</button>}
        <button type="button" disabled={busy} onClick={() => { setTab("definition"); void change("review"); }}>Review definition</button>
        <p className="genesis-muted">Fictional traits and lore never grant permission to use tools or connected accounts.</p>
      </aside>

      <main className="genesis-panel genesis-discovery">
        <nav className="genesis-tabs" aria-label="Creation sections">{(Object.keys(TAB_LABELS) as Tab[]).filter((item) => item !== "quest" || session.quest).map((item) => <button type="button" aria-current={tab === item ? "page" : undefined} key={item} onClick={() => setTab(item)}>{TAB_LABELS[item]}</button>)}</nav>
        {session.quest && tab === "quest" && <GenesisQuest session={{ ...session, quest: session.quest }} busy={busy} stage={questStage} onStage={setQuestStage} change={change} onSave={saveUpdates} onAwaken={() => void awaken()} onMeet={() => void operation(() => openEntity(definition.entity_id!))} />}
        <div hidden={tab !== "conversation"}>
          <label>Creation mode<select value={session.mode} disabled={busy} onChange={(event) => void change("set_mode", { mode: event.target.value })}>{GENESIS_MODES.map((choice) => <option value={choice.value} key={choice.value}>{choice.label}</option>)}</select></label>
          <div className="genesis-messages" aria-label="Creation conversation" aria-live="polite">
            {session.awakening && <article className="genesis-message assistant"><strong>{definition.name} · Awakening</strong><p>{session.awakening}</p><GenesisChime /></article>}
            {session.messages.length === 0 && <article className="genesis-message assistant"><strong>Genesis guide</strong><p>What is the most important quality this Entity should possess? You can begin with a feeling, a purpose, or someone you have already imagined.</p></article>}
            {session.messages.map((message) => <article className={`genesis-message ${message.role === "user" ? "user" : "assistant"}`} key={message.id}><strong>{message.role === "user" ? "You" : message.role === "entity" ? definition.name : "Genesis guide"}</strong><p>{message.content}</p></article>)}
          </div>
          {session.choices && session.choices.length > 0 && <div className="genesis-seeds" aria-label="Suggested answers">{session.choices.map((choice) => <button type="button" key={choice} disabled={busy} onClick={() => void send(choice)}>{choice}</button>)}</div>}
          {session.messages.length === 0 && <div className="genesis-seeds">{SPARKS.map((spark) => <button type="button" key={spark} onClick={() => setComposer(spark)}>{spark}</button>)}</div>}
          {(session.mode === "voice" || session.mode === "hybrid") && <GenesisVoice disabled={busy} response={latestResponse} onTranscript={(text) => setComposer((previous) => previous ? `${previous}\n${text}` : text)} onSubmit={(text) => void send(text)} onCommand={voiceCommand} />}
          <form onSubmit={(event) => { event.preventDefault(); void send(); }}><label htmlFor="genesis-composer">Tell me what you imagine</label><textarea id="genesis-composer" rows={4} maxLength={16000} value={composer} disabled={busy} onChange={(event) => setComposer(event.target.value)} placeholder="An ancient, compassionate guardian who helps us find our own strength…" /><div className="genesis-actions"><button className="genesis-primary" type="submit" disabled={busy || !composer.trim()}>Continue discovery</button><button type="button" disabled={busy} onClick={() => void send("Please skip this question and ask the next useful question. Do not invent confirmed facts.")}>Skip question</button><button type="button" disabled={busy} onClick={() => void send("Offer a few creative suggestions based on my confirmed choices. Keep every new detail as a proposal awaiting my approval.")}>Suggest a direction</button></div></form>
          {pending.filter((proposal) => !proposal.source_ref).length > 0 && <section aria-label="Suggested details"><h3>Possibilities to choose</h3><p className="genesis-muted">Suggestions become part of your Entity only when you accept them.</p>{renderProposals()}<button type="button" disabled={busy} onClick={() => void approveSuggestions()}>Accept suggestions without conflicts</button></section>}
          <details><summary>Conversation privacy</summary><p>Clear this session’s conversation and saved composer without deleting confirmed identity details.</p><button type="button" disabled={busy} onClick={() => setConfirmation("transcript")}>Delete transcript</button>{confirmation === "transcript" && <div className="genesis-confirm"><p>Delete the creation transcript?</p><button type="button" onClick={() => void operation(async () => { if (!latest.current) return; adopt(await genesisAction(latest.current, "clear_transcript")); setComposer(""); setConfirmation(null); })}>Confirm transcript deletion</button><button type="button" onClick={() => setConfirmation(null)}>Keep transcript</button></div>}</details>
        </div>
        <div hidden={tab !== "definition"}><GenesisDefinition definition={definition} busy={busy} onSave={saveUpdates} onDraftChanges={setPendingEdits} /></div>
        <div hidden={tab !== "sources"}>
          <h3>Bring your story</h3><p>Import a biography, a character sheet, lore, or an exported Entity. Review proposed details before applying them.</p>
          <label className="genesis-upload">Choose Markdown document<input type="file" accept=".md,.markdown,text/markdown" disabled={busy} onChange={(event) => { void importFile(event.target.files?.[0]); event.target.value = ""; }} /><small>.md or .markdown · maximum 256 KiB per source · up to 20 sources</small></label>
          {session.sources.length === 0 && <p className="genesis-muted">Your original document is preserved alongside extracted details. Unrecognized sections remain available to you.</p>}
          {session.sources.map((source) => {
            const sourceProposals = pending.filter((proposal) => proposal.source_ref === source.id);
            const selected = selectedProposals.filter((id) => sourceProposals.some((proposal) => proposal.id === id));
            return <article className="genesis-source" key={source.id}><h4>{source.filename} {source.committed ? "· Reviewed" : "· Preview"}</h4><details><summary>Read original document</summary><pre>{source.content}</pre></details>{source.unrecognized_sections.length > 0 && <details><summary>Preserved unrecognized sections ({source.unrecognized_sections.length})</summary>{source.unrecognized_sections.map((section, index) => <pre key={index}>{section}</pre>)}</details>}{renderProposals(source.id)}
              {sourceProposals.length > 0 && <><label className="genesis-check"><input type="checkbox" checked={replaceImportFacts} onChange={(event) => setReplaceImportFacts(event.target.checked)} />Replace confirmed choices for any conflicts I selected above.</label><button type="button" className="genesis-primary" disabled={busy} onClick={() => void change("commit_import", { source_id: source.id, proposal_ids: selected, replace_confirmed: replaceImportFacts }).then((result) => { if (result) { setSelectedProposals([]); setReplaceImportFacts(false); } })}>{selected.length === 0 ? "Keep current identity" : `Commit ${selected.length} selected ${selected.length === 1 ? "detail" : "details"}`}</button></>}
              <button type="button" disabled={busy} onClick={() => setConfirmation(`source:${source.id}`)}>Delete source document</button>{confirmation === `source:${source.id}` && <div className="genesis-confirm"><p>Delete this document and its uncommitted suggestions? Confirmed identity details remain.</p><button type="button" onClick={() => void change("delete_source", { source_id: source.id }).then((result) => { if (result) setConfirmation(null); })}>Confirm source deletion</button><button type="button" onClick={() => setConfirmation(null)}>Keep source</button></div>}
            </article>;
          })}
        </div>
        <div hidden={tab !== "chronicle"}><h3>Creation chronicle</h3><p>Every accepted change is part of your Entity’s beginning. This is a record of creation, separate from fictional memories.</p>{session.revisions.length ? <ol className="genesis-chronicle">{session.revisions.map((revision, index) => <li key={`${revision.revision}-${index}`}><strong>{revision.reason}</strong><span>{new Date(revision.created_at).toLocaleString()} · revision {revision.revision}</span></li>)}</ol> : <p className="genesis-muted">Your first confirmed choice will begin the chronicle.</p>}<h4>The first memory</h4><p>Imagine a founding moment together. Any memory you add is labeled as created lore.</p><button type="button" disabled={busy} onClick={() => { setTab("conversation"); void send("Help me imagine a fictional founding memory for this Entity. Label it as created lore and propose it for my approval."); }}>Imagine a first memory</button></div>
        <GenesisRetention days={session.retention_days ?? null} busy={busy} onSave={(days) => change("set_retention", { days })} />
      </main>
      <GenesisStudio session={session} busy={busy} onUpdate={saveUpdates} onRefresh={refresh} />
    </div>
    {celebration && definition.entity_id && <GenesisCelebration entityId={definition.entity_id} name={definition.name} presence={session.quest?.presence ?? null} newBadges={celebration} onClose={() => setCelebration(null)} onMeet={() => { setCelebration(null); void operation(() => openEntity(definition.entity_id!)); }} />}
  </section>;
}
