<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isAndroid, isDemo } from './lib/api';
  import { focusFirst, installNav } from './lib/nav';
  import { reducedMotion } from './lib/motion';
  import { router } from './lib/router.svelte';
  import { startAppUpdateChecks } from './lib/appUpdate.svelte';
  import StateNotice from './lib/components/StateNotice.svelte';
  import Repos from './lib/components/Repos.svelte';
  import Colonnes from './lib/components/ecrans/Colonnes.svelte';
  import { androidPageWidth, applyLayout, computeLayout, computeZoom, layout, screenHasSide, screenIsWide, type EcranInfo, type EcranMode } from './lib/layout';
  import type { DeviceInfo, Profile } from './lib/types';
  import Accueil from './screens/Accueil.svelte';
  import FermerJeu from './lib/components/FermerJeu.svelte';
  import { enCours } from './lib/enCours.svelte';
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

  // L'interface est dessinée pour 1280×800 (Steam Deck) et s'adapte à la fenêtre par le ZOOM NATIF de la
  // vue (comme Ctrl+ d'un navigateur) : le moteur redessine textes et images à la résolution réelle (net).
  // Ni `zoom` CSS (WebKitGTK l'applique aussi aux vh : hauteur corrigée deux fois) ni `transform: scale`
  // (image agrandie, floue). Le zoom est piloté par la hauteur et plafonné (lib/layout.ts) : la largeur en
  // plus d'un écran 21:9 ou 32:9 sert aux colonnes latérales et aux tuiles, au lieu de rester vide.
  // Recalcul sur l'événement `resize`, regroupé sur l'image suivante (une rafale de redimensionnement = un
  // seul calcul) ; dans un navigateur (démo), le zoom est celui du navigateur et seule la mise en page suit.
  /** Zoom réellement appliqué à la vue (la taille « 100 % » de la fenêtre = innerWidth × zoom appliqué). */
  let appliedZoom = 1;
  let resizeQueued = false;
  function onResize() {
    if (resizeQueued) return;
    resizeQueued = true;
    // Une seule image demandée par rafale d'événements `resize` : ce n'est pas une boucle (aucune relance).
    requestAnimationFrame(() => {
      resizeQueued = false;
      void rescale();
    });
  }
  function relayout() {
    applyLayout(computeLayout(window.innerWidth, window.innerHeight, document.documentElement.dataset.ecran as EcranMode));
  }
  // Un seul recalcul à la fois : `setZoom` est asynchrone (IPC) et déclenche lui-même un `resize` ; un calcul
  // lancé pendant l'attente lirait innerWidth au nouveau zoom × l'ancien. Les demandes arrivées pendant un
  // calcul sont regroupées en UN recalcul à la fin (pas de relance en boucle : seulement si une demande est venue).
  let rescaling: Promise<void> | null = null;
  let rescaleAgain = false;
  function rescale(): Promise<void> {
    if (rescaling) {
      rescaleAgain = true;
      return rescaling;
    }
    rescaling = doRescale().finally(() => {
      rescaling = null;
      if (rescaleAgain) {
        rescaleAgain = false;
        void rescale();
      }
    });
    return rescaling;
  }
  async function doRescale() {
    if (isDemo) return relayout();
    if (isAndroid) {
      // Android : la WebView n'a pas de zoom natif réglable ; la largeur logique de la page est imposée
      // par la balise viewport (vue large activée dans MainActivity) pour voir au moins 1280×800.
      // Le gabarit est recalculé au `resize` qui suit l'application de la balise.
      // Proportions de la VUE (fenêtre de l'app), pas de `screen` qui décrit l'écran 0 : sur l'écran du bas de la
      // Thor (1080×1240), screen.* donnait les proportions de l'écran du haut (constat et essai du lot thor2, 22/11).
      // Écran du bas en double écran (`data-ecran="bas"`, posé par DoubleEcran.kt) : échelle du Deck (lib/layout.ts).
      // Taille de la vue annoncée en dp par DoubleEcran.kt (lib/layout.ts `androidPageWidth`) ; outerWidth en repli.
      const info = (window as unknown as { __ludoEcran?: EcranInfo }).__ludoEcran;
      const width = (info?.vueLargeurDp ?? 0) > 0 || window.outerWidth > 0
        ? androidPageWidth(info ?? { mode: document.documentElement.dataset.ecran as EcranMode }, window.outerWidth, window.outerHeight)
        : Math.ceil(Math.max(1280, (800 * screen.width) / screen.height));
      document.querySelector('meta[name="viewport"]')?.setAttribute('content', `width=${width}, user-scalable=no`);
      return relayout();
    }
    const previous = appliedZoom;
    const next = computeZoom(window.innerWidth * previous, window.innerHeight * previous);
    try {
      if (Math.abs(next - previous) >= 0.005) {
        appliedZoom = next;
        const { getCurrentWebview } = await import('@tauri-apps/api/webview');
        await getCurrentWebview().setZoom(next);
        // Compensation optique des icônes : trait affiné quand l'interface grandit (1 à 1×, 0,76 à 1,74×).
        document.documentElement.style.setProperty('--icon-optique', String(Math.max(0.6, 1 / Math.sqrt(next))));
      }
    } catch {
      appliedZoom = previous; // zoom refusé : l'interface reste à l'échelle précédente
    } finally {
      relayout();
    }
  }

  /** Android : état des écrans relu auprès de la coque (commande `ecranEtat`), puis échelle recalculée. */
  async function syncEcran() {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const info = await invoke<EcranInfo>('ecranEtat');
      if (!info?.mode) return;
      (window as unknown as { __ludoEcran?: EcranInfo }).__ludoEcran = info;
      document.documentElement.dataset.ecran = info.mode;
      onResize();
    } catch {
      // Coque sans cette commande : l'annonce et le script de début de document suffisent.
    }
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
    // Mise à jour de l'app : vérifiée au démarrage puis au retour du focus sur la fenêtre (sans bloquer ; échec silencieux ici).
    startAppUpdateChecks();
    installNav();
    void rescale();
    window.addEventListener('resize', onResize);
    // Passage en double écran ou retour à un seul écran (DoubleEcran.kt) : l'échelle de la vue change.
    window.addEventListener('ludo-ecran', onResize);
    // L'annonce de DoubleEcran.kt a pu partir avant cette page (démarrage à froid) : état relu une fois.
    if (isAndroid) void syncEcran();
    // B : un menu ouvert se referme d'abord ; sinon, écran précédent ; à la racine sur Android, retour au
    // lanceur (Cocoon), l'app restant ouverte (lot menu-jeu2).
    const onBack = () => {
      if (document.querySelector('[data-modal]')) window.dispatchEvent(new CustomEvent('ludo:modal-back'));
      else if (!router.back() && isAndroid) void enCours.versLanceur();
    };
    window.addEventListener('ludo:back', onBack);

    void boot();
    return () => {
      window.removeEventListener('ludo:back', onBack);
      window.removeEventListener('resize', onResize);
      window.removeEventListener('ludo-ecran', onResize);
    };
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

  /** Lueur d'ambiance : l'accent à 9 % d'opacité (équivalent de color-mix(in srgb, accent 9%, transparent)). */
  function accentGlow(hex: string | undefined): string {
    const m = /^#?([0-9a-f]{6}|[0-9a-f]{3})$/i.exec(hex?.trim() ?? '');
    if (!m) return 'rgba(76, 195, 255, 0.09)';
    const h = m[1].length === 3 ? [...m[1]].map((x) => x + x).join('') : m[1];
    const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
    return `rgba(${r}, ${g}, ${b}, 0.09)`;
  }
</script>

<div
  class="app"
  class:accent-anim={accentAnim}
  style:--accent={p?.accent ?? '#4cc3ff'}
  style:--accent-soft={p?.accentSoft ?? '#1c3546'}
  style:--accent-ink={p?.accentInk ?? '#8ad8ff'}
  style:--accent-glow={accentGlow(p?.accent)}
  data-demo={isDemo ? '' : undefined}
>
  {#if isDemo}
    <p class="demo-hint">Démo · flèches pour se déplacer · Entrée = A · Échap = B · Q / E = L1 / R1 · Y pour les détails</p>
  {/if}
  {#if bootError}
    <div class="boot-error"><StateNotice title={bootError.title} detail={bootError.detail} action="Réessayer" onaction={() => void boot()} /></div>
  {:else if device}
    {#key router.screen}
      <div
        class="screen-layer"
        data-colonnes={screenHasSide(router.screen.name) && router.profile ? '' : undefined}
        data-centre={!screenHasSide(router.screen.name) && !screenIsWide(router.screen.name) ? '' : undefined}
        in:screenIn
        out:screenOut
        onintroend={focusFirst}
      >
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
    {#if $layout.side !== 'none' && router.profile && screenHasSide(router.screen.name)}
      <Colonnes {device} />
    {/if}
    <!-- Menu en jeu (lot menu-jeu) : confirmation « Fermer le jeu ? », depuis n'importe quel écran. -->
    <FermerJeu />
  {/if}
  <Repos />
</div>

<style>
  .app {
    height: 100vh;
    position: relative;
    isolation: isolate;
  }
  /* L'accent se fond vers la couleur du nouveau joueur (propriétés enregistrées dans app.css). */
  .app.accent-anim { transition: --accent var(--t-slow) var(--ease-calm), --accent-soft var(--t-slow) var(--ease-calm), --accent-ink var(--t-slow) var(--ease-calm), --accent-glow var(--t-slow) var(--ease-calm); }
  /* Couche d'ambiance : une lueur très douce et FIXE de la couleur du joueur, en haut à gauche. L'ancienne
     dérive (animation infinie de 16 s) était invisible sous le fond d'ambiance mais recalculée sans arrêt
     (mesure du lot animations : 99 % → 3,5 % de CPU sans elle) ; retirée le 22/11. */
  .app::before {
    content: ''; position: absolute; inset: -20% 40% 40% -20%; z-index: -1; pointer-events: none;
    /* Couleur précalculée (accentGlow) : color-mix() n'existe qu'à partir de Chromium 111, la WebView de la Thor est
       en 109 et ignorait toute la déclaration. */
    background: radial-gradient(closest-side, var(--accent-glow), transparent);
  }
  .screen-layer { position: absolute; inset: 0; }
  .boot-error { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }
  .demo-hint {
    position: absolute; top: 6px; left: 50%; transform: translateX(-50%); z-index: 5; margin: 0;
    font-size: 11px; color: var(--ink-4); background: rgba(21, 23, 28, 0.85); padding: 4px 10px; border-radius: 999px; white-space: nowrap;
  }
</style>
