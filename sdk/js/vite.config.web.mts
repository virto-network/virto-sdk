import { defineConfig } from "vite";
import { resolve } from "path";
import wasm from 'vite-plugin-wasm';

export default defineConfig({
  server: {
    port: 3000,
  },
  plugins: [wasm()],
  build: {
    outDir: "dist/web",
    lib: {
      entry: resolve(__dirname, "src/index.web.ts"),
      name: "SDK",
      fileName: "index",
      formats: ["es"]
    },
    rollupOptions: {
      external: [
        // Exclude all Node.js specific modules
        'jsonwebtoken',
        'ws',
        'crypto',
        'stream',
        'util',
        'buffer',
        'fs',
        'path',
        'os'
      ],
      output: {
        entryFileNames: "index.js",
        format: "es"
      }
    }
  }
});

