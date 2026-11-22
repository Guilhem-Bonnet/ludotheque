<script lang="ts" module>
  /** Dernière tuile choisie (id du jeu, ou ALL) : le retour à l'accueil rend le focus à cette tuile. */
  let lastFocused: string | null = null;
</script>

<script lang="ts">
  import StateNotice from '../lib/components/StateNotice.svelte';
  import { onMount, untrack } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { feedback } from '../lib/feedback';
  import { syncRhythm, play as playSound } from '../lib/sound';
  import { reducedMotion, rushing, DUR } from '../lib/motion';
  import Ambiance from '../lib/components/Ambiance.svelte';
  import Avatar from '../lib/components/Avatar.svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import SyncStatus from '../lib/components/SyncStatus.svelte';
  import OptionsMenu from '../lib/components/OptionsMenu.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api } from '../lib/api';
  import { focusFirst, input } from '../lib/nav';
  import { glide } from '../lib/glide';
  import { router } from '../lib/router.svelte';
  import type { DeviceInfo, Game } from '../lib/types';
  import { play } from '../lib/play';
  import { appUpdate } from '../lib/appUpdate.svelte';

  let { device }: { device: DeviceInfo } = $props();

  let games = $state<Game[]>([]);
  /** Toute la bibliothèque du profil : sert à l'état global de synchronisation de l'en-tête. */
  let all = $state<Game[]>([]);
  let focused = $state(0);
  let updatesCount = $state(0);
  let clock = $state(now());

  function now() {
    return new Date().toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  }

  onMount(() => {
    // Horloge : mise à jour au changement de minute (échéance calculée, pas de rafraîchissement périodique).
    let t: ReturnType<typeof setTimeout>;
    const nextMinute = () => {
      clock = now();
      t = setTimeout(nextMinute, 60_000 - (Date.now() % 60_000) + 50);
    };
    t = setTimeout(nextMinute, 60_000 - (Date.now() % 60_000) + 50);
    const profile = router.profile!;
    void load(profile.id);
    // Les mises à jour ne bloquent jamais l'accueil : un échec laisse simplement le compteur à 0.
    api.updates(router.profile?.id).then((u) => (updatesCount = u.length)).catch(() => (updatesCount = 0));
    const onAction = (ev: Event) => {
      const g = games[focused];
      const onTile = (document.activeElement as HTMLElement | null)?.classList.contains('tile');
      if ((ev as CustomEvent).detail === 'x' && g && onTile && !options) {
        feedback('ok');
        options = g;
        return;
      }
      if ((ev as CustomEvent).detail === 'y' && g && !options) {
        feedback('ok');
        openGame(g);
      }
    };
    window.addEventListener('ludo:action', onAction);
    return () => {
      clearTimeout(t);
      window.removeEventListener('ludo:action', onAction);
    };
  });

  let launching = $state<string | null>(null);

  /* Chargement réel des jeux récents et de la bibliothèque ; erreur = écran « Serveur injoignable ». */
  let loadError = $state<string | null>(null);
  let loaded = $state(false);
  async function load(profile: string) {
    loadError = null;
    try {
      const [g, lib] = await Promise.all([api.recent(profile), api.library(profile)]);
      games = g;
      all = lib;
      // Retour d'écran : le focus revient sur la tuile quittée, la rangée y est déjà placée (sans glisser).
      const back = lastFocused === ALL ? g.length : g.findIndex((x) => x.id === lastFocused);
      start = Math.max(0, back);
      focused = start;
      ambient = start;
      jump = true;
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
    loaded = true;
    focusFirst();
  }

  /* Menu Options du jeu sélectionné (X). */
  let options = $state<Game | null>(null);
  function tileOf(id: string) {
    return document.querySelector<HTMLElement>(`.slot.focused .tile`) ?? document.querySelector<HTMLElement>('.tile');
  }
  function closeOptions() {
    options = null;
    feedback('back');
    requestAnimationFrame(() => tileOf('')?.focus({ preventScroll: true }));
  }
  function changeGame(g: Game) {
    games = games.map((x) => (x.id === g.id ? g : x));
    all = all.map((x) => (x.id === g.id ? g : x));
    options = g;
  }
  let profileFocused = $state(false);

  /* Synchronisation manuelle (A sur le nuage de l'en-tête). */
  let syncing = $state(false);
  let syncFocused = $state(false);
  async function syncNow() {
    if (syncing) return;
    if (offline) { feedback('err'); return; }
    syncing = true;
    feedback('syncStart');
    const stopRhythm = syncRhythm();
    let lib: Game[];
    try {
      lib = await api.sync(router.profile!.id);
    } catch {
      // Serveur injoignable ou configuration illisible : rien n'a changé, aucun faux « à jour ».
      syncing = false;
      feedback('err');
      return;
    } finally {
      stopRhythm();
    }
    const byId = new Map(lib.map((g) => [g.id, g]));
    all = lib;
    games = games.map((g) => ({ ...g, sync: byId.get(g.id)?.sync ?? g.sync, syncError: byId.get(g.id)?.syncError }));
    syncing = false;
    feedback(lib.some((g) => g.syncError) ? 'err' : lib.some((g) => g.sync === 'conflit') ? 'conflict' : 'saved');
  }

  /** A ou clic sur une tuile : la fiche du jeu s'ouvre (le lancement se fait depuis la fiche ou le menu Options). */
  function openGame(g: Game) {
    router.heroGame = g.id;
    router.go({ name: 'fiche', game: g });
  }

  /* ——— Mouvement de la rangée (voir CHARTE-MOUVEMENT-SON.md, « Rangée de l'accueil ») ———
     Mise en page FIXE : chaque emplacement occupe 232 px + 20 px d'écart, quelle que soit la tuile
     choisie. L'agrandissement et le décalage des voisines sont des transformations (composées par
     la carte graphique, aucun recalcul de mise en page pendant l'animation) :
     - la tuile est dessinée à 232 px ; seule la tuile choisie est agrandie (×272/232). Mesuré : réduire
       les douze tuiles en permanence coûtait trois fois plus cher par image de défilement ;
     - les emplacements situés après la tuile choisie glissent de 40 px vers la droite.
     Grandir et décaler partagent durée et courbe : l'écart de 20 px reste constant pendant le mouvement.
     Défilement : ressort amorti critique (glide.ts), qui garde sa vitesse quand la cible change. */
  const SMALL = 232;
  const BIG = 272;
  const PITCH = SMALL + 20;
  const ALL = '#tous';
  let row: HTMLDivElement | undefined = $state();
  /** Tuile qui reçoit le focus à l'arrivée sur l'écran. */
  let start = $state(0);
  /** Prochain placement sans glisser (arrivée sur l'écran, retour d'une fiche). */
  let jump = false;

  /** Entrée directionnelle (clavier, manette) : la tuile choisie se place en deuxième position. */
  const anchor = (i: number) => Math.max(0, (i - 1) * PITCH);

  /** Pointage (souris, toucher) : on ne déplace pas ce que la main vise ; seulement révéler une tuile coupée. */
  function reveal(i: number) {
    const slot = row?.querySelectorAll<HTMLElement>('[data-slot]')[i];
    if (!row || !slot) return;
    const g = glide(row, 'x');
    const x = slot.offsetLeft;  // emplacement de la tuile choisie (non décalé) : sa position finale
    const m = 24;
    if (x - m < g.goal) g.to(x - m);
    else if (x + BIG + m > g.goal + row.clientWidth) g.to(x + BIG + m - row.clientWidth);
  }

  $effect(() => {
    const i = focused;
    const n = games.length;
    if (!row || !n) return;
    lastFocused = i >= n ? ALL : games[i].id;
    untrack(() => {
      const g = glide(row!, 'x');
      if (jump) {
        jump = false;
        // La rangée vient d'être dessinée : on attend sa mise en page pour connaître sa largeur.
        requestAnimationFrame(() => g.to(anchor(i), true));
      } else if (input.kind === 'mouse' && document.documentElement.dataset.input === 'mouse') {
        /* survol : la rangée ne bouge pas sous le pointeur (molette, glisser, flèches de bord). Le CSS
           souris dépend de html[data-input], posé par nav.ts (PR entrees) : sans lui, ancien comportement. */
      } else if (input.kind === 'mouse' || input.kind === 'touch') reveal(i);
      else g.to(anchor(i));
    });
  });

  /* Molette : une souris à crans fait avancer la rangée d'une tuile par cran (alignée sur les tuiles) ;
     un pavé tactile défile directement, à la vitesse des doigts. Le focus ne bouge pas. */
  $effect(() => {
    const el = row;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey) return;
      const g = glide(el, 'x');
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) {
        g.cancel(); // défilement horizontal natif (pavé tactile) : il a la main
        return;
      }
      e.preventDefault();
      const notched = e.deltaMode !== 0 || Math.abs(e.deltaY) >= 50;
      if (notched) {
        const base = Math.round(g.goal / PITCH) * PITCH;
        g.to(base + Math.sign(e.deltaY) * PITCH);
      } else {
        g.cancel();
        el.scrollLeft += e.deltaY;
      }
    };
    // Défilement fait à la main (doigt, pavé) : horodaté, pour qu'un toucher qui arrête l'inertie
    // ne soit pas pris pour un choix de tuile.
    const onScroll = (e: Event) => { if (!glide(el, 'x').own) { handScrollAt = e.timeStamp; handMoving = true; } };
    // Fin de l'inertie annoncée par le moteur (`scrollend`) ; à défaut, la fenêtre de 200 ms suffit.
    const onScrollEnd = () => (handMoving = false);
    el.addEventListener('wheel', onWheel, { passive: false });
    el.addEventListener('scroll', onScroll, { passive: true });
    el.addEventListener('scrollend', onScrollEnd, { passive: true });
    return () => {
      el.removeEventListener('wheel', onWheel);
      el.removeEventListener('scroll', onScroll);
      el.removeEventListener('scrollend', onScrollEnd);
    };
  });

  /* Flèches de bord (souris) : visibles seulement s'il reste des tuiles de ce côté. Mises à jour sur
     les événements `scroll` et ResizeObserver de la rangée, jamais à intervalle. Un clic avance d'une
     « page » de tuiles par le ressort (glide.ts), alignée sur les emplacements. */
  let canLeft = $state(false);
  let canRight = $state(false);
  $effect(() => {
    const el = row;
    void games.length; // tuiles arrivées ou retirées : largeur à défiler changée sans redimensionnement
    if (!el) return;
    const edges = () => {
      const g = glide(el, 'x');
      const x = g.goal;
      canLeft = x > 4;
      canRight = x < el.scrollWidth - el.clientWidth - 4;
    };
    edges();
    const first = requestAnimationFrame(edges); // après la mise en page des tuiles
    const ro = new ResizeObserver(edges);
    ro.observe(el);
    el.addEventListener('scroll', edges, { passive: true });
    return () => { cancelAnimationFrame(first); ro.disconnect(); el.removeEventListener('scroll', edges); };
  });
  function page(dir: -1 | 1) {
    if (!row) return;
    const g = glide(row, 'x');
    const step = Math.max(1, Math.floor((row.clientWidth - 2 * PITCH) / PITCH)) * PITCH;
    g.to(Math.round((g.goal + dir * step) / PITCH) * PITCH);
  }

  /* Toucher : un premier toucher choisit la tuile (elle grandit), un second l'ouvre — comme la souris,
     qui choisit au survol et ouvre au clic. Un toucher qui arrête la rangée lancée ne choisit rien. */
  let handScrollAt = -1e9;
  let handMoving = false;
  let down: { kind: string; focused: number; stop: boolean } | null = null;
  function onRowPointerDown(e: PointerEvent) {
    down = { kind: e.pointerType, focused, stop: e.pointerType !== 'mouse' && (handMoving || e.timeStamp - handScrollAt < 200) };
  }
  function activate(i: number, el: HTMLElement | undefined, open: () => void) {
    const d = down;
    down = null;
    if (d && d.kind !== 'mouse') {
      if (d.stop) return;
      if (d.focused !== i) {
        el?.focus({ preventScroll: true });
        focused = i;
        return;
      }
    }
    open();
  }

  /* Le fond suit la tuile un instant après elle (140 ms) : la tuile répond d'abord, le monde suit ;
     en défilement rapide, le fond ne change qu'à l'arrêt (pas de fondus empilés d'images floutées).
     Délai unique recalculé à chaque changement (anti-rebond sur le dernier événement). */
  let ambient = $state(0);
  $effect(() => {
    const i = focused;
    if (i === untrack(() => ambient)) return;
    const t = setTimeout(() => (ambient = i), 140);
    return () => clearTimeout(t);
  });


  /** Mise en scène du lancement : la tuile se soulève, les autres s'estompent, puis l'écran change. */
  function launch(game: Game, el: HTMLElement) {
    launching = game.id;
    void play(game, el);
  }

  const current = $derived(games[focused]);
  const world = $derived(games[ambient] ?? current);

  // Ambiance (direction C) : la lumière se place au-dessus de la tuile sélectionnée.
  const lightX = $derived(Math.min(14 + ambient * 18, 86));
  const offline = $derived(device.server === 'hors_ligne');
</script>

<div class="screen">
  <Ambiance art={world?.art} cover={world?.cover} background={world?.background} x={lightX} key={world?.id} />
  <header class="rise" style:--i={0}>
    <button class="who pill focusable" aria-label="Profil de {router.profile!.name}" onclick={() => router.go({ name: 'profil' })} onfocus={() => (profileFocused = true)} onblur={() => (profileFocused = false)}>
      <Avatar profile={router.profile!} size={46} />
      <div class="who-text">
        <span class="name">{router.profile!.name}</span>
        <span class="device">{device.name}</span>
      </div>
    </button>
    <div class="status">
      <SyncStatus games={all} {offline} busy={syncing} onclick={syncNow} onfocus={() => (syncFocused = true)} onblur={() => (syncFocused = false)} />
      <span class="clock">{clock}</span>
    </div>
  </header>

  {#if loadError}
    <div class="notice-wrap"><StateNotice title="Serveur injoignable" detail={loadError} action="Réessayer" onaction={() => void load(router.profile!.id)} /></div>
  {:else if loaded && games.length === 0}
    <div class="notice-wrap"><StateNotice icon="grid" title="Aucune partie récente" detail="Les jeux joués apparaissent ici. Tous les jeux sont dans la Bibliothèque." action="Bibliothèque" onaction={() => router.go({ name: 'bibliotheque' })} /></div>
  {/if}
  <section aria-label="Jeux récents" class="row-wrap">
    <div class="row" bind:this={row} data-own-scroll onpointerdowncapture={onRowPointerDown}>
      {#each games as game, i (game.id)}
        <!-- Une seule tuile par jeu : elle grandit au focus sans être recréée (le focus reste dessus). -->
        <div class="slot rise" class:focused={i === focused} class:after={i > focused} style:--i={i} data-slot>
          <div class="lift">
            <Tile
              {game}
              size={SMALL}
              big={false}
              words={i === focused}
              scale={i === focused ? BIG / SMALL : 1}
              hero={router.heroGame === game.id}
              dimmed={launching !== null && launching !== game.id}
              lifting={launching === game.id}
              autofocus={i === start}
              onfocus={() => (focused = i)}
              onclick={(el) => activate(i, el, () => openGame(game))}
            />
          </div>
          {#if i === focused && game.last}
            <div class="device-tab">
              <Icon name={game.last.deviceKind} size={16} color="var(--ink-4)" />
              <span>{game.last.device} · {game.last.when}</span>
              {#if game.sync === 'a_jour'}
                <span class="tab-state"><Icon name="cloud-ok" size={15} color="var(--ink-4)" label="À jour" /></span>
              {:else if game.sync === 'envoi_en_attente'}
                <span class="tab-state"><Icon name="cloud-pending" size={15} color="var(--pending)" label="Envoi en attente" /></span>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      {#if games.length}
        <!-- Fin de rangée : toute la collection (comme « Tous les logiciels » de la console). -->
        <div class="slot rise" class:focused={focused === games.length} class:after={focused < games.length} style:--i={games.length} data-slot>
          <div class="lift">
            <button class="all-games focusable" data-hover-focus data-autofocus={start === games.length ? '' : undefined}
              onfocus={() => (focused = games.length)}
              onclick={(e) => activate(games.length, e.currentTarget, () => router.go({ name: 'bibliotheque' }))}>
              <Icon name="grid" size={40} />
              <span>Tous les jeux</span>
            </button>
          </div>
        </div>
      {/if}
    </div>
    <!-- Souris seulement (html[data-input=mouse]) : la manette et le clavier font défiler en changeant de tuile. -->
    {#if canLeft}<button class="edge left" tabindex="-1" aria-label="Jeux précédents" data-sound="none" onpointerdown={(e) => e.preventDefault()} transition:fade={{ duration: reducedMotion() ? 0 : DUR.quick }} onclick={() => page(-1)}><svg width="28" height="28" viewBox="0 0 24 24" aria-hidden="true"><path d="M14.5 6 8.5 12l6 6" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" /></svg></button>{/if}
    {#if canRight}<button class="edge right" tabindex="-1" aria-label="Jeux suivants" data-sound="none" onpointerdown={(e) => e.preventDefault()} transition:fade={{ duration: reducedMotion() ? 0 : DUR.quick }} onclick={() => page(1)}><svg width="28" height="28" viewBox="0 0 24 24" aria-hidden="true"><path d="m9.5 6 6 6-6 6" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" /></svg></button>{/if}

    {#if current}
      <!-- Le titre change avec la tuile : l'ancien s'efface (100 ms), PUIS le nouveau monte de 8 px — jamais deux
           titres superposés. En défilement rapide : remplacement direct, simple fondu court. -->
      <div class="caption-slot">
        {#key current.id}
          <div class="caption" in:fly={{ y: rushing() ? 0 : 8, duration: reducedMotion() ? 0 : rushing() ? 80 : DUR.std, delay: reducedMotion() || rushing() ? 0 : 100, easing: cubicOut }} out:fade={{ duration: reducedMotion() || rushing() ? 0 : 100 }}>
            <h1>{current.title}</h1>
            {#if current.playtime}<span>{current.playtime} de jeu</span>{/if}
          </div>
        {/key}
      </div>
    {/if}
  </section>

  <!-- Haut depuis un raccourci : retour à la tuile choisie (pas à la tuile la plus proche en géométrie). -->
  <nav aria-label="Raccourcis" class="rise" style:--i={6} data-nav-up=".slot.focused .focusable">
    <button class="shortcut focusable" onclick={() => router.go({ name: 'bibliotheque' })}>
      <span class="round"><Icon name="grid" size={26} /></span>Bibliothèque
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'mises-a-jour' })}>
      <span class="round"><Icon name="update" size={26} />{#if updatesCount + (appUpdate.status === 'available' ? 1 : 0)}<span class="count">{updatesCount + (appUpdate.status === 'available' ? 1 : 0)}</span>{/if}</span>Mises à jour
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'qui-joue' })}>
      <span class="round"><Icon name="people" size={26} /></span>Profils
    </button>
    <button class="shortcut focusable" onclick={() => router.go({ name: 'reglages' })}>
      <span class="round"><Icon name="settings" size={26} /></span>Réglages
    </button>
  </nav>

  {#if options}
    <OptionsMenu game={options} profile={router.profile!.id} onclose={closeOptions}
      onplay={() => { const g = options!; options = null; const el = tileOf(g.id); if (el) launch(g, el); }}
      ondetails={() => { const g = options!; options = null; router.heroGame = g.id; router.go({ name: 'fiche', game: g }); }}
      onchange={changeGame} />
  {/if}
  {#if options}
    <ButtonHints hints={[{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Fermer' }]} />
  {:else if syncFocused}
    <ButtonHints hints={[{ key: 'A', label: 'Synchroniser' }]} />
  {:else if profileFocused}
    <ButtonHints hints={[{ key: 'A', label: 'Profil' }]} />
  {:else}
    <ButtonHints hints={[{ key: 'A', label: 'Ouvrir' }, { key: 'X', label: 'Options' }]} />
  {/if}
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; position: relative; isolation: isolate; }
  /* Pastilles translucides (reprises de la direction B). */
  .pill { background: rgba(255, 255, 255, 0.1); }
  .who.pill { all: unset; cursor: pointer; display: flex; align-items: center; padding: 5px 20px 5px 5px; border-radius: 40px; gap: 12px; background: rgba(255, 255, 255, 0.1); transition: background-color var(--t-quick) ease-out, transform var(--t-quick) var(--ease-pop), box-shadow var(--t-std) var(--ease-land); }
  .who.pill.focusable:focus { background: rgba(255, 255, 255, 0.2); transform: translateY(-2px); box-shadow: 0 0 0 3px var(--accent), 0 8px 18px rgba(0, 0, 0, 0.3); }
  .clock { font-weight: 600; color: var(--ink); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 28px 48px 0; }
  .who { display: flex; align-items: center; gap: 14px; }
  .who-text { display: flex; flex-direction: column; gap: 2px; }
  .name { font-size: 17px; font-weight: 600; }
  .device { font-size: 12px; color: rgba(255, 255, 255, 0.72); }
  .status { display: flex; align-items: center; gap: 14px; font-size: 15px; color: var(--ink); }

  .row-wrap { padding: 48px 0 0 48px; position: relative; }
  .row { position: relative; display: flex; align-items: flex-start; gap: 20px; overflow-x: auto; overscroll-behavior-x: contain; overflow-y: visible; padding: 22px 64px 16px 24px; margin: -22px 0 0 -24px; scrollbar-width: none; touch-action: pan-x; }
  /* Marge gauche de 24 px : la tuile agrandie depuis son centre (souris) déborde de 20 px sans être rognée. */
  .row::-webkit-scrollbar { display: none; }

  /* ——— Mouvement de la rangée (bloc propre à cette PR) ———
     Emplacement fixe de 232 px ; la tuile choisie est agrandie à 272 px, ancrée à gauche et centrée en
     hauteur (elle déborde de 20 px en haut et en bas, comme avant). Durée 260 ms, courbe signature : une tuile qui se pose. En défilement rapide (`data-rush`),
     150 ms : chaque pas finit avant le suivant (≈ 110 ms) au lieu d'empiler des états intermédiaires. */
  .row { --move-t: var(--t-move); --move-ease: var(--ease-land); }
  :global(html[data-rush]) .row { --move-t: var(--t-rush); }
  .slot { display: flex; flex-direction: column; flex-shrink: 0; width: 232px; transition: transform var(--move-t) var(--move-ease); }
  .slot.after { transform: translateX(40px); }
  .lift { width: 232px; height: 232px; margin: 20px 0; transform-origin: 0 50%; transition: transform var(--move-t) var(--move-ease); }
  .slot.focused .lift { transform: scale(1.17241); }
  /* Souris : la tuile survolée grandit sur place (depuis son centre), au-dessus de ses voisines, sans les
     pousser : rien ne bouge sous le pointeur (DEMANDE entrees 22:03, mesure : 0 px de déplacement). */
  .slot.focused { position: relative; z-index: 1; }
  :global(html[data-input='mouse']) .slot.after { transform: none; }
  :global(html[data-input='mouse']) .lift { transform-origin: 50% 50%; }
  @media (prefers-reduced-motion: reduce) {
    .slot, .lift { transition: none; }
  }
  /* Flèches de bord : pastille translucide par-dessus la rangée, à la souris seulement. */
  .edge {
    all: unset; cursor: pointer; position: absolute; top: calc(48px + 136px - 28px); z-index: 2; display: none;
    width: 56px; height: 56px; border-radius: 50%; align-items: center; justify-content: center;
    background: rgba(12, 14, 20, 0.68); color: var(--ink); box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35), inset 0 0 0 1px rgba(255, 255, 255, 0.14);
    transition: transform var(--t-quick) var(--ease-pop), background-color var(--t-quick) ease-out;
  }
  :global(html[data-input='mouse']) .edge { display: flex; }
  .edge.left { left: 12px; }
  .edge.right { right: 20px; }
  .edge:hover { background: rgba(12, 14, 20, 0.75); transform: scale(1.08); }
  .edge:active { transform: scale(0.94); }
  @media (prefers-reduced-motion: reduce) { .edge, .edge:hover, .edge:active { transition: none; transform: none; } }
  /* Fin de rangée : la dernière tuile, décalée par transformation, ne compte pas dans la largeur à
     défiler ; une cale garde la même marge qu'à gauche quand « Tous les jeux » est choisie. */
  .row::after { content: ''; flex: 0 0 28px; }
  /* ——— fin du bloc mouvement ——— */

  .all-games {
    all: unset; cursor: pointer; box-sizing: border-box; width: 232px; height: 232px; border-radius: var(--radius-tile);
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 14px;
    background: rgba(255, 255, 255, 0.08); box-shadow: inset 0 0 0 2px rgba(255, 255, 255, 0.12);
    font-size: 18px; font-weight: 600; color: var(--ink);
    transition: background-color var(--t-quick) ease-out, box-shadow var(--t-std) var(--ease-land);
  }
  .all-games:focus { background: rgba(255, 255, 255, 0.14); box-shadow: inset 0 0 0 4px var(--accent), 0 22px 44px rgba(0, 0, 0, 0.45); }

  /* Ligne discrète sous la tuile en focus : appareil et heure de la dernière partie, en gris neutre. */
  .device-tab {
    width: 264px; margin: 12px 4px 0; white-space: nowrap; color: rgba(255, 255, 255, 0.7);
    display: flex; align-items: center; gap: 7px; font-size: 13px;
  }
  .tab-state { margin-left: auto; display: flex; }
  @media (prefers-reduced-motion: no-preference) {
    .device-tab { animation: tab-out 240ms var(--ease-land) 80ms both; }
    /* En défilement rapide, la ligne attend l'arrêt pour apparaître (moins de texte qui clignote). */
    :global(html[data-rush]) .device-tab { animation: none; opacity: 0; }
    @keyframes tab-out { from { transform: translateY(-4px); opacity: 0; } to { transform: none; opacity: 1; } }
  }

  .caption-slot { margin-top: 26px; height: 40px; position: relative; }
  .caption { position: absolute; inset: 0 auto auto 0; white-space: nowrap; display: flex; align-items: baseline; gap: 16px; }
  .caption h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; text-shadow: 0 2px 14px rgba(0, 0, 0, 0.45); }
  .caption span { font-size: 16px; color: rgba(255, 255, 255, 0.72); }

  nav { margin-top: auto; display: flex; justify-content: center; gap: 36px; padding-bottom: 26px; }
  .shortcut { all: unset; cursor: pointer; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 13px; color: var(--ink-2); border-radius: 14px; padding: 4px; }
  .shortcut:disabled { opacity: 0.45; cursor: default; }
  .shortcut:focus { box-shadow: none; color: var(--accent-ink); }
  .shortcut:focus .round { transform: translateY(-3px) scale(1.06); background: var(--accent-soft); color: var(--accent-ink); box-shadow: 0 0 0 3px var(--ground), 0 0 0 6px var(--accent), 0 10px 20px rgba(0,0,0,.35); }
  .round { transition: transform var(--t-quick) var(--ease-pop), background-color var(--t-quick) ease-out, box-shadow var(--t-quick) var(--ease-land); width: 60px; height: 60px; border-radius: 50%; background: rgba(255, 255, 255, 0.13); display: flex; align-items: center; justify-content: center; position: relative; color: var(--ink); }
  .count { position: absolute; top: -2px; right: -2px; min-width: 22px; height: 22px; border-radius: 11px; background: var(--accent); color: var(--ground); font-size: 12px; font-weight: 700; display: flex; align-items: center; justify-content: center; }
  .notice-wrap { padding: 40px 48px 0; }
</style>
