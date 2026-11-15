<script lang="ts">
  import { onMount } from 'svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { feedback } from '../lib/feedback';
  import { reducedMotion } from '../lib/motion';
  import Ambiance from '../lib/components/Ambiance.svelte';
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
      if ((ev as CustomEvent).detail === 'y' && g) {
        feedback('ok');
        router.heroGame = g.id;
        router.go({ name: 'fiche', game: g });
      }
    };
    window.addEventListener('ludo:action', onAction);
    return () => {
      clearInterval(t);
      window.removeEventListener('ludo:action', onAction);
    };
  });

  let launching = $state<string | null>(null);

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
  <Ambiance art={current?.art} x={lightX} key={current?.id} />
  <header class="rise" style:--i={0}>
    <div class="who pill">
      <Avatar profile={router.profile!} size={46} />
      <div class="who-text">
        <span class="name">{router.profile!.name}</span>
        <span class="device">{device.name}</span>
      </div>
    </div>
    <div class="status">
      {#if offline}
        <span class="state pill"><span class="dot off"></span>Hors ligne</span>
      {:else}
        <span class="state pill"><span class="dot"></span>À jour</span>
      {/if}
      <span class="clock">{clock}</span>
    </div>
  </header>

  <section aria-label="Jeux récents" class="row-wrap">
    <div class="row">
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
            onclick={(el) => launch(game, el)}
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
          <div class="caption" in:fly={{ y: 8, duration: reducedMotion() ? 0 : 220, easing: cubicOut }}>
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
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; position: relative; isolation: isolate; }
  /* Pastilles translucides (reprises de la direction B). */
  .pill { background: rgba(255, 255, 255, 0.1); }
  .who.pill { padding: 5px 20px 5px 5px; border-radius: 40px; gap: 12px; }
  .state.pill { padding: 8px 14px; border-radius: 20px; }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 0 var(--ok); }
  .dot.off { background: var(--offline); }
  .clock { font-weight: 600; color: var(--ink); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 28px 48px 0; }
  .who { display: flex; align-items: center; gap: 14px; }
  .who-text { display: flex; flex-direction: column; gap: 2px; }
  .name { font-size: 17px; font-weight: 600; }
  .device { font-size: 12px; color: rgba(255, 255, 255, 0.72); }
  .status { display: flex; align-items: center; gap: 14px; font-size: 15px; color: var(--ink); }
  .state { display: flex; align-items: center; gap: 8px; }

  .row-wrap { padding: 48px 0 0 48px; }
  .row { display: flex; align-items: flex-start; gap: 20px; overflow-x: auto; overflow-y: visible; padding: 22px 64px 16px 12px; margin: -22px 0 0 -12px; scroll-padding-inline: 24px 64px; scrollbar-width: none; }
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

  .caption-slot { margin-top: 26px; height: 40px; }
  .caption { display: flex; align-items: baseline; gap: 16px; }
  .caption h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .caption span { font-size: 16px; color: rgba(255, 255, 255, 0.72); }

  nav { margin-top: auto; display: flex; justify-content: center; gap: 36px; padding-bottom: 26px; }
  .shortcut { all: unset; cursor: pointer; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 13px; color: var(--ink-2); border-radius: 14px; padding: 4px; }
  .shortcut:disabled { opacity: 0.45; cursor: default; }
  .shortcut:focus { box-shadow: none; color: var(--accent-ink); }
  .shortcut:focus .round { transform: translateY(-3px) scale(1.06); background: var(--accent-soft); color: var(--accent-ink); box-shadow: 0 0 0 3px var(--ground), 0 0 0 6px var(--accent), 0 10px 20px rgba(0,0,0,.35); }
  .round { transition: transform var(--t-quick) var(--ease-pop), background-color var(--t-quick) ease-out, box-shadow var(--t-quick) var(--ease-land); width: 60px; height: 60px; border-radius: 50%; background: rgba(255, 255, 255, 0.13); display: flex; align-items: center; justify-content: center; position: relative; color: var(--ink); }
  .count { position: absolute; top: -2px; right: -2px; min-width: 22px; height: 22px; border-radius: 11px; background: var(--accent); color: var(--ground); font-size: 12px; font-weight: 700; display: flex; align-items: center; justify-content: center; }
</style>
