import { useId, type ReactElement } from "react";
import { PALETTES, type Crest, type Motion, type Palette } from "../lib/questClient";

const GLYPHS: Record<Crest, ReactElement> = {
  orbit: <><circle cx="32" cy="32" r="6" fill="currentColor" /><ellipse cx="32" cy="32" rx="19" ry="8" transform="rotate(-24 32 32)" /><circle cx="49" cy="25" r="2.5" fill="currentColor" /></>,
  bloom: <>{[0, 60, 120, 180, 240, 300].map((angle) => <ellipse key={angle} cx="32" cy="20" rx="5" ry="10" transform={`rotate(${angle} 32 32)`} />)}<circle cx="32" cy="32" r="4" fill="currentColor" /></>,
  wave: <><path d="M12 28c5-6 10-6 15 0s10 6 15 0 10-6 10-6" /><path d="M12 38c5-6 10-6 15 0s10 6 15 0 10-6 10-6" /></>,
  lantern: <><path d="M26 18h12M24 22h16l-2 22H26z" /><path d="M32 28c3 3 3 7 0 10-3-3-3-7 0-10z" fill="currentColor" /><path d="M28 48h8" /></>,
  compass: <><path d="M32 12l5 15 15 5-15 5-5 15-5-15-15-5 15-5z" fill="currentColor" fillOpacity=".25" /><circle cx="32" cy="32" r="3" fill="currentColor" /></>,
  constellation: <><path d="M16 40l10-16 12 6 10-12M38 30l4 16" />{[[16, 40], [26, 24], [38, 30], [48, 18], [42, 46]].map(([x, y]) => <circle key={`${x}-${y}`} cx={x} cy={y} r="2.8" fill="currentColor" />)}</>,
};

/** Decorative unless labelled; never conveys state that is not also in text. */
export default function PresenceCrest({ palette, crest, motion = "still", size = 48, label }: {
  palette: Palette; crest: Crest; motion?: Motion; size?: number; label?: string;
}) {
  const gradient = useId();
  const [from, to] = PALETTES[palette].colors;
  return <svg className={`presence-crest is-${motion}`} width={size} height={size} viewBox="0 0 64 64" role={label ? "img" : undefined} aria-label={label} aria-hidden={label ? undefined : true}>
    <defs><linearGradient id={gradient} x1="0" y1="0" x2="1" y2="1"><stop offset="0" stopColor={from} /><stop offset="1" stopColor={to} /></linearGradient></defs>
    <circle cx="32" cy="32" r="30" fill={`url(#${gradient})`} opacity=".22" />
    <circle cx="32" cy="32" r="29" fill="none" stroke={`url(#${gradient})`} strokeWidth="2" />
    <g className="presence-crest-glyph" fill="none" stroke={to} strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" color={to}>{GLYPHS[crest]}</g>
  </svg>;
}
