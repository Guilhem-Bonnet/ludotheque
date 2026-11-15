<script lang="ts">
  // Jeu d'icônes de la Ludothèque : traits de 1,8 à 2,2 px, coins arrondis, viewBox 24.
  export type IconName =
    | 'cloud-ok' | 'cloud-pending' | 'cloud-sync' | 'cloud-off' | 'conflict'
    | 'update' | 'download' | 'grid' | 'people' | 'settings' | 'battery'
    | 'deck' | 'pc' | 'thor' | 'tv' | 'check-circle' | 'spinner' | 'circle'
    | 'play' | 'history' | 'trash' | 'pencil' | 'check' | 'volume' | 'rumble' | 'pin';

  let { name, size = 24, color = 'currentColor', label }: { name: IconName; size?: number; color?: string; label?: string } = $props();
  const stroke = $derived(['cloud-ok', 'cloud-pending', 'conflict', 'check-circle', 'spinner'].includes(name) ? 2.1 : 1.8);
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke={color}
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  {#if name === 'cloud-ok'}
    <path d="M7 18h10a4 4 0 0 0 .6-7.96A6 6 0 0 0 6.1 9.5 4.25 4.25 0 0 0 7 18z" /><path d="m9.5 13.5 2 2 3.5-3.5" />
  {:else if name === 'cloud-pending'}
    <path d="M7 18h10a4 4 0 0 0 .6-7.96A6 6 0 0 0 6.1 9.5 4.25 4.25 0 0 0 7 18z" stroke-dasharray="2.5 2.5" /><path d="M12 16v-5m-2.2 2.2L12 11l2.2 2.2" />
  {:else if name === 'cloud-sync'}
    <path d="M7 18h10a4 4 0 0 0 .6-7.96A6 6 0 0 0 6.1 9.5 4.25 4.25 0 0 0 7 18z" /><path d="M9.5 13a2.5 2.5 0 0 1 4.3-1.7M14.5 13a2.5 2.5 0 0 1-4.3 1.7" />
  {:else if name === 'cloud-off'}
    <path d="M7 18h10a4 4 0 0 0 .6-7.96A6 6 0 0 0 6.1 9.5 4.25 4.25 0 0 0 7 18z" /><path d="M4 4l16 16" />
  {:else if name === 'conflict'}
    <path d="M8 4h7l4 4v9a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z" /><path d="M12.5 8.5v4" /><circle cx="12.5" cy="15.5" r="0.6" fill={color} />
  {:else if name === 'update'}
    <path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" />
  {:else if name === 'download'}
    <path d="M12 4v11m-4.5-4.5L12 15l4.5-4.5" /><path d="M5 19h14" />
  {:else if name === 'grid'}
    <rect x="3.5" y="3.5" width="7" height="7" rx="1.6" /><rect x="13.5" y="3.5" width="7" height="7" rx="1.6" /><rect x="3.5" y="13.5" width="7" height="7" rx="1.6" /><rect x="13.5" y="13.5" width="7" height="7" rx="1.6" />
  {:else if name === 'people'}
    <circle cx="9" cy="8.5" r="3.5" /><path d="M3 19.5c.8-3 3.2-4.5 6-4.5s5.2 1.5 6 4.5" /><circle cx="17" cy="9.5" r="2.5" /><path d="M16.5 14.6c2.2.2 3.8 1.5 4.5 3.9" />
  {:else if name === 'play'}
    <path d="M8 5.5v13l10-6.5z" />
  {:else if name === 'history'}
    <path d="M3.5 12a8.5 8.5 0 1 0 2.5-6" /><path d="M3.5 4.5V9H8" /><path d="M12 8v4.5l3 2" />
  {:else if name === 'trash'}
    <path d="M4.5 7h15M9.5 7V5h5v2M6.5 7l1 12.5h9l1-12.5M10 11v5M14 11v5" />
  {:else if name === 'pencil'}
    <path d="M4.5 19.5l1-4L15.5 5.5l3 3-10 10z" /><path d="M13.5 7.5l3 3" />
  {:else if name === 'check'}
    <path d="m5.5 12.5 4 4 9-9" />
  {:else if name === 'volume'}
    <path d="M4.5 9.5h3l4.5-4v13l-4.5-4h-3z" /><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11" />
  {:else if name === 'rumble'}
    <rect x="7" y="4.5" width="10" height="15" rx="3" /><path d="M3.5 9v6M20.5 9v6" />
  {:else if name === 'pin'}
    <path d="M12 21s6-5.6 6-11a6 6 0 0 0-12 0c0 5.4 6 11 6 11z" /><circle cx="12" cy="10" r="2.2" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" /><path d="M12 2.5v3m0 13v3M4.6 4.6l2.1 2.1m10.6 10.6 2.1 2.1M2.5 12h3m13 0h3M4.6 19.4l2.1-2.1M17.3 6.7l2.1-2.1" />
  {:else if name === 'battery'}
    <rect x="2" y="7" width="17" height="10" rx="2.5" /><rect x="4.5" y="9.5" width="9" height="5" rx="1" fill={color} stroke="none" /><path d="M21.5 10.5v3" />
  {:else if name === 'deck'}
    <rect x="2" y="7" width="20" height="10" rx="3" /><rect x="7.5" y="9" width="9" height="6" rx="1" />
  {:else if name === 'pc'}
    <rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" />
  {:else if name === 'tv'}
    <rect x="2.5" y="4.5" width="19" height="12" rx="2" /><path d="M8 20h8" />
  {:else if name === 'thor'}
    <rect x="5" y="2.5" width="14" height="19" rx="2.5" /><rect x="7.5" y="5" width="9" height="6" rx="1" /><rect x="7.5" y="13" width="9" height="5" rx="1" />
  {:else if name === 'check-circle'}
    <circle cx="12" cy="12" r="9" /><path d="m8 12.5 2.6 2.6L16 9.6" />
  {:else if name === 'spinner'}
    <circle cx="12" cy="12" r="9" stroke-opacity="0.35" /><path d="M12 3a9 9 0 0 1 9 9" />
  {:else}
    <circle cx="12" cy="12" r="9" />
  {/if}
</svg>
