import { useEffect, useState } from "react";
import type { FieldUpdate, GenesisSession } from "../lib/genesisClient";
import { CRESTS, getEntityJourney, getQuestCatalog, MOTIONS, PALETTES, TEMPERAMENTS, TRAIT_LABELS, type BadgeView, type BondTierDefinition, type EntityJourney, type LongArcQuestDefinition, type LongArcQuestView, type PresenceChoice, type QuestCatalog, type QuestStageId, type QuestView } from "../lib/questClient";
import PresenceCrest from "./PresenceCrest";

type Change = (action: string, args?: Record<string, unknown>) => Promise<GenesisSession | undefined>;

function activityLabel(metric: NonNullable<EntityJourney["next_milestone"]>["metric"], count: number) {
  const labels = {
    mentor_messages: ["message", "messages"],
    conversation_sessions: ["session", "sessions"],
    active_days: ["active day", "active days"],
    active_weeks: ["active week", "active weeks"],
    successful_skill_uses: ["skill use", "skill uses"],
    confirmed_memories: ["confirmed memory", "confirmed memories"],
  } as const;
  return `${count} ${labels[metric][count === 1 ? 0 : 1]}`;
}

function presenceChoice(card: QuestView["presence"]): PresenceChoice {
  return card ? {
    temperament: card.temperament,
    identity: card.identity,
    crest: card.crest,
    motion: card.motion,
  } : { temperament: "gentle", identity: "aether", crest: "orbit", motion: "calm" };
}

export function QuestLevel({ quest }: { quest: QuestView }) {
  const { level } = quest;
  const span = level.next_level_at === null ? 1 : level.next_level_at - level.level_floor;
  const progress = level.next_level_at === null ? 1 : (level.xp - level.level_floor) / span;
  return <div className="quest-level">
    <span><strong>Level {level.level} · {level.title}</strong><span>{level.xp} XP{level.next_level_at !== null ? ` · ${level.next_level_at - level.xp} to next` : ""}</span></span>
    <progress max={1} value={progress} aria-label={`Bond level progress, ${level.xp} XP`} />
  </div>;
}

export function TierPath({ tiers, level }: { tiers: BondTierDefinition[]; level: QuestView["level"] }) {
  let current = 0;
  tiers.forEach((tier, index) => { if (tier.floor <= level.xp) current = index; });
  return <ol className="quest-tier-path" aria-label="Bond tier path">
    {tiers.map((tier, index) => {
      const state = index === current ? "Current" : index === current + 1 ? "Next" : null;
      const label = `${tier.title} · ${tier.floor} XP${state ? ` · ${state}` : ""}`;
      return <li key={tier.level} className={state ? `is-${state.toLowerCase()}` : ""} aria-label={label} aria-current={state === "Current" ? "step" : undefined}>
        <strong>{tier.title}</strong><small>{tier.floor} XP{state ? ` · ${state}` : ""}</small>
      </li>;
    })}
  </ol>;
}

/** Always-visible progress through the seven stages. */
export function QuestRail({ quest, onSelect }: { quest: QuestView; onSelect: (stage: QuestStageId) => void }) {
  return <nav className="quest-rail" aria-label={`Genesis quest, ${quest.completed} of ${quest.stages.length} stages complete`}>
    <ol>{quest.stages.map((stage) => <li key={stage.stage} className={stage.complete ? "is-complete" : stage.stage === quest.current ? "is-current" : ""}>
      <button type="button" onClick={() => onSelect(stage.stage)} aria-current={stage.stage === quest.current ? "step" : undefined}>
        <span className="quest-node" aria-hidden="true" /><span>{stage.title}</span><small>{stage.complete ? "Complete" : stage.optional ? "Optional" : stage.detail}</small>
      </button>
    </li>)}</ol>
    <QuestLevel quest={quest} />
  </nav>;
}

export function BadgeShelf({ badges }: { badges: BadgeView[] }) {
  const earned = badges.filter((badge) => badge.earned_at).length;
  return <section className="quest-badges" aria-label="Badges">
    <h4>Badges · {earned} of {badges.length}</h4>
    <ul>{badges.map((badge) => <li key={badge.badge} className={badge.earned_at ? "is-earned" : ""} title={badge.hint}>
      <span className="quest-badge-mark" aria-hidden="true">{badge.earned_at ? "✦" : "·"}</span>
      <span><strong>{badge.title}</strong><small>{badge.earned_at ? `Earned · ${badge.xp} XP` : badge.hint}</small></span>
    </li>)}</ul>
  </section>;
}

function LongArcQuests({ definitions, views }: { definitions: LongArcQuestDefinition[]; views: LongArcQuestView[] }) {
  const safeDefinitions = definitions ?? [];
  const safeViews = views ?? [];
  if (safeViews.length === 0) return null;
  return <section className="quest-long-arc" aria-label="Seasonal quests">
    <h4>Seasonal quests</h4>
    <div>{safeViews.map((view) => {
      const definition = safeDefinitions.find((candidate) => candidate.id === view.quest_id);
      if (!definition) return null;
      const scope = definition.scope === "entity" ? "Entity" : "Family";
      const familyDetail = `${view.active_entities} ${view.active_entities === 1 ? "Entity" : "Entities"} active`;
      const needsSecondEntity = definition.scope === "family" && view.progress >= definition.target && view.active_entities < definition.minimum_entities;
      return <article key={view.quest_id} aria-label={definition.title}>
        <header><strong>{definition.title}</strong><span>{!view.available ? "Unavailable" : view.complete ? "Complete" : "In progress"}</span></header>
        <small>{scope} · {view.season_label}</small>
        <p>{definition.summary}</p>
        {view.available ? <>
          <progress max={definition.target} value={Math.min(view.progress, definition.target)} aria-label={`${definition.title} progress`} />
          <small>{view.progress} of {definition.target}{needsSecondEntity ? " · A second Entity is needed" : !view.complete && definition.scope === "family" ? ` · ${familyDetail}` : ""}</small>
        </> : <small role="status">Progress unavailable. Try again shortly.</small>}
      </article>;
    })}</div>
  </section>;
}

export default function GenesisQuest({ session, busy, stage, onStage, change, onSave, onAwaken, onMeet }: {
  session: GenesisSession & { quest: QuestView }; busy: boolean; stage: QuestStageId; onStage: (stage: QuestStageId) => void;
  change: Change; onSave: (updates: FieldUpdate[]) => Promise<void>; onAwaken: () => void; onMeet: () => void;
}) {
  const { quest, definition } = session;
  const [catalog, setCatalog] = useState<QuestCatalog | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [journey, setJourney] = useState<EntityJourney | null>(null);
  const [reservation, setReservation] = useState({ name: definition.name, purpose: definition.purpose });
  const [presence, setPresence] = useState<PresenceChoice>(() => presenceChoice(quest.presence));
  const awakened = Boolean(session.awakening && definition.entity_id);

  useEffect(() => { void getQuestCatalog().then(setCatalog).catch((error) => setCatalogError(error instanceof Error ? error.message : String(error))); }, []);
  const acceptedCard = catalog?.temperament_cards.find((card) => session.proposals.some((proposal) => proposal.status === "accepted" && proposal.rationale.startsWith(`Temperament card: ${card.title}.`)));
  useEffect(() => {
    if (acceptedCard && !quest.presence) setPresence((previous) => ({ ...previous, temperament: acceptedCard.temperament }));
  }, [acceptedCard, quest.presence]);
  useEffect(() => { setReservation({ name: definition.name, purpose: definition.purpose }); }, [definition.name, definition.purpose]);
  useEffect(() => {
    if (!awakened || !definition.entity_id) return;
    let current = true;
    void getEntityJourney(definition.entity_id).then((next) => { if (current) setJourney(next); }).catch(() => { if (current) setJourney(null); });
    return () => { current = false; };
  }, [awakened, definition.entity_id, session.revision]);

  const view = quest.stages.find((item) => item.stage === stage) ?? quest.stages[0];
  const pendingTemperament = session.proposals.filter((proposal) => proposal.status === "pending" && proposal.field === "personality" && proposal.rationale.startsWith("Temperament card:"));
  const answer = (scenario: string) => quest.trial_choices.find((choice) => choice.scenario_id === scenario)?.choice_id;
  const unlocked = { palettes: new Set(journey?.unlocked.palettes ?? quest.unlocked.palettes), crests: new Set(journey?.unlocked.crests ?? quest.unlocked.crests) };
  const lockHint = (badge: string | null) => catalog?.badges.find((item) => item.badge === badge)?.hint ?? "";
  const reservationDirty = reservation.name.trim() !== definition.name || reservation.purpose.trim() !== definition.purpose;

  const panel = () => {
    if (!catalog) return <p className="genesis-muted" role="status">{catalogError || "Preparing the quest…"}</p>;
    switch (view.stage) {
      case "reservation": return <form onSubmit={(event) => { event.preventDefault(); void onSave((["name", "purpose"] as const).filter((field) => reservation[field].trim() !== definition[field]).map((field) => ({ field, value: reservation[field].trim() }))).catch(() => undefined); }}>
        <label>Reserved name<input value={reservation.name} maxLength={80} disabled={busy} onChange={(event) => setReservation({ ...reservation, name: event.target.value })} /></label>
        <label>Purpose<textarea rows={2} maxLength={2000} value={reservation.purpose} disabled={busy} onChange={(event) => setReservation({ ...reservation, purpose: event.target.value })} placeholder="Help our family plan calm mornings" /></label>
        <button type="submit" className="genesis-primary" disabled={busy || !reservationDirty || !reservation.name.trim()}>Reserve</button>
      </form>;
      case "soul": return <>
        <div className="quest-cards">{catalog.temperament_cards.map((card) => <article key={card.id} className="quest-card">
          <header><strong>{card.title}</strong><span>{TEMPERAMENTS[card.temperament]}</span></header>
          <p>{card.tagline}</p>
          <dl>{Object.entries(card.stats).map(([trait, value]) => <div key={trait}><dt>{TRAIT_LABELS[trait] ?? trait}</dt><dd className="quest-stat" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={value} aria-label={TRAIT_LABELS[trait] ?? trait}><span style={{ width: `${value}%` }} /></dd></div>)}</dl>
          <button type="button" disabled={busy} onClick={() => void change("choose_temperament", { card_id: card.id })}>Suggest {card.title}</button>
        </article>)}</div>
        {pendingTemperament.map((proposal) => <div className="quest-pending" key={proposal.id}><p>{proposal.rationale}</p><div className="genesis-actions"><button type="button" className="genesis-primary" disabled={busy} onClick={() => void change("review_proposal", { proposal_id: proposal.id, accept: true, replace_confirmed: false })}>Accept temperament</button><button type="button" disabled={busy} onClick={() => void change("review_proposal", { proposal_id: proposal.id, accept: false, replace_confirmed: false })}>Not this one</button></div></div>)}
        <p className="genesis-muted">A card is a suggestion. It joins the personality only when you accept it.</p>
      </>;
      case "ethics": return <>
        <p>{quest.soul_path === "forge" ? "All three trials are answered. Birth will forge a soul from your choices." : "Answer all three to forge a soul from your values. Otherwise your Entity begins with a balanced soul."}</p>
        {catalog.trials.map((trial, index) => <fieldset key={trial.id} className="quest-trial"><legend>Trial {index + 1} · {trial.title}</legend><p>{trial.description}</p>
          <div className="quest-choices">{trial.choices.map((choice) => <button type="button" key={choice.id} aria-pressed={answer(trial.id) === choice.id} disabled={busy} onClick={() => void change("choose_trial", { scenario_id: trial.id, choice_id: choice.id })}><strong>{choice.label}</strong><span>{choice.description}</span></button>)}</div>
        </fieldset>)}
        {quest.trial_choices.length > 0 && <button type="button" disabled={busy} onClick={() => void change("clear_trials")}>Clear answers</button>}
      </>;
      case "instincts": return <>
        <p>Every Entity is born with these. They are part of the signed constitution and cannot be changed by story or style.</p>
        <pre className="quest-instincts">{catalog.instincts}</pre>
        {quest.instincts_acknowledged_at ? <p className="genesis-notice">Read on {new Date(quest.instincts_acknowledged_at).toLocaleDateString()}.</p> : <button type="button" className="genesis-primary" disabled={busy} onClick={() => void change("acknowledge_instincts")}>I have read these</button>}
      </>;
      case "presence": return <div className="quest-presence">
        <div className="quest-presence-preview"><PresenceCrest palette={presence.identity} crest={presence.crest} motion={presence.motion} size={96} label={`${definition.name || "Entity"} crest preview`} /><div><strong>{definition.name || "Unnamed"}</strong><span>{TEMPERAMENTS[presence.temperament]} · {PALETTES[presence.identity].label}</span></div></div>
        <label>Temperament<select value={presence.temperament} disabled={busy} onChange={(event) => setPresence({ ...presence, temperament: event.target.value as PresenceChoice["temperament"] })}>{Object.entries(TEMPERAMENTS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
        <fieldset><legend>Palette</legend><div className="quest-swatches">{catalog.palettes.map(({ palette, unlocked_by }) => { const open = unlocked.palettes.has(palette); return <button type="button" key={palette} aria-pressed={presence.identity === palette} disabled={busy || !open} title={open ? PALETTES[palette].label : `Locked · ${lockHint(unlocked_by)}`} onClick={() => setPresence({ ...presence, identity: palette })}><span style={{ background: `linear-gradient(135deg, ${PALETTES[palette].colors[0]}, ${PALETTES[palette].colors[1]})` }} aria-hidden="true" />{PALETTES[palette].label}{!open && <small>Locked</small>}</button>; })}</div></fieldset>
        <fieldset><legend>Crest</legend><div className="quest-swatches">{catalog.crests.map(({ crest, unlocked_by }) => { const open = unlocked.crests.has(crest); return <button type="button" key={crest} aria-pressed={presence.crest === crest} disabled={busy || !open} title={open ? CRESTS[crest] : `Locked · ${lockHint(unlocked_by)}`} onClick={() => setPresence({ ...presence, crest })}><PresenceCrest palette={presence.identity} crest={crest} size={28} />{CRESTS[crest]}{!open && <small>Locked</small>}</button>; })}</div></fieldset>
        <fieldset><legend>Motion</legend><div className="quest-swatches">{Object.entries(MOTIONS).map(([value, label]) => <button type="button" key={value} aria-pressed={presence.motion === value} disabled={busy} onClick={() => setPresence({ ...presence, motion: value as PresenceChoice["motion"] })}>{label}</button>)}</div></fieldset>
        <button type="button" className="genesis-primary" disabled={busy || !definition.name.trim()} onClick={() => void change("set_presence", { presence })}>{quest.presence ? "Update Presence" : "Choose this Presence"}</button>
        <p className="genesis-muted">Presence is how {definition.name || "your Entity"} appears in Hive. It is never read by the model and grants nothing.</p>
      </div>;
      case "birth": return <>
        <p>{awakened ? `${definition.name} has been signed and awakened.` : quest.soul_path === "forge" ? "The Soul Forge will shape the soul from your three trial answers." : "Your Entity will begin with a balanced soul. Answer the trials first to forge a unique one."}</p>
        <ul className="quest-checklist">{quest.stages.slice(0, 5).map((item) => <li key={item.stage} className={item.complete ? "is-complete" : ""}>{item.title} · {item.complete ? "complete" : item.optional ? "optional" : item.detail}</li>)}</ul>
        {awakened ? <button type="button" className="genesis-primary" disabled={busy} onClick={onMeet}>Meet {definition.name}</button> : <button type="button" className="genesis-primary" disabled={busy || !quest.stages[0].complete} onClick={onAwaken}>Awaken {definition.name || "Entity"}</button>}
      </>;
      case "awakening": {
        const milestoneDefinition = journey?.next_milestone ? catalog.badges.find((badge) => badge.badge === journey.next_milestone?.badge) : undefined;
        const milestone = journey?.next_milestone;
        const metric = milestoneDefinition?.requirement?.metric ?? milestone?.metric;
        return awakened ? <>
          <TierPath tiers={catalog.tiers} level={journey?.level ?? quest.level} />
          {journey && <dl className="quest-activity" aria-label="Journey activity">
            <div><dt>Conversation</dt><dd>{activityLabel("mentor_messages", journey.mentor_messages)}</dd></div>
            <div><dt>Returns</dt><dd>{activityLabel("conversation_sessions", journey.conversation_sessions)}</dd></div>
            <div><dt>Rhythm</dt><dd>{activityLabel("active_days", journey.active_days)} · {activityLabel("active_weeks", journey.active_weeks)}</dd></div>
            <div><dt>Skills</dt><dd>{activityLabel("successful_skill_uses", journey.successful_skill_uses)}</dd></div>
            <div><dt>Memory</dt><dd>{activityLabel("confirmed_memories", journey.confirmed_memories)}</dd></div>
          </dl>}
          {milestone && metric && <div className="quest-milestone"><span>{milestoneDefinition?.title ?? milestone.title}</span><progress max={milestone.target} value={milestone.progress} aria-label={`${milestoneDefinition?.title ?? milestone.title} progress`} /><small>{milestone.progress} of {activityLabel(metric, milestone.target)}</small></div>}
          {journey && <LongArcQuests definitions={catalog.long_arc_quests ?? []} views={journey.long_arc_quests ?? []} />}
          <ol className="quest-first">{(journey?.first_quests ?? []).map((item) => <li key={item.id} className={item.complete ? "is-complete" : ""}><strong>{item.title}</strong><p>“{item.prompt}”</p><small>{item.complete ? "Complete" : `Unlocks ${catalog.badges.find((badge) => badge.badge === item.milestone)?.title ?? "a badge"}`}</small></li>)}</ol>
          <button type="button" className="genesis-primary" disabled={busy} onClick={onMeet}>Meet {definition.name}</button>
          <p className="genesis-muted">Nothing is sent for you. Copy a prompt, or say whatever feels right.</p>
        </> : <p>Awaken {definition.name || "your Entity"} to begin their first quests.</p>;
      }
    }
  };

  return <section className="quest" aria-label="Genesis quest">
    <header className="quest-stage-head"><div className="genesis-eyebrow">Stage {quest.stages.indexOf(view) + 1} of {quest.stages.length}{view.optional ? " · optional" : ""}</div><h3>{view.title}</h3><p>{view.summary}</p></header>
    {panel()}
    <div className="genesis-actions quest-nav">
      {quest.stages.indexOf(view) > 0 && <button type="button" onClick={() => onStage(quest.stages[quest.stages.indexOf(view) - 1].stage)}>Back</button>}
      {quest.stages.indexOf(view) < quest.stages.length - 1 && <button type="button" onClick={() => onStage(quest.stages[quest.stages.indexOf(view) + 1].stage)}>{view.optional && !view.complete ? "Skip for now" : "Next stage"}</button>}
    </div>
    <BadgeShelf badges={journey?.badges ?? quest.badges} />
  </section>;
}
