<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Avatar from './lib/components/Avatar.svelte';
  import CoverArt from './lib/components/CoverArt.svelte';
import Batterie from './lib/components/Batterie.svelte';
  import { reducedMotion } from './lib/motion';
  import type { EtatHaut, JeuHaut } from './lib/ecranHaut.svelte';
  import type { Console, Profile, SyncState } from './lib/types';

  // Écran du HAUT de l'Ayn Thor (1920×1080 en paysage, OLED), affichage seul : l'écran du bas pousse
  // l'état du jeu choisi par `window.__ludoHaut` (DoubleEcran.kt → evaluateJavascript). Aucune
  // minuterie, aucune animation infinie : un fondu enchaîné quand le jeu change, puis plus rien ne
  // bouge (l'écran reste allumé pendant toute la navigation : CPU et OLED au repos).
  // WebView système de la Thor = Chromium 109 (dumpsys webviewupdate, 22/11) : ni color-mix() (111)
  // ni text-wrap (114) ; rien ici n'en dépend.
  // État arrivé avant le montage (main.ts le garde) : affiché tout de suite.
  const w = window as unknown as { __ludoHaut: (e: EtatHaut) => void; __ludoHautRecu?: EtatHaut };
  let etat = $state<EtatHaut>(w.__ludoHautRecu ?? { ecran: 'accueil' });
  w.__ludoHaut = (e) => (etat = e);

  // Jaquette ENTIÈRE : le cadre prend le rapport largeur/hauteur réel de l'image (3DS, DS, GameCube n'ont pas
  // celui de la Switch), lu à son chargement ; 5:7 tant qu'elle n'est pas arrivée. Aucun recadrage.
  let ratios = $state<Record<string, number>>({});
  function mesure(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    const src = img.getAttribute('src') ?? '';
    if (img.naturalWidth > 0 && img.naturalHeight > 0) ratios[src] = img.naturalWidth / img.naturalHeight;
  }

  const jeu = $derived(etat.jeu);
  const profil = $derived(etat.profil ? ({ id: '', ...etat.profil } as Profile) : undefined);
  const accent = $derived(etat.profil?.accent ?? '#4cc3ff');

  const SYNC: Record<SyncState, { label: string; color: string }> = {
    a_jour: { label: 'À jour', color: 'var(--ok)' },
    synchronisation: { label: 'Synchronisation…', color: 'var(--sync)' },
    envoi_en_attente: { label: 'Envoi en attente', color: 'var(--pending)' },
    conflit: { label: 'Conflit à régler', color: 'var(--conflict)' },
    hors_ligne: { label: 'Hors ligne', color: 'var(--offline)' },
    aucune: { label: 'Aucune', color: 'var(--ink-4)' }, // la carte s'intitule déjà « Sauvegarde »
    a_recuperer: { label: 'Nouvelle sur le serveur', color: 'var(--sync)' },
  };

  const CONSOLES: Record<Console, string> = { switch: 'Switch', switch2: 'Switch 2', '3ds': '3DS', ds: 'DS', wiiu: 'Wii U', gc: 'GameCube' };

  function ici(j: JeuHaut): string {
    if (j.install === 'installe') return `Jouable ici · ${j.emulator}`;
    if (j.stream) return 'Flux depuis le PC';
    if (j.install === 'telechargement') return 'Téléchargement…';
    return 'Non installé ici';
  }

  /** Couleur d'ambiance : couleur vive de la jaquette (direction C), jamais un gris terne. */
  const tint = $derived(jeu ? (jeu.art?.from ?? jeu.tile) : accent);
  const tint2 = $derived(jeu ? (jeu.art?.to ?? jeu.tile) : accent);
  const photo = $derived(jeu?.background ?? jeu?.cover);
  let failed = $state('');

  const d = (ms: number) => (reducedMotion() ? 0 : ms);
</script>

<main class="haut" class:repos-mode={!jeu} style:--tint={tint} style:--tint2={tint2} style:--accent={accent}>
  {#key jeu?.id ?? 'repos'}
    <div class="fond" in:fade={{ duration: d(420) }} out:fade={{ duration: d(420) }} aria-hidden="true">
      <div class="aplat"></div>
      {#if photo && failed !== photo}
        <img class="photo" src={photo} alt="" draggable="false" onerror={() => (failed = photo ?? '')} />
      {/if}
      <div class="voile"></div>
    </div>
  {/key}

  {#if jeu}
    {#key jeu.id}
      <section class="jeu" in:fade={{ duration: d(280) }}>
        <div class="cartouche" in:fly={{ y: 18, duration: d(420), easing: cubicOut }} style:background={jeu.tile}
          style:--r={jeu.cover && failed !== jeu.cover ? (ratios[jeu.cover] ?? 5 / 7) : 5 / 7}>
          {#if jeu.cover && failed !== jeu.cover}
            <img src={jeu.cover} alt="Jaquette de {jeu.title}" draggable="false" onload={mesure} onerror={(e) => (failed = (e.currentTarget as HTMLImageElement).getAttribute('src') ?? '')} />
          {:else if jeu.art}
            <CoverArt art={jeu.art} />
          {/if}
        </div>
        <div class="infos">
          <div class="pastilles">
            <span class="pastille console">{CONSOLES[jeu.console] ?? jeu.console}</span>
            <span class="pastille">{ici(jeu)}</span>
            {#if etat.lancement}<span class="pastille vive">Lancement…</span>{/if}
          </div>
          <h1>{jeu.title}</h1>
          <div class="cartes">
            <div class="carte">
              <span class="titre">Temps de jeu</span>
              <span class="valeur">{jeu.playtime || '—'}</span>
            </div>
            <div class="carte">
              <span class="titre">Dernière partie</span>
              {#if jeu.last}
                <span class="valeur moyen">{jeu.last.device}</span>
                <span class="detail">{jeu.last.when}</span>
              {:else}
                <span class="valeur moyen">Jamais lancé</span>
              {/if}
            </div>
            <div class="carte">
              <span class="titre">Sauvegarde</span>
              <span class="valeur moyen etat"><i style:background={SYNC[jeu.sync]?.color}></i>{SYNC[jeu.sync]?.label ?? '—'}</span>
            </div>
          </div>
        </div>
      </section>
    {/key}
  {:else}
    <section class="repos" in:fade={{ duration: d(280) }}>
      {#if profil}
        <Avatar profile={profil} size={168} />
        <h1>{profil.name}</h1>
      {:else}
        <h1>Ludothèque</h1>
      {/if}
      <p>Choisir un jeu sur l'écran du bas</p>
    </section>
  {/if}

  {#if jeu && profil}
    <footer class="joueur">
      <Avatar profile={profil} size={40} />
      <span>{profil.name}</span>
      <Batterie />
    </footer>
  {/if}
</main>

<style>
  /* Mise en page en unités d'écran : 1920×1080 en paysage (usage normal), lisible aussi en portrait. */
  .haut {
    position: fixed; inset: 0; overflow: hidden; background: #000; color: #fff;
    font-size: clamp(14px, 1.6vmin + 0.7vmax, 40px);
  }
  .fond, .aplat, .voile { position: absolute; inset: 0; }
  .aplat { background: radial-gradient(110% 90% at 28% 45%, var(--tint) 0%, var(--tint2) 26%, transparent 72%); opacity: .55; }
  .photo { position: absolute; inset: -4%; width: 108%; height: 108%; object-fit: cover; filter: blur(14px) saturate(1.2) brightness(.5); opacity: .75; }
  .repos-mode .aplat { background: radial-gradient(70% 70% at 50% 45%, var(--tint) 0%, transparent 70%); opacity: .45; }
  .repos-mode .voile { background: radial-gradient(90% 90% at 50% 45%, transparent 0%, #000 100%); }
  /* OLED : bords vraiment noirs, centre lumineux. */
  .voile { background: radial-gradient(120% 95% at 34% 50%, transparent 0%, rgba(0,0,0,.35) 55%, #000 100%); }

  .jeu {
    /* Haut : bande réservée au profil (en haut à droite), le contenu ne la touche jamais. */
    position: absolute; inset: 0; display: flex; align-items: center; gap: 5vw;
    padding: 15vh 7vw 6vh; box-sizing: border-box;
  }
  .cartouche {
    /* Rapport réel de la jaquette (--r) dans une boîte de 34vw × 72vh au plus (34 : place pour « 12 h 40 » à côté d'une jaquette 3DS presque carrée) : largeur bornée par l'une ou
       l'autre, hauteur déduite. Image en `contain` : rien n'est coupé, même avant la mesure. */
    flex: none; aspect-ratio: var(--r, 0.714); width: min(34vw, calc(72vh * var(--r, 0.714))); height: auto;
    border-radius: 1.6em 1.6em 1em 1em; overflow: hidden;
    box-shadow: 0 2.4em 5em rgba(0,0,0,.6), inset 0 0 0 .14em rgba(255,255,255,.18); position: relative; isolation: isolate;
  }
  .cartouche img { width: 100%; height: 100%; object-fit: contain; display: block; }
  .cartouche :global(svg) { width: 100%; height: 100%; display: block; }
  .infos { min-width: 0; display: flex; flex-direction: column; gap: 1.4em; }
  .pastilles { display: flex; flex-wrap: wrap; gap: .6em; align-items: center; }
  .pastille { padding: .35em .9em; border-radius: 999px; background: rgba(255,255,255,.12); font-weight: 650; font-size: .9em; white-space: nowrap; }
  .pastille.console { background: rgba(0,0,0,.35); font-weight: 750; letter-spacing: .04em; }
  .pastille.vive { background: var(--accent); color: #10131a; }
  /* 2,6 em (et non plus 3,4) : la jaquette entière d'une 3DS est presque carrée, la colonne de texte est plus
     étroite ; trois lignes de titre tiennent sous la bande du profil (contrôle des 80 jeux). */
  h1 { margin: 0; font-size: 2.6em; line-height: 1.04; font-weight: 800; letter-spacing: -.01em; text-wrap: balance;
       display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .cartes { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: .8em; margin-top: .6em; }
  .carte { padding: 1em 1.1em; border-radius: 1em; background: rgba(255,255,255,.1); display: flex; flex-direction: column; gap: .3em; min-width: 0; }
  .titre { font-size: .82em; font-weight: 650; color: rgba(255,255,255,.75); }
  /* Jamais de « … » : un temps de jeu à 3 chiffres passe à la ligne plutôt que d'être coupé (contrôle des 80 jeux). */
  .valeur { font-size: 1.3em; font-weight: 800; line-height: 1.15; overflow-wrap: break-word; }
  .valeur.moyen { font-size: 1.3em; }
  /* Texte seul sur deux lignes au plus ; la valeur « Sauvegarde » (pastille + libellé) reste en flex (revue). */
  .valeur.moyen:not(.etat) { white-space: normal; line-height: 1.15; overflow-wrap: break-word; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .valeur.etat { white-space: normal; line-height: 1.15; }
  .detail { font-size: .85em; color: rgba(255,255,255,.8); }
  .etat { display: flex; gap: .45em; align-items: center; }
  .etat i { flex: none; width: .62em; height: .62em; border-radius: 50%; }

  .repos { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: .8em; }
  .repos h1 { font-size: 2.8em; }
  .repos p { margin: 0; font-size: 1.15em; font-weight: 600; color: rgba(255,255,255,.75); }

  .joueur {
    /* Profil en haut à droite (retour de Guilhem, 22/11), dégagé du reste : pastille translucide. */
    position: absolute; right: 3vw; top: 4vh; display: flex; gap: .6em; align-items: center;
    padding: .3em 1.1em .3em .3em; border-radius: 999px; background: rgba(0,0,0,.35);
    font-weight: 650; color: rgba(255,255,255,.92);
  }

  @media (orientation: portrait) {
    .jeu { flex-direction: column; justify-content: center; text-align: center; padding: 6vh 6vw; gap: 4vh; }
    .jeu { padding-top: 12vh; }
    .cartouche { width: min(70vw, calc(46vh * var(--r, 0.714))); }
    .pastilles { justify-content: center; }
  }
</style>
