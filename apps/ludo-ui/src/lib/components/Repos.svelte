<script lang="ts">
  // Repos de l'interface sur un appareil à batterie (Thor, Deck, portable ; rien sur un PC fixe) :
  // après DELAI_REPOS sans aucune entrée, un voile noir assombrit l'écran (dalles AMOLED : le noir ne
  // consomme presque rien), les animations CSS sont suspendues (`html[data-repos]`) et, sur Android,
  // l'écran du haut baisse sa luminosité (EnergiePlugin.kt). Le premier appui ou toucher réveille
  // tout instantanément et n'agit pas (comme une console) : le geste entier qui réveille est avalé
  // (touche, clic du toucher, appui manette via `entreeAvalee` lu par nav.ts). La mise en veille de
  // l'écran reste celle du système.
  //
  // Minuterie : délai de garde UNIQUE calculé (exception admise par CLAUDE.md), jamais une scrutation.
  // Une entrée note seulement l'heure ; à l'échéance, s'il reste du temps depuis la dernière entrée,
  // le délai est réarmé une fois pour ce reste. Aucune minuterie quand la page est cachée ou sans focus
  // (jeu lancé, écran éteint) : elle est armée au retour.
  import { onMount, untrack } from 'svelte';
  import { isAndroid, isDemo } from '../api';
  import { batterie, suivreBatterie } from '../batterie.svelte';
  import { AVALE_MS, repos as etatRepos } from '../repos';

  /** Inactivité avant le repos (ms). `?repos=<ms>` en démonstration pour les captures. */
  const DELAI_REPOS = (isDemo && Number(new URLSearchParams(location.search).get('repos'))) || 60_000;
  const coqueBureau = !isDemo && !isAndroid;

  let repos = $state(false);
  let derniere = Date.now();
  let garde: ReturnType<typeof setTimeout> | null = null;
  /** Bureau : focus annoncé par la coque (`ludo://focus`), plus fiable que document.hasFocus() sous WebKitGTK (cf. nav.ts). */
  let focusNatif = true;

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
    etatRepos.actif = actif;
    document.documentElement.toggleAttribute('data-repos', actif);
    if (isAndroid) {
      void import('@tauri-apps/api/core').then(({ invoke }) => invoke('energie_repos', { actif })).catch(() => {});
    }
  }
  // Android : la WebView du bas (Presentation non focalisable) n'a pas forcément le focus DOM ; la sortie
  // de l'app (jeu, écran éteint) y passe par la visibilité (onStop).
  const actif = () =>
    batterie.connue && document.visibilityState === 'visible' && (isAndroid || (coqueBureau ? focusNatif : document.hasFocus()));

  /** Entrée quelconque : nouvelle heure de référence ; réveil si l'interface est au repos. */
  function entree(): boolean {
    derniere = Date.now();
    let reveil = false;
    if (repos) {
      poser(false);
      etatRepos.avaleJusqua = Date.now() + AVALE_MS;
      reveil = true;
    }
    if (!garde && actif()) armer(DELAI_REPOS);
    return reveil;
  }
  /** Toucher, clic, molette : celui qui réveille verra son `click` avalé (la cible sous le voile ne reçoit rien). */
  function geste(ev: Event) {
    if (entree()) etatRepos.avaleClic = ev.type === 'pointerdown' || ev.type === 'touchstart';
    else if (ev.type === 'pointerdown') etatRepos.avaleClic = false; // nouveau geste : plus rien à avaler
  }
  /** Touche qui réveille : avalée, ainsi que son relâchement (Entrée / Espace activent un bouton au keyup). */
  function touche(ev: KeyboardEvent) {
    if (ev.type === 'keyup') {
      if (ev.key === etatRepos.toucheReveil) {
        etatRepos.toucheReveil = '';
        ev.preventDefault();
        ev.stopImmediatePropagation();
      }
      return;
    }
    if (entree()) {
      etatRepos.toucheReveil = ev.key;
      ev.preventDefault();
      ev.stopImmediatePropagation();
    }
  }
  function clic(ev: MouseEvent) {
    if (etatRepos.actif || etatRepos.avaleClic) {
      etatRepos.avaleClic = false;
      ev.preventDefault();
      ev.stopImmediatePropagation();
    }
  }

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
    let mort = false;
    const passif = { capture: true, passive: true } as const;
    const actifOpts = { capture: true, passive: false } as const;
    const gestes = ['pointerdown', 'wheel', 'touchstart'] as const;
    for (const t of gestes) window.addEventListener(t, geste, passif);
    window.addEventListener('keydown', touche, actifOpts);
    window.addEventListener('keyup', touche, actifOpts);
    window.addEventListener('click', clic, actifOpts);
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
    window.addEventListener('pointermove', bouge, passif);
    document.addEventListener('visibilitychange', etat);
    window.addEventListener('focus', etat);
    window.addEventListener('blur', etat);
    // Bureau : manette native (pad.rs) et focus de la coque arrivent en événements Tauri.
    const stops: (() => void)[] = [];
    if (coqueBureau) {
      void import('@tauri-apps/api/event')
        .then(async ({ listen }) => {
          const a = await listen('ludo://manette', () => void entree());
          const b = await listen<boolean>('ludo://focus', ({ payload }) => {
            focusNatif = payload;
            etat();
          });
          if (mort) {
            a();
            b();
          } else stops.push(a, b);
        })
        .catch(() => {});
    }
    return () => {
      mort = true;
      for (const t of gestes) window.removeEventListener(t, geste, passif);
      window.removeEventListener('keydown', touche, actifOpts);
      window.removeEventListener('keyup', touche, actifOpts);
      window.removeEventListener('click', clic, actifOpts);
      window.removeEventListener('pointermove', bouge, passif);
      document.removeEventListener('visibilitychange', etat);
      window.removeEventListener('focus', etat);
      window.removeEventListener('blur', etat);
      for (const s of stops) s();
      desarmer();
      poser(false);
    };
  });

  // Batterie apparue ou disparue (lecture asynchrone, UPower) : (dés)armement. Seul `batterie.connue`
  // est suivi : `etat()` lit aussi `repos`, qui relancerait l'effet (et réveillerait) à chaque repos.
  $effect(() => {
    void batterie.connue;
    untrack(etat);
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
