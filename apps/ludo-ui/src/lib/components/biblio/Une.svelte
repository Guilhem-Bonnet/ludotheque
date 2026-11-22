<script lang="ts">
  import CoverArt from '../CoverArt.svelte';
  import ConsoleChip from '../ConsoleChip.svelte';
  import type { Game } from '../../types';

  // Bannière « À la une » : la dernière partie du profil, à reprendre en un appui, et les jeux joués
  // juste avant. Fond aux couleurs du jeu (direction « Ambiance »), image du jeu à gauche.
  let { game, others, autofocus = true, onplay, onopen }: {
    game: Game;
    /** Faux au retour d'une fiche : le focus revient sur le jeu quitté, pas sur « Jouer ». */
    autofocus?: boolean;
    others: Game[];
    onplay: (g: Game) => void;
    onopen: (g: Game) => void;
  } = $props();

  /** Mélange de la teinte du jeu avec le fond sombre, calculé ici : color-mix() manque à la WebView de la Thor (Chromium 109). */
  function mix(hex: string, part: number): string {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    if (!m) return '#10141a';
    const n = parseInt(m[1], 16);
    const c = (sh: number, base: number) => Math.round(((n >> sh) & 255) * part + base * (1 - part));
    return `rgb(${c(16, 0x10)}, ${c(8, 0x14)}, ${c(0, 0x1a)})`;
  }
  const tint = $derived(game.art?.from ?? game.tile);

  let failed = $state('');
  const cover = $derived(game.cover && failed !== game.cover ? game.cover : undefined);
  const where = $derived(game.last ? `${game.last.device}, ${game.last.when.charAt(0).toLowerCase()}${game.last.when.slice(1)}` : '');
</script>

<section class="une" aria-label="À la une" style:--bg={mix(tint, 0.4)} style:--art-bg={mix(tint, 0.7)} data-game={game.id}>
  <div class="art" aria-hidden="true">
    {#if cover}
      <img src={cover} alt="" onerror={() => (failed = cover ?? '')} />
    {:else if game.art}
      <CoverArt art={game.art} />
    {/if}
  </div>
  <div class="main">
    <span class="kicker">À la une · Dernière partie</span>
    <h2>{game.title}</h2>
    <span class="meta"><ConsoleChip console={game.console} on="art" />{#if game.playtime}<span>{game.playtime}</span>{/if}{#if where}<span>{where}</span>{/if}</span>
    <div class="actions">
      <button class="btn primary focusable" data-autofocus={autofocus ? '' : undefined} data-nav-down="[data-row='0'] [data-col='0'] .focusable" onclick={() => onplay(game)}>Jouer</button>
      <button class="btn focusable" data-nav-down="[data-row='0'] [data-col='1'] .focusable, [data-row='0'] .focusable[data-col='1']" onclick={() => onopen(game)}>Fiche du jeu</button>
    </div>
  </div>
  {#if others.length}
    <div class="side">
      <span class="kicker">Joués avant</span>
      {#each others as g (g.id)}
        <button class="recent focusable" data-hover-focus onclick={() => onopen(g)}>
          <span class="dot" style:background={g.art?.to ?? g.tile} aria-hidden="true"></span>
          <span class="rt">{g.title}</span>
          {#if g.last}<span class="rw">{g.last.when}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .une {
    position: relative; flex-shrink: 0; height: 260px; display: flex; overflow: hidden; border-radius: 22px;
    background: var(--bg);
    color: #fff;
  }
  .art { position: relative; overflow: hidden; isolation: isolate; width: 260px; height: 260px; flex-shrink: 0; background: var(--art-bg); }
  .art img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .art :global(svg) { width: 100%; height: 100%; display: block; }
  .main { flex-grow: 1; min-width: 0; padding: 28px 36px; display: flex; flex-direction: column; gap: 10px; }
  .kicker { font-size: 13px; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; color: rgba(255, 255, 255, 0.78); }
  h2 { margin: 0; font-size: 36px; font-weight: 600; line-height: 1.12; letter-spacing: -0.01em; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .meta { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 14px; font-size: 16px; color: rgba(255, 255, 255, 0.86); }
  .meta > span { white-space: nowrap; }
  .actions { display: flex; gap: 12px; margin-top: auto; }
  .btn {
    all: unset; cursor: pointer; box-sizing: border-box; height: 48px; padding: 0 24px;
    display: inline-flex; align-items: center; border-radius: 12px;
    background: rgba(255, 255, 255, 0.16); font-size: 17px; font-weight: 500;
  }
  .btn.primary { background: #fff; color: #15171c; font-weight: 600; }
  .btn.focusable:focus { box-shadow: 0 0 0 3px var(--bg), 0 0 0 6px var(--accent); }
  .side { width: 340px; flex-shrink: 0; padding: 28px 28px 28px 0; display: flex; flex-direction: column; gap: 10px; }
  .recent {
    all: unset; cursor: pointer; box-sizing: border-box; height: 52px; padding: 0 12px; border-radius: 12px;
    display: flex; align-items: center; gap: 12px; background: rgba(0, 0, 0, 0.2);
  }
  .recent:focus-visible, .recent:focus { box-shadow: 0 0 0 3px var(--accent); }
  .dot { width: 32px; height: 32px; border-radius: 8px; flex-shrink: 0; }
  .rt { flex-grow: 1; min-width: 0; font-size: 14px; line-height: 1.2; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .rw { font-size: 13px; color: rgba(255, 255, 255, 0.78); white-space: nowrap; }
</style>
