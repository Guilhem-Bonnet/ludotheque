<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { Conflict, ConflictSide } from '../lib/types';

  // Conflit de sauvegarde : deux cartes, aucune présélectionnée ; l'autre version reste dans l'historique.
  let { conflict }: { conflict: Conflict } = $props();
  let chosen = $state<'local' | 'server' | null>(null);

  onMount(() => {
    focusFirst();
    // Une seule note douce à l'arrivée : attire l'attention sans alarmer.
    setTimeout(() => feedback('conflict'), 180);
  });

  const sides = $derived<[string, ConflictSide][]>([['local', conflict.local], ['server', conflict.server]]);

  function use(which: string) {
    chosen = which as 'local' | 'server';
    // La carte choisie se soulève, l'autre s'efface vers le bas ; puis l'accord « sauvegarde ».
    // Côté Tauri : commande `resolve` (Utiliser cette sauvegarde), puis lancement.
    setTimeout(() => feedback('saved'), 260);
    setTimeout(() => router.back(), 1100);
  }
</script>

<div class="screen">
  <header class="rise" style:--i={0}>
    <span class="title"><Icon name="conflict" size={34} color="var(--conflict)" /><h1>Conflit de sauvegarde</h1></span>
    <span class="sub">{conflict.game.title} · {router.profile?.name} · L'autre version sera conservée dans l'historique.</span>
  </header>

  <main>
    {#each sides as [key, side] (key)}
      <button class="card focusable rise" style:--i={key === 'local' ? 1 : 2} class:chosen={chosen === key} class:dropped={chosen !== null && chosen !== key} onclick={() => use(key)} aria-pressed={chosen === key} disabled={chosen !== null}>
        <span class="card-head">
          <span class="dev"><Icon name={side.deviceKind} size={28} />{side.device}</span>
          {#if side.ahead}<span class="ahead">Plus de temps de jeu</span>{/if}
        </span>
        <span class="shot" role="img" aria-label="Capture de la dernière partie sur {side.device}">Capture de la dernière partie</span>
        <span class="facts">
          <span><span class="k">Dernière partie</span>{side.when}</span>
          <span><span class="k">Temps de jeu</span>{side.playtime} <span class="delta" class:hot={side.ahead}>{side.delta}</span></span>
        </span>
      </button>
    {/each}
  </main>

  <ButtonHints hints={[{ key: 'A', label: 'Utiliser cette sauvegarde' }, { key: 'Y', label: 'Détails' }, { key: 'B', label: 'Annuler' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; }
  header { padding: 48px 72px 0; display: flex; flex-direction: column; gap: 12px; }
  .title { display: flex; align-items: center; gap: 14px; }
  h1 { margin: 0; font-size: 34px; font-weight: 600; letter-spacing: -0.01em; }
  .sub { font-size: 17px; color: var(--ink-3); }
  main { flex-grow: 1; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 32px; padding: 40px 72px; }
  .card { all: unset; cursor: pointer; box-sizing: border-box; border-radius: 18px; background: var(--surface); padding: 28px; display: flex; flex-direction: column; gap: 22px; }
  .card { transition: transform var(--t-slow) var(--ease-land), opacity var(--t-slow) ease-out, box-shadow var(--t-std) var(--ease-land); }
  .card:focus { box-shadow: 0 0 0 4px var(--ground), 0 0 0 8px var(--accent); transform: translateY(-4px); }
  .card.chosen { transform: translateY(-8px) scale(1.02); box-shadow: 0 0 0 4px var(--ground), 0 0 0 8px var(--ok), 0 24px 50px rgba(0,0,0,.45); }
  .card.dropped { opacity: 0.25; transform: translateY(14px) scale(0.98); }
  .card-head { display: flex; align-items: center; justify-content: space-between; }
  .dev { display: flex; align-items: center; gap: 12px; font-size: 22px; font-weight: 600; }
  .ahead { font-size: 13px; font-weight: 600; color: var(--ground); background: var(--accent-ink); padding: 5px 10px; border-radius: 8px; }
  .shot { flex-grow: 1; min-height: 160px; border-radius: 12px; background: var(--raised); display: flex; align-items: flex-end; padding: 16px; font-size: 14px; color: var(--ink-3); }
  .facts { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; font-size: 17px; }
  .facts > span { display: flex; flex-direction: column; gap: 4px; }
  .k { color: var(--ink-4); font-size: 14px; }
  .delta { font-size: 14px; color: var(--ink-3); }
  .delta.hot { color: var(--accent-ink); }
</style>
