<script lang="ts">
  // Barre d'aide des boutons de la manette, en bas de chaque écran.
  type Hint = { key: 'A' | 'B' | 'X' | 'Y'; label: string };
  let { hints, note }: { hints: Hint[]; note?: string } = $props();
</script>

<footer>
  {#if note}<span class="note">{note}</span>{/if}
  <span class="hints">
    {#each hints as h (h.key)}
      <span class="hint"><span class="key" class:primary={h.key === 'A'}>{h.key}</span>{h.label}</span>
    {/each}
  </span>
</footer>

<style>
  footer {
    height: 54px; flex-shrink: 0; border-top: 1px solid var(--line);
    display: flex; align-items: center; justify-content: space-between; gap: 24px;
    padding: 0 48px; font-size: 14px; color: var(--ink-2);
  }
  .note { color: var(--ink-4); }
  .hints { display: flex; gap: 26px; margin-left: auto; }
  .hint { display: flex; align-items: center; gap: 8px; }
  .key {
    width: 24px; height: 24px; border-radius: 50%; background: var(--hint); color: var(--ink);
    font-size: 12px; font-weight: 700; display: flex; align-items: center; justify-content: center;
  }
  .key.primary { background: var(--ink); color: var(--ground); }
</style>
