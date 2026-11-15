<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Game } from '../types';

  // Indicateur global de synchronisation (en-tête), à la manière du nuage de Steam : l'état le plus
  // important parmi toutes les sauvegardes du profil sur cet appareil.
  // Priorité : Hors ligne > Conflit de sauvegarde > Synchronisation… > Envoi en attente > À jour.
  let { games, offline = false }: { games: Game[]; offline?: boolean } = $props();

  const spec = $derived.by(() => {
    const n = (s: Game['sync']) => games.filter((g) => g.sync === s).length;
    if (offline) return { key: 'off', icon: 'cloud-off', color: 'var(--offline)', label: 'Hors ligne' } as const;
    const conflits = n('conflit');
    if (conflits) return { key: 'conflit', icon: 'conflict', color: 'var(--conflict)', label: conflits > 1 ? `${conflits} conflits de sauvegarde` : 'Conflit de sauvegarde' } as const;
    if (n('synchronisation')) return { key: 'sync', icon: 'cloud-sync', color: 'var(--sync)', label: 'Synchronisation…' } as const;
    const attente = n('envoi_en_attente');
    if (attente) return { key: 'attente', icon: 'cloud-pending', color: 'var(--pending)', label: attente > 1 ? `${attente} envois en attente` : 'Envoi en attente' } as const;
    return { key: 'ok', icon: 'cloud-ok', color: 'var(--ok)', label: 'À jour' } as const;
  });
</script>

<span class="sync {spec.key}" role="status" aria-live="polite">
  {#key spec.key}
    <span class="ico"><Icon name={spec.icon} size={20} color={spec.color} /></span>
  {/key}
  <span class="label">{spec.label}</span>
</span>

<style>
  .sync {
    display: inline-flex; align-items: center; gap: 8px; padding: 7px 14px 7px 11px; border-radius: 20px;
    background: rgba(255, 255, 255, 0.1); font-size: 15px; color: var(--ink); white-space: nowrap;
    transition: background-color var(--t-std) ease-out;
  }
  .ico { display: inline-flex; }
  .conflit { background: color-mix(in srgb, var(--conflict) 22%, transparent); }
  .attente { background: color-mix(in srgb, var(--pending) 18%, transparent); }
  @media (prefers-reduced-motion: no-preference) {
    /* Changement d'état : le nouveau nuage arrive avec un petit rebond. */
    .ico { animation: arrive 320ms var(--ease-pop) both; }
    @keyframes arrive { from { transform: scale(0.6); opacity: 0; } to { transform: none; opacity: 1; } }
    /* En cours : les flèches tournent doucement. En attente : le nuage respire. */
    .sync.sync .ico { animation: arrive 320ms var(--ease-pop) both, spin 1.6s linear 320ms infinite; }
    .attente .ico { animation: arrive 320ms var(--ease-pop) both, breathe 2.4s var(--ease-calm) 320ms infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @keyframes breathe { 0%, 100% { opacity: 1; } 50% { opacity: 0.55; } }
  }
</style>
