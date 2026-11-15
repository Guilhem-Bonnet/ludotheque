<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
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

  onMount(() => {
    api.library(router.profile!.id).then((g) => {
      games = g;
      focusFirst();
    });
    const onTab = (ev: Event) => {
      tab = (tab + (ev as CustomEvent<number>).detail + TABS.length) % TABS.length;
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
        {#each TABS as t, i (t.id)}
          <button role="tab" aria-selected={i === tab} class="tab" class:on={i === tab} onclick={() => (tab = i)} tabindex="-1">{t.label}</button>
        {/each}
        <span class="bumper">R1</span>
      </div>
      <div class="filters">
        <button class="pill focusable" aria-pressed={filter === 'installes'} onclick={() => toggle('installes')}>Installés</button>
        <button class="pill focusable" aria-pressed={filter === 'non_installes'} onclick={() => toggle('non_installes')}>Non installés</button>
      </div>
    </div>
  </header>

  <main>
    {#if shown.length === 0}
      <p class="empty">Aucun jeu pour ce filtre</p>
    {/if}
    {#each shown as g, i (g.id)}
      {@const st = stateOf(g)}
      <div class="cell">
        <Tile game={g} size={168} autofocus={i === 0} onclick={() => router.go({ name: 'fiche', game: g })} />
        <span class="t">{g.title}</span>
        <span class="s" style:color={st[1]}>{st[0]}</span>
      </div>
    {/each}
  </main>

  <ButtonHints hints={[{ key: 'A', label: 'Ouvrir' }, { key: 'X', label: 'Installer' }, { key: 'B', label: 'Retour' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; }
  header { padding: 28px 48px 0; display: flex; flex-direction: column; gap: 22px; }
  .line { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .meta { font-size: 15px; color: var(--ink-3); }
  .tabs { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .bumper { font-size: 12px; font-weight: 700; color: var(--ink-4); border: 1.5px solid var(--hint); border-radius: 6px; padding: 2px 6px; margin: 0 6px; }
  .tab { all: unset; cursor: pointer; font-size: 16px; padding: 9px 16px; border-radius: var(--radius-control); color: var(--ink-2); }
  .tab.on { background: var(--accent); color: var(--ground); font-weight: 600; }
  .filters { display: flex; gap: 8px; }
  .pill { all: unset; cursor: pointer; font-size: 14px; padding: 7px 14px; border-radius: 999px; border: 1.5px solid var(--hint); color: var(--ink-2); }
  .pill[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-ink); }
  main { flex-grow: 1; overflow-y: auto; padding: 26px 48px 24px; display: grid; grid-template-columns: repeat(auto-fill, minmax(168px, 1fr)); gap: 28px 20px; align-content: start; }
  .cell { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .t { font-size: 15px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .s { font-size: 13px; margin-top: -6px; }
  .empty { grid-column: 1 / -1; color: var(--ink-3); font-size: 17px; }
</style>
