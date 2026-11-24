import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and a predictable build output.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    // WebKit on macOS 11+ (Tauri uses WKWebView). Vite 8 transpiles with oxc.
    target: "safari15",
    sourcemap: false,
    chunkSizeWarningLimit: 900,
  },
});
