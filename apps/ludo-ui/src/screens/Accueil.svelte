<script lang="ts">
  import { onMount } from 'svelte';
  import Avatar from '../lib/components/Avatar.svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { DeviceInfo, Game } from '../lib/types';
  import { play } from '../lib/play';

  let { device }: { device: DeviceInfo } = $props();

  let games = $state<Game[]>([]);
  let focused = $state(0);
  let updatesCount = $state(0);
  let clock = $state(now());

  function now() {
    return new Date().toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  }

  onMount(() => {
    const t = setInterval(() => (clock = now()), 15_000);
    const profile = router.profile!;
    Promise.all([api.recent(profile.id), api.updates()]).then(([g, u]) => {
      games = g;
      updatesCount = u.length;
      focusFirst();
    });
    const onAction = (ev: Event) => {
      const g = games[focused];
      if ((ev as CustomEvent).detail === 'y' && g) router.go({ name: 'fiche', game: g });
    };
    window.addEventListener('ludo:action', onAction);
    return () => {
      clearInterval(t);
      window.removeEventListener('ludo:action', onAction);
    };
  });

  const current = $derived(games[focused]);
  const offline = $derived(device.server === 'hors_ligne');
</script>

<div class="screen">
  <header>
    <div class="who">
      <Avatar profile={router.profile!} size={52} ring />
      <div class="who-text">
        <span class="name">{router.profile!.name}</span>
        <span class="device">{device.name}</span>
      </div>
    </div>
    <div class="status">
      {#if offline}
        <span class="state"><Icon name="cloud-off" size={22} color="var(--offline)" />Hors ligne</span>
      {:else}
        <span class="state"><Icon name="cloud-ok" size={22} color="var(--ok)" />À jour</span>
      {/if}
      <span>{clock}</span>
    </div>
  </header>

  <section aria-label="Jeux récents" class="row-wrap">
    <div class="row">
      {#each games as game, i (game.id)}
        <!-- Une seule tuile par jeu : elle grandit au focus sans être recréée (le focus reste dessus). -->
        <div class="slot" class:focused={i === focused}>
          <Tile {game} size={i === focused ? 272 : 232} big={i === focused} autofocus={i === 0} onfocus={() => (focused = i)} onclick={() => play(game)} />
          {#if i === focused && game.last}
            <div class="device-tab">
              <Icon name={game.last.deviceKind} size={22} color="var(--accent-ink)" />
              <span>{game.last.device} · {game.last.when}</span>
              {#if game.sync === 'a_jour'}
                <span class="tab-state"><Icon name="cloud-ok" size={18} color="var(--ok)" label="À jour" /></span>
              {:else if game.sync === 'envoi_en_attente'}
                <span class="tab-state"><Icon name="cloud-pending" size={18} color="var(--pending)" label="Envoi en attente" /></span>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>

    {#if current}
      <div class="caption">
        <h1>{current.title}</h1>
        {#if current.playtime}<span>{current.playtime} de jeu</span>{/if}
      </div>
    {/if}
  </section>

  <nav aria-label="Raccourcis">
    <button class="shortcut focusable" onclick={() => router.go({ name: 'bibliotheque' })}>
      <span class="round"><Icon name="grid" size={26} /></span>Bibliothèque
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'mises-a-jour' })}>
      <span class="round"><Icon name="update" size={26} />{#if updatesCount}<span class="count">{updatesCount}</span>{/if}</span>Mises à jour
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'qui-joue' })}>
      <span class="round"><Icon name="people" size={26} /></span>Profils
    </button>
    <button class="shortcut focusable" disabled>
      <span class="round"><Icon name="settings" size={26} /></span>Réglages
    </button>
  </nav>

  <ButtonHints hints={[{ key: 'A', label: 'Jouer' }, { key: 'Y', label: 'Détails' }, { key: 'X', label: 'Options' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 28px 48px 0; }
  .who { display: flex; align-items: center; gap: 14px; }
  .who-text { display: flex; flex-direction: column; gap: 2px; }
  .name { font-size: 18px; font-weight: 600; }
  .device { font-size: 13px; color: var(--ink-3); }
  .status { display: flex; align-items: center; gap: 22px; font-size: 15px; color: var(--ink-2); }
  .state { display: flex; align-items: center; gap: 8px; }

  .row-wrap { padding: 48px 0 0 48px; }
  .row { display: flex; align-items: flex-start; gap: 20px; overflow-x: auto; overflow-y: visible; padding: 10px 48px 10px 10px; margin: -10px 0 0 -10px; scrollbar-width: none; }
  .row::-webkit-scrollbar { display: none; }
  .slot { display: flex; flex-direction: column; flex-shrink: 0; margin-top: 20px; }
  .slot.focused { width: 272px; margin-top: 0; }

  /* Geste signature : l'onglet d'appareil glisse sous la tuile en focus. */
  .device-tab {
    margin: -6px 10px 0; padding: 18px 14px 12px; border-radius: 0 0 14px 14px; white-space: nowrap;
    background: var(--accent-soft); color: var(--accent-ink);
    display: flex; align-items: center; gap: 10px; font-size: 14px;
  }
  .tab-state { margin-left: auto; display: flex; }
  @media (prefers-reduced-motion: no-preference) {
    .device-tab { animation: tab-out 220ms cubic-bezier(.2, .8, .2, 1) both; }
    @keyframes tab-out { from { transform: translateY(-18px); opacity: 0; } to { transform: none; opacity: 1; } }
  }

  .caption { margin-top: 26px; display: flex; align-items: baseline; gap: 16px; }
  .caption h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .caption span { font-size: 16px; color: var(--ink-3); }

  nav { margin-top: auto; display: flex; justify-content: center; gap: 36px; padding-bottom: 26px; }
  .shortcut { all: unset; cursor: pointer; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 13px; color: var(--ink-2); border-radius: 14px; padding: 4px; }
  .shortcut:disabled { opacity: 0.45; cursor: default; }
  .round { width: 60px; height: 60px; border-radius: 50%; background: var(--control); display: flex; align-items: center; justify-content: center; position: relative; color: var(--ink); }
  .count { position: absolute; top: -2px; right: -2px; min-width: 22px; height: 22px; border-radius: 11px; background: var(--accent); color: var(--ground); font-size: 12px; font-weight: 700; display: flex; align-items: center; justify-content: center; }
</style>
