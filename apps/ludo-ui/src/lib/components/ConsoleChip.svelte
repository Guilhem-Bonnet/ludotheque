<script lang="ts">
  import type { Console } from '../types';

  let { console: c, size = 'm' }: { console: Console; size?: 's' | 'm' | 'l' } = $props();

  const LABELS: Record<Console, [string, string, string]> = {
    switch: ['SWITCH', '#e60012', '#ffffff'],
    switch2: ['SWITCH 2', '#ff4d5e', '#1b0b0e'],
    '3ds': ['3DS', '#d7d9de', '#1b1d22'],
    ds: ['DS', '#4a4f5c', '#eef0f4'],
    wiiu: ['WII U', '#009ac7', '#04121a'],
    gc: ['GC', '#6a5acd', '#ffffff'],
  };
  const spec = $derived(LABELS[c]);
</script>

<span class="chip {size}" style:background={spec[1]} style:color={spec[2]}>{spec[0]}</span>

<style>
  .chip { font-weight: 600; letter-spacing: 0.06em; border-radius: 6px; white-space: nowrap; align-self: flex-start; }
  .s { font-size: 10px; padding: 3px 7px; border-radius: 5px; }
  .m { font-size: 11px; padding: 4px 8px; }
  .l { font-size: 13px; padding: 5px 10px; }
</style>
