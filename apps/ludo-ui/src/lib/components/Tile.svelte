<script lang="ts">
  import ConsoleChip from './ConsoleChip.svelte';
  import SyncBadge from './SyncBadge.svelte';
  import type { Game } from '../types';

  // Tuile typographique d'un jeu (aucune illustration officielle : titre + couleur propre au jeu).
  // Mouvement : la tuile grandit au focus avec un léger rebond, son ombre s'élève (couche secondaire),
  // et un halo d'accent respire lentement tant qu'elle a le focus (couche d'ambiance).
  let {
    game,
    size = 232,
    big = false,
    still = false,
    hero = false,
    autofocus = false,
    sound,
    onfocus,
    onclick,
  }: {
    game: Game;
    size?: number;
    big?: boolean;
    still?: boolean;
    /** Élément partagé de la transition vers l'écran suivant (la tuile « s'envole »). */
    hero?: boolean;
    autofocus?: boolean;
    /** Son joué à l'appui (défaut : « ok » ; « none » quand l'action joue son propre son). */
    sound?: string;
    onfocus?: () => void;
    onclick?: () => void;
  } = $props();

  const titleSize = $derived(Math.round(size * (game.short.length > 14 ? 0.115 : 0.13)));
</script>

<button
  class="tile"
  class:focusable={!still}
  class:still
  class:big
  tabindex={still ? -1 : undefined}
  style:width="{size}px"
  style:height="{size}px"
  style:background={game.tile}
  style:view-transition-name={hero ? 'hero-tile' : undefined}
  data-autofocus={autofocus ? '' : undefined}
  data-sound={sound}
  aria-label={game.title}
  {onfocus}
  {onclick}
>
  <span class="top">
    <ConsoleChip console={game.console} size={size > 250 ? 'l' : size < 190 ? 's' : 'm'} />
    <SyncBadge {game} size={Math.round(size * 0.14)} />
  </span>
  <span class="title" style:font-size="{titleSize}px" style:color={game.tileInk}>{game.short}</span>
</button>

<style>
  .tile {
    all: unset; cursor: pointer; flex-shrink: 0; box-sizing: border-box;
    border-radius: var(--radius-tile); padding: 20px;
    display: flex; flex-direction: column; justify-content: space-between;
    position: relative;
    box-shadow: 0 6px 14px rgba(0, 0, 0, 0.25);
    transition:
      width var(--t-std) var(--ease-pop),
      height var(--t-std) var(--ease-pop),
      padding var(--t-std) var(--ease-land),
      box-shadow var(--t-std) var(--ease-land),
      transform var(--t-quick) var(--ease-pop);
  }
  .title { line-height: 1.04; font-weight: 700; letter-spacing: -0.02em; white-space: pre-line; text-wrap: balance; transition: font-size var(--t-std) var(--ease-pop); }
  .tile.big { border-radius: 18px; padding: 22px; box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45); }
  .tile:focus { box-shadow: 0 0 0 4px var(--ground), 0 0 0 8px var(--accent), 0 18px 40px rgba(0, 0, 0, 0.45); }

  /* Halo qui respire (ambiance) : très discret, uniquement sur la tuile en focus. */
  .tile::after {
    content: ''; position: absolute; inset: -8px; border-radius: 26px; pointer-events: none;
    box-shadow: 0 0 22px 2px var(--accent); opacity: 0; transition: opacity var(--t-std) ease-out;
  }
  .tile:focus::after { animation: breathe 3.2s var(--ease-calm) infinite; }
  @keyframes breathe { 0%, 100% { opacity: 0.1; } 50% { opacity: 0.32; } }

  .tile.still { cursor: default; }
  .tile.still:focus { box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45); }
  .tile.still::after { display: none; }
  .top { display: flex; justify-content: space-between; align-items: center; }
</style>
