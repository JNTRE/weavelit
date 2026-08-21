import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The production build emits fixed, unhashed names because the Web UI Client
// Module embeds an exact compile-time allowlist of generated assets.
export default defineConfig({
  root: ".",
  base: "/",
  publicDir: false,
  plugins: [react()],
  build: {
    outDir: "dist",
    assetsDir: "assets",
    emptyOutDir: true,
    sourcemap: false,
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    manifest: false,
    ssrManifest: false,
    modulePreload: { polyfill: false },
    rollupOptions: {
      input: "index.html",
      output: {
        entryFileNames: "assets/weavelit-application.js",
        chunkFileNames: "assets/weavelit-groups-workspace.js",
        assetFileNames: "assets/weavelit-application.[ext]",
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: false,
    css: false,
    restoreMocks: true,
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["./src/test-setup.ts"],
  },
});
