/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
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
          surface: "var(--color-surface)",
          border: "var(--color-border)",
          "border-dim": "var(--color-border-dim)",
          text: "var(--color-text)",
          "text-bright": "var(--color-text-bright)",
          "text-dim": "var(--color-text-dim)",
        },
      },
    },
  },
  plugins: [],
};
