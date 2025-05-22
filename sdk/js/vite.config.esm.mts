import { defineConfig } from "vite";
import { resolve } from "path";
import wasm from 'vite-plugin-wasm';

export default defineConfig({
  server: {
    port: 3000,
  },
  plugins: [wasm()],
  ssr: {
    external: ['jsonwebtoken']
  },
  build: {
    outDir: "dist/esm",
    lib: {
      entry: resolve(__dirname, "src/sdk.ts"),
      name: "SDK",
      fileName: "sdk",
      formats: ["es"]
    },
    rollupOptions: {
      output: {
        entryFileNames: "sdk.js",
        format: "es"
      }
    }
  }
});