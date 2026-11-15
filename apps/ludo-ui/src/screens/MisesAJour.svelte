<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import ConsoleChip from '../lib/components/ConsoleChip.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api } from '../lib/api';
  import { focusFirst } from '../lib/nav';
  import type { UpdateItem } from '../lib/types';

  let items = $state<UpdateItem[]>([]);
  onMount(() => {
    api.updates().then((u) => {
      items = u;
      focusFirst();
    });
  });

  const GROUPS: { id: UpdateItem['group']; label: string }[] = [
    { id: 'jeux', label: 'Jeux' },
    { id: 'nouveaux', label: 'Nouveaux dans la bibliothèque' },
    { id: 'emulateurs', label: 'Émulateurs et Ludothèque' },
  ];
</script>

<div class="screen">
  <header>
    <h1>Mises à jour</h1>
    {#if items.length}<button class="all focusable" data-autofocus>Tout mettre à jour</button>{/if}
  </header>

  <main>
    {#if items.length === 0}<p class="empty">Tout est à jour</p>{/if}
    {#each GROUPS as grp (grp.id)}
      {@const list = items.filter((i) => i.group === grp.id)}
      {#if list.length}
        <section>
          <h2>{grp.label}</h2>
          {#each list as it (it.id)}
            <div class="item">
              <span class="thumb" style:background={it.tile ?? 'var(--control)'}>{#if !it.tile}<Icon name="settings" size={26} color="var(--ink-2)" />{/if}</span>
              <span class="text"><span class="title">{it.title}</span><span class="detail">{it.detail}</span></span>
              {#if it.console}<ConsoleChip console={it.console} size="m" />{/if}
              <button class="act focusable">{it.action}</button>
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
  .empty { color: var(--ink-3); font-size: 17px; }
</style>
