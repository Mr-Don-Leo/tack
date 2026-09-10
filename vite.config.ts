import { defineConfig } from "vite";
import { resolve } from "node:path";

// Tauri serves the frontend from a fixed port in dev and from `dist/` in release.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5273,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        quickadd: resolve(import.meta.dirname, "quickadd.html"),
      },
    },
  },
});
