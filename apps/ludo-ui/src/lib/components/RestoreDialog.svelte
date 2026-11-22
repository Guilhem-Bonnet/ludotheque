<script lang="ts">
  import { onMount } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import { api } from '../api';
  import { feedback } from '../feedback';
  import { reducedMotion } from '../motion';
  import { syncRhythm, play as playSound } from '../sound';
  import type { Game, SaveRevision } from '../types';

  // Restauration d'une sauvegarde : fenêtre de chargement (on ne joue pas pendant qu'on remplace la
  // sauvegarde). Étapes cochées une à une, barre continue, sons de synchronisation, accord à la fin.
  let { game, rev, profile, ondone, onfail }: { game: Game; rev: SaveRevision; profile: string; ondone: () => void; onfail: () => void } = $props();

  let step = $state(0);
  let failed = $state<string | null>(null);
  let progress = $state(0);
  const steps = ['Téléchargement de la version', 'Remplacement de la sauvegarde locale', 'Sauvegarde restaurée'];

  onMount(() => {
    feedback('syncStart');
    const stopRhythm = syncRhythm();
    api.restore(profile, game.id, rev.rev, (s, p) => { step = s; progress = s === 0 ? p : 1; })
      .finally(stopRhythm)
      .then(() => {
        step = 2;
        feedback('saved');
        setTimeout(ondone, 900);
      })
      .catch((e) => {
        // Échec (conflit : la sauvegarde a changé ailleurs entre-temps, ou serveur injoignable) : rien n'est remplacé.
        // Le texte brut de l'erreur (souvent technique, en anglais) n'est pas affiché : seule sa nature.
        const msg = String(e);
        failed = /conflit|conflict/i.test(msg)
          ? 'La sauvegarde a changé sur un autre appareil'
          : /injoignable|connexion|connection|timed out|délai/i.test(msg)
            ? 'Serveur injoignable'
            : "Erreur sur l'appareil";
        feedback('err');
        requestAnimationFrame(() => document.querySelector<HTMLElement>('.card .close')?.focus());
      });
    // B pendant la restauration : rien (l'opération doit aller au bout pour ne rien perdre).
    const block = () => (failed ? onfail() : feedback('err'));
    window.addEventListener('ludo:modal-back', block);
    return () => window.removeEventListener('ludo:modal-back', block);
  });

  const d = (ms: number) => (reducedMotion() ? 0 : ms);
</script>

<div class="backdrop" transition:fade={{ duration: d(200) }} aria-hidden="true"></div>
<div class="card" data-modal role="dialog" aria-label="Restauration de la sauvegarde" aria-live="polite"
  in:scale={{ start: 0.94, duration: d(320), easing: cubicOut }} out:fade={{ duration: d(180) }}>
  <h2>Restauration de la sauvegarde</h2>
  <p class="sub">{game.title} · {rev.device} · {rev.when} · {rev.playtime}</p>
  <ol>
    {#each steps as label, i (label)}
      <li class:done={i < step || step === 2} class:now={i === step && step < 2}>
        <span class="mark">
          {#if i < step || step === 2}<span class="check"><Icon name="check" size={14} color="#0b0f14" /></span>
          {:else if i === step && failed}<span class="xmark">!</span>
          {:else if i === step}<span class="spinner"></span>
          {:else}<span class="dot"></span>{/if}
        </span>
        {label}
      </li>
    {/each}
  </ol>
  <div class="bar" class:failbar={failed} role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(progress * 100)}>
    <!-- App réelle : la restauration ne remonte pas d'avancement ; barre en attente plutôt que figée à 0. -->
    <span class:indet={progress === 0 && step === 0 && !failed} style:width="{progress === 0 && step === 0 && !failed ? 30 : progress * 100}%"></span>
  </div>
  {#if failed}
    <p class="fail">Restauration impossible · {failed}. Aucune sauvegarde remplacée.</p>
    <button class="close focusable" onclick={onfail}>Fermer</button>
  {:else}
    <p class="note">{step === 2 ? 'Version actuelle sur tous les appareils' : "L'ancienne version reste dans l'historique"}</p>
  {/if}
</div>

<style>
  .backdrop { position: absolute; inset: 0; z-index: 10; background: rgba(5, 8, 12, 0.6); backdrop-filter: blur(4px); }
  .card {
    /* Centrage par `translate` (propriété distincte de `transform`, que la transition `scale` occupe) :
       la carte reste centrée quelle que soit sa hauteur (message d'échec, texte long, petit écran). */
    position: absolute; left: 50%; top: 50%; translate: -50% -50%; width: min(520px, calc(100vw - 48px)); z-index: 11; box-sizing: border-box;
    background: #161a22; border-radius: 20px; padding: 30px 32px; box-shadow: 0 30px 80px rgba(0, 0, 0, 0.55);
    display: flex; flex-direction: column; gap: 14px;
  }
  h2 { margin: 0; font-size: 22px; font-weight: 600; }
  .sub { margin: 0; font-size: 14px; color: var(--ink-3); }
  ol { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
  li { display: flex; align-items: center; gap: 12px; font-size: 16px; color: var(--ink-4); transition: color var(--t-std) ease-out; }
  li.now { color: var(--ink); font-weight: 500; }
  li.done { color: var(--ink-2); }
  .mark { width: 22px; height: 22px; display: inline-flex; align-items: center; justify-content: center; }
  .check { width: 22px; height: 22px; border-radius: 50%; background: var(--ok); display: inline-flex; align-items: center; justify-content: center; animation: pop 320ms var(--ease-pop); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: rgba(255, 255, 255, 0.25); }
  .spinner { width: 18px; height: 18px; border-radius: 50%; border: 2px solid rgba(255, 255, 255, 0.18); border-top-color: var(--accent); animation: spin 0.9s linear infinite; }
  .bar { height: 6px; border-radius: 3px; background: rgba(255, 255, 255, 0.1); overflow: hidden; margin-top: 6px; }
  .bar span { display: block; height: 100%; border-radius: 3px; background: var(--accent); transition: width 160ms linear; }
  .xmark { width: 22px; height: 22px; border-radius: 50%; background: #ff6b6b; color: #0b0f14; font-weight: 700; font-size: 14px; display: inline-flex; align-items: center; justify-content: center; }
  .failbar span { background: #ff6b6b; }
  .bar span.indet { animation: indet 1.2s ease-in-out infinite; }
  @keyframes indet { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  .fail { margin: 0; font-size: 14px; color: #ff9b9b; }
  .close { all: unset; cursor: pointer; align-self: flex-end; padding: 10px 18px; border-radius: 10px; background: var(--raised); font-size: 15px; }
  .close.focusable:focus { box-shadow: inset 0 0 0 2px var(--accent); }
  .note { margin: 0; font-size: 13px; color: var(--ink-4); }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes pop { from { transform: scale(0.5); } to { transform: none; } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation-duration: 2.4s; } .check { animation: none; } .bar span.indet { animation-duration: 3s; } }
</style>
