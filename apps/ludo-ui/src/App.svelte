<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isDemo } from './lib/api';
  import { focusFirst, installNav } from './lib/nav';
  import { reducedMotion } from './lib/motion';
  import { router } from './lib/router.svelte';
  import { startAppUpdateChecks } from './lib/appUpdate.svelte';
  import StateNotice from './lib/components/StateNotice.svelte';
  import type { DeviceInfo, Profile } from './lib/types';
  import Accueil from './screens/Accueil.svelte';
  import Bibliotheque from './screens/Bibliotheque.svelte';
  import Conflit from './screens/Conflit.svelte';
  import FicheJeu from './screens/FicheJeu.svelte';
  import MisesAJour from './screens/MisesAJour.svelte';
  import Passage from './screens/Passage.svelte';
  import Profil from './screens/Profil.svelte';
  import Reglages from './screens/Reglages.svelte';
  import QuiJoue from './screens/QuiJoue.svelte';

  let device = $state<DeviceInfo | null>(null);

  // Transition d'écran : un seul mouvement, doux. L'ancien écran s'efface (160 ms, accélère en partant) ;
  // le nouveau arrive juste après en fondu, en montant de 8 px et en se posant (420 ms, décélération
  // exponentielle). Pas de glissement latéral ni de cascade en même temps : un seul geste à suivre.
  const easeOutExpo = (t: number) => (t === 1 ? 1 : 1 - Math.pow(2, -10 * t));
  function screenIn(_node: Element) {
    if (reducedMotion()) return { duration: 0 };
    if (router.enter === 'hero') {
      // L'écran suivant fait voler la tuile : il doit être visible presque tout de suite.
      router.enter = 'normal';
      return { duration: 160, css: (t: number) => `opacity: ${t};` };
    }
    return {
      delay: 110,
      duration: 420,
      easing: easeOutExpo,
      css: (t: number) => `opacity: ${t}; transform: translateY(${(1 - t) * 8}px) scale(${0.994 + 0.006 * t});`,
    };
  }
  function screenOut(_node: Element) {
    if (reducedMotion()) return { duration: 0 };
    return { duration: 160, easing: (t: number) => t * t, css: (t: number) => `opacity: ${t};` };
  }
  // Le fondu d'accent n'est activé qu'après le premier affichage (sinon l'accent part du bleu par défaut).
  let accentAnim = $state(false);

  // L'interface est dessinée pour 1280×800 (Steam Deck) et s'adapte à la fenêtre : canevas logique de la
  // taille de la fenêtre divisée par l'échelle, agrandi par `transform: scale` (pas `zoom` : WebKitGTK
  // applique déjà le zoom aux unités vh, la hauteur était corrigée deux fois et l'app ne remplissait plus
  // la fenêtre). Recalcul sur l'événement de redimensionnement.
  let scale = $state(1);
  let view = $state({ w: 1280, h: 800 });
  function rescale() {
    const w = window.innerWidth;
    const h = window.innerHeight;
    scale = Math.max(0.75, Math.min(w / 1280, h / 800));
    view = { w: w / scale, h: h / scale };
  }

  /** Démonstration seulement : `?ecran=bibliotheque|fiche|maj|passage|conflit` ouvre un écran directement. */
  async function openDemoScreen(profiles: Profile[]) {
    const ecran = new URLSearchParams(location.search).get('ecran');
    if (!ecran) return;
    router.profile ??= profiles[0];
    const games = await api.library(router.profile.id);
    const byId = (id: string) => games.find((g) => g.id === id) ?? games[0];
    if (ecran === 'bibliotheque') router.go({ name: 'bibliotheque' });
    else if (ecran === 'maj') router.go({ name: 'mises-a-jour' });
else if (ecran === 'profil') router.go({ name: 'profil' });
else if (ecran === 'reglages') router.go({ name: 'reglages' });
    else if (ecran === 'fiche') router.go({ name: 'fiche', game: byId('switch-lm3') });
    else if (ecran === 'passage' || ecran === 'conflit') {
      const r = await api.prepare(router.profile.id, ecran === 'conflit' ? 'switch-botw' : 'nds-diamant');
      if ('conflict' in r) router.go({ name: 'conflit', conflict: r.conflict });
      else router.go({ name: 'passage', preparation: r.preparation });
    }
  }

  onMount(() => {
    // Mise à jour de l'app : vérifiée au démarrage puis toutes les 30 min (sans bloquer ; échec silencieux ici).
    startAppUpdateChecks();
    installNav();
    rescale();
    window.addEventListener('resize', rescale);
    // B : un menu ouvert se referme d'abord ; sinon, écran précédent.
    const onBack = () => {
      if (document.querySelector('[data-modal]')) window.dispatchEvent(new CustomEvent('ludo:modal-back'));
      else router.back();
    };
    window.addEventListener('ludo:back', onBack);

    void boot();
    return () => window.removeEventListener('ludo:back', onBack);
  });

  /* Démarrage : appareil (configuration) puis profils (serveur). Échec = écran d'erreur, jamais de démo. */
  let bootError = $state<{ title: string; detail: string } | null>(null);
  async function boot() {
    bootError = null;
    let d: DeviceInfo;
    try {
      d = await api.device();
    } catch (e) {
      bootError = { title: 'Configuration introuvable', detail: e instanceof Error ? e.message : String(e) };
      return;
    }
    let profiles: Profile[];
    try {
      profiles = await api.profiles();
    } catch (e) {
      bootError = { title: 'Serveur injoignable', detail: e instanceof Error ? e.message : String(e) };
      return;
    }
    {
      device = d;
      const mine = profiles.find((p) => p.id === d.defaultProfile);
      if (mine) {
        router.profile = mine;
        router.start({ name: 'accueil' });
      } else {
        router.start({ name: 'qui-joue' });
      }
      if (isDemo) openDemoScreen(profiles);
      setTimeout(() => (accentAnim = true), 600);
    }
  }

  const p = $derived(router.profile);
</script>

<div
  class="app"
  class:accent-anim={accentAnim}
  style:width="{view.w}px"
  style:height="{view.h}px"
  style:transform="scale({scale})"
  style:--accent={p?.accent ?? '#4cc3ff'}
  style:--accent-soft={p?.accentSoft ?? '#1c3546'}
  style:--accent-ink={p?.accentInk ?? '#8ad8ff'}
  data-demo={isDemo ? '' : undefined}
>
  {#if isDemo}
    <p class="demo-hint">Démo · flèches pour se déplacer · Entrée = A · Échap = B · Q / E = L1 / R1 · Y pour les détails</p>
  {/if}
  {#if bootError}
    <div class="boot-error"><StateNotice title={bootError.title} detail={bootError.detail} action="Réessayer" onaction={() => void boot()} /></div>
  {:else if device}
    {#key router.screen}
      <div class="screen-layer" in:screenIn out:screenOut onintroend={focusFirst}>
      {#if router.screen.name === 'qui-joue'}
        <QuiJoue {device} />
      {:else if router.screen.name === 'accueil' && router.profile}
        <Accueil {device} />
      {:else if router.screen.name === 'bibliotheque'}
        <Bibliotheque {device} />
      {:else if router.screen.name === 'fiche'}
        <FicheJeu game={router.screen.game} />
      {:else if router.screen.name === 'mises-a-jour'}
        <MisesAJour />
      {:else if router.screen.name === 'passage'}
        <Passage preparation={router.screen.preparation} />
      {:else if router.screen.name === 'conflit'}
        <Conflit conflict={router.screen.conflict} />
      {:else if router.screen.name === 'profil' && router.profile}
        <Profil {device} />
      {:else if router.screen.name === 'reglages'}
        <Reglages {device} />
      {/if}
      </div>
    {/key}
  {/if}
</div>

<style>
  .app {
    transform-origin: 0 0;
    position: relative;
    isolation: isolate;
  }
  /* L'accent se fond vers la couleur du nouveau joueur (propriétés enregistrées dans app.css). */
  .app.accent-anim { transition: --accent var(--t-slow) var(--ease-calm), --accent-soft var(--t-slow) var(--ease-calm), --accent-ink var(--t-slow) var(--ease-calm); }
  /* Couche d'ambiance : une lueur très douce de la couleur du joueur, qui dérive lentement en haut à gauche. */
  .app::before {
    content: ''; position: absolute; inset: -20% 40% 40% -20%; z-index: -1; pointer-events: none;
    background: radial-gradient(closest-side, color-mix(in srgb, var(--accent) 9%, transparent), transparent);
    animation: drift 16s var(--ease-calm) infinite alternate;
  }
  .screen-layer { position: absolute; inset: 0; }
  .boot-error { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }
  .demo-hint {
    position: absolute; top: 6px; left: 50%; transform: translateX(-50%); z-index: 5; margin: 0;
    font-size: 11px; color: var(--ink-4); background: rgba(21, 23, 28, 0.85); padding: 4px 10px; border-radius: 999px; white-space: nowrap;
  }
  @keyframes drift { from { transform: translate(0, 0); } to { transform: translate(6%, 4%); } }
</style>
