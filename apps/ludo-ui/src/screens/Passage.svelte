<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { focusFirst } from '../lib/nav';
  import type { Preparation } from '../lib/types';

  // Écran de passage : n'apparaît que s'il y a quelque chose à faire (téléchargement, synchro).
  let { preparation }: { preparation: Preparation } = $props();

  onMount(focusFirst);
</script>

<div class="screen">
  <main>
    <div class="art"><Tile game={preparation.game} size={340} big still /></div>
    <div class="side">
      <div class="head">
        <h1>Préparation</h1>
        <span>{preparation.game.title} · {preparation.game.emulator}</span>
      </div>
      <ol>
        {#each preparation.steps as step (step.label)}
          <li class={step.state}>
            <span class="line">
              {#if step.state === 'fait'}
                <Icon name="check-circle" size={26} color="var(--ok)" />
              {:else if step.state === 'en_cours'}
                <span class="spin"><Icon name="spinner" size={26} color="var(--accent)" /></span>
              {:else}
                <Icon name="circle" size={26} color="#5b6170" />
              {/if}
              <span class="label">{step.label}</span>
              {#if step.progress !== undefined}<span class="pct">{step.progress} %</span>{/if}
            </span>
            {#if step.progress !== undefined}
              <span class="bar" role="progressbar" aria-valuenow={step.progress} aria-valuemin="0" aria-valuemax="100" aria-label={step.label}>
                <span style:width="{step.progress}%"></span>
              </span>
            {/if}
            {#if step.detail}<span class="detail">{step.detail}</span>{/if}
          </li>
        {/each}
      </ol>
    </div>
  </main>
  <ButtonHints note="Sauvegardes conservées sur le serveur" hints={[{ key: 'B', label: 'Annuler' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; }
  main { flex-grow: 1; display: flex; align-items: center; gap: 72px; padding: 0 96px; }
  .art { pointer-events: none; }
  .art :global(.tile) { cursor: default; }
  .side { flex-grow: 1; display: flex; flex-direction: column; gap: 36px; }
  .head { display: flex; flex-direction: column; gap: 10px; }
  h1 { margin: 0; font-size: 36px; font-weight: 600; letter-spacing: -0.01em; }
  .head span { font-size: 17px; color: var(--ink-3); }
  ol { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 22px; font-size: 18px; }
  li { display: flex; flex-direction: column; gap: 12px; color: var(--ink-2); }
  li.a_venir { color: var(--ink-4); }
  li.en_cours .label { font-weight: 500; color: var(--ink); }
  .line { display: flex; align-items: center; gap: 16px; }
  .pct { margin-left: auto; color: var(--ink-2); }
  .bar { margin-left: 42px; height: 8px; border-radius: 4px; background: var(--raised); display: block; overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: 4px; background: var(--accent); }
  .detail { margin-left: 42px; font-size: 14px; color: var(--ink-4); }
  @media (prefers-reduced-motion: no-preference) {
    .spin { display: flex; animation: spin 900ms linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
  }
</style>
