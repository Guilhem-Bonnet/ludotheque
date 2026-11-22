<script lang="ts" module>
  import type { IconName } from './Icon.svelte';
  import type { Game } from '../types';

  /** État affiché sur une tuile ; `null` quand tout va bien (« À jour » ne s'affiche pas sur une tuile). */
  export interface TileState {
    icon: IconName;
    /** Couleur d'état de l'interface (pastille sur jaquette illustrée). */
    color: string;
    bg: string;
    /** Mot court posé dans le bandeau de la cartouche. */
    short: string;
    /** Libellé complet (lecteur d'écran, infobulle). */
    label: string;
    /** Demande une action ou signale un risque : toujours visible, en capsule. */
    attention: boolean;
  }

  export function tileState(game: Game): TileState | null {
    if (game.sync === 'conflit') return { icon: 'conflict', color: 'var(--conflict)', bg: '#33284a', short: 'Conflit', label: 'Conflit de sauvegarde', attention: true };
    if (game.sync === 'envoi_en_attente') return { icon: 'cloud-pending', color: 'var(--pending)', bg: '#3b3020', short: 'Envoi en attente', label: 'Envoi en attente', attention: true };
    // Sauvegarde plus récente sur le serveur (jouée sur un autre appareil) : rien à faire, elle est
    // récupérée d'elle-même au lancement. Bonne nouvelle, pas une alerte.
    if (game.sync === 'a_recuperer') return { icon: 'cloud-download', color: 'var(--sync)', bg: '#1c3546', short: 'Nouvelle sauvegarde', label: 'Sauvegarde plus récente sur le serveur, récupérée au lancement', attention: false };
    if (game.sync === 'synchronisation') return { icon: 'cloud-sync', color: 'var(--sync)', bg: '#1c3546', short: 'Synchronisation…', label: 'Synchronisation…', attention: false };
    if (game.install === 'mise_a_jour') return { icon: 'update', color: 'var(--update)', bg: '#1c3546', short: 'Mise à jour', label: 'Mise à jour disponible', attention: false };
    if (game.install === 'telechargement') {
      const pct = game.progress === undefined ? '' : `${Math.round(game.progress)} %`;
      return { icon: 'download', color: 'var(--accent-ink)', bg: 'var(--accent-soft)', short: pct || 'Téléchargement…', label: `Téléchargement… ${pct}`.trim(), attention: false };
    }
    // Android : jeu absent de l'appareil mais jouable en flux depuis le PC.
    if (game.install === 'non_installe' && game.stream) return { icon: 'pc', color: 'var(--accent-ink)', bg: 'transparent', short: 'Via Moonlight', label: 'Jouable en flux depuis le PC (Moonlight)', attention: false };
    if (game.install === 'non_installe') return { icon: 'download', color: 'var(--ink-3)', bg: 'transparent', short: 'Non installé', label: 'Non installé', attention: false };
    return null;
  }
</script>

<script lang="ts">
  import Icon from './Icon.svelte';

  // Indicateur d'état d'une tuile.
  // - `tone="art"` (jaquette illustrée) : pastille ronde sombre, pictogramme à la couleur de l'état.
  // - `tone="band"` (cartouche) : imprimé dans le bandeau, à l'encre du bandeau, comme le nom de la
  //   console — aucune pastille rapportée. Les états qui demandent une action (conflit, envoi en
  //   attente) passent en capsule inversée (fond = encre du bandeau) pour se voir de loin ; les autres
  //   restent un simple pictogramme. `labelled` ajoute le mot court (tuile en focus, assez large) ;
  //   l'état se lit à la forme et au mot, pas à la couleur (flux vidéo compressé, daltonisme).
  let { game, size = 34, tone = 'art', labelled = false }: { game: Game; size?: number; tone?: 'art' | 'band'; labelled?: boolean } = $props();

  const spec = $derived(tileState(game));
</script>

{#if spec}
  {#if tone === 'band'}
    <span class="mark" class:attention={spec.attention} class:quiet={game.install === 'non_installe' && !game.stream && !spec.attention} class:flux={game.install === 'non_installe' && !!game.stream && !spec.attention} class:labelled style:--h="{size}px" title={spec.label}>
      <Icon name={spec.icon} size={Math.round(size * (spec.attention ? 0.7 : 0.78))} color="currentColor" label={labelled ? undefined : spec.label} />
      {#if labelled}<span class="word">{spec.short}</span>{/if}
    </span>
  {:else}
    <span class="badge" style:width="{size}px" style:height="{size}px" style:background={spec.bg}>
      <Icon name={spec.icon} size={Math.round(size * 0.58)} color={spec.color} label={spec.label} />
    </span>
  {/if}
{/if}

<style>
  .badge { border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; }

  /* Cartouche : même encre que le nom de la console. */
  .mark {
    display: inline-flex; align-items: center; gap: calc(var(--h) * 0.22); flex-shrink: 0; min-width: 0;
    height: var(--h); color: var(--band-ink, #fff); line-height: 1; white-space: nowrap;
  }
  .mark.quiet { opacity: 0.72; }
  /* Jouable en flux depuis le PC (Moonlight) : jamais effacé comme « Non installé ». Capsule cerclée
     à l'encre du bandeau : se distingue de loin par la forme (pas seulement par le pictogramme, trop
     petit sur une tuile de bibliothèque), sans prendre l'aplat des états qui demandent une action. */
  .mark.flux {
    border: max(1.5px, calc(var(--h) * 0.07)) solid currentColor; border-radius: calc(var(--h) / 2);
    padding: 0 calc(var(--h) * 0.2); box-sizing: border-box;
  }
  .mark.flux.labelled { padding: 0 calc(var(--h) * 0.34) 0 calc(var(--h) * 0.22); }
  .mark.attention {
    background: var(--band-ink, #fff); color: var(--band, #15171c);
    border-radius: calc(var(--h) / 2); padding: 0 calc(var(--h) * 0.24);
  }
  .mark.attention.labelled { padding: 0 calc(var(--h) * 0.38) 0 calc(var(--h) * 0.26); }
  .word { font-size: calc(var(--h) * 0.56); font-weight: 600; letter-spacing: 0.01em; overflow: hidden; text-overflow: ellipsis; }
</style>
