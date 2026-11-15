<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Game } from '../types';

  // Indicateur global de synchronisation (en-tête), à la manière du nuage de Steam : l'état le plus
  // important parmi toutes les sauvegardes du profil sur cet appareil.
  // Priorité : Hors ligne > Synchronisation… (en cours) > Conflit de sauvegarde > Envoi en attente > À jour.
  // C'est aussi un bouton : A lance une synchronisation manuelle (comme « Synchroniser » dans Steam).
  let {
    games,
    offline = false,
    busy = false,
    onclick,
    onfocus,
    onblur,
  }: { games: Game[]; offline?: boolean; busy?: boolean; onclick?: () => void; onfocus?: () => void; onblur?: () => void } = $props();

  const spec = $derived.by(() => {
    const n = (s: Game['sync']) => games.filter((g) => g.sync === s).length;
    if (offline) return { key: 'off', icon: 'cloud-off', color: 'var(--offline)', label: 'Hors ligne' } as const;
    if (busy || n('synchronisation')) return { key: 'sync', icon: 'cloud-sync', color: 'var(--sync)', label: 'Synchronisation…' } as const;
    const conflits = n('conflit');
    if (conflits) return { key: 'conflit', icon: 'conflict', color: 'var(--conflict)', label: conflits > 1 ? `${conflits} conflits de sauvegarde` : 'Conflit de sauvegarde' } as const;
    const attente = n('envoi_en_attente');
    if (attente) return { key: 'attente', icon: 'cloud-pending', color: 'var(--pending)', label: attente > 1 ? `${attente} envois en attente` : 'Envoi en attente' } as const;
    return { key: 'ok', icon: 'cloud-ok', color: 'var(--ok)', label: 'À jour' } as const;
  });
</script>

<button class="cloud focusable {spec.key}" aria-label="{spec.label} · Synchroniser" aria-live="polite" data-sound="tab" {onclick} {onfocus} {onblur}>
  {#key spec.key}
    <span class="ico">
      <Icon name={spec.icon} size={20} color={spec.color} />
      {#if spec.key === 'sync'}
        <!-- Chargement : un arc qui tourne autour du nuage immobile. -->
        <svg class="orbit" viewBox="0 0 32 32" aria-hidden="true"><circle cx="16" cy="16" r="14" fill="none" stroke={spec.color} stroke-width="2" stroke-linecap="round" stroke-dasharray="22 66"></circle></svg>
      {/if}
    </span>
  {/key}
  <span class="label">{spec.label}</span>
</button>

<style>
  .cloud {
    all: unset; cursor: pointer; box-sizing: border-box;
    display: inline-flex; align-items: center; gap: 8px; padding: 7px 14px 7px 11px; border-radius: 20px;
    background: rgba(255, 255, 255, 0.1); font-size: 15px; color: var(--ink); white-space: nowrap;
    transition: background-color var(--t-std) ease-out, transform var(--t-quick) var(--ease-pop), box-shadow var(--t-std) var(--ease-land);
  }
  .ico { display: inline-flex; position: relative; }
  .orbit { position: absolute; left: 50%; top: 50%; width: 30px; height: 30px; margin: -15px 0 0 -15px; opacity: 0.9; }
  .sync .ico :global(svg:not(.orbit)) { opacity: 0.85; }
  .cloud.sync { gap: 13px; padding-left: 15px; }
  .conflit { background: color-mix(in srgb, var(--conflict) 22%, transparent); }
  .attente { background: color-mix(in srgb, var(--pending) 18%, transparent); }
  /* Focus : la pastille se soulève, l'anneau d'accent se pose. */
  .cloud.focusable:focus { background: rgba(255, 255, 255, 0.2); transform: translateY(-2px); box-shadow: 0 0 0 3px var(--accent), 0 8px 18px rgba(0, 0, 0, 0.3); }
  @media (prefers-reduced-motion: no-preference) {
    /* Changement d'état : le nouveau nuage arrive avec un petit rebond. */
    .ico { animation: arrive 320ms var(--ease-pop) both; }
    @keyframes arrive { from { transform: scale(0.6); opacity: 0; } to { transform: none; opacity: 1; } }
    /* En cours : l'arc tourne autour du nuage, qui respire à peine. En attente : le nuage respire. */
    .orbit { animation: spin 1.1s cubic-bezier(.5, .15, .5, .85) infinite; }
    .cloud.sync .ico :global(svg:not(.orbit)) { animation: breathe 1.8s var(--ease-calm) infinite; }
    .attente .ico { animation: arrive 320ms var(--ease-pop) both, breathe 2.4s var(--ease-calm) 320ms infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @keyframes breathe { 0%, 100% { opacity: 1; } 50% { opacity: 0.55; } }
  }
  @media (prefers-reduced-motion: reduce) { .cloud.focusable:focus { transform: none; } }
</style>
