<script lang="ts">
  import { onMount } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  import { AVATARS, currentKind, FAMILLES, GALLERY, GALLERY_PREFIX, withAvatar } from '../avatars';
  import { focusFirst } from '../nav';
  import { reducedMotion } from '../motion';
  import type { AvatarKind, Profile } from '../types';

  // Choix de l'image de profil : galerie d'icônes de profil et de sprites animés, rangée par famille (licence),
  // puis les personnages dessinés de la Ludothèque. Les images animées ne jouent qu'au focus (Avatar.svelte).
  // Manette, clavier, souris et toucher : chaque disque est un bouton `.focusable` (navigation spatiale de
  // nav.ts) ; B ou Échap ferment (événement `ludo:modal-back`).
  let { profile, onpick, onclose }: { profile: Profile; onpick: (kind: AvatarKind) => void; onclose: () => void } = $props();

  type Choice = { kind: AvatarKind; label: string; preview: Profile };
  // Aperçu de chaque image avec ses propres couleurs (accent automatique), quel que soit l'accent du profil.
  const auto = $derived({ ...profile, accentAuto: true });
  const current = $derived(currentKind(profile.avatarKind));
  const groups: { name: string; items: Choice[] }[] = $derived([
    ...FAMILLES.map((name) => ({
      name,
      items: GALLERY.filter((g) => g.famille === name).map((g) => {
        const kind = `${GALLERY_PREFIX}${g.slug}` as AvatarKind;
        return { kind, label: g.label, preview: withAvatar(auto, kind) };
      }),
    })),
    { name: 'Personnages de la Ludothèque', items: AVATARS.map((a) => ({ kind: a.kind, label: a.label, preview: withAvatar(auto, a.kind) })) },
  ].filter((g) => g.items.length > 0));

  // Nom de l'image sous le focus (l'image actuelle à l'ouverture).
  let focusLabel = $state<string | null>(null);
  const label = $derived(focusLabel ?? groups.flatMap((g) => g.items).find((c) => c.kind === current)?.label ?? '');

  let list: HTMLDivElement | undefined = $state();
  onMount(() => {
    focusFirst();
    // focusFirst ne fait pas défiler : l'image actuelle peut être plus bas dans la liste, on l'amène en vue.
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const el = document.activeElement;
      if (el instanceof HTMLElement && list?.contains(el)) el.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    }));
    const onBack = () => onclose();
    window.addEventListener('ludo:modal-back', onBack);
    return () => window.removeEventListener('ludo:modal-back', onBack);
  });

  const d = (ms: number) => (reducedMotion() ? 0 : ms);
</script>

<div class="backdrop" transition:fade={{ duration: d(200) }} aria-hidden="true"></div>
<div class="wrap">
<div class="panel" data-modal role="dialog" aria-modal="true" aria-label="Image de profil" in:fly={{ y: 24, duration: d(320), easing: cubicOut }} out:fade={{ duration: d(160) }}>
  <header>
    <h2>Image de profil</h2>
    <span class="current">{label}</span>
    <button class="close focusable" aria-label="Fermer" data-sound="none" onclick={onclose}><svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" fill="none"></path></svg></button>
  </header>
  <div class="groups" bind:this={list}>
    {#each groups as g (g.name)}
      <section>
        <h3>{g.name}</h3>
        <div class="grid">
          {#each g.items as c (c.kind)}
            <button
              class="choice focusable"
              class:chosen={c.kind === current}
              style:--c={c.preview.accent}
              aria-label={c.label}
              aria-pressed={c.kind === current}
              data-autofocus={c.kind === current ? '' : undefined}
              data-hover-focus
              data-sound="none"
              onfocus={() => (focusLabel = c.label)}
              onclick={() => onpick(c.kind)}
            >
              <Avatar profile={c.preview} size={72} />
              {#if c.kind === current}<span class="tick"><Icon name="check" size={14} color="#0b0f14" /></span>{/if}
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>
</div>
</div>

<style>
  .backdrop { position: absolute; inset: 0; z-index: 10; background: rgba(5, 8, 12, 0.62); }
  /* Le panneau prend la hauteur de son contenu, centré, sans dépasser l'écran (Thor, Deck, ultrawide). */
  .wrap { position: absolute; z-index: 11; inset: 40px 56px; display: flex; align-items: center; justify-content: center; pointer-events: none; }
  .panel {
    pointer-events: auto; width: 100%; max-width: 1100px; max-height: 100%;
    display: flex; flex-direction: column; border-radius: 20px; background: #161a22;
    border: 1px solid rgba(255, 255, 255, 0.08); box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5); overflow: hidden;
  }
  header { display: flex; align-items: center; gap: 16px; padding: 22px 28px 12px; }
  h2 { margin: 0; font-size: 22px; font-weight: 600; }
  .current { font-size: 16px; color: var(--ink-2); }
  .close { all: unset; cursor: pointer; margin-left: auto; width: 44px; height: 44px; border-radius: 50%; display: flex; align-items: center; justify-content: center; background: rgba(255, 255, 255, 0.08); }
  .close.focusable:focus { box-shadow: inset 0 0 0 2px var(--accent); background: rgba(255, 255, 255, 0.14); }
  /* scroll-padding : le disque sous le focus (agrandi, avec son anneau) reste entier quand la liste défile. */
  .groups { flex: 1; overflow-y: auto; padding: 4px 28px 28px; display: flex; flex-direction: column; gap: 18px; scrollbar-width: none; scroll-padding-block: 24px; }
  section { display: flex; flex-direction: column; gap: 10px; }
  h3 { margin: 0; font-size: 13px; font-weight: 600; letter-spacing: 0.05em; text-transform: uppercase; color: var(--ink-3); }
  .grid { display: flex; flex-wrap: wrap; gap: 14px; padding: 6px 4px; }
  .choice { all: unset; cursor: pointer; position: relative; display: block; line-height: 0; border-radius: 50%; transition: transform var(--t-std) var(--ease-pop); }
  /* Anneau porté par le bouton (rond) : le style en ligne de l'avatar ne le masque pas. */
  .choice { box-shadow: 0 6px 14px rgba(0, 0, 0, 0.3); }
  .choice.focusable:focus { transform: translateY(-4px) scale(1.08); }
  .choice.focusable:focus, .choice.chosen { box-shadow: 0 0 0 3px #161a22, 0 0 0 6px var(--c), 0 10px 20px rgba(0, 0, 0, 0.35); }
  .tick { position: absolute; right: -2px; bottom: -2px; width: 24px; height: 24px; border-radius: 50%; background: var(--c); display: flex; align-items: center; justify-content: center; box-shadow: 0 0 0 3px #161a22; }
  @media (prefers-reduced-motion: reduce) { .choice.focusable:focus { transform: none; } }
</style>
