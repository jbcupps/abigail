interface ConfirmationModalProps {
  title: string;
  message: string;
  detail?: string;
  confirmLabel: string;
  variant: "danger" | "warning";
  onConfirm: () => void;
  onCancel: () => void;
  loading?: boolean;
}

export default function ConfirmationModal({
  title,
  message,
  detail,
  confirmLabel,
  variant,
  onConfirm,
  onCancel,
  loading,
}: ConfirmationModalProps) {
  const isDanger = variant === "danger";

  const borderColor = isDanger ? "border-red-700" : "border-amber-700";
  const confirmBg = isDanger
    ? "bg-red-800 hover:bg-red-700 text-red-100"
    : "bg-amber-800 hover:bg-amber-700 text-amber-100";
  const iconColor = isDanger ? "text-red-500" : "text-amber-500";

  return (
    <div
      className="fixed inset-0 bg-black/80 flex items-center justify-center z-50"
      role="dialog"
      aria-modal="true"
      onClick={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        className={`bg-theme-bg-elevated border ${borderColor} rounded-lg p-6 max-w-md w-full mx-4`}
      >
        <div className="flex items-start gap-3 mb-4">
          <span className={`text-xl ${iconColor}`}>
            {isDanger ? "\u26A0" : "\u26A0"}
          </span>
          <div>
            <h2 className="text-theme-text-bright text-lg font-bold">{title}</h2>
            <p className="text-theme-text-dim text-sm mt-1">{message}</p>
            {detail && (
              <p className="text-theme-text-dim text-xs mt-2 opacity-70">
                {detail}
              </p>
            )}
          </div>
        </div>

        <div className="flex justify-end gap-3 mt-6">
          <button
            className="px-4 py-2 text-sm border border-theme-border-dim rounded text-theme-text-dim hover:text-theme-text hover:border-theme-border"
            onClick={onCancel}
            disabled={loading}
          >
            Cancel
          </button>
          <button
            className={`px-4 py-2 text-sm rounded font-bold ${confirmBg} disabled:opacity-50`}
            onClick={onConfirm}
            disabled={loading}
          >
            {loading ? "..." : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
