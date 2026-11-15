<script lang="ts">
  import Ambiance from '../lib/components/Ambiance.svelte';
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { play } from '../lib/play';
  import { router } from '../lib/router.svelte';
  import type { Game, SaveRevision } from '../lib/types';

  let { game }: { game: Game } = $props();
  let history = $state<SaveRevision[]>([]);
  let restored = $state<number | null>(null);

  function restore(r: SaveRevision) {
    // La version restaurée devient « Actuelle » (une nouvelle révision côté serveur) ; accord « sauvegarde ».
    restored = r.rev;
    feedback('saved');
    history = history.map((h) => ({ ...h, current: h.rev === r.rev }));
  }

  onMount(() => {
    api.history(router.profile!.id, game.id).then((h) => (history = h));
    focusFirst();
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
              <button class="row focusable" class:pop={restored === r.rev} disabled={r.current} data-sound="none" onclick={() => restore(r)}>
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
    </section>
  </main>
  <ButtonHints hints={[{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Retour' }]} />
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
</style>
