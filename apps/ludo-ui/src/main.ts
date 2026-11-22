import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'
import EcranHaut from './EcranHaut.svelte'
import { startEcranHaut } from './lib/ecranHaut.svelte'

// Ayn Thor : la seconde WebView de l'écran du haut charge la même interface avec `?ecran=haut`
// (DoubleEcran.kt) ; elle n'affiche que l'ambiance du jeu choisi en bas.
const haut = new URLSearchParams(location.search).get('ecran') === 'haut'

const app = mount(haut ? EcranHaut : App, {
  target: document.getElementById('app')!,
})
if (!haut) startEcranHaut()

export default app
