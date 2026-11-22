<script lang="ts">
  import { fade } from 'svelte/transition';
  import { reducedMotion, DUR } from '../motion';
  import { soften, SMALL_BLUR } from '../soften';
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

  // Mélange précalculé (`p` de `a`, le reste de `b`) : la WebView de la Thor (Chromium 109) ignore
  // `color-mix()` (Chrome 111+), ce qui supprimait toute la déclaration du fond (DEMANDE thor2 22/11 08:51).
  function mix(a: string, b: string, p: number) {
    const rgb = (h: string) => {
      const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(h.trim());
      if (!m) return null;
      const v = m[1].length === 3 ? m[1].replace(/./g, (c) => c + c) : m[1];
      return [0, 2, 4].map((i) => parseInt(v.slice(i, i + 2), 16));
    };
    const x = rgb(a), y = rgb(b);
    if (!x || !y) return a;
    return `rgb(${x.map((c, i) => Math.round(c * p + y[i] * (1 - p))).join(', ')})`;
  }
  const wash = $derived(
    art
      ? `radial-gradient(120% 90% at ${x}% 16%, ${art.from} 0%, ${mix(art.to, '#0b0f14', 0.55)} 46%, #0b0f14 100%)`
      : null,
  );
  // Une image qui ne charge pas retombe sur la couleur de la jaquette.
  let failed = $state('');
  const wide = $derived(!!background && failed !== background);
  const photo = $derived(wide ? background : cover && failed !== cover ? cover : undefined);
  const d = () => (reducedMotion() ? 0 : DUR.world);

  // Flou à petite taille (soften.ts) : l'image est réduite une fois par image et par taille de fond, le flou
  // CSS ne porte que sur cette petite surface, un `transform` l'étire. Une repeinture coûte alors autant
  // qu'une image ordinaire. Redessiné au redimensionnement (ResizeObserver), jamais à intervalle.
  function paint(el: HTMLCanvasElement, first: string) {
    const scaler = el.parentElement!;
    const frame = scaler.parentElement!;
    const box = frame.closest('.ambiance')!;
    let img = new Image();
    const draw = () => {
      if (!img.naturalWidth) return;
      const r = box.getBoundingClientRect();
      // Fond de petite taille (captures 3DS/DS en 400×240) : un peu plus flouté pour ne pas pixeliser.
      const small = img.naturalWidth < 900;
      const [blur, sat, light] = wide ? (small ? [10, 1.15, 0.6] : [4, 1.1, 0.62]) : [18, 1.25, 0.6];
      const g = soften(img, el, r.width, r.height, blur);
      if (!g) return;
      el.style.filter = `blur(${SMALL_BLUR}px) saturate(${sat}) brightness(${light})`;
      scaler.style.transform = `translate(${g.offset}px, ${g.offset}px) scale(${g.scale})`;
      frame.dataset.ready = '';
    };
    const drop = () => { img.onload = img.onerror = null; };
    // Nouvelle source sans recréer le canvas (repli fond → jaquette après une erreur, image changée
    // sous la même clé) : on recharge, l'ancienne image est débranchée.
    const load = (src: string) => {
      drop();
      img = new Image();
      img.decoding = 'async';
      img.onload = draw;
      img.onerror = () => (failed = src);
      img.src = src;
    };
    load(first);
    const ro = new ResizeObserver(draw);
    ro.observe(box);
    return { update: load, destroy: () => { drop(); ro.disconnect(); } };
  }
</script>

<div class="ambiance" aria-hidden="true">
  {#key key ?? photo ?? art?.motif}
    <!-- Fondu enchaîné sans creux : l'ancienne couche reste opaque sous la nouvelle (insérée après elle)
         pendant tout son fondu d'entrée, puis disparaît d'un coup. -->
    <div class="layer" in:fade={{ duration: d() }} out:fade={{ delay: d(), duration: 0 }}>
      {#if wash}<div class="wash" style:background={wash}></div>{/if}
      {#if photo}
        <div class="photo"><div class="scaler"><canvas use:paint={photo}></canvas></div></div>
      {/if}
    </div>
  {/key}
  <div class="veil" class:on-photo={!!photo} class:dim style:--x="{x}%"></div>
</div>

<style>
  .ambiance { position: absolute; inset: 0; z-index: -1; background: #0b0f14; pointer-events: none; overflow: hidden; }
  .layer, .wash { position: absolute; inset: 0; }
  .wash { transition: background var(--t-world) var(--ease-land); }
  /* Image du jeu : petite surface floutée (filtre posé par paint()), étirée par `.scaler` depuis le coin. */
  .photo { position: absolute; inset: 0; opacity: 0; transition: opacity var(--t-std) ease-out; }
  .photo:global([data-ready]) { opacity: 1; }
  .scaler { position: absolute; left: 0; top: 0; transform-origin: 0 0; }
  canvas { display: block; }
  /* Le monde se pose : à l'arrivée d'une image, léger recul de 6 % à 0 en 1,6 s, une seule fois.
     (Avant : dérive infinie de 24 s, qui obligeait à repeindre le fond flouté à chaque image.) */
  @media (prefers-reduced-motion: no-preference) {
    .photo:global([data-ready]) { animation: settle var(--t-world-settle) var(--ease-land) both; }
    @keyframes settle { from { transform: scale(1.06); } to { transform: none; } }
  }
  /* Voile : un halo lumineux au-dessus de la tuile, le bas assombri pour les raccourcis et les textes. */
  .veil { position: absolute; inset: 0; transition: background var(--t-world) var(--ease-land); }
  .veil.on-photo {
    background:
      radial-gradient(70% 60% at var(--x) 30%, rgba(255, 255, 255, 0.07), transparent 70%),
      linear-gradient(to bottom, rgba(8, 10, 14, 0.25) 0%, rgba(8, 10, 14, 0.35) 45%, rgba(8, 10, 14, 0.8) 100%);
  }
  .veil.on-photo.dim { background: linear-gradient(to right, rgba(8, 10, 14, 0.35) 0%, rgba(8, 10, 14, 0.6) 40%, rgba(8, 10, 14, 0.72) 100%); }
  .veil.dim:not(.on-photo) { background: rgba(8, 10, 14, 0.35); }
</style>
