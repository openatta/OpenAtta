import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { resolve } from "path";

export default defineConfig({
  plugins: [vue()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        updater: resolve(__dirname, "updater.html"),
        installer: resolve(__dirname, "installer.html"),
      },
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
});
