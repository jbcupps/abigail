import {
  useEffect,
  useRef,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";

export function SetupButton({
  variant = "secondary",
  className = "",
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary";
}) {
  return (
    <button
      type={type}
      className={`inline-flex min-h-10 items-center justify-center rounded-theme-md px-4 py-2 text-sm font-medium transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-theme-primary disabled:cursor-not-allowed disabled:opacity-40 ${variant === "primary" ? "bg-theme-primary text-white hover:bg-theme-primary-muted" : "border border-theme-border text-theme-text hover:border-theme-primary"} ${className}`}
      {...props}
    />
  );
}

export function SetupField({
  label,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { label: string }) {
  return (
    <label className="block text-sm text-theme-text">
      {label}
      <input
        {...props}
        className="mt-1 min-h-10 w-full rounded-theme-md border border-theme-border bg-theme-input-bg px-3 py-2 text-theme-text focus:border-theme-primary focus:outline-none"
      />
    </label>
  );
}

// Keyboard focus stays within the secure form; dismissal returns it to its opener.
export function SetupDialog({
  children,
  onClose,
  busy,
}: {
  children: ReactNode;
  onClose: () => void;
  busy: boolean;
}) {
  const element = useRef<HTMLElement>(null);
  useEffect(() => {
    const prior = document.activeElement as HTMLElement | null;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    element.current
      ?.querySelector<HTMLElement>("button, input, select")
      ?.focus();
    return () => {
      document.body.style.overflow = overflow;
      prior?.focus();
    };
  }, []);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-theme-overlay p-4">
      <section
        ref={element}
        role="dialog"
        aria-modal="true"
        aria-labelledby="connect-title"
        className="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-theme-lg border border-theme-border bg-theme-bg-elevated p-6 shadow-theme-dropdown"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !busy) {
            event.preventDefault();
            onClose();
          }
          if (event.key !== "Tab") return;
          const controls = Array.from(
            element.current?.querySelectorAll<HTMLElement>(
              "button:not(:disabled), input:not(:disabled), select:not(:disabled)",
            ) ?? [],
          );
          const first = controls[0],
            last = controls[controls.length - 1];
          if (!first) {
            event.preventDefault();
            return;
          }
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }}
      >
        {children}
      </section>
    </div>
  );
}
