<script lang="ts">
  import Ambiance from '../lib/components/Ambiance.svelte';
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import RestoreDialog from '../lib/components/RestoreDialog.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { play } from '../lib/play';
  import { router } from '../lib/router.svelte';
  import type { Game, GameContent, SaveRevision } from '../lib/types';

  let { game }: { game: Game } = $props();
  let history = $state<SaveRevision[]>([]);
  let restored = $state<number | null>(null);

  let restoring = $state<SaveRevision | null>(null);

  /** Restaurer : fenêtre de chargement, puis la version devient « Actuelle » (nouvelle révision côté serveur). */
  function restore(r: SaveRevision) {
    restoring = r;
  }
  function restoreDone() {
    const r = restoring!;
    restoring = null;
    restored = r.rev;
    history = history.map((h) => ({ ...h, current: h.rev === r.rev }));
    requestAnimationFrame(() => document.querySelector<HTMLElement>('button.primary.focusable')?.focus({ preventScroll: true }));
  }

  /* Onglets : Sauvegardes / Contenu additionnel (mises à jour et DLC), L1 et R1 comme la Bibliothèque. */
  let tab = $state<'saves' | 'content'>('saves');
  let content = $state<GameContent[]>(game.content ?? []);
  let progress = $state<Record<number, number>>({});
  let justInstalled = $state<number | null>(null);
  let failedId = $state<number | null>(null);

  function showTab(t: 'saves' | 'content') {
    if (t === tab || (t === 'content' && content.length === 0)) return;
    tab = t;
    feedback('tab');
  }

  async function install(c: GameContent) {
    if (c.installed || progress[c.id] !== undefined) return;
    failedId = null;
    progress = { ...progress, [c.id]: 0 };
    try {
      await api.installContent(game.id, c.id, (p) => (progress = { ...progress, [c.id]: p }));
      content = content.map((x) => (x.id === c.id ? { ...x, installed: true } : x));
      justInstalled = c.id;
      feedback('downloaded');
    } catch {
      failedId = c.id;
      feedback('err');
    } finally {
      const { [c.id]: _, ...rest } = progress;
      progress = rest;
    }
  }

  onMount(() => {
    api.history(router.profile!.id, game.id).then((h) => (history = h));
    focusFirst();
    const onTab = (ev: Event) => showTab((ev as CustomEvent<number>).detail > 0 ? 'content' : 'saves');
    window.addEventListener('ludo:tab', onTab);
    return () => window.removeEventListener('ludo:tab', onTab);
  });

  const installed = $derived(game.install !== 'non_installe');
</script>

<div class="screen">
  <Ambiance art={game.art} x={14} />
  <main>
    <aside>
      <div class="art"><Tile {game} size={320} big still hero={router.heroGame === game.id} /></div>
      <div class="actions">
        <button class="primary focusable" data-autofocus onclick={() => play(game)}>{installed ? 'Jouer' : 'Installer'}</button>
        {#if installed}<button class="secondary focusable">Désinstaller</button>{/if}
      </div>
      <dl>
        <dt>Émulateur</dt><dd>{game.emulator}</dd>
        {#if game.version}<dt>Version</dt><dd>{game.version}</dd>{/if}
        {#if game.size}<dt>{installed ? "Sur l'appareil" : 'Taille'}</dt><dd>{game.size}</dd>{/if}
        {#if game.playtime}<dt>Temps de jeu</dt><dd>{game.playtime}</dd>{/if}
      </dl>
    </aside>

    <section aria-labelledby="hist">
      {#if content.length > 0}
        <div class="tabs" role="tablist">
          <span class="bumper" aria-hidden="true">L1</span>
          <button class="tab" role="tab" aria-selected={tab === 'saves'} class:on={tab === 'saves'} onclick={() => showTab('saves')}>Sauvegardes</button>
          <button class="tab" role="tab" aria-selected={tab === 'content'} class:on={tab === 'content'} onclick={() => showTab('content')}>Contenu additionnel<span class="count">{content.length}</span></button>
          <span class="bumper" aria-hidden="true">R1</span>
        </div>
      {/if}
      {#if tab === 'content'}
        <ul class="content" aria-label="Contenu additionnel">
          {#each content as c, i (c.id)}
            <li class="rise" style:--i={i}>
              <button class="row focusable" class:pop={justInstalled === c.id} class:done={c.installed} data-sound="none" aria-disabled={c.installed} onclick={() => install(c)}>
                <Icon name={c.role === 'maj' ? 'update' : 'download'} size={22} color="var(--ink-2)" />
                <span class="cname">{c.name}</span>
                <span class="role">{c.role === 'maj' ? 'Mise à jour' : 'DLC'}</span>
                <span class="csize">{c.size}</span>
                {#if c.installed}
                  <span class="tag ok"><Icon name="check" size={14} color="#8be6b5" />Installé</span>
                {:else if progress[c.id] !== undefined}
                  <span class="mini" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(progress[c.id] * 100)} aria-label="Installation de {c.name}"><span style:width="{progress[c.id] * 100}%"></span></span>
                {:else}
                  <span class="restore">{failedId === c.id ? 'Réessayer' : 'Installer'}</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
        <span class="policy">Mises à jour et DLC installés dans le dossier de contenu externe de l'émulateur, de la même façon sur tous les appareils.</span>
      {:else}
      <div class="head">
        <h1 id="hist">Historique des sauvegardes</h1>
        {#if game.sync === 'a_jour'}<span class="state"><Icon name="cloud-ok" size={20} color="var(--ok)" />À jour</span>{/if}
      </div>
      {#if history.length === 0}
        <p class="empty">Aucune sauvegarde</p>
      {:else}
        <ul>
          {#each history as r (r.rev)}
            <li class="rise" style:--i={history.indexOf(r)}>
              <button class="row focusable" class:pop={restored === r.rev} disabled={r.current} data-sound="ok" onclick={() => restore(r)}>
                <Icon name={r.deviceKind} size={24} color="var(--ink-2)" />
                <span class="dev">{r.device}</span>
                <span class="when">{r.when}</span>
                <span class="time">{r.playtime}</span>
                {#if r.current}<span class="tag ok">Actuelle</span>
                {:else if r.discarded}<span class="tag discarded">Écartée lors d'un conflit</span>
                {:else}<span class="restore">Restaurer</span>{/if}
              </button>
            </li>
          {/each}
        </ul>
        <span class="policy">20 dernières versions, une par jour sur 30 jours, une par mois ensuite.</span>
      {/if}
      {/if}
    </section>
  </main>
  {#if restoring}
    <RestoreDialog {game} rev={restoring} profile={router.profile!.id} ondone={restoreDone} onfail={() => { restoring = null; requestAnimationFrame(() => document.querySelector<HTMLElement>('button.primary.focusable')?.focus()); }} />
  {/if}
  <ButtonHints hints={restoring ? [] : [{ key: 'A', label: tab === 'content' ? 'Installer' : 'Sélectionner' }, { key: 'B', label: 'Retour' }]} note={content.length > 0 && !restoring ? 'L1 / R1 : Sauvegardes, Contenu additionnel' : undefined} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; position: relative; isolation: isolate; }
  main { flex-grow: 1; display: flex; gap: 56px; padding: 44px 56px 0; min-height: 0; }
  aside { width: 320px; flex-shrink: 0; display: flex; flex-direction: column; gap: 22px; }
  .art { pointer-events: none; }
  .actions { display: flex; gap: 12px; }
  .primary, .secondary { all: unset; cursor: pointer; flex-grow: 1; text-align: center; padding: 14px 0; border-radius: 12px; }
  .primary { font-size: 18px; font-weight: 600; background: var(--ink); color: var(--ground); }
  .secondary { font-size: 16px; background: var(--control); color: var(--ink); }
  dl { margin: 0; display: grid; grid-template-columns: auto 1fr; gap: 10px 18px; font-size: 15px; }
  dt { color: var(--ink-4); }
  dd { margin: 0; }
  section { flex-grow: 1; display: flex; flex-direction: column; gap: 18px; min-width: 0; overflow-y: auto; }
  .head { display: flex; align-items: baseline; justify-content: space-between; }
  h1 { margin: 0; font-size: 28px; font-weight: 600; }
  .state { display: flex; align-items: center; gap: 8px; font-size: 15px; color: var(--ink-2); }
  ul { list-style: none; margin: 0; padding: 4px; display: flex; flex-direction: column; gap: 8px; }
  .row { all: unset; cursor: pointer; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: 16px; padding: 16px 20px; border-radius: 12px; background: var(--surface); font-size: 16px; }
  .row:disabled { cursor: default; }
  .row:focus { background: var(--raised); }
  .dev { width: 140px; }
  .when { width: 200px; color: var(--ink-2); }
  .time { color: var(--ink-2); }
  .tag { margin-left: auto; font-size: 13px; font-weight: 600; padding: 4px 10px; border-radius: 8px; }
  .tag.ok { background: #23402f; color: #8be6b5; }
  .tag.discarded { background: #33284a; color: #d9b8ff; }
  .restore { margin-left: auto; font-size: 15px; font-weight: 600; padding: 8px 16px; border-radius: var(--radius-control); background: var(--control); color: var(--ink); }
  .row:focus .restore { background: var(--accent); color: var(--ground); }
  .policy, .empty { font-size: 14px; color: var(--ink-4); }
  .tabs { display: flex; align-items: center; gap: 6px; }
  .bumper { font-size: 12px; font-weight: 700; color: var(--ink-4); border: 1.5px solid var(--hint); border-radius: 6px; padding: 2px 6px; margin: 0 4px; }
  .tab { all: unset; cursor: pointer; font-size: 20px; font-weight: 600; padding: 8px 14px; border-radius: var(--radius-control); color: var(--ink-3); display: inline-flex; align-items: center; gap: 8px; transition: color var(--t-quick) ease-out, background-color var(--t-std) var(--ease-land); }
  .tab.on { color: var(--ground); background: var(--accent); }
  .count { font-size: 12px; font-weight: 700; min-width: 20px; height: 20px; border-radius: 10px; display: inline-flex; align-items: center; justify-content: center; background: rgba(255, 255, 255, 0.16); }
  .tab.on .count { background: rgba(0, 0, 0, 0.18); }
  .cname { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .role { font-size: 12px; font-weight: 600; letter-spacing: 0.04em; color: var(--ink-3); background: rgba(255, 255, 255, 0.08); padding: 3px 8px; border-radius: 6px; }
  .csize { width: 80px; text-align: right; color: var(--ink-2); font-variant-numeric: tabular-nums; }
  /* Colonne de droite de largeur fixe : rien ne bouge quand « Installer » devient une barre puis « Installé ». */
  .content .tag, .content .restore, .content .mini { margin-left: 8px; width: 120px; box-sizing: border-box; justify-content: center; text-align: center; flex-shrink: 0; }
  .content .row.done { cursor: default; }
  .tag { display: inline-flex; align-items: center; gap: 4px; }
  .mini { width: 120px; height: 6px; border-radius: 3px; background: rgba(255, 255, 255, 0.12); overflow: hidden; }
  .mini span { display: block; height: 100%; background: var(--accent); transition: width 90ms linear; }
</style>
