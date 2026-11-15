<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { Console, DeviceInfo, Game } from '../lib/types';

  let { device }: { device: DeviceInfo } = $props();

  const TABS: { id: Console | 'tout'; label: string }[] = [
    { id: 'tout', label: 'Tout' }, { id: 'switch', label: 'Switch' }, { id: 'switch2', label: 'Switch 2' },
    { id: '3ds', label: '3DS' }, { id: 'ds', label: 'DS' }, { id: 'wiiu', label: 'Wii U' }, { id: 'gc', label: 'GameCube' },
  ];

  let games = $state<Game[]>([]);
  let tab = $state(0);
  let filter = $state<'tous' | 'installes' | 'non_installes'>('tous');
  let slide = $state<'left' | 'right' | 'none'>('none');

  // Indicateur d'onglet qui glisse d'un onglet à l'autre (continuité), au lieu d'un fond qui saute.
  let tabEls = $state<HTMLElement[]>([]);
  let indicator = $state({ x: 0, w: 0, ready: false });
  $effect(() => {
    const el = tabEls[tab];
    if (!el) return;
    const prev = untrack(() => indicator);
    indicator = { x: el.offsetLeft, w: el.offsetWidth, ready: prev.ready || prev.w > 0 };
  });

  onMount(() => {
    api.library(router.profile!.id).then((g) => {
      games = g;
      focusFirst();
    });
    const onTab = (ev: Event) => {
      const d = (ev as CustomEvent<number>).detail;
      slide = d > 0 ? 'right' : 'left';
      tab = (tab + d + TABS.length) % TABS.length;
      feedback('tab');
      focusFirst();
    };
    window.addEventListener('ludo:tab', onTab);
    return () => window.removeEventListener('ludo:tab', onTab);
  });

  const shown = $derived(
    games.filter((g) => {
      const t = TABS[tab].id;
      if (t !== 'tout' && g.console !== t) return false;
      if (filter === 'installes') return g.install !== 'non_installe';
      if (filter === 'non_installes') return g.install === 'non_installe';
      return true;
    }),
  );
  const installed = $derived(games.filter((g) => g.install !== 'non_installe').length);

  function stateOf(g: Game): [string, string] {
    if (g.sync === 'conflit') return ['Conflit de sauvegarde', 'var(--conflict)'];
    if (g.sync === 'envoi_en_attente') return ['Envoi en attente', 'var(--pending)'];
    if (g.install === 'mise_a_jour') return ['Mise à jour disponible', 'var(--update)'];
    if (g.install === 'telechargement') return [`Téléchargement… ${g.progress ?? 0} %`, 'var(--accent-ink)'];
    if (g.install === 'non_installe') return ['Non installé', 'var(--ink-4)'];
    return ['Installé · À jour', 'var(--ink-3)'];
  }

  function toggle(f: 'installes' | 'non_installes') {
    filter = filter === f ? 'tous' : f;
  }
</script>

<div class="screen">
  <header>
    <div class="line">
      <h1>Bibliothèque</h1>
      <span class="meta">{games.length} jeux · {installed} installés · {device.freeSpace}</span>
    </div>
    <div class="line">
      <div class="tabs" role="tablist" aria-label="Consoles">
        <span class="bumper">L1</span>
        <span class="indicator" class:ready={indicator.ready} style:transform="translateX({indicator.x}px)" style:width="{indicator.w}px" aria-hidden="true"></span>
        {#each TABS as t, i (t.id)}
          <button role="tab" aria-selected={i === tab} class="tab" class:on={i === tab} bind:this={tabEls[i]} onclick={() => (tab = i)} tabindex="-1">{t.label}</button>
        {/each}
        <span class="bumper">R1</span>
      </div>
      <div class="filters">
        <button class="pill focusable" aria-pressed={filter === 'installes'} onclick={() => toggle('installes')}>Installés</button>
        <button class="pill focusable" aria-pressed={filter === 'non_installes'} onclick={() => toggle('non_installes')}>Non installés</button>
      </div>
    </div>
  </header>

  {#key `${tab}-${filter}`}
  <main class="grid-{slide}">
    {#if shown.length === 0}
      <p class="empty">Aucun jeu pour ce filtre</p>
    {/if}
    {#each shown as g, i (g.id)}
      {@const st = stateOf(g)}
      <div class="cell" style:--i={Math.min(i, 8)}>
        <Tile game={g} size={168} autofocus={i === 0} onclick={() => router.go({ name: 'fiche', game: g })} />
        <span class="t">{g.title}</span>
        <span class="s" style:color={st[1]}>{st[0]}</span>
      </div>
    {/each}
  </main>
  {/key}

  <ButtonHints hints={[{ key: 'A', label: 'Ouvrir' }, { key: 'X', label: 'Installer' }, { key: 'B', label: 'Retour' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; }
  header { padding: 28px 48px 0; display: flex; flex-direction: column; gap: 22px; }
  .line { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .meta { font-size: 15px; color: var(--ink-3); }
  .tabs { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; position: relative; }
  .indicator { position: absolute; left: 0; top: 50%; height: 38px; margin-top: -19px; border-radius: var(--radius-control); background: var(--accent); z-index: 0; }
  .indicator.ready { transition: transform 260ms var(--ease-land), width 260ms var(--ease-land); }
  .bumper { font-size: 12px; font-weight: 700; color: var(--ink-4); border: 1.5px solid var(--hint); border-radius: 6px; padding: 2px 6px; margin: 0 6px; }
  .tab { all: unset; cursor: pointer; font-size: 16px; padding: 9px 16px; border-radius: var(--radius-control); color: var(--ink-2); }
  .tabs .tab { position: relative; z-index: 1; }
  .tab.on { color: var(--ground); font-weight: 600; }
  .filters { display: flex; gap: 8px; }
  .pill { all: unset; cursor: pointer; font-size: 14px; padding: 7px 14px; border-radius: 999px; border: 1.5px solid var(--hint); color: var(--ink-2); }
  .pill[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-ink); }
  main { flex-grow: 1; overflow-y: auto; padding: 26px 48px 24px; display: grid; grid-template-columns: repeat(auto-fill, minmax(168px, 1fr)); gap: 28px 20px; align-content: start; }
  /* Changement d'onglet : la grille glisse du côté de l'onglet choisi, cellules en cascade de 25 ms. */
  @keyframes grid-from-right { from { opacity: 0; transform: translateX(24px); } }
  @keyframes grid-from-left { from { opacity: 0; transform: translateX(-24px); } }
  .grid-right .cell { animation: grid-from-right var(--t-std) var(--ease-land) both; animation-delay: calc(var(--i) * 25ms); }
  .grid-left .cell { animation: grid-from-left var(--t-std) var(--ease-land) both; animation-delay: calc(var(--i) * 25ms); }
  .tab { transition: background-color var(--t-quick) ease-out, color var(--t-quick) ease-out; }
  .cell { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .t { font-size: 15px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .s { font-size: 13px; margin-top: -6px; }
  .empty { grid-column: 1 / -1; color: var(--ink-3); font-size: 17px; }
</style>
