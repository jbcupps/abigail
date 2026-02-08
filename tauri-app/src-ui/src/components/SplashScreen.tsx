import { useRef, useCallback } from "react";

interface SplashScreenProps {
  onComplete: () => void;
}

export default function SplashScreen({ onComplete }: SplashScreenProps) {
  const videoRef = useRef<HTMLVideoElement>(null);

  const handleEnd = useCallback(() => {
    onComplete();
  }, [onComplete]);

  const handleClick = useCallback(() => {
    onComplete();
  }, [onComplete]);

  return (
    <div
      className="fixed inset-0 bg-black flex items-center justify-center cursor-pointer z-[9999]"
      onClick={handleClick}
    >
      <video
        ref={videoRef}
        src="/video/startup.mp4"
        autoPlay
        muted
        playsInline
        onEnded={handleEnd}
        onError={handleEnd}
        className="max-w-full max-h-full object-contain"
      />
      <button
        className="absolute bottom-6 right-6 text-gray-600 hover:text-gray-400 text-xs font-mono"
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
