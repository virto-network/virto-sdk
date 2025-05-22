import { defineConfig } from 'vite'
import wasm from 'vite-plugin-wasm'

export default defineConfig({
  plugins: [wasm()],
  server: {
    port: 5173,
    host: true
  },
  optimizeDeps: {
    exclude: ['@virtonetwork/libwallet']
  }
}) 