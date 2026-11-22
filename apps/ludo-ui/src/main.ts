import { mount } from 'svelte'
import './app.css'

// Ayn Thor : la seconde WebView de l'écran du haut charge la même interface avec `?ecran=haut`
// (DoubleEcran.kt) ; elle n'affiche que l'ambiance du jeu choisi en bas.
const haut = new URLSearchParams(location.search).get('ecran') === 'haut'

// Import à la demande : la vue du haut n'évalue ni App ni ses écouteurs globaux (nav.ts, sons…).
// (Promesses plutôt qu'un await de premier niveau : cible de construction de Vite par défaut.)
const target = document.getElementById('app')!
if (haut) {
  void import('./EcranHaut.svelte').then(({ default: EcranHaut }) => mount(EcranHaut, { target }))
} else {
  void import('./App.svelte').then(({ default: App }) => {
    mount(App, { target })
    return import('./lib/ecranHaut.svelte').then(({ startEcranHaut }) => startEcranHaut())
  })
}
