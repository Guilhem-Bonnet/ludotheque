<script lang="ts">
  import ConsoleChip from './ConsoleChip.svelte';
  import SyncBadge from './SyncBadge.svelte';
  import type { Game } from '../types';

  // Tuile typographique d'un jeu (aucune illustration officielle : titre + couleur propre au jeu).
  //
  // Mouvement au focus (couches primaire / secondaire / ambiance) :
  // - la tuile grandit (rebond léger) et se soulève de 4 px ;
  // - son ombre descend et s'adoucit (contre-mouvement), l'anneau d'accent se pose (95 % → 100 %) ;
  // - le titre suit 50 ms plus tard, la pastille console et l'état 70 ms plus tard (follow-through) ;
  // - un halo de la couleur du joueur respire lentement (ambiance).
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
    sound?: string;
    onfocus?: () => void;
    onclick?: (el: HTMLElement) => void;
  } = $props();

  let el: HTMLButtonElement | undefined = $state();
  const titleSize = $derived(Math.round(size * (game.short.length > 14 ? 0.115 : 0.13)));
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
  tabindex={still ? -1 : undefined}
  style:width="{size}px"
  style:height="{size}px"
  style:background={game.tile}
  style:view-transition-name={hero ? 'hero-tile' : undefined}
  data-autofocus={autofocus ? '' : undefined}
  data-sound={sound}
  data-hero={hero ? '' : undefined}
  aria-label={game.title}
  {onfocus}
  onclick={() => el && onclick?.(el)}
>
  <span class="ring" aria-hidden="true"></span>
  <span class="gleam" aria-hidden="true"></span>
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
    position: relative; isolation: isolate;
    box-shadow: 0 4px 10px rgba(0, 0, 0, 0.22);
    transition:
      width var(--t-std) var(--ease-pop),
      height var(--t-std) var(--ease-pop),
      padding var(--t-std) var(--ease-land),
      box-shadow 320ms var(--ease-land),
      transform 280ms var(--ease-pop),
      opacity 220ms ease-out,
      filter 220ms ease-out;
  }
  .tile.big { border-radius: 18px; padding: 22px; }

  /* Focus : se soulève, l'ombre descend et s'adoucit (contre-mouvement). */
  .tile:focus { transform: translateY(-4px); box-shadow: 0 22px 44px rgba(0, 0, 0, 0.5), 0 6px 12px rgba(0, 0, 0, 0.25); }

  /* Anneau d'accent : se pose en grandissant de 95 % à 100 %. */
  .ring {
    position: absolute; inset: -8px; border-radius: calc(var(--radius-tile) + 8px); pointer-events: none;
    box-shadow: inset 0 0 0 4px var(--accent); opacity: 0; transform: scale(0.95);
    transition: opacity 150ms ease-out, transform 220ms var(--ease-pop);
  }
  .big .ring { border-radius: 26px; }
  .tile:focus .ring { opacity: 1; transform: scale(1); }

  /* Halo qui respire (ambiance), très discret. */
  .tile::after {
    content: ''; position: absolute; inset: -8px; border-radius: 26px; pointer-events: none; z-index: -1;
    box-shadow: 0 0 26px 2px var(--accent); opacity: 0; transition: opacity var(--t-std) ease-out;
  }
  .tile:focus::after { animation: breathe 3.2s var(--ease-calm) infinite; }
  @keyframes breathe { 0%, 100% { opacity: 0.08; } 50% { opacity: 0.28; } }

  /* Suivi décalé : titre puis pastilles. */
  .title { line-height: 1.04; font-weight: 700; letter-spacing: -0.02em; white-space: pre-line; text-wrap: balance;
    transition: font-size var(--t-std) var(--ease-pop), transform 300ms var(--ease-land) 50ms; }
  .tile:focus .title { transform: translateY(-2px); }
  .top { display: flex; justify-content: space-between; align-items: center; transition: transform 300ms var(--ease-pop) 70ms; transform-origin: left center; }
  .tile:focus .top { transform: scale(1.04); }

  /* Mise en scène du lancement. */
  .tile.dimmed { opacity: 0.4; filter: blur(1.5px) saturate(0.8); transform: scale(0.97); }
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
  }
</style>
