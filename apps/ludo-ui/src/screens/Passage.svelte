<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { isDemo } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { replay } from '../lib/motion';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { Preparation } from '../lib/types';

  // Écran de passage : n'apparaît que s'il y a quelque chose à faire (téléchargement, synchro).
  // Récit : le jeu arrive (élément partagé depuis la tuile), chaque étape se coche en se dessinant,
  // la barre avance en continu, puis la montée douce du lancement accompagne un dernier « pop ».
  let { preparation }: { preparation: Preparation } = $props();

  // Copie locale volontaire : l'écran fait avancer ses propres étapes.
  // svelte-ignore state_referenced_locally
  let steps = $state(preparation.steps.map((s) => ({ ...s })));
  let launching = $state(false);
  let art: HTMLElement | undefined = $state();

  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

  /** Avance une barre de progression jusqu'à 100 % en ~1,8 s (démonstration). */
  function fill(i: number) {
    return new Promise<void>((done) => {
      const start = performance.now();
      const from = steps[i].progress ?? 0;
      const tickFn = (t: number) => {
        const k = Math.min(1, (t - start) / 1800);
        steps[i].progress = Math.round(from + (100 - from) * (1 - Math.pow(1 - k, 2)));
        if (k < 1) requestAnimationFrame(tickFn);
        else done();
      };
      requestAnimationFrame(tickFn);
    });
  }

  async function runDemo() {
    for (let i = 0; i < steps.length; i++) {
      const s = steps[i];
      if (s.state === 'fait') continue;
      s.state = 'en_cours';
      if (s.progress !== undefined) {
        await fill(i);
        s.detail = '65,8 Mo · terminé';
        s.label = 'Téléchargement terminé';
        feedback('downloaded');
      } else if (i < steps.length - 1) {
        await wait(700);
        if (s.label.startsWith('Sauvegarde')) s.label = 'Sauvegarde : récupérée';
      }
      if (i === steps.length - 1) {
        launching = true;
        s.label = 'Lancement…';
        feedback('launch');
        replay(art, 'pop', 340);
        await wait(1500);
        s.state = 'fait';
        await wait(500);
        router.back();
        return;
      }
      s.state = 'fait';
      await wait(250);
    }
  }

  onMount(() => {
    focusFirst();
    feedback('loading');
    if (isDemo) void runDemo();
  });
</script>

<div class="screen" class:launching>
  <main>
    <div class="art" bind:this={art}><Tile game={preparation.game} size={340} big still hero /></div>
    <div class="side">
      <div class="head rise" style:--i={1}>
        <h1>{launching ? 'Lancement' : 'Préparation'}</h1>
        <span>{preparation.game.title} · {preparation.game.emulator}</span>
      </div>
      <ol>
        {#each steps as step, i (i)}
          <li class="{step.state} rise" style:--i={i + 2}>
            <span class="line">
              {#if step.state === 'fait'}
                <span class="draw" style:--len="16"><Icon name="check-circle" size={26} color="var(--ok)" /></span>
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
  .art { pointer-events: none; transition: transform var(--t-slow) var(--ease-land), filter var(--t-slow) ease-out; }
  .launching .art { transform: translateY(-6px); filter: brightness(1.08); }
  .side { flex-grow: 1; display: flex; flex-direction: column; gap: 36px; }
  .head { display: flex; flex-direction: column; gap: 10px; }
  h1 { margin: 0; font-size: 36px; font-weight: 600; letter-spacing: -0.01em; }
  .head span { font-size: 17px; color: var(--ink-3); }
  ol { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 22px; font-size: 18px; }
  li { display: flex; flex-direction: column; gap: 12px; color: var(--ink-2); transition: color var(--t-std) ease-out; }
  li.a_venir { color: var(--ink-4); }
  li.en_cours .label { font-weight: 500; color: var(--ink); }
  .line { display: flex; align-items: center; gap: 16px; }
  .line > span:first-child { display: flex; }
  .pct { margin-left: auto; color: var(--ink-2); min-width: 4ch; text-align: right; }
  .bar { margin-left: 42px; height: 8px; border-radius: 4px; background: var(--raised); display: block; overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: 4px; background: var(--accent); transition: width 120ms linear; }
  .detail { margin-left: 42px; font-size: 14px; color: var(--ink-4); }
  .spin { animation: spin 900ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
