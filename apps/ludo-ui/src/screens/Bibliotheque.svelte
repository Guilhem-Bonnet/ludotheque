<script lang="ts">
  import StateNotice from '../lib/components/StateNotice.svelte';
  import { onMount, untrack } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import OptionsMenu from '../lib/components/OptionsMenu.svelte';
  import Clavier from '../lib/components/biblio/Clavier.svelte';
  import Une from '../lib/components/biblio/Une.svelte';
  import { match, pieces, playtimeSeconds, type Match } from '../lib/components/biblio/search';
  import { biblio as st, SORTS, type Avail } from '../lib/components/biblio/state.svelte';
  import { play } from '../lib/play';
  import { api, isAndroid, isDemo } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst, input, virtualKey } from '../lib/nav';
  import { glideIntoView } from '../lib/glide';
  import { router } from '../lib/router.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import type { Console, DeviceInfo, Game, RemoteDevice } from '../lib/types';

  // Bibliothèque façon boutique de console : bannière « À la une » et une rangée par console quand
  // rien n'est filtré ; dès qu'une recherche, un filtre, un tri ou une console est choisi, grille
  // complète. Recherche tolérante (accents, fautes légères), résultats pendant la frappe.
  // Entrées : Y, « / » ou Ctrl+F ouvrent la recherche (manette : clavier à l'écran) ; B / Échap
  // effacent la recherche, puis quittent l'écran ; L1 / R1 changent de console.

  let { device }: { device: DeviceInfo } = $props();

  const TABS: { id: Console | 'tout'; label: string }[] = [
    { id: 'tout', label: 'Tout' }, { id: 'switch', label: 'Switch' }, { id: 'switch2', label: 'Switch 2' },
    { id: '3ds', label: '3DS' }, { id: 'ds', label: 'DS' }, { id: 'wiiu', label: 'Wii U' }, { id: 'gc', label: 'GameCube' },
  ];
  /** Jeux par rangée de la vue d'accueil (au-delà : « Tout voir »). */
  const ROW_MAX = 12;
  const SYNC_STATES = ['conflit', 'envoi_en_attente', 'a_recuperer'];

  st.enter(router.profile!.id);

  let games = $state<Game[]>([]);
  /** Rang « récent » : ordre de la rangée de l'accueil (jeux joués du plus récent au plus ancien). */
  let recentRank = $state(new Map<string, number>());
  let remotes = $state<RemoteDevice[]>([]);
  const remote = $derived(remotes.find((r) => r.id === st.where));
  let slide = $state<'left' | 'right' | 'none'>('none');

  /* Menu Options (X) du jeu en focus, comme à l'accueil. */
  let focusedId = $state<string | null>(st.focus);
  /**
   * Jeu à retrouver au retour d'une fiche : il porte `data-autofocus` (App rend le focus à la fin de la
   * transition d'écran). Oublié dès que la vue change (onglet, filtre, tri, recherche).
   */
  let restore = $state<string | null>(st.focus);
  let options = $state<Game | null>(null);
  function closeOptions() {
    const id = options?.id;
    options = null;
    feedback('back');
    requestAnimationFrame(() => focusGame(id));
  }
  function changeGame(g: Game) {
    games = games.map((x) => (x.id === g.id ? g : x));
    options = g;
  }

  /** Focus sur la tuile d'un jeu (grille, rangée ou bannière) ; faux si le jeu n'est pas affiché. */
  function focusGame(id: string | null | undefined): boolean {
    if (!id) return false;
    const el = document.querySelector<HTMLElement>(`[data-cell="${CSS.escape(id)}"] .tile`);
    if (!el) return false;
    el.focus({ preventScroll: true });
    return true;
  }

  // Indicateur d'onglet qui glisse d'un onglet à l'autre (continuité), au lieu d'un fond qui saute.
  let tabEls = $state<HTMLElement[]>([]);
  let indicator = $state({ x: 0, w: 0, ready: false });
  $effect(() => {
    const el = tabEls[st.tab];
    if (!el) return;
    const prev = untrack(() => indicator);
    indicator = { x: el.offsetLeft, w: el.offsetWidth, ready: prev.ready || prev.w > 0 };
  });

  // Recherche : la saisie (`st.query`) est appliquée après la dernière frappe (anti-rebond, délai de
  // garde unique relancé à chaque frappe ; aucune lecture périodique).
  /** Requête utile : vide si elle ne contient ni lettre ni chiffre (« - » ne filtre rien). */
  const clean = (q: string) => (/[\p{L}\p{N}]/u.test(q) ? q.trim() : '');
  let term = $state(clean(st.query));
  let debounce: ReturnType<typeof setTimeout> | undefined;
  function onQuery() {
    restore = null;
    clearTimeout(debounce);
    debounce = setTimeout(() => (term = clean(st.query)), 110);
  }
  function clearQuery() {
    clearTimeout(debounce);
    st.query = '';
    term = '';
    feedback('back');
  }

  let searchEl: HTMLInputElement | undefined = $state();
  let mainEl: HTMLElement | undefined = $state();
  let keyboard = $state(false);
  /** Manette (ou touches d'une manette sous Android) : clavier à l'écran ; clavier, souris, toucher : champ natif. */
  const padLike = () => input.kind === 'pad' || (isAndroid && input.kind === 'key');
  function openSearch() {
    if (padLike()) {
      keyboard = true;
      feedback('ok');
    } else {
      searchEl?.focus();
      searchEl?.select();
    }
  }
  function closeKeyboard() {
    if (!keyboard) return; // déjà fermé (B pendant l'animation de sortie)
    keyboard = false;
    term = clean(st.query);
    feedback('back');
    // Retour sur le premier résultat (ou sur le champ s'il n'y en a pas).
    requestAnimationFrame(() => {
      const first = document.querySelector<HTMLElement>('main .tile.focusable');
      (first ?? searchEl)?.focus({ preventScroll: true });
    });
  }
  function onSearchKey(ev: KeyboardEvent) {
    if (ev.key === 'Escape') {
      // Échap : efface la saisie, puis quitte le champ vers les résultats (un Échap de plus, hors du champ, quitte l'écran).
      ev.preventDefault();
      ev.stopPropagation();
      if (st.query) clearQuery();
      else { searchEl?.blur(); focusResults(); }
    } else if ((ev.key === 'ArrowDown' || ev.key === 'Enter') && !ev.isComposing) {
      ev.preventDefault();
      ev.stopPropagation();
      clearTimeout(debounce);
      term = clean(st.query);
      requestAnimationFrame(focusResults);
    }
  }
  function focusResults() {
    const first = document.querySelector<HTMLElement>('main .focusable');
    if (first) { first.focus({ preventScroll: true }); glideIntoView(first); }
  }

  let loadError = $state<string | null>(null);
  let loaded = $state(false);
  async function load() {
    loadError = null;
    loaded = false;
    const profile = router.profile!.id;
    try {
      const [lib, recent] = await Promise.all([api.library(profile), api.recent(profile).catch(() => [] as Game[])]);
      games = lib;
      recentRank = new Map(recent.filter((g) => g.last).map((g, i) => [g.id, i]));
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
    loaded = true;
    // Retour d'une fiche : même jeu, même défilement ; sinon premier élément.
    requestAnimationFrame(() => {
      if (mainEl && st.scroll) mainEl.scrollTop = st.scroll;
      if (!focusGame(st.focus)) focusFirst();
    });
  }

  onMount(() => {
    void load();
    // Démonstration seulement : `?clavier` ouvre le clavier à l'écran (captures sans manette).
    if (isDemo && new URLSearchParams(location.search).has('clavier')) keyboard = true;
    // Autres appareils : facultatif (serveur ancien ou injoignable = rien à montrer).
    api.devices().then((d) => (remotes = d)).catch(() => (remotes = []));
    const onTab = (ev: Event) => {
      if (keyboard || options) return;
      const d = (ev as CustomEvent<number>).detail;
      setTab((st.tab + d + TABS.length) % TABS.length, d > 0 ? 'right' : 'left');
    };
    const onAction = (ev: Event) => {
      if (keyboard || options) return;
      const d = (ev as CustomEvent).detail;
      if (d === 'y') { openSearch(); return; }
      if (d !== 'x') return;
      const onTile = (document.activeElement as HTMLElement | null)?.classList.contains('tile');
      const g = games.find((x) => x.id === focusedId);
      if (onTile && g) { feedback('ok'); options = g; }
    };
    // B / Échap avec une recherche en cours : efface d'abord la recherche (l'écran reste).
    // Écouteur en capture sur window : appelé avant celui d'App (bulle), qui ferait revenir en arrière.
    const onBack = (ev: Event) => {
      if (keyboard || options || document.querySelector('[data-modal]')) return;
      if (!st.query) return;
      ev.stopImmediatePropagation();
      clearQuery();
      focusResults();
    };
    // « / » ou Ctrl+F : recherche (hors d'un champ de saisie).
    const onKey = (ev: KeyboardEvent) => {
      const t = ev.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return;
      if (ev.key === '/' || ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 'f')) {
        ev.preventDefault();
        if (!keyboard && !options) { searchEl?.focus(); searchEl?.select(); }
      }
    };
    window.addEventListener('ludo:tab', onTab);
    window.addEventListener('ludo:action', onAction);
    window.addEventListener('ludo:back', onBack, { capture: true });
    window.addEventListener('keydown', onKey);
    return () => {
      // État gardé pour le retour (fiche → bibliothèque).
      st.focus = focusedId;
      st.scroll = mainEl?.scrollTop ?? 0;
      clearTimeout(debounce);
      window.removeEventListener('ludo:tab', onTab);
      window.removeEventListener('ludo:action', onAction);
      window.removeEventListener('ludo:back', onBack, { capture: true });
      window.removeEventListener('keydown', onKey);
    };
  });

  function setTab(i: number, dir: 'left' | 'right') {
    if (i === st.tab) return;
    restore = null;
    slide = dir;
    st.tab = i;
    st.scroll = 0;
    feedback('tab');
    focusFirst();
  }

  /* Disponibilité sur l'appareil regardé. */
  function here(g: Game): boolean {
    if (remote) return !!remote.games[g.id]?.installed;
    return g.install !== 'non_installe';
  }
  function flux(g: Game): boolean {
    if (remote) { const r = remote.games[g.id]; return !!r && r.stream && !r.installed; }
    return g.install === 'non_installe' && !!g.stream;
  }
  const hasFlux = $derived(games.some(flux));
  const toSync = $derived(remote ? 0 : games.filter((g) => SYNC_STATES.includes(g.sync)).length);

  function keep(g: Game): boolean {
    const t = TABS[st.tab].id;
    if (t !== 'tout' && g.console !== t) return false;
    if (st.avail === 'installes' && !here(g)) return false;
    if (st.avail === 'flux' && hasFlux && !flux(g)) return false;
    if (st.avail === 'serveur' && (here(g) || flux(g))) return false;
    // État de synchro : propre à cet appareil (ignoré quand un autre appareil est regardé).
    if (st.sync && !remote && !SYNC_STATES.includes(g.sync)) return false;
    if (st.played && !g.last) return false;
    return true;
  }

  const byTitle = (a: Game, b: Game) => a.title.localeCompare(b.title, 'fr', { sensitivity: 'base', numeric: true });
  const rank = (g: Game) => recentRank.get(g.id) ?? Infinity;
  function sortGames(list: Game[]): Game[] {
    const out = [...list];
    if (st.sort === 'az') return out.sort(byTitle);
    if (st.sort === 'temps') return out.sort((a, b) => playtimeSeconds(b.playtime) - playtimeSeconds(a.playtime) || byTitle(a, b));
    // Récents : jeux joués du plus récent au plus ancien, puis installés, puis le reste (A → Z).
    return out.sort((a, b) => rank(a) - rank(b) || Number(here(b)) - Number(here(a)) || byTitle(a, b));
  }

  /** Résultats affichés dans la grille, avec la correspondance de recherche (surlignage). */
  const shown = $derived.by(() => {
    const kept = games.filter(keep);
    if (!term) return sortGames(kept).map((g) => ({ g, m: null as Match | null }));
    const hits: { g: Game; m: Match | null }[] = [];
    for (const g of kept) {
      const m = match(g.title, term);
      if (m) hits.push({ g, m });
    }
    return hits.sort((a, b) => b.m!.score - a.m!.score || byTitle(a.g, b.g));
  });

  /** Vue d'accueil : onglet « Tout », rien de filtré, cet appareil. */
  // `term` (après l'anti-rebond) et non la saisie brute : la vue ne bascule qu'avec les résultats filtrés.
  const home = $derived(st.tab === 0 && !st.avail && !st.sync && !st.played && st.sort === 'recents' && !term && !remote);
  const sorted = $derived(sortGames(games));
  const featured = $derived(sorted.filter((g) => g.last && recentRank.has(g.id)));
  const rows = $derived(
    TABS.slice(1)
      .map((t, i) => ({ tab: i + 1, label: t.label, all: sorted.filter((g) => g.console === t.id) }))
      .filter((r) => r.all.length),
  );

  const installed = $derived(games.filter(here).length);
  /** Le jeu à retrouver est-il affiché (rangées de l'accueil ou grille) ? */
  const restoreShown = $derived(
    !!restore && (home ? rows.some((r) => r.all.slice(0, ROW_MAX).some((g) => g.id === restore)) : shown.some((x) => x.g.id === restore)),
  );
  const auto = (g: Game, fallback: boolean) => (restoreShown ? g.id === restore : fallback);

  /**
   * Cible haut / bas d'une rangée de l'accueil (`data-nav-up|down`, lu par la navigation de la PR #43 ;
   * avant sa fusion, calcul spatial habituel) : même
   * colonne dans la rangée voisine, ou sa dernière tuile si elle est plus courte ; au-dessus de la
   * première rangée, la bannière. Sans cible (dernière rangée) : calcul spatial habituel.
   */
  function navTo(r: number, col: number): string | undefined {
    if (r < 0) return featured.length ? '.une .btn.primary' : undefined;
    const row = rows[r];
    if (!row) return undefined;
    const last = Math.min(row.all.length, ROW_MAX) - (row.all.length > ROW_MAX ? 0 : 1);
    const c = Math.min(col, last);
    return `[data-row='${r}'] [data-col='${c}'] .focusable, [data-row='${r}'] .focusable[data-col='${c}']`;
  }

  function stateOf(g: Game): [string, string] {
    if (remote) {
      const r = remote.games[g.id];
      if (r?.installed) return [r.save ? 'Installé · Sauvegarde' : 'Installé', 'var(--ink-3)'];
      if (r?.stream) return ['Via Moonlight', 'var(--accent-ink)'];
      if (r?.save) return ['Sauvegarde seule', 'var(--ink-4)'];
      return ['Absent', 'var(--ink-4)'];
    }
    if (g.sync === 'conflit') return ['Conflit de sauvegarde', 'var(--conflict)'];
    if (g.sync === 'envoi_en_attente') return ['Envoi en attente', 'var(--pending)'];
    if (g.sync === 'a_recuperer') return ['Nouvelle sauvegarde', 'var(--sync)'];
    if (g.install === 'mise_a_jour') return ['Mise à jour disponible', 'var(--update)'];
    if (g.install === 'telechargement') return [`Téléchargement… ${g.progress ?? 0} %`, 'var(--accent-ink)'];
    if (g.install === 'non_installe' && g.stream) return ['Via Moonlight', 'var(--accent-ink)'];
    if (g.install === 'non_installe') return ['Non installé', 'var(--ink-4)'];
    return ['Installé · À jour', 'var(--ink-3)'];
  }

  /* Filtres : un appui active, un second appui retire. Son joué ici (data-sound="none" : un seul son, quelle que soit l'entrée). */
  function setAvail(a: Avail) {
    restore = null;
    st.avail = st.avail === a ? null : a;
    feedback('tab');
  }
  function toggleSync() { restore = null; st.sync = !st.sync; feedback('tab'); }
  function togglePlayed() { restore = null; st.played = !st.played; feedback('tab'); }
  function nextSort() {
    if (term) { feedback('err'); return; } // pendant une recherche : ordre de pertinence
    restore = null;
    const i = SORTS.findIndex((s) => s.id === st.sort);
    st.sort = SORTS[(i + 1) % SORTS.length].id;
    feedback('tab');
  }
  function setWhere(id: string) {
    restore = null;
    st.where = id;
    feedback('tab');
  }
  function open(g: Game) {
    focusedId = g.id;
    router.go({ name: 'fiche', game: g });
  }

  const gridKey = $derived(`${st.tab}-${st.avail}-${st.sync}-${st.played}-${st.sort}-${home}`);
  // Grille longue (300 jeux) : rendue par tranches. Une sentinelle sous la dernière cellule, observée
  // par IntersectionObserver (événement, aucune lecture périodique), ajoute la tranche suivante quand
  // elle approche de l'écran (défilement, croix vers le bas). Toute nouvelle vue repart de la 1re tranche.
  const SLICE = 40;
  let limit = $state(SLICE);
  let sentinel: HTMLElement | undefined = $state();
  $effect(() => {
    void gridKey; void term; void loaded;
    // Retour d'une fiche : assez de cellules pour retrouver le jeu choisi (recalculé à l'arrivée des jeux).
    const at = untrack(() => (restore ? shown.findIndex((x) => x.g.id === restore) : -1));
    limit = Math.max(SLICE, at + SLICE);
  });
  $effect(() => {
    // Relu à chaque tranche : un nouvel observe() rend un premier rappel, la sentinelle encore visible
    // (grand écran) fait donc ajouter la tranche suivante.
    void limit;
    if (!sentinel || !mainEl) return;
    const io = new IntersectionObserver((e) => { if (e.some((x) => x.isIntersecting)) limit += SLICE; }, { root: mainEl, rootMargin: '0px 0px 600px 0px' });
    io.observe(sentinel);
    return () => io.disconnect();
  });

  const count = $derived(term ? (shown.length ? `${shown.length} résultat${shown.length > 1 ? 's' : ''}` : 'Aucun résultat') : '');
  const hints = $derived(
    options
      ? [{ key: 'A' as const, label: 'Sélectionner' }, { key: 'B' as const, label: 'Fermer' }]
      : [
          { key: 'A' as const, label: 'Ouvrir' },
          { key: 'Y' as const, label: 'Rechercher' },
          { key: 'X' as const, label: 'Options' },
          { key: 'B' as const, label: st.query ? 'Effacer la recherche' : 'Retour' },
        ],
  );
</script>

<div class="screen">
  <header>
    <div class="line">
      <h1>Bibliothèque</h1>
      <div class="search" class:filled={!!st.query}>
        <svg class="glyph" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5"></circle><path d="M20 20l-4.2-4.2"></path></svg>
        <!-- Champ natif (clavier, souris, toucher) ; à la manette, A ou Y ouvre le clavier à l'écran. -->
        <input bind:this={searchEl} class="focusable" type="search" placeholder="Rechercher un jeu" aria-label="Rechercher un jeu"
          autocomplete="off" spellcheck="false" enterkeyhint="search" data-sound="none"
          bind:value={st.query} oninput={onQuery} onkeydown={onSearchKey}
          onclick={() => { if (padLike()) openSearch(); }} />
        {#if st.query}
          <button class="clear" tabindex="-1" aria-label="Effacer la recherche" onpointerdown={(e) => e.preventDefault()} onclick={() => { clearQuery(); searchEl?.focus(); }}>×</button>
        {:else}
          <span class="kbd" aria-hidden="true"><span class="k-pad">Y</span><span class="k-key">/</span></span>
        {/if}
      </div>
      <span class="meta" role="status" aria-live="polite">{#if count}{count}{:else if remote}{installed} jeux sur {remote.name} · inventaire du {remote.updated}{:else}{games.length} jeux · {installed} installés · {device.freeSpace}{/if}</span>
    </div>
    <div class="line wrap">
      <div class="tabs" role="tablist" aria-label="Consoles">
        <button class="bumper" tabindex="-1" aria-label="Onglet précédent" onpointerdown={(e) => e.preventDefault()} onclick={() => virtualKey('L1')}><span class="k-pad">L1</span><span class="k-key">Q</span></button>
        <span class="indicator" class:ready={indicator.ready} style:transform="translateX({indicator.x}px)" style:width="{indicator.w}px" aria-hidden="true"></span>
        {#each TABS as t, i (t.id)}
          <button role="tab" aria-selected={i === st.tab} class="tab" class:on={i === st.tab} bind:this={tabEls[i]} onclick={() => setTab(i, i > st.tab ? 'right' : 'left')} tabindex="-1">{t.label}</button>
        {/each}
        <button class="bumper" tabindex="-1" aria-label="Onglet suivant" onpointerdown={(e) => e.preventDefault()} onclick={() => virtualKey('R1')}><span class="k-pad">R1</span><span class="k-key">E</span></button>
      </div>
      <div class="filters">
        {#if remotes.length}
          <!-- Appareil regardé : celui-ci, ou un autre du foyer (ses jeux installés et en flux Moonlight). -->
          <button class="pill dev focusable" data-sound="none" aria-pressed={st.where === 'ici'} onclick={() => setWhere('ici')}><Icon name={device.kind} size={16} />{device.name}</button>
          {#each remotes as r (r.id)}
            <button class="pill dev focusable" data-sound="none" aria-pressed={st.where === r.id} onclick={() => setWhere(r.id)}><Icon name={r.kind} size={16} />{r.name}</button>
          {/each}
          <span class="sep" aria-hidden="true"></span>
        {/if}
        <button class="pill focusable" data-sound="none" aria-pressed={st.avail === 'installes'} onclick={() => setAvail('installes')}>Installés</button>
        {#if hasFlux}
          <button class="pill focusable" data-sound="none" aria-pressed={st.avail === 'flux'} onclick={() => setAvail('flux')}>Via Moonlight</button>
        {/if}
        <button class="pill focusable" data-sound="none" aria-pressed={st.avail === 'serveur'} onclick={() => setAvail('serveur')}>Sur le serveur</button>
        {#if toSync || (st.sync && !remote)}
          <button class="pill focusable" data-sound="none" aria-pressed={st.sync} onclick={toggleSync}>À synchroniser<span class="n">{toSync}</span></button>
        {/if}
        <button class="pill focusable" data-sound="none" aria-pressed={st.played} onclick={togglePlayed}>Joués</button>
        <button class="pill sort focusable" data-sound="none" onclick={nextSort} aria-label="Trier : {term ? 'pertinence' : SORTS.find((s) => s.id === st.sort)?.label}">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 4v16M3.5 16.5 7 20l3.5-3.5M17 20V4M13.5 7.5 17 4l3.5 3.5"></path></svg>{term ? 'Pertinence' : SORTS.find((s) => s.id === st.sort)?.label}
        </button>
      </div>
    </div>
  </header>

  <main bind:this={mainEl} class:home class:kb={keyboard}>
    {#key gridKey}
    <div class="view grid-{slide}">
      {#if loadError}
        <StateNotice title="Serveur injoignable" detail={loadError} action="Réessayer" onaction={() => void load()} />
      {:else if loaded && games.length === 0}
        <StateNotice icon="grid" title="Aucun jeu dans le catalogue" detail="Ajouter des ROM sur le serveur, puis relancer le balayage." autofocus={false} />
      {:else if home}
        {#if featured.length}
          <Une game={featured[0]} autofocus={!restoreShown} others={featured.slice(1, 4)} onplay={(g) => void play(g)} onopen={open} />
        {/if}
        {#each rows as row, ri (row.tab)}
          <section class="row" aria-label={row.label} data-row={ri}>
            <h2>{row.label}<span class="n">{row.all.length} jeu{row.all.length > 1 ? 'x' : ''}</span></h2>
            <!-- Gauche / droite restent dans la rangée ; haut / bas passent à la rangée voisine, même colonne. -->
            <div class="strip" data-nav-group="rangee-{ri}">
              {#each row.all.slice(0, ROW_MAX) as g, i (g.id)}
                {@const s = stateOf(g)}
                <div class="cell" style:--i={Math.min(i, 8)} data-cell={g.id} data-col={i} data-nav-up={navTo(ri - 1, i)} data-nav-down={navTo(ri + 1, i)}>
                  <Tile game={g} size={168} autofocus={auto(g, !featured.length && ri === 0 && i === 0)} onfocus={() => (focusedId = g.id)} onclick={() => open(g)} />
                  <span class="t">{g.title}</span>
                  <span class="s" style:color={s[1]}>{s[0]}</span>
                </div>
              {/each}
              {#if row.all.length > ROW_MAX}
                <button class="more focusable" data-hover-focus data-col={ROW_MAX} data-nav-up={navTo(ri - 1, ROW_MAX)} data-nav-down={navTo(ri + 1, ROW_MAX)} onclick={() => setTab(row.tab, 'right')}>
                  <span class="plus">+{row.all.length - ROW_MAX}</span>Tout voir
                </button>
              {/if}
            </div>
          </section>
        {/each}
      {:else}
        {#if loaded && shown.length === 0}
          <div class="empty">
            <p>{term ? `Aucun jeu pour « ${term} »` : 'Aucun jeu pour ce filtre'}</p>
            <button class="pill focusable" data-sound="none" onclick={() => { st.reset(); term = ''; feedback('back'); focusFirst(); }}>Tout afficher</button>
          </div>
        {/if}
        <div class="grid">
          {#each shown.slice(0, limit) as { g, m }, i (g.id)}
            {@const s = stateOf(g)}
            <div class="cell" style:--i={Math.min(i, 8)} data-cell={g.id}>
              <Tile game={g} size={168} autofocus={auto(g, i === 0)} onfocus={() => (focusedId = g.id)} onclick={() => open(g)} />
              <span class="t">{#if m && m.ranges.length}{#each pieces(g.title, m.ranges) as p, pi (pi)}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}{:else}{g.title}{/if}</span>
              <span class="s" style:color={s[1]}>{s[0]}</span>
            </div>
          {/each}
        </div>
        {#if shown.length > limit}<div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>{/if}
      {/if}
    </div>
    {/key}
  </main>

  {#if keyboard}
    <Clavier bind:value={st.query} oninput={onQuery} onclose={closeKeyboard} />
  {/if}
  {#if options}
    <OptionsMenu game={options} profile={router.profile!.id} onclose={closeOptions}
      onplay={() => { const g = options!; options = null; void play(g); }}
      ondetails={() => { const g = options!; options = null; open(g); }}
      onchange={changeGame} />
  {/if}
  {#if !keyboard}<ButtonHints {hints} />{/if}
</div>


<style>
  .screen { height: 100%; display: flex; flex-direction: column; overflow: hidden; position: relative; }
  header { padding: 24px 48px 0; display: flex; flex-direction: column; gap: 16px; }
  .line { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; flex-shrink: 0; }
  .meta { font-size: 15px; color: var(--ink-3); white-space: nowrap; }

  .search {
    flex: 1 1 auto; max-width: 560px; height: 48px; box-sizing: border-box; padding: 0 8px 0 16px;
    display: flex; align-items: center; gap: 10px; border-radius: 999px;
    background: rgba(238, 240, 244, 0.08); border: 1.5px solid var(--hint); color: var(--ink-2);
    transition: border-color var(--t-quick) ease-out, background-color var(--t-quick) ease-out;
  }
  .search:focus-within { border-color: var(--accent); background: rgba(238, 240, 244, 0.12); }
  .search input { all: unset; flex-grow: 1; min-width: 0; height: 100%; font-size: 17px; color: var(--ink); }
  .search input::placeholder { color: var(--ink-4); }
  /* Le champ porte lui-même l'anneau de focus : ici, la bordure du conteneur en tient lieu. */
  .search input:focus, .search input:focus-visible { box-shadow: none; outline: none; }
  .search input::-webkit-search-cancel-button { display: none; }
  .clear { all: unset; cursor: pointer; width: 44px; height: 44px; margin-right: -6px; border-radius: 50%; display: flex; align-items: center; justify-content: center; font-size: 22px; color: var(--ink-2); }
  .clear:hover { background: var(--hint); }
  .kbd > span { min-width: 24px; height: 24px; padding: 0 6px; box-sizing: border-box; border-radius: 12px; background: var(--hint); color: var(--ink); font-size: 13px; font-weight: 700; display: inline-flex; align-items: center; justify-content: center; }
  /* Touche montrée selon la dernière entrée (racine `data-input`, posée par la navigation) : Y à la manette, / ou Q/E au clavier. */
  .k-key { display: none; }
  :global(html[data-input='key']) .k-key { display: inline-flex; }
  :global(html[data-input='key']) .k-pad { display: none; }

  /* Onglets toujours sur une ligne (l'indicateur glisse dessus) ; faute de place (Deck), les filtres passent dessous. */
  .line.wrap { flex-wrap: wrap; row-gap: 10px; }
  .tabs { display: flex; align-items: center; gap: 6px; flex-wrap: nowrap; position: relative; flex-shrink: 0; }
  .indicator { position: absolute; left: 0; top: 50%; height: 40px; margin-top: -20px; border-radius: var(--radius-control); background: var(--accent); z-index: 0; }
  .indicator.ready { transition: transform 260ms var(--ease-land), width 260ms var(--ease-land); }
  .bumper { all: unset; cursor: pointer; font-size: 13px; font-weight: 700; color: var(--ink-2); border: 1.5px solid rgba(255, 255, 255, 0.35); border-radius: 6px; padding: 3px 7px; margin: 0 6px; min-width: 18px; text-align: center; }
  .tab { all: unset; cursor: pointer; font-size: 16px; padding: 10px 16px; border-radius: var(--radius-control); color: var(--ink-2); transition: background-color var(--t-quick) ease-out, color var(--t-quick) ease-out; }
  .tabs .tab { position: relative; z-index: 1; }
  .tab.on { color: var(--ground); font-weight: 600; }

  .filters { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; margin-left: auto; }
  /* Cibles de 44 px au moins (toucher). Pas de color-mix() dans ce fichier : WebView de la Thor = Chromium 109. */
  .pill {
    all: unset; cursor: pointer; box-sizing: border-box; min-height: 44px; padding: 0 16px;
    display: inline-flex; align-items: center; gap: 8px;
    font-size: 15px; border-radius: 999px; border: 1.5px solid var(--hint); color: var(--ink-2);
    transition: background-color var(--t-quick) ease-out, border-color var(--t-quick) ease-out, color var(--t-quick) ease-out;
  }
  .sep { width: 1px; align-self: stretch; margin: 6px 4px; background: var(--hint); }
  .pill[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-ink); }
  .pill.sort { background: rgba(238, 240, 244, 0.08); border-color: transparent; color: var(--ink); }
  .pill .n { font-size: 13px; font-weight: 600; min-width: 20px; height: 20px; padding: 0 6px; box-sizing: border-box; border-radius: 10px; background: var(--pending); color: var(--ground); display: inline-flex; align-items: center; justify-content: center; }

  main { flex-grow: 1; overflow-y: auto; overflow-x: hidden; padding: 22px 48px 28px; }
  /* Clavier à l'écran ouvert : la place libérée en bas permet de voir les résultats au-dessus. */
  main.kb { padding-bottom: 380px; }
  .view { display: flex; flex-direction: column; gap: 26px; }

  .row h2 { margin: 0 0 12px; font-size: 20px; font-weight: 600; display: flex; align-items: baseline; gap: 12px; }
  .row h2 .n { font-size: 14px; font-weight: 400; color: var(--ink-3); }
  /* Rangée horizontale : défilement natif (molette, doigt) et par ressort à la croix (glideIntoView). */
  .strip { display: flex; gap: 20px; overflow-x: auto; overflow-y: visible; padding: 10px 12px 6px; margin: -10px -12px 0; scrollbar-width: none; }
  .strip::-webkit-scrollbar { display: none; }
  .strip .cell { width: 168px; flex-shrink: 0; }
  .more {
    all: unset; cursor: pointer; box-sizing: border-box; width: 168px; height: 168px; flex-shrink: 0;
    border-radius: var(--radius-tile); border: 2px dashed var(--hint); color: var(--ink-2);
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; font-size: 16px;
  }
  .more .plus { font-size: 28px; font-weight: 600; color: var(--ink); }
  .more:focus, .more:focus-visible { border-style: solid; border-color: var(--accent); color: var(--ink); }

  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(168px, 1fr)); gap: 26px 20px; align-content: start; }
  /* 300 jeux : les cellules hors écran ne sont ni mises en page ni peintes. */
  .grid .cell { content-visibility: auto; contain-intrinsic-size: auto 240px; }
  /* Changement d'onglet : la grille glisse du côté de l'onglet choisi, cellules en cascade de 25 ms. */
  @keyframes grid-from-right { from { opacity: 0; transform: translateX(24px); } }
  @keyframes grid-from-left { from { opacity: 0; transform: translateX(-24px); } }
  .grid-right .cell { animation: grid-from-right var(--t-std) var(--ease-land) both; animation-delay: calc(var(--i) * 25ms); }
  .grid-left .cell { animation: grid-from-left var(--t-std) var(--ease-land) both; animation-delay: calc(var(--i) * 25ms); }
  .cell { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  /* Titre sur deux lignes : la partie distinctive (« Majora's Mask 3D ») reste lisible. */
  .t { font-size: 15px; font-weight: 500; line-height: 1.25; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .t mark { background: var(--accent-soft); color: var(--ink); border-radius: 3px; padding: 0 1px; }
  .s { font-size: 13px; margin-top: -4px; }
  .sentinel { height: 1px; }
  .empty { display: flex; align-items: center; gap: 18px; color: var(--ink-3); font-size: 17px; }
  .empty p { margin: 0; }
</style>
