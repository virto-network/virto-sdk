import { defineConfig } from "vite";
import { resolve } from "path";

export default defineConfig({
  server: {
    port: 3000,
  },
  build: {
    outDir: "dist/umd",
    lib: {
      entry: resolve(__dirname, "src/auth.ts"),
      name: "Auth",
      fileName: "auth",
      formats: ["umd"]
    },
  }
});
