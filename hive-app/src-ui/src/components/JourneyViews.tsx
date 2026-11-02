import type {
  BadgeDefinition,
  BadgeView,
  BondLevel,
  BondTierDefinition,
  EntityJourney,
  FirstQuest,
  JourneyMetric,
  LongArcQuestDefinition,
  LongArcQuestView,
} from "../lib/questClient";

/** Human-readable activity counts shared by Genesis Quest and the post-creation Journey. */
export function activityLabel(metric: JourneyMetric, count: number) {
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

export function QuestLevel({ level }: { level: BondLevel }) {
  const span = level.next_level_at === null ? 1 : level.next_level_at - level.level_floor;
  const progress = level.next_level_at === null ? 1 : (level.xp - level.level_floor) / span;
  return <div className="quest-level">
    <span><strong>Level {level.level} · {level.title}</strong><span>{level.xp} XP{level.next_level_at !== null ? ` · ${level.next_level_at - level.xp} to next` : ""}</span></span>
    <progress max={1} value={progress} aria-label={`Bond level progress, ${level.xp} XP`} />
  </div>;
}

export function TierPath({ tiers, level }: { tiers: BondTierDefinition[]; level: BondLevel }) {
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

export function JourneyActivity({ journey }: { journey: EntityJourney }) {
  return <dl className="quest-activity" aria-label="Journey activity">
    <div><dt>Conversation</dt><dd>{activityLabel("mentor_messages", journey.mentor_messages)}</dd></div>
    <div><dt>Returns</dt><dd>{activityLabel("conversation_sessions", journey.conversation_sessions)}</dd></div>
    <div><dt>Rhythm</dt><dd>{activityLabel("active_days", journey.active_days)} · {activityLabel("active_weeks", journey.active_weeks)}</dd></div>
    <div><dt>Skills</dt><dd>{activityLabel("successful_skill_uses", journey.successful_skill_uses)}</dd></div>
    <div><dt>Memory</dt><dd>{activityLabel("confirmed_memories", journey.confirmed_memories)}</dd></div>
  </dl>;
}

export function JourneyMilestone({
  milestone,
  badge,
}: {
  milestone: NonNullable<EntityJourney["next_milestone"]>;
  badge?: BadgeDefinition;
}) {
  const metric = badge?.requirement?.metric ?? milestone.metric;
  const title = badge?.title ?? milestone.title;
  return <div className="quest-milestone">
    <span>{title}</span>
    <progress max={milestone.target} value={milestone.progress} aria-label={`${title} progress`} />
    <small>{milestone.progress} of {activityLabel(metric, milestone.target)}</small>
  </div>;
}

export function LongArcQuests({ definitions, views }: { definitions: LongArcQuestDefinition[]; views: LongArcQuestView[] }) {
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

export function FirstQuests({
  quests,
  badges,
}: {
  quests: FirstQuest[];
  badges: Array<Pick<BadgeDefinition, "badge" | "title">>;
}) {
  if (quests.length === 0) return null;
  return <ol className="quest-first" aria-label="Suggested first quests">
    {quests.map((item) => <li key={item.id} className={item.complete ? "is-complete" : ""}>
      <strong>{item.title}</strong>
      <p>“{item.prompt}”</p>
      <small>{item.complete ? "Complete" : `Unlocks ${badges.find((badge) => badge.badge === item.milestone)?.title ?? "a badge"}`}</small>
    </li>)}
  </ol>;
}

/** Shared post-awakening body used by Genesis Quest and the Entity Journey panel. */
export function JourneyProgressBody({
  journey,
  catalog,
}: {
  journey: EntityJourney;
  catalog: { badges: BadgeDefinition[]; tiers: BondTierDefinition[]; long_arc_quests: LongArcQuestDefinition[] };
}) {
  const milestone = journey.next_milestone;
  const milestoneDefinition = milestone
    ? catalog.badges.find((badge) => badge.badge === milestone.badge)
    : undefined;
  return <>
    <TierPath tiers={catalog.tiers} level={journey.level} />
    <JourneyActivity journey={journey} />
    {milestone && <JourneyMilestone milestone={milestone} badge={milestoneDefinition} />}
    <LongArcQuests definitions={catalog.long_arc_quests ?? []} views={journey.long_arc_quests ?? []} />
    <FirstQuests quests={journey.first_quests ?? []} badges={catalog.badges} />
  </>;
}
