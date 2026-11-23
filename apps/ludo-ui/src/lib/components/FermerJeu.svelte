<script lang="ts">
  // Confirmation « Fermer le jeu ? » (menu en jeu, lot menu-jeu du 22/11). Affichée par la carte
  // « En cours » (X) ou par un lancement pendant qu'un autre jeu est ouvert (play.ts). Le focus est
  // posé sur l'action demandée : X puis A ferme, comme demandé (« rapide dès le premier appui ») ;
  // B ou « Annuler » referme sans rien faire.
  import { onMount, untrack } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { enCours } from '../enCours.svelte';
  import { raisonSansArretDirect } from '../arretDirect';
  import { feedback } from '../feedback';
  import { reducedMotion } from '../motion';
  import { focusFirst } from '../nav';
  import { isAndroid } from '../api';
  import ButtonHints from './ButtonHints.svelte';

  const titre = $derived(enCours.titre);
  const demande = $derived(enCours.confirmation);
  // Android sans arrêt direct (Shizuku absent, arrêté ou sans accès) : la page d'Android s'ouvre.
  // La raison n'est expliquée qu'une fois (par raison), l'étape reste annoncée à chaque fois.
  const parPage = $derived(isAndroid && enCours.etat.arretDirect !== 'pret');
  let raison = $state<string | null>(null);
  const CLE_RAISON = 'ludo.arretDirect.raisonVue';
  let erreur = $state<string | null>(null);
  let retour: HTMLElement | null = null;

  $effect(() => {
    if (!demande) return;
    retour = document.activeElement as HTMLElement | null;
    erreur = null;
    raison = null;
    // Lu sans dépendance : la fermeture met l'état à jour, elle ne doit pas relancer cet effet.
    const etat = untrack(() => enCours.etat.arretDirect);
    const texte = isAndroid ? raisonSansArretDirect(etat) : null;
    if (texte) {
      let vue: string | null = null;
      try {
        vue = localStorage.getItem(CLE_RAISON);
      } catch {
        // Stockage indisponible : la raison s'affiche simplement à chaque fois.
      }
      if (vue !== etat) {
        raison = texte;
        try {
          localStorage.setItem(CLE_RAISON, etat ?? '');
        } catch {
          // idem
        }
      }
    }
    focusFirst();
  });

  function annuler() {
    if (!enCours.confirmation || enCours.occupe) return;
    enCours.confirmation = null;
    feedback('back');
    const el = retour;
    requestAnimationFrame(() => el?.isConnected && el.focus({ preventScroll: true }));
  }

  async function fermer() {
    const d = enCours.confirmation;
    if (!d || enCours.occupe) return;
    let issue;
    try {
      issue = await enCours.fermer();
    } catch (e) {
      erreur = e instanceof Error ? e.message : String(e);
      feedback('err');
      focusFirst();
      return;
    }
    enCours.confirmation = null;
    feedback('ok');
    // La carte disparaît avec le bouton qui avait le focus : il revient à la tuile choisie de l'accueil.
    requestAnimationFrame(() => {
      const tuile = document.querySelector<HTMLElement>('.slot.focused .tile');
      if (tuile) tuile.focus({ preventScroll: true });
      else focusFirst();
    });
    // Arrêt direct : le jeu suivant part aussitôt. Sinon la page d'Android (« Forcer l'arrêt ») s'ouvre
    // et le jeu suivant part au retour ici.
    if (!issue.direct && d.ensuite) enCours.apresRetour = d.ensuite;
    else d.ensuite?.();
  }

  async function reprendre() {
    enCours.confirmation = null;
    try {
      await enCours.reprendre();
    } catch {
      feedback('err');
    }
  }

  onMount(() => {
    window.addEventListener('ludo:modal-back', annuler);
    return () => window.removeEventListener('ludo:modal-back', annuler);
  });

  const d = (ms: number) => (reducedMotion() ? 0 : ms);
</script>

{#if demande}
  <div class="voile" transition:fade={{ duration: d(200) }}></div>
  <div class="boite" data-modal role="alertdialog" aria-labelledby="fermer-titre" aria-describedby="fermer-texte"
    in:scale={{ start: 0.98, duration: d(260), easing: cubicOut }} out:scale={{ start: 0.98, duration: d(160) }}>
    {#if demande.titreEnsuite}
      <h2 id="fermer-titre">{titre} est en cours</h2>
      <p id="fermer-texte">Fermer ce jeu pour lancer {demande.titreEnsuite} ? La progression non sauvegardée dans le jeu sera perdue.</p>
      {#if raison}<p class="etape">{raison}</p>{/if}
      {#if parPage}<p class="etape">Android affiche d'abord la page de l'appli : « Forcer l'arrêt » ferme le jeu, puis le suivant se lance.</p>{/if}
      <div class="acts">
        <button class="act plein focusable" data-autofocus disabled={enCours.occupe} onclick={() => void fermer()}><span class="touche" aria-hidden="true">A</span>{enCours.occupe ? 'Fermeture…' : 'Fermer et lancer'}</button>
        <button class="act focusable" disabled={enCours.occupe} onclick={() => void reprendre()}>Reprendre {titre}</button>
      </div>
    {:else}
      <h2 id="fermer-titre">Fermer le jeu ?</h2>
      <p id="fermer-texte">{titre}. La progression non sauvegardée dans le jeu sera perdue ; la dernière sauvegarde part ensuite vers le serveur.</p>
      {#if raison}<p class="etape">{raison}</p>{/if}
      {#if parPage}<p class="etape">Android affiche ensuite la page de l'appli : « Forcer l'arrêt » ferme le jeu et ramène ici.</p>{/if}
      <div class="acts">
        <button class="act plein focusable" data-autofocus disabled={enCours.occupe} onclick={() => void fermer()}><span class="touche" aria-hidden="true">A</span>{enCours.occupe ? 'Fermeture…' : 'Fermer le jeu'}</button>
        <button class="act focusable" disabled={enCours.occupe} onclick={annuler}><span class="touche" aria-hidden="true">B</span>Annuler</button>
      </div>
    {/if}
    {#if erreur}<p class="erreur" role="alert">{erreur}</p>{/if}
  </div>
  <!-- Consignes de la boîte, par-dessus celles de l'écran dessous (qui décriraient l'accueil). -->
  <div class="consignes">
    <ButtonHints hints={[{ key: 'A', label: demande.titreEnsuite ? 'Fermer et lancer' : 'Fermer le jeu' }, { key: 'B', label: 'Annuler' }]} />
  </div>
{/if}

<style>
  .voile { position: fixed; inset: 0; background: rgba(8, 9, 12, 0.62); z-index: 40; }
  .boite {
    position: fixed; z-index: 41; left: 50%; top: 50%; translate: -50% -50%;
    width: min(560px, calc(100vw - 48px)); box-sizing: border-box;
    background: #1f232c; border: 1px solid rgba(255, 255, 255, 0.14); border-radius: 22px;
    padding: 26px; display: flex; flex-direction: column; gap: 14px; color: var(--ink, #f5f7fa);
  }
  h2 { margin: 0; font-size: 24px; font-weight: 800; line-height: 1.2; }
  p { margin: 0; font-size: 16px; line-height: 1.45; color: #d6dbe4; }
  .acts { display: flex; flex-direction: column; gap: 10px; margin-top: 6px; }
  .act {
    min-height: 56px; border-radius: 16px; font: inherit; font-size: 18px; font-weight: 700;
    background: transparent; color: #f5f7fa; border: 2px solid rgba(255, 255, 255, 0.62); cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center; gap: 10px;
  }
  .touche {
    width: 26px; height: 26px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
    font-size: 14px; font-weight: 800; border: 2px solid currentColor; box-sizing: border-box; flex-shrink: 0;
  }
  /* Fond opaque : --ground est translucide sur l’accueil (ambiance), la barre dessous transparaîtrait. */
  .consignes { position: fixed; left: 0; right: 0; bottom: 0; z-index: 42; background: #15171c; }
  /* Écran du bas de la Thor (≈ 723×630 px logiques) : boîte presque pleine largeur, actions côte à côte
     comme sur une console (A à gauche, B à droite), au-dessus de la barre de consignes. */
  :global(html[data-ecran='bas']) .boite { width: calc(100vw - 32px); padding: 22px; gap: 12px; translate: -50% calc(-50% - 27px); }
  :global(html[data-ecran='bas']) h2 { font-size: 23px; }
  :global(html[data-ecran='bas']) .acts { flex-direction: row; }
  :global(html[data-ecran='bas']) .act { flex: 1; min-width: 0; padding: 0 12px; }
  .act.plein { background: var(--accent); color: #0e1116; border-color: transparent; font-weight: 800; }
  .act:focus-visible, .act:focus { outline: 3px solid #f5f7fa; outline-offset: 3px; }
  .act:disabled { opacity: 0.6; cursor: default; }
  .erreur { color: #ff8a94; font-size: 15px; }
  .etape { font-size: 15px; color: #c9d0db; }
</style>
