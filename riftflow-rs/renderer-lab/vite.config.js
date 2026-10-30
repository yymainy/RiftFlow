import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// The lab page is built into the shell's existing `renderer-dist`, next to the
// shipped bundle: both stay loadable so any regression is one env var away from the
// reference implementation.
export default defineConfig({
  plugins: [vue()],
  base: '/',
  build: {
    outDir: '../renderer-dist',
    emptyOutDir: false,
    assetsDir: 'lab-assets',
    rollupOptions: { input: 'lab.html' },
  },
})
