<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Game } from '../types';

  // Pastille d'état d'une tuile. Rien quand tout va bien (« À jour » ne s'affiche pas sur une tuile).
  let { game, size = 34 }: { game: Game; size?: number } = $props();

  const spec = $derived.by(() => {
    if (game.sync === 'conflit') return { icon: 'conflict', color: 'var(--conflict)', bg: '#33284a', label: 'Conflit de sauvegarde' } as const;
    if (game.sync === 'envoi_en_attente') return { icon: 'cloud-pending', color: 'var(--pending)', bg: '#3b3020', label: 'Envoi en attente' } as const;
    if (game.sync === 'a_recuperer') return { icon: 'cloud-download', color: 'var(--sync)', bg: '#1c3546', label: 'Sauvegarde à récupérer' } as const;
    if (game.sync === 'synchronisation') return { icon: 'cloud-sync', color: 'var(--sync)', bg: '#1c3546', label: 'Synchronisation…' } as const;
    if (game.install === 'mise_a_jour') return { icon: 'update', color: 'var(--update)', bg: '#1c3546', label: 'Mise à jour disponible' } as const;
    if (game.install === 'telechargement') return { icon: 'download', color: 'var(--accent-ink)', bg: 'var(--accent-soft)', label: 'Téléchargement…' } as const;
    if (game.install === 'non_installe') return { icon: 'download', color: 'var(--ink-3)', bg: 'transparent', label: 'Non installé' } as const;
    return null;
  });
</script>

{#if spec}
  <span class="badge" style:width="{size}px" style:height="{size}px" style:background={spec.bg}>
    <Icon name={spec.icon} size={Math.round(size * 0.58)} color={spec.color} label={spec.label} />
  </span>
{/if}

<style>
  .badge { border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; }
</style>
