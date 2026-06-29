/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Abigail Hive control-plane frontend. Dev server runs on 1421 to avoid
// colliding with the Entity Runtime app (1422) and the legacy monolith (1420).
export default defineConfig({
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1421,
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
