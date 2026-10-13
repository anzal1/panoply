import { defineConfig } from 'vite';

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1', fs: { allow: ['..'] } },
  build: {
    target: 'safari16',
    sourcemap: false,
    assetsInlineLimit: 0,
    modulePreload: false,
    reportCompressedSize: false,
  },
});
