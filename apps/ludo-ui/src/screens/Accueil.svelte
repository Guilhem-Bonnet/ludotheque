<script lang="ts">
  import StateNotice from '../lib/components/StateNotice.svelte';
  import { onMount } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { feedback } from '../lib/feedback';
  import { play as playSound } from '../lib/sound';
  import { reducedMotion } from '../lib/motion';
  import Ambiance from '../lib/components/Ambiance.svelte';
  import Avatar from '../lib/components/Avatar.svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import SyncStatus from '../lib/components/SyncStatus.svelte';
  import OptionsMenu from '../lib/components/OptionsMenu.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { DeviceInfo, Game } from '../lib/types';
  import { play } from '../lib/play';
  import { appUpdate } from '../lib/appUpdate.svelte';

  let { device }: { device: DeviceInfo } = $props();

  let games = $state<Game[]>([]);
  /** Toute la bibliothèque du profil : sert à l'état global de synchronisation de l'en-tête. */
  let all = $state<Game[]>([]);
  let focused = $state(0);
  let updatesCount = $state(0);
  let clock = $state(now());

  function now() {
    return new Date().toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  }

  onMount(() => {
    const t = setInterval(() => (clock = now()), 15_000);
    const profile = router.profile!;
    void load(profile.id);
    // Les mises à jour ne bloquent jamais l'accueil : un échec laisse simplement le compteur à 0.
    api.updates(router.profile?.id).then((u) => (updatesCount = u.length)).catch(() => (updatesCount = 0));
    const onAction = (ev: Event) => {
      const g = games[focused];
      const onTile = (document.activeElement as HTMLElement | null)?.classList.contains('tile');
      if ((ev as CustomEvent).detail === 'x' && g && onTile && !options) {
        feedback('ok');
        options = g;
        return;
      }
      if ((ev as CustomEvent).detail === 'y' && g && !options) {
        feedback('ok');
        openGame(g);
      }
    };
    window.addEventListener('ludo:action', onAction);
    return () => {
      clearInterval(t);
      window.removeEventListener('ludo:action', onAction);
    };
  });

  let launching = $state<string | null>(null);

  /* Chargement réel des jeux récents et de la bibliothèque ; erreur = écran « Serveur injoignable ». */
  let loadError = $state<string | null>(null);
  let loaded = $state(false);
  async function load(profile: string) {
    loadError = null;
    try {
      const [g, lib] = await Promise.all([api.recent(profile), api.library(profile)]);
      games = g;
      all = lib;
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
    loaded = true;
    focusFirst();
  }

  /* Menu Options du jeu sélectionné (X). */
  let options = $state<Game | null>(null);
  function tileOf(id: string) {
    return document.querySelector<HTMLElement>(`.slot.focused .tile`) ?? document.querySelector<HTMLElement>('.tile');
  }
  function closeOptions() {
    options = null;
    feedback('back');
    requestAnimationFrame(() => tileOf('')?.focus({ preventScroll: true }));
  }
  function changeGame(g: Game) {
    games = games.map((x) => (x.id === g.id ? g : x));
    all = all.map((x) => (x.id === g.id ? g : x));
    options = g;
  }
  let profileFocused = $state(false);

  /* Synchronisation manuelle (A sur le nuage de l'en-tête). */
  let syncing = $state(false);
  let syncFocused = $state(false);
  async function syncNow() {
    if (syncing) return;
    if (offline) { feedback('err'); return; }
    syncing = true;
    feedback('syncStart');
    const tick = setInterval(() => playSound('syncTick'), 900);
    let lib: Game[];
    try {
      lib = await api.sync(router.profile!.id);
    } catch {
      // Serveur injoignable ou configuration illisible : rien n'a changé, aucun faux « à jour ».
      syncing = false;
      feedback('err');
      return;
    } finally {
      clearInterval(tick);
    }
    const byId = new Map(lib.map((g) => [g.id, g]));
    all = lib;
    games = games.map((g) => ({ ...g, sync: byId.get(g.id)?.sync ?? g.sync, syncError: byId.get(g.id)?.syncError }));
    syncing = false;
    feedback(lib.some((g) => g.syncError) ? 'err' : lib.some((g) => g.sync === 'conflit') ? 'conflict' : 'saved');
  }

  /** A ou clic sur une tuile : la fiche du jeu s'ouvre (le lancement se fait depuis la fiche ou le menu Options). */
  function openGame(g: Game) {
    router.heroGame = g.id;
    router.go({ name: 'fiche', game: g });
  }

  /* Défilement de la rangée géré ici (pas par la navigation) : la tuile choisie se place en deuxième
     position, d'un défilement doux. Positions calculées, pas mesurées : les tuiles non choisies font
     232 px + 20 px d'écart, donc la cible ne dépend pas de l'agrandissement en cours. */
  let row: HTMLDivElement | undefined = $state();
  $effect(() => {
    const i = focused;
    if (!row) return;
    row.scrollTo({ left: Math.max(0, (i - 1) * 252), behavior: reducedMotion() ? 'auto' : 'smooth' });
  });

  /** Mise en scène du lancement : la tuile se soulève, les autres s'estompent, puis l'écran change. */
  function launch(game: Game, el: HTMLElement) {
    launching = game.id;
    void play(game, el);
  }

  const current = $derived(games[focused]);

  // Ambiance (direction C) : la lumière se place au-dessus de la tuile sélectionnée.
  const lightX = $derived(Math.min(14 + focused * 18, 86));
  const offline = $derived(device.server === 'hors_ligne');
</script>

<div class="screen">
  <Ambiance art={current?.art} cover={current?.cover} background={current?.background} x={lightX} key={current?.id} />
  <header class="rise" style:--i={0}>
    <button class="who pill focusable" aria-label="Profil de {router.profile!.name}" onclick={() => router.go({ name: 'profil' })} onfocus={() => (profileFocused = true)} onblur={() => (profileFocused = false)}>
      <Avatar profile={router.profile!} size={46} />
      <div class="who-text">
        <span class="name">{router.profile!.name}</span>
        <span class="device">{device.name}</span>
      </div>
    </button>
    <div class="status">
      <SyncStatus games={all} {offline} busy={syncing} onclick={syncNow} onfocus={() => (syncFocused = true)} onblur={() => (syncFocused = false)} />
      <span class="clock">{clock}</span>
    </div>
  </header>

  {#if loadError}
    <div class="notice-wrap"><StateNotice title="Serveur injoignable" detail={loadError} action="Réessayer" onaction={() => void load(router.profile!.id)} /></div>
  {:else if loaded && games.length === 0}
    <div class="notice-wrap"><StateNotice icon="grid" title="Aucune partie récente" detail="Les jeux joués apparaissent ici. Tous les jeux sont dans la Bibliothèque." action="Bibliothèque" onaction={() => router.go({ name: 'bibliotheque' })} /></div>
  {/if}
  <section aria-label="Jeux récents" class="row-wrap">
    <div class="row" bind:this={row} data-own-scroll>
      {#each games as game, i (game.id)}
        <!-- Une seule tuile par jeu : elle grandit au focus sans être recréée (le focus reste dessus). -->
        <div class="slot rise" class:focused={i === focused} style:--i={i}>
          <Tile
            {game}
            size={i === focused ? 272 : 232}
            big={i === focused}
            hero={router.heroGame === game.id}
            dimmed={launching !== null && launching !== game.id}
            lifting={launching === game.id}
            autofocus={i === 0}
            onfocus={() => (focused = i)}
            onclick={() => openGame(game)}
          />
          {#if i === focused && game.last}
            <div class="device-tab">
              <Icon name={game.last.deviceKind} size={16} color="var(--ink-4)" />
              <span>{game.last.device} · {game.last.when}</span>
              {#if game.sync === 'a_jour'}
                <span class="tab-state"><Icon name="cloud-ok" size={15} color="var(--ink-4)" label="À jour" /></span>
              {:else if game.sync === 'envoi_en_attente'}
                <span class="tab-state"><Icon name="cloud-pending" size={15} color="var(--pending)" label="Envoi en attente" /></span>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>

    {#if current}
      <!-- Le titre change avec la tuile : l'ancien s'efface, le nouveau monte de 8 px. -->
      <div class="caption-slot">
        {#key current.id}
          <div class="caption" in:fly={{ y: 8, duration: reducedMotion() ? 0 : 260, delay: reducedMotion() ? 0 : 60, easing: cubicOut }} out:fade={{ duration: reducedMotion() ? 0 : 120 }}>
            <h1>{current.title}</h1>
            {#if current.playtime}<span>{current.playtime} de jeu</span>{/if}
          </div>
        {/key}
      </div>
    {/if}
  </section>

  <nav aria-label="Raccourcis" class="rise" style:--i={6}>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'bibliotheque' })}>
      <span class="round"><Icon name="grid" size={26} /></span>Bibliothèque
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'mises-a-jour' })}>
      <span class="round"><Icon name="update" size={26} />{#if updatesCount + (appUpdate.status === 'available' ? 1 : 0)}<span class="count">{updatesCount + (appUpdate.status === 'available' ? 1 : 0)}</span>{/if}</span>Mises à jour
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'qui-joue' })}>
      <span class="round"><Icon name="people" size={26} /></span>Profils
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'reglages' })}>
      <span class="round"><Icon name="settings" size={26} /></span>Réglages
    </button>
  </nav>

  {#if options}
    <OptionsMenu game={options} profile={router.profile!.id} onclose={closeOptions}
      onplay={() => { const g = options!; options = null; const el = tileOf(g.id); if (el) launch(g, el); }}
      ondetails={() => { const g = options!; options = null; router.heroGame = g.id; router.go({ name: 'fiche', game: g }); }}
      onchange={changeGame} />
  {/if}
  {#if options}
    <ButtonHints hints={[{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Fermer' }]} />
  {:else if syncFocused}
    <ButtonHints hints={[{ key: 'A', label: 'Synchroniser' }]} />
  {:else if profileFocused}
    <ButtonHints hints={[{ key: 'A', label: 'Profil' }]} />
  {:else}
    <ButtonHints hints={[{ key: 'A', label: 'Ouvrir' }, { key: 'X', label: 'Options' }]} />
  {/if}
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; position: relative; isolation: isolate; }
  /* Pastilles translucides (reprises de la direction B). */
  .pill { background: rgba(255, 255, 255, 0.1); }
  .who.pill { all: unset; cursor: pointer; display: flex; align-items: center; padding: 5px 20px 5px 5px; border-radius: 40px; gap: 12px; background: rgba(255, 255, 255, 0.1); transition: background-color var(--t-quick) ease-out, transform var(--t-quick) var(--ease-pop), box-shadow var(--t-std) var(--ease-land); }
  .who.pill.focusable:focus { background: rgba(255, 255, 255, 0.2); transform: translateY(-2px); box-shadow: 0 0 0 3px var(--accent), 0 8px 18px rgba(0, 0, 0, 0.3); }
  .clock { font-weight: 600; color: var(--ink); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 28px 48px 0; }
  .who { display: flex; align-items: center; gap: 14px; }
  .who-text { display: flex; flex-direction: column; gap: 2px; }
  .name { font-size: 17px; font-weight: 600; }
  .device { font-size: 12px; color: rgba(255, 255, 255, 0.72); }
  .status { display: flex; align-items: center; gap: 14px; font-size: 15px; color: var(--ink); }

  .row-wrap { padding: 48px 0 0 48px; }
  .row { display: flex; align-items: flex-start; gap: 20px; overflow-x: auto; overscroll-behavior-x: contain; overflow-y: visible; padding: 22px 64px 16px 12px; margin: -22px 0 0 -12px; scroll-padding-inline: 24px 64px; scrollbar-width: none; }
  .row::-webkit-scrollbar { display: none; }
  .slot { display: flex; flex-direction: column; flex-shrink: 0; margin-top: 20px; }
  .slot.focused { width: 272px; margin-top: 0; }

  /* Ligne discrète sous la tuile en focus : appareil et heure de la dernière partie, en gris neutre. */
  .device-tab {
    margin: 12px 4px 0; white-space: nowrap; color: rgba(255, 255, 255, 0.7);
    display: flex; align-items: center; gap: 7px; font-size: 13px;
  }
  .tab-state { margin-left: auto; display: flex; }
  @media (prefers-reduced-motion: no-preference) {
    .device-tab { animation: tab-out 240ms var(--ease-land) 80ms both; }
    @keyframes tab-out { from { transform: translateY(-4px); opacity: 0; } to { transform: none; opacity: 1; } }
  }

  .caption-slot { margin-top: 26px; height: 40px; position: relative; }
  .caption { position: absolute; inset: 0 auto auto 0; white-space: nowrap; display: flex; align-items: baseline; gap: 16px; }
  .caption h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; text-shadow: 0 2px 14px rgba(0, 0, 0, 0.45); }
  .caption span { font-size: 16px; color: rgba(255, 255, 255, 0.72); }

  nav { margin-top: auto; display: flex; justify-content: center; gap: 36px; padding-bottom: 26px; }
  .shortcut { all: unset; cursor: pointer; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 13px; color: var(--ink-2); border-radius: 14px; padding: 4px; }
  .shortcut:disabled { opacity: 0.45; cursor: default; }
  .shortcut:focus { box-shadow: none; color: var(--accent-ink); }
  .shortcut:focus .round { transform: translateY(-3px) scale(1.06); background: var(--accent-soft); color: var(--accent-ink); box-shadow: 0 0 0 3px var(--ground), 0 0 0 6px var(--accent), 0 10px 20px rgba(0,0,0,.35); }
  .round { transition: transform var(--t-quick) var(--ease-pop), background-color var(--t-quick) ease-out, box-shadow var(--t-quick) var(--ease-land); width: 60px; height: 60px; border-radius: 50%; background: rgba(255, 255, 255, 0.13); display: flex; align-items: center; justify-content: center; position: relative; color: var(--ink); }
  .count { position: absolute; top: -2px; right: -2px; min-width: 22px; height: 22px; border-radius: 11px; background: var(--accent); color: var(--ground); font-size: 12px; font-weight: 700; display: flex; align-items: center; justify-content: center; }
  .notice-wrap { padding: 40px 48px 0; }
</style>
