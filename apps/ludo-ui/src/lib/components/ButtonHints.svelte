<script lang="ts">
  import { onMount } from 'svelte';

  // Barre d'aide des boutons. Détail : quand un bouton de la manette est appuyé, la touche dessinée
  // ici s'enfonce et s'éclaire en même temps (la barre « répond » au joueur).
  type Key = 'A' | 'B' | 'X' | 'Y';
  type Hint = { key: Key; label: string };
  let { hints, note }: { hints: Hint[]; note?: string } = $props();

  let lit = $state<Key | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    const onKey = (ev: Event) => {
      const k = (ev as CustomEvent<string>).detail as Key;
      if (!hints.some((h) => h.key === k)) return;
      lit = null;
      requestAnimationFrame(() => (lit = k));
      clearTimeout(timer);
      timer = setTimeout(() => (lit = null), 260);
    };
    window.addEventListener('ludo:key', onKey);
    return () => window.removeEventListener('ludo:key', onKey);
  });
</script>

<footer>
  {#if note}<span class="note">{note}</span>{/if}
  <span class="hints">
    {#each hints as h (h.key)}
      <span class="hint" class:lit={lit === h.key}><span class="key" class:primary={h.key === 'A'}>{h.key}</span>{h.label}</span>
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
  .hint { display: flex; align-items: center; gap: 8px; transition: color var(--t-quick) ease-out; }
  .key {
    width: 24px; height: 24px; border-radius: 50%; background: var(--hint); color: var(--ink);
    font-size: 12px; font-weight: 700; display: flex; align-items: center; justify-content: center;
    transition: transform var(--t-std) var(--ease-pop), background-color var(--t-quick) ease-out, box-shadow var(--t-std) var(--ease-land);
  }
  .key.primary { background: var(--ink); color: var(--ground); }
  .hint.lit { color: var(--ink); }
  .hint.lit .key { transform: scale(0.86); background: var(--accent); color: var(--ground); box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 25%, transparent); transition-duration: 60ms; }
</style>
