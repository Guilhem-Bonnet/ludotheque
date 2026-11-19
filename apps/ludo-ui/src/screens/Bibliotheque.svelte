<script lang="ts">
  import StateNotice from '../lib/components/StateNotice.svelte';
  import { onMount, untrack } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import OptionsMenu from '../lib/components/OptionsMenu.svelte';
  import { play } from '../lib/play';
  import { api } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst, virtualKey } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import type { Console, DeviceInfo, Game, RemoteDevice } from '../lib/types';

  let { device }: { device: DeviceInfo } = $props();

  const TABS: { id: Console | 'tout'; label: string }[] = [
    { id: 'tout', label: 'Tout' }, { id: 'switch', label: 'Switch' }, { id: 'switch2', label: 'Switch 2' },
    { id: '3ds', label: '3DS' }, { id: 'ds', label: 'DS' }, { id: 'wiiu', label: 'Wii U' }, { id: 'gc', label: 'GameCube' },
  ];

  let games = $state<Game[]>([]);
  /* Appareil regardé : celui-ci (« ici ») ou un autre du foyer (Ayn Thor…), d'après son inventaire. */
  let remotes = $state<RemoteDevice[]>([]);
  let where = $state<string>('ici');
  const remote = $derived(remotes.find((r) => r.id === where));
  let tab = $state(0);
  let filter = $state<'tous' | 'installes' | 'non_installes'>('tous');
  let slide = $state<'left' | 'right' | 'none'>('none');

  /* Menu Options (X) du jeu en focus, comme à l'accueil. */
  let focusedId = $state<string | null>(null);
  let options = $state<Game | null>(null);
  function closeOptions() {
    const id = options?.id;
    options = null;
    feedback('back');
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-game="${id}"] .tile`)?.focus({ preventScroll: true }));
  }
  function changeGame(g: Game) {
    games = games.map((x) => (x.id === g.id ? g : x));
    options = g;
  }

  // Indicateur d'onglet qui glisse d'un onglet à l'autre (continuité), au lieu d'un fond qui saute.
  let tabEls = $state<HTMLElement[]>([]);
  let indicator = $state({ x: 0, w: 0, ready: false });
  $effect(() => {
    const el = tabEls[tab];
    if (!el) return;
    const prev = untrack(() => indicator);
    indicator = { x: el.offsetLeft, w: el.offsetWidth, ready: prev.ready || prev.w > 0 };
  });

  let loadError = $state<string | null>(null);
  let loaded = $state(false);
  async function load() {
    loadError = null;
    try {
      games = await api.library(router.profile!.id);
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
    loaded = true;
    focusFirst();
  }

  onMount(() => {
    void load();
    // Autres appareils : facultatif (serveur ancien ou injoignable = rien à montrer).
    api.devices().then((d) => (remotes = d)).catch(() => (remotes = []));
    const onTab = (ev: Event) => {
      const d = (ev as CustomEvent<number>).detail;
      slide = d > 0 ? 'right' : 'left';
      tab = (tab + d + TABS.length) % TABS.length;
      feedback('tab');
      focusFirst();
    };
    const onAction = (ev: Event) => {
      if ((ev as CustomEvent).detail !== 'x' || options) return;
      const onTile = (document.activeElement as HTMLElement | null)?.classList.contains('tile');
      const g = games.find((x) => x.id === focusedId);
      if (onTile && g) { feedback('ok'); options = g; }
    };
    window.addEventListener('ludo:tab', onTab);
    window.addEventListener('ludo:action', onAction);
    return () => { window.removeEventListener('ludo:tab', onTab); window.removeEventListener('ludo:action', onAction); };
  });

  const shown = $derived(
    games.filter((g) => {
      const t = TABS[tab].id;
      if (t !== 'tout' && g.console !== t) return false;
      const here = remote ? onRemote(g) : g.install !== 'non_installe';
      if (filter === 'installes') return here;
      if (filter === 'non_installes') return !here;
      return true;
    }),
  );
  const installed = $derived(games.filter((g) => (remote ? onRemote(g) : g.install !== 'non_installe')).length);

  /** Le jeu est-il jouable sur l'appareil regardé (installé, ou en flux depuis le PC) ? */
  function onRemote(g: Game): boolean {
    const r = remote?.games[g.id];
    return !!r && (r.installed || r.stream);
  }

  function stateOf(g: Game): [string, string] {
    if (remote) {
      const r = remote.games[g.id];
      if (r?.installed) return [r.save ? 'Installé · Sauvegarde' : 'Installé', 'var(--ink-3)'];
      if (r?.stream) return ['Via Moonlight', 'var(--accent-ink)'];
      if (r?.save) return ['Sauvegarde seule', 'var(--ink-4)'];
      return ['Absent', 'var(--ink-4)'];
    }
    if (g.sync === 'conflit') return ['Conflit de sauvegarde', 'var(--conflict)'];
    if (g.sync === 'envoi_en_attente') return ['Envoi en attente', 'var(--pending)'];
    if (g.sync === 'a_recuperer') return ['Nouvelle sauvegarde', 'var(--sync)'];
    if (g.install === 'mise_a_jour') return ['Mise à jour disponible', 'var(--update)'];
    if (g.install === 'telechargement') return [`Téléchargement… ${g.progress ?? 0} %`, 'var(--accent-ink)'];
    if (g.install === 'non_installe' && g.stream) return ['Via Moonlight', 'var(--accent-ink)'];
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
      <span class="meta">{#if remote}{installed} jeux sur {remote.name} · inventaire du {remote.updated}{:else}{games.length} jeux · {installed} installés · {device.freeSpace}{/if}</span>
    </div>
    <div class="line">
      <div class="tabs" role="tablist" aria-label="Consoles">
        <button class="bumper" tabindex="-1" aria-label="Onglet précédent" onpointerdown={(e) => e.preventDefault()} onclick={() => virtualKey('L1')}>L1</button>
        <span class="indicator" class:ready={indicator.ready} style:transform="translateX({indicator.x}px)" style:width="{indicator.w}px" aria-hidden="true"></span>
        {#each TABS as t, i (t.id)}
          <button role="tab" aria-selected={i === tab} class="tab" class:on={i === tab} bind:this={tabEls[i]} onclick={() => (tab = i)} tabindex="-1">{t.label}</button>
        {/each}
        <button class="bumper" tabindex="-1" aria-label="Onglet suivant" onpointerdown={(e) => e.preventDefault()} onclick={() => virtualKey('R1')}>R1</button>
      </div>
      <div class="filters">
        {#if remotes.length}
          <!-- Appareil regardé : celui-ci, ou un autre du foyer (ses jeux installés et en flux Moonlight). -->
          <button class="pill dev focusable" aria-pressed={where === 'ici'} onclick={() => { where = 'ici'; feedback('tab'); }}><Icon name={device.kind} size={16} />{device.name}</button>
          {#each remotes as r (r.id)}
            <button class="pill dev focusable" aria-pressed={where === r.id} onclick={() => { where = r.id; feedback('tab'); }}><Icon name={r.kind} size={16} />{r.name}</button>
          {/each}
          <span class="sep" aria-hidden="true"></span>
        {/if}
        <button class="pill focusable" aria-pressed={filter === 'installes'} onclick={() => toggle('installes')}>Installés</button>
        <button class="pill focusable" aria-pressed={filter === 'non_installes'} onclick={() => toggle('non_installes')}>Non installés</button>
      </div>
    </div>
  </header>

  {#key `${tab}-${filter}`}
  <main class="grid-{slide}">
    {#if loadError}
      <StateNotice title="Serveur injoignable" detail={loadError} action="Réessayer" onaction={() => void load()} />
    {:else if loaded && games.length === 0}
      <StateNotice icon="grid" title="Aucun jeu dans le catalogue" detail="Ajouter des ROM sur le serveur, puis relancer le balayage." autofocus={false} />
    {:else if loaded && shown.length === 0}
      <p class="empty">Aucun jeu pour ce filtre</p>
    {/if}
    {#each shown as g, i (g.id)}
      {@const st = stateOf(g)}
      <div class="cell" style:--i={Math.min(i, 8)} data-game={g.id}>
        <Tile game={g} size={168} autofocus={i === 0} onfocus={() => (focusedId = g.id)} onclick={() => router.go({ name: 'fiche', game: g })} />
        <span class="t">{g.title}</span>
        <span class="s" style:color={st[1]}>{st[0]}</span>
      </div>
    {/each}
  </main>
  {/key}

  {#if options}
    <OptionsMenu game={options} profile={router.profile!.id} onclose={closeOptions}
      onplay={() => { const g = options!; options = null; void play(g); }}
      ondetails={() => { const g = options!; options = null; router.go({ name: 'fiche', game: g }); }}
      onchange={changeGame} />
  {/if}
  <ButtonHints hints={options ? [{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Fermer' }] : [{ key: 'A', label: 'Ouvrir' }, { key: 'X', label: 'Options' }, { key: 'B', label: 'Retour' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; position: relative; }
  header { padding: 28px 48px 0; display: flex; flex-direction: column; gap: 22px; }
  .line { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .meta { font-size: 15px; color: var(--ink-3); }
  .tabs { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; position: relative; }
  .indicator { position: absolute; left: 0; top: 50%; height: 38px; margin-top: -19px; border-radius: var(--radius-control); background: var(--accent); z-index: 0; }
  .indicator.ready { transition: transform 260ms var(--ease-land), width 260ms var(--ease-land); }
  .bumper { all: unset; cursor: pointer; font-size: 12px; font-weight: 700; color: var(--ink-2); border: 1.5px solid rgba(255, 255, 255, 0.35); border-radius: 6px; padding: 2px 6px; margin: 0 6px; }
  .tab { all: unset; cursor: pointer; font-size: 16px; padding: 9px 16px; border-radius: var(--radius-control); color: var(--ink-2); }
  .tabs .tab { position: relative; z-index: 1; }
  .tab.on { color: var(--ground); font-weight: 600; }
  .filters { display: flex; gap: 8px; }
  .pill { all: unset; cursor: pointer; font-size: 14px; padding: 7px 14px; border-radius: 999px; border: 1.5px solid var(--hint); color: var(--ink-2); }
  .pill.dev { display: inline-flex; align-items: center; gap: 6px; }
  .sep { width: 1px; align-self: stretch; margin: 4px 6px; background: var(--hint); }
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
