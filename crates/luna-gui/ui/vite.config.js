import { defineConfig } from "vite";

// Tauri expects a fixed port and the dist dir declared in tauri.conf.json.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "es2022",
    outDir: "dist",
  },
});
