<script lang="ts">
  import CoverArt from './CoverArt.svelte';
  import SyncBadge, { tileState } from './SyncBadge.svelte';
  import { cartShape, GRIP_RIDGES } from './cartouche';
  import type { Game } from '../types';

  // Tuile d'un jeu = face d'une cartouche de SA console (contours dans cartouche.ts) : boîtier aux
  // couleurs de la console, bandeau en haut (nom de la console dans une police libre propre à chaque
  // console, état de synchro imprimé à l'encre du bandeau), fenêtre d'étiquette dessous avec l'image du
  // jeu, ou, à défaut, la jaquette illustrée et son titre. Tout est vectoriel (SVG + CSS) : net au zoom
  // natif de la WebView comme sur la densité 369 de la Thor, sans filtre par tuile.
  // Au focus, les deux plans de la jaquette illustrée glissent à des vitesses différentes (parallaxe).
  //
  // Mouvement au focus (couches primaire / secondaire / ambiance) :
  // - la tuile grandit (rebond léger) et se soulève de 4 px ;
  // - son ombre descend et s'adoucit (contre-mouvement), l'anneau d'accent se pose (95 % → 100 %) ;
  // - le titre suit 50 ms plus tard, le bandeau 70 ms plus tard (follow-through) ;
  // - un halo de la couleur du joueur respire deux fois puis se pose (ambiance, sans boucle infinie).
  // États de mise en scène : `dimmed` (s'estompe derrière la tuile choisie), `lifting` (se soulève
  // avant de partir), `shine` (un reflet de lumière la traverse, au lancement).
  let {
    game,
    size = 232,
    big = false,
    still = false,
    hero = false,
    dimmed = false,
    lifting = false,
    shine = false,
    autofocus = false,
    words,
    scale = 1,
    sound,
    onfocus,
    onclick,
  }: {
    game: Game;
    size?: number;
    big?: boolean;
    still?: boolean;
    hero?: boolean;
    dimmed?: boolean;
    lifting?: boolean;
    shine?: boolean;
    autofocus?: boolean;
    /** Mot d'état dans le bandeau (« Nouvelle sauvegarde »…) ; par défaut selon la taille (≥ 250 px). */
    words?: boolean;
    /** Agrandissement appliqué par le parent (transformation) : l'anneau de focus le compense pour garder 4 px. */
    scale?: number;
    sound?: string;
    onfocus?: () => void;
    onclick?: (el: HTMLElement) => void;
  } = $props();

  let el: HTMLButtonElement | undefined = $state();
  // Une image qui ne charge pas (serveur hors ligne, CSP) retombe sur la jaquette illustrée.
  let failed = $state('');
  const cover = $derived(game.cover && failed !== game.cover ? game.cover : undefined);
  // Couleurs du boîtier par console : teintes propres à l'app, inspirées des boîtes de chaque console ;
  // texte sombre sur les bandeaux clairs (3DS, DS). Chaque paire bandeau/encre tient au moins 4,5:1
  // (nom de console en 13 px) : Switch 2 et Wii U approfondis le 21/11 (3,2 et 3,5:1 avant), et
  // écartés l'un de l'autre (bleu franc / bleu canard) pour ne plus se confondre.
  const CASES: Record<string, { band: string; ink: string }> = {
    switch: { band: '#d8202c', ink: '#ffffff' },
    switch2: { band: '#1668b8', ink: '#ffffff' },
    '3ds': { band: '#e9ebee', ink: '#b3141f' },
    ds: { band: '#c3c7cc', ink: '#24272c' },
    wiiu: { band: '#0e7490', ink: '#ffffff' },
    gc: { band: '#5c4fa6', ink: '#ffffff' },
  };
  const CONSOLE_NAMES: Record<string, string> = { switch: 'SWITCH', switch2: 'SWITCH 2', '3ds': '3DS', ds: 'DS', wiiu: 'WII U', gc: 'GAMECUBE' };
  const box = $derived(CASES[game.console] ?? { band: '#3a3f4b', ink: '#ffffff' });
  const shape = $derived(cartShape(game.console, size));
  const win = $derived(shape.window);
  const ridges = $derived(GRIP_RIDGES[game.console] ?? 0);
  // Nom de la console : jamais sous 13 px (lisible sur le Deck, zoom 1) ; sous 130 px de tuile (menu
  // d'options), le nom est déjà écrit à côté de la tuile : le bandeau ne garde que l'état.
  const showName = $derived(size >= 130);
  const nameSize = $derived(Math.min(18, Math.max(13, Math.round(shape.band * 0.4))));
  // Le mot d'état s'ajoute au pictogramme quand la tuile est assez large (tuile en focus de l'accueil) ;
  // dans la grille, la ligne sous la tuile le dit déjà.
  // (Nommé `syncState` : une variable `state` masquait la rune `$state` pour svelte-check.)
  const syncState = $derived(tileState(game));
  const stateWords = $derived(words ?? size >= 250);
  // Titre de la jaquette illustrée : borné à 3 lignes dans la fenêtre ; un titre long (> 24 signes)
  // passe en plus petit avant d'être coupé (bug signalé par le lot bugs le 21/11, titre de 70 signes).
  const titleSize = $derived(Math.max(13, Math.round(size * (game.short.length > 24 ? 0.095 : game.short.length > 14 ? 0.112 : 0.13))));
  // Rayons du contour, repris par l'anneau de focus (+8 px), l'ombre et le halo.
  const rad = $derived(shape.radius);
  // Mot d'état : jamais sous 13 px (hauteur de marque × 0,56 ≥ 13).
  const markH = $derived(stateWords ? Math.max(24, Math.round(shape.band * 0.66)) : Math.round(shape.band * 0.72));
</script>

<button
  bind:this={el}
  class="tile"
  class:focusable={!still}
  class:still
  class:big
  class:dimmed
  class:lifting
  class:shine
  style:--ring-k={scale}
  style:--band={box.band}
  style:--band-ink={box.ink}
  style:--band-h="{shape.band}px"
  style:--r="{rad.tl}px {rad.tr}px {rad.br}px {rad.bl}px"
  style:--r-ring="{rad.tl + 8}px {rad.tr + 8}px {rad.br + 8}px {rad.bl + 8}px"
  tabindex={still ? -1 : undefined}
  style:width="{size}px"
  style:height="{size}px"
  style:view-transition-name={hero ? 'hero-tile' : undefined}
  data-autofocus={autofocus ? '' : undefined}
  data-sound={sound}
  data-hero={hero ? '' : undefined}
  data-game={game.id}
  data-console={game.console}
  aria-label={syncState ? `${game.title}, ${syncState.label}` : game.title}
  {onfocus}
  onclick={() => el && onclick?.(el)}
>
  <svg class="shell" viewBox="0 0 {size} {size}" aria-hidden="true">
    <path class="body" d={shape.outline}></path>
    <path class="edge" d="M{shape.edge[0]} 0.75H{shape.edge[1]}"></path>
    {#if shape.grooves}<path class="groove" d={shape.grooves}></path>{/if}
    {#if shape.disc}
      <circle class="disc" cx={shape.disc.cx} cy={shape.disc.cy} r={shape.disc.r}></circle>
      <circle class="hub" cx={shape.disc.cx} cy={shape.disc.cy} r={Math.max(1.5, shape.disc.r * 0.3)}></circle>
    {/if}
  </svg>
  <span
    class="photo"
    aria-hidden="true"
    style:top="{win.top}px"
    style:left="{win.left}px"
    style:right="{win.right}px"
    style:bottom="{win.bottom}px"
    style:border-radius="{win.rTop}px {win.rTop}px {win.rBottom}px {win.rBottom}px"
    style:background={cover || game.art ? undefined : game.tile}
  >
    {#if cover}
      <img src={cover} alt="" draggable="false" loading="lazy" onerror={() => (failed = cover)} />
    {:else}
      {#if game.art}<CoverArt art={game.art} />{/if}
      <!-- Vraie image : le nom est déjà dans l'image et sous la tuile, il ne la recouvre pas. -->
      <span class="title" style:font-size="{titleSize}px" style:padding="{Math.round(size * 0.07)}px" style:color={game.tileInk} class:shadowed={game.tileInk.toLowerCase() === '#ffffff'}><span class="t">{game.short}</span></span>
    {/if}
  </span>
  <span class="ring" aria-hidden="true"></span>
  <span class="gleam" aria-hidden="true"></span>
  <span class="top" style:padding-left="{shape.padLeft}px" style:padding-right="{shape.padRight}px">
    {#if showName}<span class="band-label" style:font-size="{nameSize}px">{CONSOLE_NAMES[game.console] ?? game.console}</span>{/if}
    {#if ridges && showName && !(stateWords && syncState)}
      <span class="ridges" aria-hidden="true">{#each { length: ridges } as _}<i></i>{/each}</span>
    {/if}
    <span class="fill"></span>
    <SyncBadge {game} size={markH} tone="band" labelled={stateWords} />
  </span>
</button>

<style>
  /* Polices des bandeaux : sous-ensembles OFL embarqués (src/assets/fonts/README.md), aucun appel réseau. */
  @font-face { font-family: 'Cart Switch'; src: url('../../assets/fonts/cart-switch.woff2') format('woff2'); font-weight: 800; font-display: block; }
  @font-face { font-family: 'Cart Switch2'; src: url('../../assets/fonts/cart-switch2.woff2') format('woff2'); font-weight: 700; font-display: block; }
  @font-face { font-family: 'Cart 3DS'; src: url('../../assets/fonts/cart-3ds.woff2') format('woff2'); font-weight: 600; font-display: block; }
  @font-face { font-family: 'Cart DS'; src: url('../../assets/fonts/cart-ds.woff2') format('woff2'); font-weight: 700; font-display: block; }
  @font-face { font-family: 'Cart GC'; src: url('../../assets/fonts/cart-gc.woff2') format('woff2'); font-weight: 800; font-display: block; }
  @font-face { font-family: 'Cart WiiU'; src: url('../../assets/fonts/cart-wiiu.woff2') format('woff2'); font-weight: 400; font-display: block; }

  .tile {
    all: unset; cursor: pointer; flex-shrink: 0; box-sizing: border-box;
    border-radius: var(--r, var(--radius-tile)); padding: 0;
    display: flex; flex-direction: column; justify-content: flex-start;
    position: relative; isolation: isolate; overflow: visible;
    box-shadow: 0 4px 10px rgba(0, 0, 0, 0.22);
    transition:
      width 280ms var(--ease-land),
      height 280ms var(--ease-land),
      box-shadow 320ms var(--ease-land),
      transform 280ms var(--ease-pop),
      opacity 220ms ease-out;
  }

  /* Boîtier : contour vectoriel propre à la console ; l'arête haute est éclairée d'un cheveu (plastique
     moulé), le bord cerné d'un trait sombre très fin (lisible sur un fond clair comme sombre). */
  .shell { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; z-index: -2; pointer-events: none; }
  .body { fill: var(--band); stroke: rgba(0, 0, 0, 0.22); stroke-width: 1; }
  .edge { stroke: rgba(255, 255, 255, 0.3); stroke-width: 1; fill: none; }
  .groove { stroke: var(--band-ink); stroke-opacity: 0.22; stroke-width: 1.25; fill: none; stroke-linecap: round; }
  .disc { fill: var(--band-ink); opacity: 0.92; }
  .hub { fill: var(--band); }

  /* Fenêtre d'étiquette : image ou jaquette, enchâssée (bord intérieur ombré d'un cheveu). */
  .photo { position: absolute; overflow: hidden; z-index: -1; background: rgba(0, 0, 0, 0.25); }
  .photo img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .photo::after { content: ''; position: absolute; inset: 0; border-radius: inherit; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.2); pointer-events: none; }

  .top { display: flex; align-items: center; gap: 8px; height: var(--band-h); box-sizing: border-box; flex-shrink: 0;
    transition: transform 300ms var(--ease-pop) 70ms; transform-origin: center; }
  .fill { flex: 1; min-width: 0; }
  /* Nom de la console : une police libre par console (esprit, pas copie), posée sur la ligne optique du bandeau. */
  .band-label { font-weight: 700; letter-spacing: 0.06em; color: var(--band-ink); line-height: 1.1; padding-top: 1px; white-space: nowrap; font-synthesis: none; flex-shrink: 0; }
  /* Place trop juste : c'est le mot d'état qui cède (points de suspension), jamais le nom de la console. */
  .top :global(.mark) { flex-shrink: 1; min-width: 0; }
  /* « Non installé » : pictogramme moins effacé que par défaut (0,72 tombait sous 3:1 sur les bandeaux sombres). */
  .top :global(.mark.quiet) { opacity: 0.9; }
  .tile[data-console='switch'] .band-label { font-family: 'Cart Switch', var(--font); font-weight: 800; letter-spacing: 0.05em; }
  .tile[data-console='switch2'] .band-label { font-family: 'Cart Switch2', var(--font); font-weight: 700; letter-spacing: 0.08em; }
  .tile[data-console='3ds'] .band-label { font-family: 'Cart 3DS', var(--font); font-weight: 600; letter-spacing: 0.04em; }
  .tile[data-console='ds'] .band-label { font-family: 'Cart DS', var(--font); font-weight: 700; letter-spacing: 0.1em; }
  .tile[data-console='gc'] .band-label { font-family: 'Cart GC', var(--font); font-weight: 800; letter-spacing: 0.03em; padding-top: 3px; }
  .tile[data-console='wiiu'] .band-label { font-family: 'Cart WiiU', var(--font); font-weight: 400; letter-spacing: 0.07em; }
  /* Stries de préhension (DS, 3DS), gravées dans le plastique, à l'encre du bandeau. */
  .ridges { display: inline-flex; gap: 3px; height: 46%; align-items: stretch; flex-shrink: 0; margin-left: 4px; }
  .ridges i { width: 2px; border-radius: 1px; background: var(--band-ink); opacity: 0.42; }

  /* Focus : se soulève, l'ombre descend et s'adoucit (contre-mouvement). */
  .tile:focus { transform: translateY(-4px); box-shadow: 0 22px 44px rgba(0, 0, 0, 0.5), 0 6px 12px rgba(0, 0, 0, 0.25); }

  /* Anneau d'accent : se pose en grandissant de 95 % à 100 %. */
  .ring {
    position: absolute; inset: calc(-8px / var(--ring-k, 1)); border-radius: var(--r-ring, calc(var(--radius-tile) + 8px)); pointer-events: none;
    /* Anneau d'accent doublé d'un filet clair intérieur : il reste visible même quand l'accent du profil
       a la teinte du boîtier (bleu sur Switch 2, par exemple) ou dans un flux vidéo compressé. */
    box-shadow: inset 0 0 0 calc(4px / var(--ring-k, 1)) var(--accent), inset 0 0 0 calc(6px / var(--ring-k, 1)) rgba(255, 255, 255, 0.85); opacity: 0; transform: scale(0.95);
    transition: opacity 150ms ease-out, transform 220ms var(--ease-pop);
  }
  .tile:focus .ring { opacity: 1; transform: scale(1); }

  /* Halo qui respire deux fois puis se pose (ambiance) : pas de boucle infinie, qui coûtait une image
     par trame tant que la tuile gardait le focus (analyse du 21/11 : 31 % de CPU pendant une partie). */
  .tile::after {
    content: ''; position: absolute; inset: -8px; border-radius: var(--r-ring, calc(var(--radius-tile) + 8px)); pointer-events: none; z-index: -3;
    box-shadow: 0 0 26px 2px var(--accent); opacity: 0; transition: opacity var(--t-std) ease-out;
  }
  .tile:focus::after { animation: breathe 3.2s var(--ease-calm) 2 forwards; }
  @keyframes breathe { 0%, 100% { opacity: 0.12; } 50% { opacity: 0.28; } }

  /* Titre de la jaquette illustrée : 3 lignes au plus, coupé proprement au-delà. */
  .title {
    position: absolute; inset: auto 0 0 0; line-height: 1.04; font-weight: 700; letter-spacing: -0.02em;
    white-space: pre-line; text-wrap: balance; overflow-wrap: anywhere;
    transition: font-size var(--t-std) var(--ease-pop), transform 300ms var(--ease-land) 50ms;
  }
  /* Bornage dans un bloc sans rembourrage : WebKit peindrait sinon la 4e ligne dans le rembourrage. */
  .title .t { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; overflow: hidden; max-height: 3.12em; }
  .tile:focus .title { transform: translateY(-2px); }
  .title.shadowed { text-shadow: 0 2px 10px rgba(0, 0, 0, 0.28); }

  /* Parallaxe de la jaquette : le premier plan glisse plus que le décor. */
  .tile:focus :global(.far) { transform: translate(-1px, -1px); }
  .tile:focus :global(.near) { transform: translate(-3px, -2px) scale(1.02); }
  .tile.lifting :global(.near) { transform: translate(-4px, -4px) scale(1.04); }
  .tile:focus .top { transform: scale(1.04); }

  /* Mise en scène du lancement : l'estompe passe par l'opacité seule (plus de flou par tuile). */
  .tile.dimmed { opacity: 0.4; transform: scale(0.97); }
  .tile.lifting { transform: translateY(-12px) scale(1.05); box-shadow: 0 30px 60px rgba(0, 0, 0, 0.55); transition-duration: 220ms; }
  .tile.lifting .ring { opacity: 1; transform: scale(1.02); }

  /* Reflet de lumière qui traverse la tuile (lancement), synchronisé avec la montée des quatre notes. */
  .gleam { position: absolute; inset: 0; border-radius: inherit; overflow: hidden; pointer-events: none; }
  .gleam::before {
    content: ''; position: absolute; top: -20%; bottom: -20%; width: 45%; left: -60%;
    background: linear-gradient(100deg, transparent, rgba(255, 255, 255, 0.22), transparent);
    transform: skewX(-12deg);
  }
  .shine .gleam::before { animation: gleam 700ms cubic-bezier(.45, 0, .2, 1) 80ms both; }
  @keyframes gleam { from { left: -60%; } to { left: 120%; } }

  .tile.still { cursor: default; }
  .tile.still:focus { transform: none; box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45); }
  .tile.still .ring, .tile.still::after { display: none; }

  @media (prefers-reduced-motion: reduce) {
    .tile:focus, .tile.lifting, .tile.dimmed { transform: none; }
    .tile:focus .title, .tile:focus .top { transform: none; }
    .tile:focus :global(.far), .tile:focus :global(.near), .tile.lifting :global(.near) { transform: none; }
    .tile:focus::after { animation: none; opacity: 0.12; }
  }
</style>
