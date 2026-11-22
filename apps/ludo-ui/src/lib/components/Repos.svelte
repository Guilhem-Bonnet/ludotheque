<script lang="ts">
  // Repos de l'interface sur un appareil à batterie (Thor, Deck, portable ; rien sur un PC fixe) :
  // après DELAI_REPOS sans aucune entrée, un voile noir assombrit l'écran (dalles AMOLED : le noir ne
  // consomme presque rien), les animations CSS sont suspendues (`html[data-repos]`) et, sur Android,
  // l'écran du haut baisse sa luminosité (EnergiePlugin.kt). Le premier appui ou toucher réveille
  // tout instantanément et n'agit pas (comme une console) ; la mise en veille de l'écran reste celle
  // du système.
  //
  // Minuterie : délai de garde UNIQUE calculé (exception admise par CLAUDE.md), jamais une scrutation.
  // Une entrée note seulement l'heure ; à l'échéance, s'il reste du temps depuis la dernière entrée,
  // le délai est réarmé une fois pour ce reste. Aucune minuterie quand la page est cachée ou sans focus
  // (jeu lancé, écran éteint) : elle est armée au retour.
  import { onMount, untrack } from 'svelte';
  import { isAndroid, isDemo } from '../api';
  import { batterie, suivreBatterie } from '../batterie.svelte';

  /** Inactivité avant le repos (ms). `?repos=<ms>` en démonstration pour les captures. */
  const DELAI_REPOS = (isDemo && Number(new URLSearchParams(location.search).get('repos'))) || 60_000;

  let repos = $state(false);
  let derniere = Date.now();
  let garde: ReturnType<typeof setTimeout> | null = null;

  function armer(delai: number) {
    if (garde) clearTimeout(garde);
    garde = setTimeout(echeance, delai);
  }
  function desarmer() {
    if (garde) clearTimeout(garde);
    garde = null;
  }
  function echeance() {
    garde = null;
    const reste = derniere + DELAI_REPOS - Date.now();
    if (reste > 50) return armer(reste);
    poser(true);
  }
  function poser(actif: boolean) {
    if (repos === actif) return;
    repos = actif;
    document.documentElement.toggleAttribute('data-repos', actif);
    if (isAndroid) {
      void import('@tauri-apps/api/core').then(({ invoke }) => invoke('energie_repos', { actif })).catch(() => {});
    }
  }
  /** Entrée quelconque : réveil (l'entrée est avalée si elle réveille) et nouvelle heure de référence. */
  function entree(ev?: Event) {
    derniere = Date.now();
    if (repos) {
      poser(false);
      if (ev && ev.type === 'keydown') {
        ev.preventDefault();
        ev.stopImmediatePropagation();
      }
    }
    if (!garde && actif()) armer(DELAI_REPOS);
  }
  // Android : la WebView du bas (Presentation non focalisable) n'a pas forcément le focus DOM ; la sortie
  // de l'app (jeu, écran éteint) y passe par la visibilité (onStop).
  const actif = () => batterie.connue && document.visibilityState === 'visible' && (isAndroid || document.hasFocus());

  function etat() {
    if (actif()) {
      entree();
    } else {
      desarmer();
      poser(false);
    }
  }

  onMount(() => {
    suivreBatterie();
    const opts = { capture: true, passive: false } as const;
    const evts = ['keydown', 'pointerdown', 'wheel', 'touchstart'] as const;
    for (const t of evts) window.addEventListener(t, entree, opts);
    // Souris : un VRAI déplacement (coordonnées changées) suffit à rester éveillé. Les moteurs envoient
    // aussi des mouvements simulés quand le contenu bouge sous un pointeur immobile (cf. nav.ts) : ignorés.
    let px = NaN;
    let py = NaN;
    const bouge = (e: PointerEvent) => {
      if (e.clientX === px && e.clientY === py) return;
      px = e.clientX;
      py = e.clientY;
      entree();
    };
    window.addEventListener('pointermove', bouge, { passive: true, capture: true });
    document.addEventListener('visibilitychange', etat);
    window.addEventListener('focus', etat);
    window.addEventListener('blur', etat);
    // Manette native du bureau (pad.rs) : ses appuis arrivent en événement Tauri, pas en touches.
    let stop: (() => void) | undefined;
    if (!isDemo && !isAndroid) {
      void import('@tauri-apps/api/event')
        .then(({ listen }) => listen('ludo://manette', () => entree()))
        .then((u) => (stop = u))
        .catch(() => {});
    }
    return () => {
      for (const t of evts) window.removeEventListener(t, entree, opts);
      window.removeEventListener('pointermove', bouge, { capture: true });
      document.removeEventListener('visibilitychange', etat);
      window.removeEventListener('focus', etat);
      window.removeEventListener('blur', etat);
      stop?.();
      desarmer();
      poser(false);
    };
  });

  // Batterie apparue après le montage (lecture asynchrone) : premier armement. Seul `batterie.connue`
  // est suivi : `etat()` lit aussi `repos`, qui relancerait l'effet (et réveillerait) à chaque repos.
  $effect(() => {
    if (batterie.connue) untrack(etat);
  });
</script>

<!-- Voile : capte le toucher qui réveille (la cible dessous ne reçoit rien), puis s'efface. -->
<div class="voile" class:repos aria-hidden="true" onpointerdown={(e) => { e.preventDefault(); e.stopPropagation(); }}></div>

<style>
  .voile {
    position: fixed;
    inset: 0;
    z-index: 9000;
    background: #000;
    opacity: 0;
    pointer-events: none;
    transition: opacity 260ms var(--ease-exit, ease-in);
  }
  .voile.repos {
    opacity: 0.82;
    pointer-events: auto;
    transition: opacity 1400ms var(--ease-calm, ease);
  }
  :global(html[data-repos] *),
  :global(html[data-repos] *::before),
  :global(html[data-repos] *::after) {
    animation-play-state: paused !important;
  }
  @media (prefers-reduced-motion: reduce) {
    .voile,
    .voile.repos {
      transition: none;
    }
  }
</style>
