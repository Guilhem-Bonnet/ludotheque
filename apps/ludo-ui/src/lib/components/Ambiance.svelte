<script lang="ts">
  import { fade } from 'svelte/transition';
  import { reducedMotion } from '../motion';
  import type { Art } from '../types';

  // Fond « ambiance » (direction C, validée le 15/11) : le fond prend la couleur de la jaquette du jeu
  // en cours, avec une source de lumière placée à `x` % de la largeur (au-dessus de la tuile).
  // Changer de jeu fait un fondu enchaîné de 420 ms. La même ambiance suit le jeu de l'accueil à
  // l'écran de passage et à la fiche, pour que la tuile reste dans « son » monde pendant le vol.
  let { art, x = 18, key }: { art?: Art; x?: number; key?: string } = $props();

  const wash = $derived(
    art
      ? `radial-gradient(120% 90% at ${x}% 16%, ${art.from} 0%, color-mix(in srgb, ${art.to} 55%, #0b0f14) 46%, #0b0f14 100%)`
      : null,
  );
  const d = () => (reducedMotion() ? 0 : 420);
</script>

<div class="ambiance" aria-hidden="true">
  {#if wash}
    {#key key ?? art?.motif}
      <div class="wash" style:background={wash} in:fade={{ duration: d() }} out:fade={{ duration: d() }}></div>
    {/key}
  {/if}
</div>

<style>
  .ambiance { position: absolute; inset: 0; z-index: -1; background: #0b0f14; pointer-events: none; }
  .wash { position: absolute; inset: 0; transition: background 520ms var(--ease-land); }
</style>
