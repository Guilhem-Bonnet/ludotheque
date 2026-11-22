<script lang="ts">
  // Bureau (PC, Deck) : actions de l'écran « En jeu » (menu en jeu, lot menu-jeu du 22/11).
  // Reprendre réduit la fenêtre (le jeu est juste derrière) ; Fermer le jeu demande confirmation puis
  // termine l'émulateur proprement (SIGTERM) ; la sauvegarde part ensuite comme après toute partie.
  // Le bouton Guide pressé pendant la partie (pad.rs) ramène la fenêtre et donne le focus à « Reprendre ».
  import { onMount } from 'svelte';
  import { enCours } from '../enCours.svelte';
  import { feedback } from '../feedback';

  let { titre }: { titre: string } = $props();
  let zone = $state<HTMLElement | null>(null);
  let reprendreEl = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    enCours.titre = titre;
  });

  async function reprendre() {
    feedback('launch');
    await enCours.reprendre().catch(() => feedback('err'));
  }

  function fermer() {
    feedback('ok');
    enCours.confirmation = {};
  }

  onMount(() => {
    const onAction = (ev: Event) => {
      if ((ev as CustomEvent).detail === 'x' && !enCours.confirmation && zone?.contains(document.activeElement)) fermer();
    };
    window.addEventListener('ludo:action', onAction);
    let stop: (() => void) | undefined;
    void import('@tauri-apps/api/event')
      .then(({ listen }) => listen('ludo://menu-jeu', () => reprendreEl?.focus({ preventScroll: true })))
      .then((u) => (stop = u))
      .catch(() => undefined);
    return () => {
      window.removeEventListener('ludo:action', onAction);
      stop?.();
    };
  });
</script>

<span class="acts" bind:this={zone}>
  <button class="act focusable" data-autofocus data-sound="none" bind:this={reprendreEl} onclick={() => void reprendre()}>Reprendre</button>
  <button class="act focusable" data-sound="none" onclick={fermer}>Fermer le jeu</button>
</span>

<style>
  /* Même gabarit que les boutons de l'écran de passage (Passage.svelte), cible ≥ 44 px. */
  .acts { display: flex; gap: 12px; flex-wrap: wrap; margin-left: 42px; }
  .act { all: unset; cursor: pointer; min-height: 44px; box-sizing: border-box; display: inline-flex; align-items: center; padding: 12px 24px; border-radius: 12px; font-size: 16px; font-weight: 600; background: var(--control); color: var(--ink); }
  .act:focus { box-shadow: inset 0 0 0 2px var(--accent); }
</style>
