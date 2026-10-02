// Tailwind 4 loads the app's legacy theme through @config in src/index.css.
// Source paths are anchored there so builds also work from the repository root.
export default {
  plugins: {
    "@tailwindcss/postcss": {},
    autoprefixer: {},
  },
};
