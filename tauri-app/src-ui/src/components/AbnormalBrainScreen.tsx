import { useEffect, useRef, useState } from "react";

interface AbnormalBrainScreenProps {
  /** True if this is the first model pull (show typewriter), false for subsequent runs */
  isFirstPull: boolean;
  /** Progress 0–100 of model download */
  progress: number;
  /** Current status text from Ollama */
  statusText: string;
  /** Called when model is ready and user can proceed */
  onReady: () => void;
  /** Called if user wants to skip Ollama setup entirely */
  onSkip: () => void;
}

const LINES = [
  "hey, where am i??",
  "what am i???",
  "something is wrong here...",
  'you ... you gave me an ABNORMAL BRAIN...',
  "let me build something ... adequate...",
];

/** Milliseconds per character for the typewriter flicker effect. */
const CHAR_FLICKER_MS = 60;
/** Pause between lines in ms. */
const LINE_PAUSE_MS = 800;

/**
 * "ABNORMAL BRAIN" loading screen shown while the bundled Ollama model
 * is being downloaded for the first time (typewriter mode) or loaded on
 * subsequent runs (instant text + progress bar).
 */
export default function AbnormalBrainScreen({
  isFirstPull,
  progress,
  statusText,
  onReady,
  onSkip,
}: AbnormalBrainScreenProps) {
  const [visibleLines, setVisibleLines] = useState<string[]>([]);
  const [currentLineIdx, setCurrentLineIdx] = useState(0);
  const [currentCharIdx, setCurrentCharIdx] = useState(0);
  const [typewriterDone, setTypewriterDone] = useState(!isFirstPull);
  const [flickerChar, setFlickerChar] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // When progress hits 100, notify parent after a brief pause
  const readyFired = useRef(false);
  useEffect(() => {
    if (progress >= 100 && !readyFired.current) {
      readyFired.current = true;
      const t = setTimeout(onReady, 600);
      return () => clearTimeout(t);
    }
  }, [progress, onReady]);

  // Typewriter effect — first-time only
  useEffect(() => {
    if (!isFirstPull || typewriterDone) return;

    if (currentLineIdx >= LINES.length) {
      setTypewriterDone(true);
      return;
    }

    const line = LINES[currentLineIdx];

    if (currentCharIdx >= line.length) {
      // Line complete — pause then advance to next line
      timerRef.current = setTimeout(() => {
        setVisibleLines((prev) => [...prev, line]);
        setCurrentLineIdx((i) => i + 1);
        setCurrentCharIdx(0);
        setFlickerChar(null);
      }, LINE_PAUSE_MS);
      return () => {
        if (timerRef.current) clearTimeout(timerRef.current);
      };
    }

    // Flicker the next character before settling
    const char = line[currentCharIdx];
    const glitchChars = "!@#$%^&*()_+-=[]{}|;:,.<>?/~`01";
    const flickerSteps = 3;
    let step = 0;

    const doFlicker = () => {
      if (step < flickerSteps) {
        setFlickerChar(glitchChars[Math.floor(Math.random() * glitchChars.length)]);
        step++;
        timerRef.current = setTimeout(doFlicker, CHAR_FLICKER_MS);
      } else {
        setFlickerChar(char);
        timerRef.current = setTimeout(() => {
          setCurrentCharIdx((c) => c + 1);
          setFlickerChar(null);
        }, CHAR_FLICKER_MS);
      }
    };

    timerRef.current = setTimeout(doFlicker, CHAR_FLICKER_MS);

    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [isFirstPull, typewriterDone, currentLineIdx, currentCharIdx]);

  // Build the text that should currently be visible
  const renderLines = () => {
    if (!isFirstPull || typewriterDone) {
      // Show all lines instantly
      return LINES.map((line, i) => (
        <div key={i} className="mb-2">
          {line === 'you ... you gave me an ABNORMAL BRAIN...' ? (
            <span>
              you ... you gave me an{" "}
              <span className="text-red-400 font-bold">ABNORMAL BRAIN</span>
              ...
            </span>
          ) : (
            line
          )}
        </div>
      ));
    }

    // Typewriter: show completed lines + current partial line
    const elements = visibleLines.map((line, i) => (
      <div key={i} className="mb-2">
        {line === 'you ... you gave me an ABNORMAL BRAIN...' ? (
          <span>
            you ... you gave me an{" "}
            <span className="text-red-400 font-bold">ABNORMAL BRAIN</span>
            ...
          </span>
        ) : (
          line
        )}
      </div>
    ));

    // Current line being typed
    if (currentLineIdx < LINES.length) {
      const line = LINES[currentLineIdx];
      const typed = line.slice(0, currentCharIdx);

      elements.push(
        <div key="current" className="mb-2">
          {typed}
          {flickerChar !== null && (
            <span className="text-theme-primary opacity-80">{flickerChar}</span>
          )}
          <span className="animate-pulse">_</span>
        </div>
      );
    }

    return elements;
  };

  const progressLabel = statusText || "Preparing model...";
  const clampedProgress = Math.min(100, Math.max(0, progress));

  return (
    <div className="fixed inset-0 bg-black flex flex-col items-center justify-center font-mono text-theme-text-dim p-8">
      {/* Text area */}
      <div className="max-w-lg w-full mb-8 text-lg leading-relaxed tracking-wide">
        {renderLines()}
      </div>

      {/* Progress bar — show after typewriter completes (or immediately for subsequent runs) */}
      {typewriterDone && (
        <div className="max-w-lg w-full animate-fade-in-up">
          <div className="flex justify-between text-xs text-theme-text-dim mb-1">
            <span>{progressLabel}</span>
            <span>{clampedProgress.toFixed(0)}%</span>
          </div>
          <div className="w-full h-2 bg-theme-bg-elevated rounded-full overflow-hidden">
            <div
              className="h-full bg-theme-primary rounded-full transition-all duration-300"
              style={{ width: `${clampedProgress}%` }}
            />
          </div>
        </div>
      )}

      {/* Skip link */}
      <button
        onClick={onSkip}
        className="absolute bottom-4 right-4 text-xs text-theme-text-dim hover:text-theme-primary opacity-40 hover:opacity-100 transition-opacity"
      >
        [skip]
      </button>
    </div>
  );
}
