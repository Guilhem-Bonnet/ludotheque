<script lang="ts">
  import type { Console } from '../types';

  // Pastille de console, monochrome comme dans Cocoon ou ES-DE : la couleur appartient à la jaquette.
  // `on="art"` (par défaut) : sur une jaquette, fond sombre translucide ; `on="ui"` : sur l'interface.
  let { console: c, size = 'm', on = 'art' }: { console: Console; size?: 's' | 'm' | 'l'; on?: 'art' | 'ui' } = $props();

  const LABELS: Record<Console, string> = {
    switch: 'SWITCH', switch2: 'SWITCH 2', '3ds': '3DS', ds: 'DS', wiiu: 'WII U', gc: 'GAMECUBE',
  };
</script>

<span class="chip {size} {on}">{LABELS[c]}</span>

<style>
  .chip { font-weight: 700; letter-spacing: 0.06em; border-radius: 6px; white-space: nowrap; align-self: flex-start; color: #fff; }
  .art { background: rgba(0, 0, 0, 0.28); }
  .ui { background: rgba(255, 255, 255, 0.1); color: var(--ink-2); }
  .s { font-size: 10px; padding: 3px 7px; border-radius: 5px; }
  .m { font-size: 11px; padding: 4px 8px; }
  .l { font-size: 13px; padding: 5px 10px; }
</style>
