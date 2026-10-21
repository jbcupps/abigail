import { type ReactNode, useId, useState } from "react";

export interface TooltipLink {
  href: string;
  label: string;
}

interface HelpTooltipProps {
  label: string;
  title?: string;
  description: ReactNode;
  links?: TooltipLink[];
  side?: "top" | "bottom";
  align?: "start" | "end";
  testId?: string;
}

export default function HelpTooltip({
  label,
  title,
  description,
  links = [],
  side = "bottom",
  align = "start",
  testId,
}: HelpTooltipProps) {
  const [open, setOpen] = useState(false);
  const tooltipId = useId();

  const positionClass = side === "top" ? "bottom-full mb-2" : "top-full mt-2";
  const alignClass = align === "end" ? "right-0" : "left-0";

  return (
    <span
      className="relative inline-flex"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={(event) => {
        const nextTarget = event.relatedTarget as Node | null;
        if (!nextTarget || !event.currentTarget.contains(nextTarget)) {
          setOpen(false);
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          setOpen(false);
        }
      }}
      data-testid={testId}
    >
      <button
        type="button"
        className="inline-flex h-4 w-4 items-center justify-center rounded-full border border-theme-border-dim bg-theme-bg-elevated text-[10px] font-mono font-bold text-theme-text-dim transition-colors hover:border-theme-primary hover:text-theme-primary focus-visible:border-theme-primary focus-visible:text-theme-primary"
        aria-label={label}
        aria-describedby={open ? tooltipId : undefined}
      >
        i
      </button>
      {open && (
        <span
          id={tooltipId}
          role="tooltip"
          className={`absolute ${positionClass} ${alignClass} z-[70] w-72 max-w-[min(20rem,calc(100vw-2rem))] rounded-md border border-theme-border bg-theme-bg-elevated p-3 text-left shadow-[var(--shadow-dropdown)]`}
        >
          {title && (
            <span className="mb-1 block text-[11px] font-bold uppercase tracking-widest text-theme-text-bright">
              {title}
            </span>
          )}
          <span className="block text-xs leading-5 text-theme-text-dim">
            {description}
          </span>
          {links.length > 0 && (
            <span className="mt-3 block border-t border-theme-border-dim pt-2">
              <span className="mb-1 block text-[10px] uppercase tracking-widest text-theme-text-dim">
                Links
              </span>
              <span className="flex flex-col gap-1">
                {links.map((link) => (
                  <a
                    key={link.href}
                    href={link.href}
                    target="_blank"
                    rel="noreferrer"
                    className="text-xs text-theme-primary-dim underline decoration-theme-border hover:text-theme-primary"
                  >
                    {link.label}
                  </a>
                ))}
              </span>
            </span>
          )}
        </span>
      )}
    </span>
  );
}
