interface ThinkingIndicatorProps {
  status?: string | null;
  label?: string;
}

export default function ThinkingIndicator({ status, label }: ThinkingIndicatorProps) {
  return (
    <p className="text-theme-text-dim flex items-center gap-2">
      {label && <span>{label}</span>}
      {status ? (
        <span>{status}</span>
      ) : (
        <span className="thinking-dots">
          <span>.</span>
          <span>.</span>
          <span>.</span>
        </span>
      )}
    </p>
  );
}
