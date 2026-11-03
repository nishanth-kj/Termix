import { defineConfig } from "vite";

export default defineConfig({
  root: "ui",
  // Tauri serves the files itself, so paths stay relative.
  base: "./",
  envPrefix: "VITE_",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
  },
});
