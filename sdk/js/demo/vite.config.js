import wasm from "vite-plugin-wasm";
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [
    wasm()
  ],
  build: {
    target: "ES2022", // Supports native top-level await
    rollupOptions: {
      output: {
        format: 'es' // Use ES modules format
      }
    }
  },
  optimizeDeps: {
    esbuildOptions: {
      target: "esnext" // Configure esbuild to support top-level await
    }
  },
  server: {
    port: 3000,
    fs: {
      allow: ['../dist', '.']
    }
  }
});