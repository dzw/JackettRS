import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { viteSingleFile } from "vite-plugin-singlefile";

// Build output goes straight into the Rust server's wwwroot; the server
// compiles it into the binary via include_str! and also serves it directly.
export default defineConfig({
  plugins: [vue(), viteSingleFile()],
  build: {
    outDir: "../jackett-server/wwwroot",
    emptyOutDir: true,
    target: "es2020",
  },
  server: {
    port: 5173,
    proxy: {
      // "npm run dev" proxies API calls to a locally running jackett-server.
      // Override the target with JACKETT_API (e.g. a test instance on :9118).
      "/api": process.env.JACKETT_API || "http://127.0.0.1:9117",
      "/torznab": process.env.JACKETT_API || "http://127.0.0.1:9117",
      "/health": process.env.JACKETT_API || "http://127.0.0.1:9117",
    },
  },
});
