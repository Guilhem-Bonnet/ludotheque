<script lang="ts">
  import type { AvatarKind } from '../types';

  // Personnages des images de profil (dessins originaux). Le fond est porté par Avatar.svelte.
  let { kind }: { kind: AvatarKind } = $props();
</script>

<svg viewBox="0 0 100 100" aria-hidden="true">
  {#if kind === 'renard'}
    <path d="M24 30l14 14-14 8z" fill="#c2410c"></path>
    <path d="M76 30L62 44l14 8z" fill="#c2410c"></path>
    <path d="M22 42c8-6 18-8 28-8s20 2 28 8L50 86z" fill="#ff7a1a"></path>
    <path d="M34 58c6 4 10 10 16 28 6-18 10-24 16-28-6-2-11-2-16 2-5-4-10-4-16-2z" fill="#fff4e6"></path>
    <circle class="eye" cx="40" cy="54" r="3.2" fill="#2a1a10"></circle>
    <circle class="eye" cx="60" cy="54" r="3.2" fill="#2a1a10"></circle>
    <circle cx="50" cy="80" r="3" fill="#2a1a10"></circle>
  {:else if kind === 'champi'}
    <rect x="38" y="52" width="24" height="30" rx="10" fill="#fff4dc"></rect>
    <circle class="eye" cx="44.5" cy="64" r="2.6" fill="#3a2a20"></circle>
    <circle class="eye" cx="55.5" cy="64" r="2.6" fill="#3a2a20"></circle>
    <path d="M16 56c0-20 15-34 34-34s34 14 34 34z" fill="#ff4d5e"></path>
    <circle cx="34" cy="40" r="6" fill="#fff"></circle>
    <circle cx="58" cy="34" r="5" fill="#fff"></circle>
    <circle cx="70" cy="48" r="4" fill="#fff"></circle>
  {:else if kind === 'chat'}
    <path d="M26 30l10 18-14 4z" fill="#3b2a5a"></path>
    <path d="M74 30L64 48l14 4z" fill="#3b2a5a"></path>
    <ellipse cx="50" cy="60" rx="30" ry="25" fill="#3b2a5a"></ellipse>
    <ellipse class="eye" cx="39" cy="58" rx="5" ry="6" fill="#ffe066"></ellipse>
    <ellipse class="eye" cx="61" cy="58" rx="5" ry="6" fill="#ffe066"></ellipse>
    <path d="M47 68h6l-3 3z" fill="#ff8fb1"></path>
  {:else if kind === 'etoile'}
    <path d="M50 16l9.5 19.5 21.5 3-15.5 15 3.7 21.3L50 64.8 30.8 74.8l3.7-21.3L19 38.5l21.5-3z" fill="#ffe066" stroke="#ffcf33" stroke-width="2" stroke-linejoin="round"></path>
    <circle class="eye" cx="44" cy="46" r="2.8" fill="#3a2a10"></circle>
    <circle class="eye" cx="56" cy="46" r="2.8" fill="#3a2a10"></circle>
    <path d="M45 54c3 2.5 7 2.5 10 0" stroke="#3a2a10" stroke-width="2" fill="none" stroke-linecap="round"></path>
  {:else if kind === 'planete'}
    <circle cx="50" cy="52" r="20" fill="#ffe1ef"></circle>
    <ellipse cx="50" cy="54" rx="36" ry="9" fill="none" stroke="#fff" stroke-width="4" transform="rotate(-14 50 54)"></ellipse>
    <circle cx="43" cy="46" r="4" fill="#ffb3d1"></circle>
    <circle cx="57" cy="58" r="3" fill="#ffb3d1"></circle>
  {:else}
    <path d="M22 50c10-16 34-20 50 0-16 20-40 16-50 0z" fill="#ff7b54"></path>
    <path d="M70 50l14-12v24z" fill="#ff7b54"></path>
    <circle cx="36" cy="47" r="3.4" fill="#fff"></circle>
    <circle class="eye" cx="36.6" cy="47.4" r="1.8" fill="#1a2a3a"></circle>
    <path d="M48 40c4 6 4 14 0 20" stroke="#ffb199" stroke-width="2.4" fill="none" stroke-linecap="round"></path>
  {/if}
</svg>

<style>
  svg { width: 100%; height: 100%; display: block; }
  /* Clignement des yeux, rare et rapide : le personnage est « vivant » sans distraire. */
  .eye { transform-box: fill-box; transform-origin: center; animation: blink 5.5s infinite; }
  @keyframes blink { 0%, 94%, 100% { transform: scaleY(1); } 96% { transform: scaleY(0.1); } }
  @media (prefers-reduced-motion: reduce) { .eye { animation: none; } }
</style>
