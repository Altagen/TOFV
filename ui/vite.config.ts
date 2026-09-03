import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const root = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  base: "./",
  server: {
    port: 1420,
    strictPort: true,
    clearScreen: false,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(root, "index.html"),
        journal: resolve(root, "log.html"),
        otp: resolve(root, "otp.html"),
      },
    },
  },
});
