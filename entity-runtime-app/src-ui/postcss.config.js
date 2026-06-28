import { fileURLToPath } from "url";
import { dirname, join } from "path";

// PostCSS finds this file relative to the Vite root, but Tailwind's own config
// search starts from process.cwd() (the repo root when the dev server is
// launched from there), so it would otherwise miss src-ui/tailwind.config.js
// and emit an empty stylesheet. Point it at the config explicitly.
const here = dirname(fileURLToPath(import.meta.url));

export default {
  plugins: {
    "@tailwindcss/postcss": { config: join(here, "tailwind.config.js") },
    autoprefixer: {},
  },
};
