<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';

  // Pictogramme nuage avec état de chargement commun à toute l'interface (en-tête, Options, Réglages) :
  // pendant le chargement, le nuage reste immobile et respire, un arc tourne autour.
  let { name, size = 22, color, busy = false }: { name: IconName; size?: number; color?: string; busy?: boolean } = $props();
</script>

<span class="cloud" class:busy style:--s="{size}px">
  <Icon {name} {size} {color} />
  {#if busy}
    <svg class="orbit" viewBox="0 0 32 32" aria-hidden="true"><circle cx="16" cy="16" r="14" fill="none" stroke={color ?? 'var(--sync)'} stroke-width="2" stroke-linecap="round" stroke-dasharray="22 66"></circle></svg>
  {/if}
</span>

<style>
  .cloud { display: inline-flex; position: relative; }
  .busy { margin: 0 3px; }
  .orbit { position: absolute; left: 50%; top: 50%; width: calc(var(--s) * 1.5); height: calc(var(--s) * 1.5); transform: translate(-50%, -50%); opacity: 0.9; }
  @media (prefers-reduced-motion: no-preference) {
    .orbit { animation: spin 1.1s cubic-bezier(.5, .15, .5, .85) infinite; }
    .busy > :global(svg:not(.orbit)) { animation: breathe 1.8s ease-in-out infinite; }
    @keyframes spin { from { transform: translate(-50%, -50%) rotate(0deg); } to { transform: translate(-50%, -50%) rotate(360deg); } }
    @keyframes breathe { 0%, 100% { opacity: 1; } 50% { opacity: 0.55; } }
  }
</style>
