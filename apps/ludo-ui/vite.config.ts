import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { viteSingleFile } from 'vite-plugin-singlefile'

// `vite build --mode demo` produit une démo en un seul fichier HTML (JS, CSS et polices intégrés),
// publiable telle quelle ; le build normal reste celui de l'app Tauri.
export default defineConfig(({ mode }) => ({
  base: './',
  plugins: mode === 'demo' ? [svelte(), viteSingleFile()] : [svelte()],
  build: mode === 'demo' ? { outDir: 'dist-demo', assetsInlineLimit: 100_000_000 } : {},
}))
