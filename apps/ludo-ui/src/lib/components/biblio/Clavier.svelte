<script lang="ts">
  import { onMount } from 'svelte';
  import { fly } from 'svelte/transition';
  import ButtonHints from '../ButtonHints.svelte';
  import { feedback } from '../../feedback';
  import { reducedMotion } from '../../motion';

  // Clavier à l'écran pour chercher à la manette (Deck, PC au salon, Thor) : la saisie se fait sans
  // dépendre du clavier de Steam ou d'Android. Les résultats se mettent à jour au-dessus, à chaque
  // lettre. A tape la touche choisie, X efface, Y met une espace, B efface puis ferme.
  // Fenêtre modale (`data-modal`) : la croix reste dans le clavier.
  let { value = $bindable(''), oninput, onclose }: { value?: string; oninput?: () => void; onclose: () => void } = $props();

  const ROWS = ['1234567890', 'azertyuiop', 'qsdfghjklm', "wxcvbn'-&."].map((r) => r.split(''));

  let first: HTMLButtonElement | undefined = $state();

  function type(ch: string) {
    value += ch;
    oninput?.();
  }
  function erase() {
    if (!value) { feedback('err'); return; }
    value = value.slice(0, -1);
    oninput?.();
    feedback('back');
  }
  function space() {
    if (value && !value.endsWith(' ')) { value += ' '; oninput?.(); }
    feedback('move');
  }

  onMount(() => {
    first?.focus({ preventScroll: true });
    // B (relayé par App quand une fenêtre est ouverte) : efface une lettre, puis ferme une fois vide.
    const onBack = () => (value ? erase() : onclose());
    const onAction = (ev: Event) => {
      const d = (ev as CustomEvent).detail;
      if (d === 'x') erase();
      else if (d === 'y') space();
    };
    window.addEventListener('ludo:modal-back', onBack);
    window.addEventListener('ludo:action', onAction);
    return () => {
      window.removeEventListener('ludo:modal-back', onBack);
      window.removeEventListener('ludo:action', onAction);
    };
  });

  const slide = () => (reducedMotion() ? { duration: 0 } : { y: 60, duration: 260, opacity: 0 });
</script>

<section class="clavier" data-modal aria-label="Clavier" transition:fly={slide()}>
  <div class="keys">
    {#each ROWS as row, r (r)}
      <div class="row">
        {#each row as ch, i (ch)}
          {#if r === 1 && i === 0}
            <button class="key focusable" data-sound="move" bind:this={first} onclick={() => type(ch)}>{ch}</button>
          {:else}
            <button class="key focusable" data-sound="move" onclick={() => type(ch)}>{ch}</button>
          {/if}
        {/each}
      </div>
    {/each}
    <div class="row">
      <button class="key wide focusable" data-sound="none" onclick={erase}>Effacer</button>
      <button class="key space focusable" data-sound="none" onclick={space}>Espace</button>
      <button class="key wide ok focusable" onclick={onclose}>OK</button>
    </div>
  </div>
  <ButtonHints hints={[{ key: 'A', label: 'Taper' }, { key: 'X', label: 'Effacer' }, { key: 'Y', label: 'Espace' }, { key: 'B', label: value ? 'Effacer' : 'Fermer' }]} />
</section>

<style>
  .clavier {
    position: absolute; left: 0; right: 0; bottom: 0; z-index: 20;
    background: rgba(31, 34, 42, 0.96); /* --surface ; pas de color-mix() : Chromium 109 (Thor) */
    border-top: 1px solid var(--line);
    box-shadow: 0 -18px 48px rgba(0, 0, 0, 0.35);
    display: flex; flex-direction: column;
  }
  .keys { padding: 18px 36px 12px; display: flex; flex-direction: column; gap: 10px; align-items: center; }
  .row { display: flex; gap: 10px; }
  .key {
    all: unset; cursor: pointer; box-sizing: border-box;
    width: 64px; height: 52px; border-radius: 12px; background: var(--raised);
    display: flex; align-items: center; justify-content: center;
    font-size: 21px; color: var(--ink);
    transition: background-color var(--t-quick) ease-out, color var(--t-quick) ease-out;
  }
  .key:focus-visible, .key:focus { background: var(--ink); color: var(--ground); font-weight: 600; }
  .key.wide { width: 138px; font-size: 17px; }
  .key.space { width: 362px; font-size: 17px; }
  .key.ok { background: var(--accent); color: var(--ground); font-weight: 600; }
  .key.ok:focus { background: var(--ink); }
  @media (hover: hover) { .key:hover { background: var(--hint); } }
</style>
