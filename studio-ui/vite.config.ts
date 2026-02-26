import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  
  // Tauri expects a fixed port
  server: {
    port: 5173,
    strictPort: true,
  },
  
  build: {
    // Tauri supports es2021
    target: ['es2021', 'chrome100', 'safari14'],
    // Don't minify for debugging
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    // Produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  
  // Prevent vite from obscuring rust errors
  clearScreen: false,
});
