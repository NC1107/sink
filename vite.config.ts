/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri expects a fixed dev server port (see src-tauri/tauri.conf.json devUrl)
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    // Pinned to IPv4. Vite's default binding answers only on [::1] here,
    // and the devUrl the webview loads is "localhost" - which resolves to
    // 127.0.0.1 for it. Same address on both sides, no dev-only surprises.
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
