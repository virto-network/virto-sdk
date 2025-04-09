import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { defineConfig } from 'vite'; 

export default defineConfig({
  plugins: [
    wasm(),
    topLevelAwait()
  ],
  server: {
    port: 3017,
    fs: {
      allow: ['../dist', '.']
    }
  }
});