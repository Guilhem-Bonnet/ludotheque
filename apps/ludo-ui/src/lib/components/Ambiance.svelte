<script lang="ts">
  import { fade } from 'svelte/transition';
  import { reducedMotion } from '../motion';
  import type { Art } from '../types';

  // Fond « ambiance » (direction C, validée le 15/11) : le fond prend la couleur de la jaquette du jeu
  // en cours, avec une source de lumière placée à `x` % de la largeur (au-dessus de la tuile).
  // Si le jeu a une vraie image, elle sert de fond : agrandie, très floutée et assombrie, avec un
  // voile qui garde le texte lisible (`dim` : plus fort sur les écrans chargés de texte, comme la fiche).
  // Changer de jeu fait un fondu enchaîné ; la même ambiance suit le jeu de l'accueil à l'écran de
  // passage et à la fiche, pour que la tuile reste dans « son » monde pendant le vol.
  // `background` : image grand format du jeu (bannière), à peine floutée pour rester reconnaissable ;
  // à défaut, la jaquette, plus floutée (un carré agrandi en plein écran ne tient pas net).
  let { art, cover, background, x = 18, key, dim = false }: { art?: Art; cover?: string; background?: string; x?: number; key?: string; dim?: boolean } = $props();

  const wash = $derived(
    art
      ? `radial-gradient(120% 90% at ${x}% 16%, ${art.from} 0%, color-mix(in srgb, ${art.to} 55%, #0b0f14) 46%, #0b0f14 100%)`
      : null,
  );
  // Une image qui ne charge pas retombe sur la couleur de la jaquette.
  let failed = $state('');
  const wide = $derived(!!background && failed !== background);
  const photo = $derived(wide ? background : cover && failed !== cover ? cover : undefined);
  // Fond de petite taille (captures 3DS/DS en 400×240) : un peu plus flouté pour ne pas pixeliser.
  let small = $state(false);
  const d = () => (reducedMotion() ? 0 : 520);
</script>

<div class="ambiance" aria-hidden="true">
  {#key key ?? photo ?? art?.motif}
    <div class="layer" in:fade={{ duration: d() }} out:fade={{ duration: d() }}>
      {#if wash}<div class="wash" style:background={wash}></div>{/if}
      {#if photo}
        <img class="photo" class:wide class:small src={photo} onload={(e) => (small = (e.currentTarget as HTMLImageElement).naturalWidth < 900)} alt="" draggable="false" onerror={() => (failed = photo ?? '')} />
      {/if}
    </div>
  {/key}
  <div class="veil" class:photo={!!photo} class:dim style:--x="{x}%"></div>
</div>

<style>
  .ambiance { position: absolute; inset: 0; z-index: -1; background: #0b0f14; pointer-events: none; overflow: hidden; }
  .layer, .wash { position: absolute; inset: 0; }
  .wash { transition: background 520ms var(--ease-land); }
  /* Image du jeu : agrandie pour que le flou ne laisse pas de bord clair, saturée pour garder la couleur. */
  .photo {
    position: absolute; inset: -8%; width: 116%; height: 116%; object-fit: cover;
    filter: blur(18px) saturate(1.25) brightness(0.6); transform: scale(1.05);
  }
  .photo.wide { inset: -3%; width: 106%; height: 106%; filter: blur(4px) saturate(1.1) brightness(0.62); transform: scale(1.02); }
  .photo.wide.small { filter: blur(10px) saturate(1.15) brightness(0.6);
  }
  @media (prefers-reduced-motion: no-preference) {
    .photo { animation: drift 24s ease-in-out infinite alternate; }
    @keyframes drift { from { transform: scale(1.04) translate(0, 0); } to { transform: scale(1.09) translate(-1%, 0.6%); } }
  }
  /* Voile : un halo lumineux au-dessus de la tuile, le bas assombri pour les raccourcis et les textes. */
  .veil { position: absolute; inset: 0; transition: background 520ms var(--ease-land); }
  .veil.photo {
    background:
      radial-gradient(70% 60% at var(--x) 30%, rgba(255, 255, 255, 0.07), transparent 70%),
      linear-gradient(to bottom, rgba(8, 10, 14, 0.25) 0%, rgba(8, 10, 14, 0.35) 45%, rgba(8, 10, 14, 0.8) 100%);
  }
  .veil.photo.dim { background: linear-gradient(to right, rgba(8, 10, 14, 0.35) 0%, rgba(8, 10, 14, 0.6) 40%, rgba(8, 10, 14, 0.72) 100%); }
  .veil.dim:not(.photo) { background: rgba(8, 10, 14, 0.35); }
</style>
