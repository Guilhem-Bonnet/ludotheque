<script lang="ts">
  // Indicateur de batterie d'angle : pictogramme + pourcentage, pastille translucide.
  // Rien sur un PC fixe. Sous 15 % hors charge : teinte ambre douce (alerte discrète, sans clignoter).
  // En charge : éclair dans la pile, remplissage vert. Aucune animation infinie (charte) : seul le
  // remplissage glisse quand le niveau change.
  import { onMount } from 'svelte';
  import { batterie, SEUIL_FAIBLE, suivreBatterie } from '../batterie.svelte';

  let { compact = false }: { compact?: boolean } = $props();

  onMount(suivreBatterie);

  const faible = $derived(!batterie.enCharge && batterie.niveau <= SEUIL_FAIBLE);
  const libelle = $derived(`Batterie ${batterie.niveau} %${batterie.enCharge ? ', en charge' : faible ? ', faible' : ''}`);
  // Remplissage intérieur : 16 unités de large au maximum, jamais moins de 1,5 pour rester visible.
  const largeur = $derived(Math.max(1.5, (batterie.niveau / 100) * 16));
</script>

{#if batterie.connue}
  <span class="batterie" class:faible class:charge={batterie.enCharge} class:compact role="img" aria-label={libelle} title={libelle}>
    <svg viewBox="0 0 24 12" width="24" height="12" aria-hidden="true">
      <rect class="coque" x="0.75" y="0.75" width="19.5" height="10.5" rx="3" />
      <rect class="borne" x="21.25" y="3.75" width="2" height="4.5" rx="1" />
      <rect class="niveau" x="2.5" y="2.5" width={largeur} height="7" rx="1.6" />
      {#if batterie.enCharge}
        <path class="eclair" d="M11.6 1.6 7.6 6.6h3l-1.2 3.8 4-5h-3z" />
      {/if}
    </svg>
    {#if !compact}<span class="pct">{batterie.niveau}&#8239;%</span>{/if}
  </span>
{/if}

<style>
  .batterie {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.07);
    color: var(--ink-2, #c9cdd6);
    font-size: 14px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    white-space: nowrap;
    user-select: none;
  }
  .batterie.compact {
    padding: 4px 8px;
  }
  .coque {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    opacity: 0.75;
  }
  .borne {
    fill: currentColor;
    opacity: 0.6;
  }
  .niveau {
    fill: currentColor;
    transition: width 600ms var(--ease-calm, ease);
  }
  .charge .niveau {
    fill: var(--ok, #5fd39a);
  }
  .eclair {
    fill: var(--ground, #15171c);
    stroke: var(--ground, #15171c);
    stroke-width: 0.6;
    stroke-linejoin: round;
  }
  .faible {
    color: var(--pending, #f2b84b);
    background: rgba(242, 184, 75, 0.12);
  }
  @media (prefers-reduced-motion: reduce) {
    .niveau {
      transition: none;
    }
  }
</style>
