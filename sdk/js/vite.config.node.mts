import { defineConfig } from "vite";
import { resolve } from "path";
import wasm from 'vite-plugin-wasm';

export default defineConfig({
  server: {
    port: 3000,
  },
  plugins: [wasm()],
  build: {
    outDir: "dist/node",
    lib: {
      entry: resolve(__dirname, "src/index.node.ts"),
      name: "SDK",
      fileName: "index",
      formats: ["es", "cjs"]
    },
    rollupOptions: {
      external: [
        // Keep Node.js modules external but don't exclude them
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
      output: [
        {
          format: "es",
          entryFileNames: "index.mjs"
        },
        {
          format: "cjs", 
          entryFileNames: "index.cjs"
        }
      ]
    }
  }
});

