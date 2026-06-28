import { useCallback, useEffect, useRef, useState } from "react";

interface SplashScreenProps {
  onComplete: () => void;
}

// Cinematic intro. Plays the startup video; a click, the video ending, or the
// Skip button advances. Falls back to a calm branded card if the video can't
// play, auto-advancing shortly after.
export default function SplashScreen({ onComplete }: SplashScreenProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [videoFailed, setVideoFailed] = useState(false);

  const handleError = useCallback(() => {
    setVideoFailed(true);
  }, []);

  useEffect(() => {
    if (!videoFailed) return;
    const timer = setTimeout(onComplete, 2000);
    return () => clearTimeout(timer);
  }, [videoFailed, onComplete]);

  return (
    <div
      className="fixed inset-0 z-[9999] flex cursor-pointer items-center justify-center bg-theme-bg"
      onClick={onComplete}
    >
      {!videoFailed ? (
        <video
          ref={videoRef}
          src="/video/startup.mp4"
          autoPlay
          muted
          playsInline
          onEnded={onComplete}
          onError={handleError}
          className="max-h-full max-w-full object-contain"
        />
      ) : (
        <div className="animate-pulse text-center">
          <h1 className="text-4xl font-semibold tracking-widest text-theme-text-bright">
            ABIGAIL
          </h1>
          <p className="mt-2 text-sm text-theme-text-dim">Starting up…</p>
        </div>
      )}
      <button
        type="button"
        className="absolute bottom-6 right-6 text-xs text-theme-text-dim hover:text-theme-text"
        onClick={(e) => {
          e.stopPropagation();
          onComplete();
        }}
      >
        Skip
      </button>
    </div>
  );
}
