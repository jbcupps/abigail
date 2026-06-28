import { fileURLToPath } from "url";
import { dirname } from "path";

// Anchor content globs to this config's directory so Tailwind generates
// utilities regardless of the current working directory (dev server launched
// from the repo root, `npm run build` from src-ui, or `cargo tauri dev`).
// Use forward slashes — Tailwind's glob engine (fast-glob) treats backslashes
// as escapes, so Windows-style paths from path.join() match nothing.
const here = dirname(fileURLToPath(import.meta.url)).replace(/\\/g, "/");

/** @type {import('tailwindcss').Config} */
export default {
  content: [`${here}/index.html`, `${here}/src/**/*.{js,ts,jsx,tsx}`],
  theme: {
    extend: {
      colors: {
        theme: {
          primary: "var(--color-primary)",
          "primary-dim": "var(--color-primary-dim)",
          "primary-muted": "var(--color-primary-muted)",
          "primary-faint": "var(--color-primary-faint)",
          "primary-glow": "var(--color-primary-glow)",
          bg: "var(--color-bg)",
          "bg-elevated": "var(--color-bg-elevated)",
          "bg-inset": "var(--color-bg-inset)",
          surface: "var(--color-surface)",
          "surface-dim": "var(--color-surface-dim)",
          "surface-bright": "var(--color-surface-bright)",
          border: "var(--color-border)",
          "border-dim": "var(--color-border-dim)",
          text: "var(--color-text)",
          "text-bright": "var(--color-text-bright)",
          "text-dim": "var(--color-text-dim)",
          "input-bg": "var(--color-input-bg)",
          "bubble-user": "var(--color-bubble-user)",
          "bubble-assistant": "var(--color-bubble-assistant)",
          "focus-ring": "var(--color-focus-ring)",
          hover: "var(--color-hover)",
          success: "var(--color-success)",
          "success-dim": "var(--color-success-dim)",
          warning: "var(--color-warning)",
          "warning-dim": "var(--color-warning-dim)",
          danger: "var(--color-danger)",
          "danger-dim": "var(--color-danger-dim)",
          info: "var(--color-info)",
          "info-dim": "var(--color-info-dim)",
          overlay: "var(--color-overlay)",
        },
      },
      fontFamily: {
        primary: "var(--font-primary)",
        mono: "var(--font-mono)",
      },
      borderRadius: {
        "theme-sm": "var(--radius-sm)",
        "theme-md": "var(--radius-md)",
        "theme-lg": "var(--radius-lg)",
      },
      boxShadow: {
        "theme-elevated": "var(--shadow-elevated)",
        "theme-inset": "var(--shadow-inset)",
        "theme-dropdown": "var(--shadow-dropdown)",
      },
      transitionDuration: {
        "theme-fast": "var(--transition-fast)",
        "theme-normal": "var(--transition-normal)",
      },
      animation: {
        "fade-in-up": "fade-in-up 0.3s ease-out",
        "glow-pulse": "glow-pulse 2s ease-in-out infinite",
      },
    },
  },
  plugins: [],
};
