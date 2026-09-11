import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify("0.2.10"),
    __BUILD_DATE__: JSON.stringify(new Date().toISOString().slice(0, 10)),
  },
  build: {
    outDir: "web-dist",
    emptyOutDir: true,
    sourcemap: false,
    minify: "esbuild",
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
