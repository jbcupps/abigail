/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Abigail Entity Runtime chat frontend. Dev server runs on 1422 to avoid
// colliding with the Hive app (1421) and the legacy monolith (1420).
export default defineConfig({
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1422,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: ["es2021", "chrome100"],
    outDir: "dist",
    minify: !process.env.TAURI_DEBUG,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },
});
