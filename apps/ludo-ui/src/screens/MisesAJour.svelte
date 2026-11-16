<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import ConsoleChip from '../lib/components/ConsoleChip.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import StateNotice from '../lib/components/StateNotice.svelte';
  import type { UpdateItem } from '../lib/types';

  let items = $state<UpdateItem[]>([]);
  // Avancement par élément : undefined = pas commencé, 0-100 = en cours, 'fait', ou message d'échec.
  let progress = $state<Record<string, number | 'fait'>>({});
  let failed = $state<Record<string, string>>({});
  let loadError = $state<string | null>(null);
  let loaded = $state(false);

  /** Installe la mise à jour ou le DLC (`id` = « <jeu>|<fichier> » côté app réelle). */
  async function run(it: UpdateItem) {
    if (progress[it.id] !== undefined && progress[it.id] !== 'fait') return;
    if (progress[it.id] === 'fait') return;
    delete failed[it.id];
    progress[it.id] = 0;
    const [game, file] = it.id.split('|');
    try {
      await api.installContent(game, Number(file), (p) => (progress[it.id] = Math.round(p * 100)));
      progress[it.id] = 'fait';
      feedback(it.action === 'Installer' ? 'downloaded' : 'updated');
    } catch (e) {
      delete progress[it.id];
      failed[it.id] = e instanceof Error ? e.message : String(e);
      feedback('err');
    }
  }

  function load() {
    loadError = null;
    api.updates().then((u) => {
      items = u;
      loaded = true;
      focusFirst();
    }).catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
      loaded = true;
      focusFirst();
    });
  }

  function runAll() {
    items.forEach((it, k) => setTimeout(() => run(it), k * 300));
  }
  onMount(load);

  const GROUPS: { id: UpdateItem['group']; label: string }[] = [
    { id: 'jeux', label: 'Jeux' },
    { id: 'nouveaux', label: 'Nouveaux dans la bibliothèque' },
    { id: 'emulateurs', label: 'Émulateurs et Ludothèque' },
  ];
</script>

<div class="screen">
  <header>
    <h1>Mises à jour</h1>
    {#if items.length}<button class="all focusable" data-autofocus onclick={runAll}>Tout mettre à jour</button>{/if}
  </header>

  <main>
    {#if loadError}
      <StateNotice title="Serveur injoignable" detail={loadError} action="Réessayer" onaction={load} />
    {:else if loaded && items.length === 0}
      <StateNotice icon="check-circle" title="Tout est à jour" detail="Aucune mise à jour ni DLC à installer pour les jeux de cet appareil." autofocus={false} />
    {/if}
    {#each GROUPS as grp (grp.id)}
      {@const list = items.filter((i) => i.group === grp.id)}
      {#if list.length}
        <section>
          <h2>{grp.label}</h2>
          {#each list as it (it.id)}
            <div class="item" class:success={progress[it.id] === 'fait'}>
              <span class="thumb" style:background={it.tile ?? 'var(--control)'}>{#if !it.tile}<Icon name="settings" size={26} color="var(--ink-2)" />{/if}</span>
              <span class="text"><span class="title">{it.title}</span><span class="detail">{it.detail}</span></span>
              {#if it.console}<ConsoleChip console={it.console} size="m" on="ui" />{/if}
              {#if progress[it.id] === 'fait'}
                <span class="done draw" style:--len="16"><Icon name="check-circle" size={24} color="var(--ok)" />{it.action === 'Installer' ? 'Installé' : 'À jour'}</span>
              {:else if progress[it.id] !== undefined}
                <span class="mini" role="progressbar" aria-valuenow={Number(progress[it.id])} aria-valuemin="0" aria-valuemax="100" aria-label="{it.title} : {progress[it.id]} %"><span style:width="{progress[it.id]}%"></span></span>
              {:else}
                {#if failed[it.id]}<span class="fail" title={failed[it.id]}>Échec</span>{/if}
                <button class="act focusable" onclick={() => run(it)}>{failed[it.id] ? 'Réessayer' : it.action}</button>
              {/if}
            </div>
          {/each}
        </section>
      {/if}
    {/each}
  </main>

  <ButtonHints hints={[{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Retour' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; }
  header { padding: 36px 64px 0; display: flex; align-items: center; justify-content: space-between; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .all { all: unset; cursor: pointer; font-size: 16px; font-weight: 600; padding: 12px 22px; border-radius: 12px; background: var(--accent); color: var(--ground); }
  main { flex-grow: 1; overflow-y: auto; padding: 28px 64px 24px; display: flex; flex-direction: column; gap: 26px; }
  section { display: flex; flex-direction: column; gap: 10px; }
  h2 { margin: 0 0 4px; font-size: 17px; font-weight: 600; color: var(--ink-3); }
  .item { display: flex; align-items: center; gap: 18px; padding: 14px 18px; border-radius: 12px; background: var(--surface); }
  .thumb { width: 56px; height: 56px; border-radius: 10px; flex-shrink: 0; display: flex; align-items: center; justify-content: center; }
  .text { display: flex; flex-direction: column; gap: 4px; flex-grow: 1; min-width: 0; }
  .title { font-size: 17px; font-weight: 500; }
  .detail { font-size: 14px; color: var(--ink-3); }
  .act { all: unset; cursor: pointer; font-size: 15px; padding: 9px 16px; border-radius: var(--radius-control); background: var(--control); color: var(--ink); }
  .act:focus { background: var(--accent); color: var(--ground); }
  .item { transition: background-color 600ms ease-out; }
  .item.success { animation: success-flash 900ms ease-out; }
  @keyframes success-flash { 0% { background: color-mix(in srgb, var(--ok) 16%, var(--surface)); } 100% { background: var(--surface); } }
  .done { display: flex; align-items: center; gap: 8px; font-size: 15px; color: var(--ink-2); animation: pop 320ms var(--ease-pop); }
  .mini { width: 120px; height: 6px; border-radius: 3px; background: var(--raised); overflow: hidden; display: block; }
  .mini span { display: block; height: 100%; background: var(--accent); border-radius: 3px; transition: width 120ms linear; }
  .empty { color: var(--ink-3); font-size: 17px; }
  .fail { font-size: 13px; color: #ff9b9b; }
</style>
