<script lang="ts">
  import { onMount } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import ConsoleChip from './ConsoleChip.svelte';
  import CloudIcon from './CloudIcon.svelte';
  import Icon from './Icon.svelte';
  import Tile from './Tile.svelte';
  import { api, CANCELLED, prepLabel, type Preparing } from '../api';
  import { feedback } from '../feedback';
  import { reducedMotion } from '../motion';
  import { focusFirst } from '../nav';
  import { syncRhythm, play as playSound } from '../sound';
  import type { Game } from '../types';

  // Menu Options d'un jeu (touche X), panneau qui glisse depuis la droite, à la manière de Steam.
  // Le focus reste dans le panneau ; B le referme et rend le focus à la tuile.
  let {
    game,
    profile,
    onclose,
    onplay,
    ondetails,
    onchange,
  }: {
    game: Game;
    profile: string;
    onclose: () => void;
    onplay: () => void;
    ondetails: () => void;
    onchange: (g: Game) => void;
  } = $props();

  const installed = $derived(game.install !== 'non_installe');
  const isSwitch = $derived(game.console === 'switch');
  let syncing = $state(false);
  let synced = $state(false);
  let failed = $state<string | null>(null);
  let confirmRemove = $state(false);
  let installing = $state<number | null>(null);
  let preparing = $state<Preparing | null>(null);
  let installError = $state<string | null>(null);
  let removing = $state(false);
  let cancelling = $state(false);

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  /** Installer pour de bon (téléchargement réel), sans quitter le menu : la progression s'affiche sur la ligne. */
  async function install() {
    if (installing !== null) return;
    installError = null;
    installing = 0;
    try {
      await api.installGame(game.id, (p, prep) => {
        installing = p;
        preparing = prep ?? null;
      });
      onchange({ ...game, install: 'installe' });
      feedback('downloaded');
      requestAnimationFrame(() => focusFirst());
    } catch (e) {
      // Annulation voulue : pas un échec, le fichier partiel est gardé pour reprendre.
      if (message(e) !== CANCELLED) {
        installError = message(e);
        feedback('err');
      }
    } finally {
      installing = null;
      preparing = null;
      cancelling = false;
    }
  }

  function cancelInstall() {
    if (installing === null || cancelling) return;
    cancelling = true;
    void api.cancelInstall(game.id);
  }

  async function syncGame() {
    if (syncing) return;
    syncing = true;
    feedback('syncStart');
    const stopRhythm = syncRhythm();
    let lib: Game[];
    try {
      lib = await api.sync(profile);
    } catch {
      syncing = false;
      synced = false;
      failed = 'Serveur injoignable';
      feedback('err');
      return;
    } finally {
      stopRhythm();
    }
    syncing = false;
    const g = lib.find((x) => x.id === game.id);
    if (g) onchange({ ...game, sync: g.sync, syncError: g.syncError });
    failed = g?.syncError ?? null;
    synced = !failed && g?.sync !== 'conflit';
    feedback(failed ? 'err' : synced ? 'saved' : 'conflict');
  }

  function switchEmulator() {
    // Switch : Ryujinx par défaut, Eden en second (décision du 14/11).
    const next = game.emulator.startsWith('Ryujinx') ? 'Eden 0.0.4' : 'Ryujinx 1.3.3';
    onchange({ ...game, emulator: next });
    feedback('tab');
  }

  async function remove() {
    if (removing) return;
    if (!confirmRemove) { confirmRemove = true; feedback('err'); return; }
    confirmRemove = false;
    removing = true;
    installError = null;
    try {
      await api.uninstallGame(game.id);
      onchange({ ...game, install: 'non_installe', sync: game.sync });
      feedback('saved');
      onclose();
    } catch (e) {
      installError = message(e);
      feedback('err');
    } finally {
      removing = false;
    }
  }

  onMount(() => {
    focusFirst();
    const onBack = () => onclose();
    window.addEventListener('ludo:modal-back', onBack);
    return () => window.removeEventListener('ludo:modal-back', onBack);
  });

  const d = (ms: number) => (reducedMotion() ? 0 : ms);
</script>

<div class="backdrop" transition:fade={{ duration: d(200) }} aria-hidden="true"></div>
<div class="panel" data-modal role="dialog" aria-label="Options : {game.title}" in:fly={{ x: 60, duration: d(320), easing: cubicOut }} out:fly={{ x: 40, duration: d(180) }}>
  <header>
    <Tile {game} size={96} still />
    <div class="meta">
      <span class="title">{game.title}</span>
      <span class="sub"><ConsoleChip console={game.console} size="s" on="ui" />{game.emulator}</span>
    </div>
  </header>

  <ul>
    <li>
      {#if installed}
        <button class="opt focusable" data-autofocus onclick={onplay}><Icon name="play" size={22} />Jouer</button>
      {:else}
        <button class="opt focusable" data-autofocus data-sound="none" aria-busy={installing !== null} onclick={install}>
          <Icon name="download" size={22} />{installing !== null ? (preparing ? prepLabel(preparing) : `Téléchargement… ${Math.round(installing * 100)} %`) : installError ? 'Réessayer' : 'Installer'}
        </button>
      {/if}
    </li>
    {#if installing !== null}
      <li><button class="opt focusable" data-sound="none" disabled={cancelling} onclick={cancelInstall}><Icon name="trash" size={22} />{cancelling ? 'Annulation…' : 'Annuler le téléchargement'}</button></li>
    {/if}
    <li><button class="opt focusable" onclick={ondetails}><Icon name="history" size={22} />Détails et sauvegardes</button></li>
    {#if installed}
      <li>
        <button class="opt focusable" data-sound="none" onclick={syncGame} aria-busy={syncing}>
          <CloudIcon name={synced ? 'cloud-ok' : 'cloud-sync'} busy={syncing} color={synced ? 'var(--ok)' : undefined} />
          {syncing ? 'Synchronisation…' : synced ? 'Sauvegarde à jour' : failed ? `Échec · ${failed}` : 'Synchroniser ce jeu'}
        </button>
      </li>
    {/if}
    {#if isSwitch}
      <li><button class="opt focusable" data-sound="none" onclick={switchEmulator}><Icon name="settings" size={22} />Émulateur<span class="value">{game.emulator.split(' ')[0]}</span></button></li>
    {/if}
    {#if installed}
      <li><button class="opt focusable danger" class:armed={confirmRemove} data-sound="none" onclick={remove}><Icon name="trash" size={22} />{removing ? 'Suppression…' : confirmRemove ? 'Confirmer : désinstaller' : 'Désinstaller'}</button></li>
    {/if}
  </ul>
  {#if installError}<p class="error" role="alert">{installError}</p>{/if}
  <p class="note">Les sauvegardes restent sur le serveur.</p>
</div>

<style>
  .backdrop { position: absolute; inset: 0; z-index: 10; background: rgba(5, 8, 12, 0.55); backdrop-filter: blur(3px); }
  .panel {
    position: absolute; top: 0; right: 0; bottom: 0; width: 420px; z-index: 11; box-sizing: border-box;
    background: #161a22; border-left: 1px solid rgba(255, 255, 255, 0.08); padding: 32px 28px;
    display: flex; flex-direction: column; gap: 24px; box-shadow: -24px 0 60px rgba(0, 0, 0, 0.45);
  }
  header { display: flex; gap: 16px; align-items: center; }
  .meta { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .title { font-size: 20px; font-weight: 600; line-height: 1.15; }
  .sub { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--ink-3); }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .opt {
    all: unset; cursor: pointer; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: 14px;
    padding: 14px 16px; border-radius: 12px; font-size: 16px; color: var(--ink);
    transition: background-color var(--t-quick) ease-out, transform var(--t-quick) var(--ease-pop);
  }
  .opt.focusable:focus { background: rgba(255, 255, 255, 0.1); box-shadow: inset 0 0 0 2px var(--accent); transform: translateX(4px); }
  .opt:disabled { opacity: 0.8; cursor: default; }
  .value { margin-left: auto; color: var(--ink-3); font-size: 14px; }
  .value::after { content: ' ›'; }
  .danger { color: #ff9b9b; }
  .danger.armed { background: rgba(255, 90, 90, 0.16); }
  .error { margin: 0; font-size: 14px; color: #ff9b9b; }
  .note { margin: auto 0 0; font-size: 13px; color: var(--ink-4); }
</style>
