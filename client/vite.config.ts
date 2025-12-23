import { defineConfig } from 'vite';
import { resolve } from 'path';

export default defineConfig({
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  server: {
    port: 5173,
    open: true,
    // Client serves over HTTP, WebTransport to API uses HTTPS
  },
  build: {
    target: 'esnext',
    outDir: 'dist',
  },
});
