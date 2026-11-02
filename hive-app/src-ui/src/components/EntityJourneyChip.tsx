import { useEffect, useState } from "react";
import { getEntityJourney, type EntityJourney } from "../lib/questClient";
import PresenceCrest from "./PresenceCrest";

/** Bond level and Presence crest for a born Entity's card. Opens the full Journey panel. */
export default function EntityJourneyChip({
  entityId,
  refreshKey,
  onOpen,
}: {
  entityId: string;
  refreshKey?: unknown;
  onOpen?: () => void;
}) {
  const [journey, setJourney] = useState<EntityJourney | null>(null);
  useEffect(() => {
    let current = true;
    void getEntityJourney(entityId).then((next) => { if (current) setJourney(next); }).catch(() => { if (current) setJourney(null); });
    return () => { current = false; };
  }, [entityId, refreshKey]);
  if (!journey) return null;
  const earned = journey.badges.filter((badge) => badge.earned_at).length;
  const summary = `Level ${journey.level.level} · ${journey.level.title}`;
  const content = <>
    {journey.presence && <PresenceCrest palette={journey.presence.identity} crest={journey.presence.crest} size={20} />}
    <span>{summary}</span>
    <span aria-label={`${earned} badges earned`}>✦ {earned}</span>
    <span aria-label="Journey activity">{journey.mentor_messages} messages · {journey.conversation_sessions} sessions · {journey.successful_skill_uses} skills · {journey.confirmed_memories} memories</span>
    {journey.next_milestone && <span title={`${journey.next_milestone.progress} of ${journey.next_milestone.target}`}>Next · {journey.next_milestone.title}</span>}
  </>;
  if (!onOpen) {
    return <div className="entity-journey-chip mt-2 flex flex-wrap items-center gap-2 text-xs text-theme-text-dim">{content}</div>;
  }
  return <button
    type="button"
    className="entity-journey-chip entity-journey-chip-button mt-2 flex flex-wrap items-center gap-2 text-xs text-theme-text-dim"
    onClick={onOpen}
    aria-label={`Open ${journey.name}'s Journey`}
  >
    {content}
    <span className="entity-journey-chip-action">View journey</span>
  </button>;
}
