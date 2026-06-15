interface LoaderScreenProps {
  message?: string;
  error?: boolean;
  onRetry?: () => void;
}

// Calm post-splash screen shown while local services finish warming up, or a
// gentle retry prompt if they don't come up within the readiness window.
export default function LoaderScreen({ message, error, onRetry }: LoaderScreenProps) {
  return (
    <div className="fixed inset-0 z-[9998] flex flex-col items-center justify-center gap-5 bg-theme-bg">
      <h1 className="text-3xl font-semibold tracking-wide text-theme-text-bright">Abigail</h1>
      {!error ? (
        <>
          <div className="flex gap-2" aria-hidden>
            <span className="h-2.5 w-2.5 animate-pulse rounded-full bg-theme-primary [animation-delay:0ms]" />
            <span className="h-2.5 w-2.5 animate-pulse rounded-full bg-theme-primary [animation-delay:150ms]" />
            <span className="h-2.5 w-2.5 animate-pulse rounded-full bg-theme-primary [animation-delay:300ms]" />
          </div>
          <p className="text-sm text-theme-text-dim">{message ?? "Getting things ready…"}</p>
        </>
      ) : (
        <>
          <p className="max-w-sm text-center text-sm text-theme-text-dim">
            {message ?? "Still trying to connect…"}
          </p>
          {onRetry && (
            <button
              type="button"
              onClick={onRetry}
              className="text-sm text-theme-primary hover:underline"
            >
              Try again
            </button>
          )}
        </>
      )}
    </div>
  );
}
