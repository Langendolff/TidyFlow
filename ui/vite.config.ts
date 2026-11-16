/// <reference types="vitest/config" />
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // O Tauri abre http://localhost:5173 em desenvolvimento (tauri.conf.json → devUrl).
  server: { port: 5173, strictPort: true },
  clearScreen: false,
  test: {
    environment: 'jsdom',
    setupFiles: 'src/test-setup.ts',
  },
})
