import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath, URL } from 'node:url'

// Tauri expects a fixed port and a predictable dist directory.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      // The ported piui components import from `@shared/ipc`; keeping the alias
      // means their import lines stay untouched.
      '@shared': fileURLToPath(new URL('./src/shared', import.meta.url))
    }
  },
  clearScreen: false,
  server: {
    port: 5273,
    strictPort: true,
    watch: {
      // The Rust side is rebuilt by Tauri, not by Vite.
      ignored: ['**/src-tauri/**']
    }
  },
  build: {
    target: 'chrome110',
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
    minify: 'esbuild'
  }
})
