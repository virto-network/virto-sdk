import wasm from "vite-plugin-wasm";
import { defineConfig } from 'vite'; 

export default defineConfig({
  plugins: [
    wasm()
  ],
  build: {
    target: "ES2022", // Soporta top-level await nativamente
    rollupOptions: {
      output: {
        format: 'es' // Usar formato ES modules
      }
    }
  },
  optimizeDeps: {
    esbuildOptions: {
      target: "esnext" // Configurar esbuild para soportar top-level await
    }
  },
  server: {
    port: 3017,
    fs: {
      allow: ['../dist', '.']
    }
  }
});