<script lang="ts">
  import AvatarArt from './AvatarArt.svelte';
  import { galleryImage, galleryOf } from '../avatars';
  import { reducedMotion } from '../motion';
  import type { AvatarArtKind, Profile } from '../types';

  // Image de profil : image de la galerie (icône de profil, sprite animé), sinon personnage dessiné sur la
  // couleur du profil, sinon initiale.
  //
  // Image animée (GIF) : jouée seulement quand l'élément porteur a le focus, ou partout si `live` (carte du
  // profil) ; sa version fixe ailleurs, pour la sobriété (Thor). Jamais animée avec `prefers-reduced-motion`.
  //
  // Mini-animation, toujours UNE fois et jamais en boucle : le personnage se réveille (petit rebond, respiration
  // courte, clignement des yeux) quand l'élément qui le porte reçoit le focus (manette, clavier, survol) ;
  // `celebrate` joue un rebond plus ample au choix du profil. Aucune animation hors focus ni fenêtre inactive,
  // et rien du tout avec `prefers-reduced-motion`.
  let { profile, size = 52, ring = false, celebrate = false, live = false }: { profile: Profile; size?: number; ring?: boolean; celebrate?: boolean; live?: boolean } = $props();

  const gallery = $derived(galleryOf(profile.avatarKind));
  let focused = $state(false);
  // Fenêtre inactive (jeu au premier plan, autre appli) : image fixe, même pour la carte du profil.
  let windowActive = $state(typeof document === 'undefined' || document.hasFocus());
  $effect(() => {
    const on = () => (windowActive = true);
    const off = () => (windowActive = false);
    window.addEventListener('focus', on);
    window.addEventListener('blur', off);
    return () => {
      window.removeEventListener('focus', on);
      window.removeEventListener('blur', off);
    };
  });
  const image = $derived(gallery ? galleryImage(gallery, (live || focused) && windowActive && !reducedMotion()) : undefined);
  let failed = $state(false);
  $effect(() => {
    void image;
    failed = false;
  });

  let el: HTMLSpanElement | undefined = $state();
  let wake = $state(false);

  function wakeUp() {
    // Mouvement réduit : aucune classe posée (elle resterait sans animationend pour la retirer).
    if (reducedMotion()) return;
    // Relance propre de l'animation si elle était déjà en cours (retrait de la classe, puis remise à l'image suivante).
    wake = false;
    requestAnimationFrame(() => (wake = true));
  }

  // Le focus de l'élément porteur (bouton du profil, pastille de l'accueil) réveille le personnage et lance
  // l'image animée ; la perte du focus la remet fixe : écoute d'événements, aucune scrutation.
  $effect(() => {
    const host = el?.closest<HTMLElement>('button, .focusable, [tabindex]');
    if (!host) return;
    const onFocus = () => {
      focused = true;
      wakeUp();
    };
    const onBlur = () => (focused = false);
    focused = document.activeElement === host;
    host.addEventListener('focus', onFocus);
    host.addEventListener('blur', onBlur);
    return () => {
      host.removeEventListener('focus', onFocus);
      host.removeEventListener('blur', onBlur);
    };
  });
</script>

<span
  bind:this={el}
  class="avatar"
  class:wake
  class:celebrate
  role="img"
  aria-label="Avatar de {profile.name}"
  style:width="{size}px"
  style:height="{size}px"
  style:font-size="{Math.round(size * 0.42)}px"
  style:background={profile.avatarKind ? profile.accent : profile.accentSoft}
  style:color={profile.accentInk}
  style:box-shadow={ring ? `0 0 0 ${Math.max(3, size / 30)}px var(--ground), 0 0 0 ${Math.max(6, size / 15)}px ${profile.accent}` : 'none'}
  onanimationend={(e) => { if (e.animationName.endsWith('wake')) wake = false; }}
>
  <span class="inner">
    {#if profile.avatar}
      <img src={profile.avatar} alt="" />
    {:else if gallery}
      {#if image && !failed}
        <img class="gallery" class:sprite={gallery.anime} src={image} alt="" loading="lazy" decoding="async" draggable="false" onerror={() => (failed = true)} />
      {:else}
        {gallery.label.slice(0, 1)}
      {/if}
    {:else if profile.avatarKind}
      <AvatarArt kind={profile.avatarKind as AvatarArtKind} />
    {:else}
      {profile.name.slice(0, 1)}
    {/if}
  </span>
</span>

<style>
  .avatar {
    border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
    font-weight: 600; flex-shrink: 0; overflow: hidden; position: relative;
    transition: width var(--t-std) var(--ease-pop), height var(--t-std) var(--ease-pop), box-shadow var(--t-std) var(--ease-land);
  }
  .inner { position: relative; width: 100%; height: 100%; display: flex; align-items: center; justify-content: center; border-radius: 50%; overflow: hidden; }
  img { width: 100%; height: 100%; object-fit: cover; }
  /* Icône de profil : l'image entière remplit le disque (elle est conçue pour un cadre rond ou carré). */
  img.gallery { user-select: none; }
  /* Sprite animé (petit, souvent plus large que haut) : entier, avec une marge, posé sur la couleur du profil. */
  img.gallery.sprite { object-fit: contain; width: 78%; height: 78%; image-rendering: auto; }

  @media (prefers-reduced-motion: no-preference) {
    /* Réveil : petit saut, se pose avec un léger écrasement, puis une respiration courte. */
    .avatar.wake .inner { animation: avatar-wake 640ms var(--ease-pop, cubic-bezier(0.34, 1.56, 0.64, 1)) 1; }
    .avatar.celebrate .inner { animation: avatar-cheer 560ms var(--ease-pop, cubic-bezier(0.34, 1.56, 0.64, 1)) 1; }
  }
  @keyframes avatar-wake {
    0% { transform: none; }
    28% { transform: translateY(-7%) scale(1.04); }
    52% { transform: translateY(0) scale(1.03, 0.96); }
    74% { transform: scale(1.025); }
    100% { transform: none; }
  }
  @keyframes avatar-cheer {
    0% { transform: none; }
    35% { transform: translateY(-10%) scale(1.08); }
    65% { transform: translateY(0) scale(1.05, 0.94); }
    100% { transform: none; }
  }
</style>
