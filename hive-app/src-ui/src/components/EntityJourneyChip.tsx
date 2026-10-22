import { useEffect, useState } from "react";
import { getEntityJourney, type EntityJourney } from "../lib/questClient";
import PresenceCrest from "./PresenceCrest";

/** Bond level and Presence crest for a born Entity's card. Silent on failure. */
export default function EntityJourneyChip({ entityId, refreshKey }: { entityId: string; refreshKey?: unknown }) {
  const [journey, setJourney] = useState<EntityJourney | null>(null);
  useEffect(() => {
    let current = true;
    void getEntityJourney(entityId).then((next) => { if (current) setJourney(next); }).catch(() => { if (current) setJourney(null); });
    return () => { current = false; };
  }, [entityId, refreshKey]);
  if (!journey) return null;
  const earned = journey.badges.filter((badge) => badge.earned_at).length;
  return <div className="mt-2 flex items-center gap-2 text-xs text-theme-text-dim">
    {journey.presence && <PresenceCrest palette={journey.presence.identity} crest={journey.presence.crest} size={20} />}
    <span>Level {journey.level.level} · {journey.level.title}</span>
    <span aria-label={`${earned} badges earned`}>✦ {earned}</span>
  </div>;
}
