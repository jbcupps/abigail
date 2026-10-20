import { useEffect, useId, useRef, useState } from "react";
import type { VisualIdentity } from "./profile";

export type PresenceState = "idle" | "listening" | "thinking" | "speaking" | "interrupted" | "error";
export interface PresenceMedia {
  kind: "portrait" | "video";
  src: string;
  poster?: string;
  loops?: Partial<Record<PresenceState, string>>;
}
// Future audio adapters must supply actual capture/playback events. Visual
// animation is never evidence of listening, reasoning, or speech.
export interface PresenceAudioAdapter {
  stop: () => void;
  setMuted: (muted: boolean) => void;
  subscribe: (listener: (event: "capture-start" | "capture-end" | "playback-start" | "playback-end" | "error") => void) => () => void;
}
interface Props {
  identity?: VisualIdentity;
  state?: PresenceState;
  label?: string;
  reducedMotion?: boolean;
  compact?: boolean;
  media?: PresenceMedia;
  muted?: boolean;
}
function localMedia(src?: string): string | undefined {
  if (!src) return undefined;
  try {
    const url = new URL(src, window.location.href);
    return url.origin === window.location.origin || url.protocol === "blob:" ? url.href : undefined;
  } catch { return undefined; }
}
export default function Presence({ identity = "aether", state = "idle", label = "Procedural portrait", reducedMotion = false, compact = false, media, muted = true }: Props) {
  const id = useId().replace(/:/g, "");
  const [failedMedia, setFailedMedia] = useState<string | null>(null);
  const [systemReduced, setSystemReduced] = useState(() => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false);
  const video = useRef<HTMLVideoElement>(null);
  const source = localMedia(media?.loops?.[state] ?? media?.src);
  const mediaKey = source && media ? `${media.kind}:${source}` : null;
  const failed = mediaKey !== null && failedMedia === mediaKey;
  const paused = reducedMotion || systemReduced;
  useEffect(() => {
    const query = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!query) return;
    const update = () => setSystemReduced(query.matches);
    update(); query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  useEffect(() => { setFailedMedia(null); }, [mediaKey]);
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    let current = true;
    if (paused) element.pause();
    else void element.play().catch(() => {
      // play() can reject after pause, unmount, or a different state loop has
      // replaced this clip. Only the current playback attempt owns fallback.
      if (current) setFailedMedia(mediaKey);
    });
    return () => { current = false; element.pause(); };
  }, [paused, mediaKey, failed]);
  return <div className={`presence-art identity-${identity} state-${state}${paused ? " motion-paused" : ""}${compact ? " is-compact" : ""}`}>
    <div className="presence-halo" aria-hidden="true" />
    {source && !failed ? media?.kind === "video" ?
      <video key={mediaKey} ref={video} className="presence-media" src={source} poster={localMedia(media.poster)} muted={muted} loop playsInline autoPlay={!paused} onError={() => setFailedMedia(mediaKey)} aria-label={label} /> :
      <img key={mediaKey} className="presence-media" src={source} alt={label} onError={() => setFailedMedia(mediaKey)} /> :
      <svg viewBox="0 0 500 560" role="img" aria-label={label} className="presence-figure">
        <defs>
          <linearGradient id={`${id}line`} x1="0" y1="0" x2="1" y2="1"><stop stopColor="var(--presence-bright)" /><stop offset=".45" stopColor="var(--presence-color)" /><stop offset="1" stopColor="var(--presence-secondary)" /></linearGradient>
          <radialGradient id={`${id}core`}><stop stopColor="var(--presence-bright)" stopOpacity=".4" /><stop offset="1" stopColor="var(--presence-color)" stopOpacity="0" /></radialGradient>
          <filter id={`${id}glow`}><feGaussianBlur stdDeviation="2.8" /></filter>
        </defs>
        <g fill="none" stroke="var(--presence-color)" opacity=".22"><ellipse cx="250" cy="455" rx="180" ry="38" /><ellipse cx="250" cy="455" rx="146" ry="26" /><path d="M250 48V86 M250 482V512 M48 274H83 M417 274H452" /><circle cx="250" cy="275" r="194" strokeDasharray="1 18" /></g>
        <ellipse cx="250" cy="275" rx="132" ry="205" fill={`url(#${id}core)`} />
        <g className="presence-body" fill="none" stroke={`url(#${id}line)`}>
          {Array.from({ length: 34 }, (_, i) => {
            const y = 128 + i * 9;
            const head = i < 17;
            const width = head ? 23 + Math.sin((i + 1) / 18 * Math.PI) * 58 : 46 + Math.sin((i - 17) / 17 * Math.PI / 2) * 96;
            return <ellipse key={i} cx={250 + Math.sin(i * .24) * 5} cy={y} rx={width} ry={9 + i * .12} strokeWidth={i % 5 === 0 ? 1.6 : .7} opacity={.4 + Math.sin(i * .2) * .25} />;
          })}
          <path className="presence-ribbon" d="M232 120 C370 135 162 218 295 258 S125 345 262 445" strokeWidth="2.4" />
          <path className="presence-ribbon ribbon-two" d="M270 118 C126 162 355 219 220 274 S367 370 240 449" strokeWidth="1.8" />
          <path d="M232 120 C370 135 162 218 295 258 S125 345 262 445" strokeWidth="7" filter={`url(#${id}glow)`} opacity=".6" />
        </g>
        <g className="presence-orbit" fill="none" stroke={`url(#${id}line)`} opacity=".55"><ellipse cx="250" cy="275" rx="154" ry="52" transform="rotate(-32 250 275)" /><circle cx="117" cy="342" r="3" fill="var(--presence-bright)" /></g>
        <circle cx="250" cy="210" r="4" fill="var(--presence-bright)" className="presence-signal" />
        <path d="M158 455H342" stroke="var(--presence-color)" opacity=".5" />
      </svg>}
    {media && (!source || failed) && <span className="presence-fallback" role="status">Media unavailable · procedural fallback</span>}
  </div>;
}
