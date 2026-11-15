<script lang="ts" module>
  let seq = 0;
</script>

<script lang="ts">
  import type { Art } from '../types';

  // Jaquette illustrée (motifs originaux, aucune image officielle) : un dégradé vif et une scène simple.
  // Deux plans pour la profondeur : `.far` (décor lointain) et `.near` (premier plan). Au focus de la
  // tuile, les plans glissent un peu, à des vitesses différentes (parallaxe, cf. Tile.svelte).
  let { art }: { art: Art } = $props();
  const id = `cv${++seq}`;
</script>

<span class="clip" aria-hidden="true"><svg viewBox="0 0 100 100" preserveAspectRatio="xMidYMid slice">
  <defs>
    <linearGradient id="{id}-bg" x1="0" y1="0" x2={art.motif === 'course' || art.motif === 'cristal' ? 1 : 0} y2="1">
      <stop offset="0" stop-color={art.from}></stop>
      <stop offset="1" stop-color={art.to}></stop>
    </linearGradient>
    <radialGradient id="{id}-halo" cx="0.72" cy="0.24" r="0.36">
      <stop offset="0" stop-color="#fffbd1" stop-opacity="0.85"></stop>
      <stop offset="1" stop-color="#fffbd1" stop-opacity="0"></stop>
    </radialGradient>
    <pattern id="{id}-chk" width="8" height="8" patternUnits="userSpaceOnUse">
      <rect width="4" height="4" fill="#fff"></rect><rect x="4" y="4" width="4" height="4" fill="#fff"></rect>
    </pattern>
    <mask id="{id}-moon"><circle cx="66" cy="30" r="17" fill="#fff"></circle><circle cx="74" cy="24" r="15" fill="#000"></circle></mask>
  </defs>
  <rect width="100" height="100" fill="url(#{id}-bg)"></rect>

  {#if art.motif === 'manoir'}
    <g class="far">
      <circle cx="72" cy="24" r="34" fill="url(#{id}-halo)"></circle>
      <circle cx="72" cy="24" r="8" fill="#fffbe0"></circle>
    </g>
    <g class="near">
      <path d="M50 104V58l13-11 13 11v-9h9v9h9v46z" fill="#063a4c" opacity="0.9"></path>
      <rect x="57" y="62" width="4" height="5" rx="1" fill="#ffe36b"></rect>
      <rect x="66" y="62" width="4" height="5" rx="1" fill="#ffe36b"></rect>
      <rect x="84" y="64" width="4" height="5" rx="1" fill="#ffe36b" opacity="0.7"></rect>
      <path d="M24 44c0-7 5-12 11-12s11 5 11 12v13l-3.7-2.6-3.6 2.6-3.7-2.6-3.7 2.6-3.6-2.6L24 57z" fill="#fff" opacity="0.92"></path>
      <circle cx="31.5" cy="43" r="1.6" fill="#0a4a5e"></circle>
      <circle cx="38.5" cy="43" r="1.6" fill="#0a4a5e"></circle>
    </g>
  {:else if art.motif === 'course'}
    <g class="far" opacity="0.28" transform="rotate(-14 50 50)">
      <rect x="-10" y="34" width="70" height="3" rx="1.5" fill="#fff"></rect>
      <rect x="20" y="42" width="90" height="2" rx="1" fill="#fff"></rect>
      <rect x="-5" y="50" width="55" height="2.5" rx="1.25" fill="#fff"></rect>
    </g>
    <g class="near">
      <g transform="rotate(-14 50 20)"><rect x="-10" y="12" width="130" height="8" fill="url(#{id}-chk)" opacity="0.85"></rect></g>
      <path d="M78 36l3.2 6.5 7.2 1-5.2 5 1.2 7.1-6.4-3.4-6.4 3.4 1.2-7.1-5.2-5 7.2-1z" fill="#fff4b0"></path>
    </g>
  {:else if art.motif === 'lune'}
    <g class="far">
      <rect width="100" height="100" fill="#ffe9a8" mask="url(#{id}-moon)"></rect>
      <circle cx="18" cy="16" r="1" fill="#fff"></circle><circle cx="40" cy="10" r="0.8" fill="#fff"></circle>
      <circle cx="88" cy="52" r="0.9" fill="#fff"></circle><circle cx="30" cy="30" r="0.7" fill="#fff"></circle>
    </g>
    <g class="near">
      <rect x="72" y="44" width="15" height="60" fill={art.ink ?? '#34138f'}></rect>
      <path d="M70 44l9.5-13 9.5 13z" fill={art.ink ?? '#34138f'}></path>
      <circle cx="79.5" cy="54" r="4.2" fill="#ffe9a8"></circle>
    </g>
  {:else if art.motif === 'prairie'}
    <g class="far">
      <circle cx="76" cy="22" r="10" fill="#fff2a0"></circle>
      <ellipse cx="30" cy="24" rx="12" ry="5" fill="#fff" opacity="0.95"></ellipse>
      <ellipse cx="38" cy="21" rx="7" ry="5" fill="#fff" opacity="0.95"></ellipse>
      <path d="M0 62c18-10 34-10 52-2s34 8 48-2v44H0z" fill="#7ee89a"></path>
    </g>
    <g class="near">
      <path d="M0 74c22-8 44-6 62 2s28 4 38 0v28H0z" fill="#45cf78"></path>
      <circle cx="70" cy="66" r="2" fill="#fff"></circle><circle cx="74" cy="69" r="1.6" fill="#ffd84d"></circle>
      <circle cx="22" cy="68" r="1.8" fill="#ff7aa8"></circle>
    </g>
  {:else if art.motif === 'cristal'}
    <g class="far">
      <path d="M0 40l30-30 10 10-30 30z" fill="#fff" opacity="0.1"></path>
      <path d="M20 100l40-40 8 8-40 40z" fill="#fff" opacity="0.08"></path>
    </g>
    <g class="near">
      <path d="M72 14l16 16-16 34-16-34z" fill="#fff" opacity="0.9"></path>
      <path d="M72 14l16 16H56z" fill="#f2fdff"></path>
      <path d="M56 30h32L72 64z" fill="#fff" opacity="0.55"></path>
    </g>
  {:else if art.motif === 'vagues'}
    <g class="far">
      <circle cx="74" cy="24" r="11" fill="#fff6c2"></circle>
      <path d="M58 58l8-26 2 26z" fill="#fff"></path><path d="M68 58l1-22 7 22z" fill="#ffe7a0"></path>
      <path d="M54 58h26l-4 6H58z" fill="#7a3b1c"></path>
    </g>
    <g class="near">
      <path d="M0 66c8-5 17-5 25 0s17 5 25 0 17-5 25 0 17 5 25 0v38H0z" fill="#fff" opacity="0.35"></path>
      <path d="M0 76c8-5 17-5 25 0s17 5 25 0 17-5 25 0 17 5 25 0v28H0z" fill="#ffffff" opacity="0.22"></path>
    </g>
  {:else if art.motif === 'montagnes'}
    <g class="far">
      <circle cx="26" cy="26" r="9" fill="#fff6c2" opacity="0.9"></circle>
      <path d="M30 74l24-38 24 38z" fill="#fff" opacity="0.28"></path>
    </g>
    <g class="near">
      <path d="M-4 104l30-46 18 26 14-18 46 38z" fill={art.ink ?? '#0f3d2e'} opacity="0.75"></path>
      <path d="M26 58l6 9-6-2-6 3z" fill="#fff" opacity="0.85"></path>
    </g>
  {:else if art.motif === 'eclair'}
    <g class="far">
      <circle cx="30" cy="30" r="22" fill="#fff" opacity="0.14"></circle>
      <circle cx="80" cy="76" r="16" fill="#fff" opacity="0.12"></circle>
    </g>
    <g class="near">
      <path d="M64 10L46 46h12l-8 32 26-42H62z" fill="#fff8c4" stroke="#ffffff" stroke-width="1.5" stroke-linejoin="round"></path>
    </g>
  {/if}
</svg></span>

<style>
  .clip { position: absolute; inset: 0; border-radius: inherit; overflow: hidden; z-index: -1; }
  svg { width: 100%; height: 100%; display: block; }
  g { transition: transform 520ms var(--ease-land); }
</style>
