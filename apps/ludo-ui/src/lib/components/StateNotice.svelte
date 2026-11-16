<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';

  // État vide ou d'erreur d'un écran (app réelle, jamais de données de démonstration) :
  // pictogramme, titre court sans pronom, détail facultatif, action facultative (« Réessayer »).
  let {
    icon = 'cloud-off',
    title,
    detail,
    action,
    onaction,
    autofocus = true,
  }: { icon?: IconName; title: string; detail?: string; action?: string; onaction?: () => void; autofocus?: boolean } = $props();
</script>

<div class="notice" role="status">
  <span class="ico"><Icon name={icon} size={40} color="var(--ink-3)" /></span>
  <p class="title">{title}</p>
  {#if detail}<p class="detail">{detail}</p>{/if}
  {#if action && onaction}
    <button class="act focusable" data-autofocus={autofocus ? '' : undefined} onclick={onaction}>{action}</button>
  {/if}
</div>

<style>
  .notice { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; padding: 40px 24px; text-align: center; }
  .ico { opacity: 0.9; }
  .title { margin: 6px 0 0; font-size: 20px; font-weight: 600; color: var(--ink); }
  .detail { margin: 0; max-width: 560px; font-size: 14px; color: var(--ink-3); overflow-wrap: anywhere; }
  .act { all: unset; cursor: pointer; margin-top: 10px; padding: 12px 22px; border-radius: 12px; background: var(--raised); font-size: 15px; font-weight: 600; color: var(--ink); }
  .act.focusable:focus { box-shadow: inset 0 0 0 2px var(--accent); background: rgba(255, 255, 255, 0.14); }
</style>
