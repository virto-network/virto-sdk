import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 3000,
  },
  build: {
    lib: {
      entry: 'src/auth.ts',
      name: 'Auth',
      fileName: (format) => `auth.${format}.js`,
      formats: ['es', 'umd'],
    },
  },
});
