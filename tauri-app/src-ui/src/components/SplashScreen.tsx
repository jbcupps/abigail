import { useRef, useCallback, useState, useEffect } from "react";

interface SplashScreenProps {
  onComplete: () => void;
}

export default function SplashScreen({ onComplete }: SplashScreenProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [videoFailed, setVideoFailed] = useState(false);

  const handleEnd = useCallback(() => {
    onComplete();
  }, [onComplete]);

  const handleClick = useCallback(() => {
    onComplete();
  }, [onComplete]);

  const handleError = useCallback(() => {
    console.warn("[SplashScreen] Video failed to load, showing fallback splash");
    setVideoFailed(true);
  }, []);

  // Fallback: auto-advance after 2.5s if video can't play
  useEffect(() => {
    if (videoFailed) {
      const timer = setTimeout(onComplete, 2500);
      return () => clearTimeout(timer);
    }
  }, [videoFailed, onComplete]);

  return (
    <div
      className="fixed inset-0 bg-black flex items-center justify-center cursor-pointer z-[9999]"
      onClick={handleClick}
    >
      {!videoFailed ? (
        <video
          ref={videoRef}
          src="/video/startup.mp4"
          autoPlay
          muted
          playsInline
          onEnded={handleEnd}
          onError={handleError}
          className="max-w-full max-h-full object-contain"
        />
      ) : (
        <div className="text-center animate-pulse">
          <h1 className="text-theme-primary text-4xl font-mono font-bold tracking-widest">
            ABIGAIL
          </h1>
          <p className="text-theme-text-dim text-sm mt-2 font-mono">
            initializing...
          </p>
        </div>
      )}
      <button
        className="absolute bottom-6 right-6 text-theme-text-dim hover:text-theme-text text-xs font-mono"
        onClick={(e) => {
          e.stopPropagation();
          onComplete();
        }}
      >
        [skip]
      </button>
    </div>
  );
}
