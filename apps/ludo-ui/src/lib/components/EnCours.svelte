<script lang="ts">
  // Carte « En cours » de l'accueil (menu en jeu, lot menu-jeu du 22/11) : le jeu ouvert en tête,
  // Reprendre (A) et Fermer le jeu (X, confirmation). Au retour depuis le jeu (Thor : bouton Mode ou
  // HOME en jeu, RetourService.kt), « Reprendre » reçoit le focus : A rend la main au jeu, X propose de le fermer.
  // Mouvement : entrée une seule fois (montée 12 px + fondu, 320 ms), repli à la fermeture (220 ms) ;
  // rien ne boucle.
  import { onMount, tick } from 'svelte';
  import { fly, slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { dureeSession, enCours } from '../enCours.svelte';
  import CoverArt from './CoverArt.svelte';
  import { feedback } from '../feedback';
  import { reducedMotion } from '../motion';
  import type { Game } from '../types';

  let { games }: { games: Game[] } = $props();

  const jeu = $derived(enCours.jeu);
  const game = $derived(jeu ? games.find((g) => g.id === jeu.game) : undefined);
  const titre = $derived(game?.title ?? jeu?.game ?? '');
  /** Durée relue à chaque retour depuis le jeu (pas d'horloge qui tourne pendant que l'accueil attend). */
  const session = $derived.by(() => {
    void enCours.retour;
    return jeu ? dureeSession(jeu.depuisMs) : '';
  });

  let carte = $state<HTMLElement | null>(null);
  let reprendreEl = $state<HTMLButtonElement | null>(null);
  let erreur = $state<string | null>(null);

  $effect(() => {
    if (titre) enCours.titre = titre;
  });
  // Carte retirée avec le focus dedans : aucun `focusout` garanti, la barre de consignes est remise.
  const ouvert = $derived(!!jeu);
  $effect(() => {
    if (!ouvert) enCours.carteFocalisee = false;
  });
  onMount(() => () => (enCours.carteFocalisee = false));

  // Retour depuis le jeu, ou accueil ouvert alors qu'un jeu est ouvert : le focus va sur « Reprendre »
  // (premier appui utile = A ou X). Posé APRÈS celui de l'accueil, qui focalise sa tuile quand la
  // bibliothèque arrive (`games` change) : deux images plus tard, une seule fois par retour.
  let aFocaliser = $state(true);
  $effect(() => {
    void enCours.retour;
    aFocaliser = true;
  });
  $effect(() => {
    if (!aFocaliser || !ouvert || games.length === 0) return;
    aFocaliser = false;
    // Une boîte ouverte (options, confirmation) garde le focus.
    const poser = () => { if (!document.querySelector('[data-modal]')) reprendreEl?.focus({ preventScroll: true }); };
    void tick().then(() => requestAnimationFrame(() => requestAnimationFrame(poser)));
  });

  async function reprendre() {
    erreur = null;
    feedback('launch');
    try {
      await enCours.reprendre();
    } catch (e) {
      erreur = e instanceof Error ? e.message : String(e);
      feedback('err');
    }
  }

  function demanderFermeture() {
    feedback('ok');
    enCours.confirmation = {};
  }

  onMount(() => {
    // X sur la carte : fermer le jeu (l'accueil réserve X aux tuiles : pas de double effet ici).
    const onAction = (ev: Event) => {
      if ((ev as CustomEvent).detail !== 'x' || !jeu || enCours.confirmation) return;
      if (!carte?.contains(document.activeElement)) return;
      demanderFermeture();
    };
    window.addEventListener('ludo:action', onAction);
    return () => window.removeEventListener('ludo:action', onAction);
  });

  const d = (ms: number) => (reducedMotion() ? 0 : ms);
</script>

{#if jeu}
  <section class="carte" bind:this={carte} aria-label="Jeu en cours : {titre}"
    onfocusin={() => (enCours.carteFocalisee = true)} onfocusout={() => (enCours.carteFocalisee = false)}
    in:fly={{ y: 12, duration: d(320), easing: cubicOut }} out:slide={{ duration: d(220) }}>
    <div class="jaquette" aria-hidden="true">
      {#if game?.cover}<img src={game.cover} alt="" />{:else if game?.art}<CoverArt art={game.art} />{/if}
    </div>
    <div class="texte">
      <span class="etat"><span class="voyant"></span>En cours</span>
      <h2>{titre}</h2>
      <span class="meta">{game?.emulator ? `${game.emulator} · ` : ''}session {session}</span>
    </div>
    <div class="acts">
      <button class="act plein focusable" data-autofocus bind:this={reprendreEl} data-sound="none" onclick={() => void reprendre()}>
        <span class="touche" aria-hidden="true">A</span>Reprendre
      </button>
      <button class="act focusable" data-sound="none" onclick={demanderFermeture}>
        <span class="touche" aria-hidden="true">X</span>Fermer le jeu
      </button>
    </div>
    {#if erreur}<p class="note erreur" role="alert">{erreur}</p>{/if}
  </section>
{/if}

<style>
  .carte {
    position: relative; z-index: 2;
    margin: 4px auto 0; width: min(760px, calc(100% - 48px)); box-sizing: border-box;
    display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 18px;
    padding: 10px 14px; border-radius: 20px;
    background: rgba(12, 14, 18, 0.55); border: 2px solid var(--accent);
  }
  .jaquette { position: relative; isolation: isolate; width: 58px; height: 78px; border-radius: 10px; overflow: hidden; background: rgba(255, 255, 255, 0.08); }
  .jaquette img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .texte { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .etat { display: flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 800; letter-spacing: 0.06em; text-transform: uppercase; }
  .voyant { width: 10px; height: 10px; border-radius: 50%; background: var(--ok); }
  h2 { margin: 0; font-size: 20px; line-height: 1.15; font-weight: 800; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .meta { font-size: 15px; color: #c9d0db; }
  .acts { display: flex; gap: 10px; }
  .act {
    min-height: 48px; padding: 0 16px; border-radius: 14px; font: inherit; font-size: 17px; font-weight: 700;
    display: inline-flex; align-items: center; gap: 10px; cursor: pointer;
    background: transparent; color: #f5f7fa; border: 2px solid rgba(255, 255, 255, 0.62);
  }
  .act.plein { background: var(--accent); color: #0e1116; border-color: transparent; font-weight: 800; }
  .act:focus-visible, .act:focus { outline: 3px solid #f5f7fa; outline-offset: 3px; }
  .touche {
    width: 26px; height: 26px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
    font-size: 14px; font-weight: 800; border: 2px solid currentColor; box-sizing: border-box;
  }
  .note { grid-column: 1 / -1; margin: 0; font-size: 14px; color: #d6dbe4; display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .erreur { color: #ff8a94; }
  /* Écran du bas de la Thor (≈ 723×630 px logiques, échelle du Deck) : une seule ligne pour garder la
     rangée de jeux visible ; titre et session sans retour à la ligne, cibles de 56 px pour le pouce. */
  :global(html[data-ecran='bas']) .carte { width: calc(100% - 32px); gap: 14px; padding: 10px 12px; }
  :global(html[data-ecran='bas']) .jaquette { width: 52px; height: 70px; }
  :global(html[data-ecran='bas']) h2 { -webkit-line-clamp: 1; line-clamp: 1; font-size: 21px; }
  :global(html[data-ecran='bas']) .meta { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  :global(html[data-ecran='bas']) .act { min-height: 56px; font-size: 18px; padding: 0 16px; }
  :global(html[data-ecran='bas']) .touche { width: 28px; height: 28px; font-size: 15px; }
  /* Écran étroit (portrait) : actions sous le titre, pleine largeur. */
  @media (max-aspect-ratio: 1/1) {
    .carte { grid-template-columns: auto minmax(0, 1fr); }
    .acts { grid-column: 1 / -1; }
    .act { flex: 1; justify-content: center; }
  }
</style>
