<script lang="ts">
  import ConsoleChip from './ConsoleChip.svelte';
  import SyncBadge from './SyncBadge.svelte';
  import type { Game } from '../types';

  // Tuile typographique d'un jeu (aucune illustration officielle : titre + couleur propre au jeu).
  let {
    game,
    size = 232,
    big = false,
    still = false,
    autofocus = false,
    onfocus,
    onclick,
  }: { game: Game; size?: number; big?: boolean; still?: boolean; autofocus?: boolean; onfocus?: () => void; onclick?: () => void } = $props();

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
  data-autofocus={autofocus ? '' : undefined}
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
  }
  .tile.big { border-radius: 18px; padding: 22px; box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45); }
  .tile:focus { box-shadow: 0 0 0 4px var(--ground), 0 0 0 8px var(--accent), 0 18px 40px rgba(0, 0, 0, 0.45); }
  .tile.still { cursor: default; }
  .tile.still:focus { box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45); }
  .top { display: flex; justify-content: space-between; align-items: center; }
  .title { line-height: 1.04; font-weight: 700; letter-spacing: -0.02em; white-space: pre-line; text-wrap: balance; }
</style>
