<script lang="ts">
  import StateNotice from '../lib/components/StateNotice.svelte';
  import Ambiance from '../lib/components/Ambiance.svelte';
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import RestoreDialog from '../lib/components/RestoreDialog.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api, CANCELLED, isAndroid, prepLabel, type Preparing } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { play } from '../lib/play';
  import { router } from '../lib/router.svelte';
  import type { Game, GameContent, SaveRevision } from '../lib/types';

  let { game }: { game: Game } = $props();
  /** `null` tant que le serveur n'a pas répondu : une liste vide n'est affichée qu'une fois la réponse reçue
   * (sinon « Aucune sauvegarde » s'affichait pendant le chargement, constat du 18/11 sur la Thor). */
  let history = $state<SaveRevision[] | null>(null);
  let restored = $state<number | null>(null);
  let historyError = $state<string | null>(null);

  let restoring = $state<SaveRevision | null>(null);

  /** Restaurer : fenêtre de chargement, puis la version devient « Actuelle » (nouvelle révision côté serveur). */
  function restore(r: SaveRevision) {
    restoring = r;
  }
  function restoreDone() {
    const r = restoring!;
    restoring = null;
    restored = r.rev;
    history = (history ?? []).map((h) => ({ ...h, current: h.rev === r.rev }));
    requestAnimationFrame(() => document.querySelector<HTMLElement>('button.primary.focusable')?.focus({ preventScroll: true }));
  }

  /* Onglets : Sauvegardes / Contenu additionnel (mises à jour et DLC), L1 et R1 comme la Bibliothèque. */
  let tab = $state<'saves' | 'content'>('saves');
  let content = $state<GameContent[]>(game.content ?? []);
  let progress = $state<Record<number, number>>({});
  let justInstalled = $state<number | null>(null);
  let failedId = $state<number | null>(null);

  function showTab(t: 'saves' | 'content') {
    if (t === tab || (t === 'content' && content.length === 0)) return;
    tab = t;
    feedback('tab');
  }

  async function install(c: GameContent) {
    if (c.installed || progress[c.id] !== undefined) return;
    failedId = null;
    progress = { ...progress, [c.id]: 0 };
    try {
      await api.installContent(game.id, c.id, (p) => (progress = { ...progress, [c.id]: p }));
      content = content.map((x) => (x.id === c.id ? { ...x, installed: true } : x));
      justInstalled = c.id;
      feedback('downloaded');
    } catch {
      failedId = c.id;
      feedback('err');
    } finally {
      const { [c.id]: _, ...rest } = progress;
      progress = rest;
    }
  }

  /** Historique du profil courant. Sans profil (appareil partagé, avant « Qui joue ? »), aucune requête :
   * l'ancienne lecture `router.profile!.id` levait une exception hors de la promesse et laissait la liste vide. */
  function loadHistory() {
    const profile = router.profile?.id;
    if (!profile) {
      historyError = 'Choisir un profil pour voir les sauvegardes';
      history = [];
      return;
    }
    historyError = null;
    api
      .history(profile, game.id)
      .then((h) => (history = h))
      .catch((e) => (historyError = message(e)));
  }

  onMount(() => {
    loadHistory();
    focusFirst();
    const onTab = (ev: Event) => showTab((ev as CustomEvent<number>).detail > 0 ? 'content' : 'saves');
    window.addEventListener('ludo:tab', onTab);
    return () => window.removeEventListener('ludo:tab', onTab);
  });

  /* Installation du jeu : l'état local suit les appels réels (la fiche ne recharge pas la bibliothèque). */
  // svelte-ignore state_referenced_locally
  let installState = $state(game.install);
  const installed = $derived(installState !== 'non_installe');
  let installing = $state<number | null>(null);
  /** Le serveur convertit le fichier (NSZ → NSP) avant l'envoi. */
  let preparing = $state<Preparing | null>(null);
  let installError = $state<string | null>(null);
  let confirmRemove = $state(false);
  let removing = $state(false);
  let cancelling = $state(false);

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function installGame() {
    if (installing !== null) return;
    installError = null;
    installing = 0;
    try {
      await api.installGame(game.id, (p, prep) => {
        installing = p;
        preparing = prep ?? null;
      });
      installState = 'installe';
      // Les mises à jour du jeu sont installées avec lui ; les DLC restent un choix.
      content = content.map((c) => (c.role === 'maj' ? { ...c, installed: true } : c));
      feedback('downloaded');
    } catch (e) {
      // Annulation voulue : pas un échec, le fichier partiel est gardé pour reprendre.
      if (message(e) !== CANCELLED) {
        installError = message(e);
        feedback('err');
      }
    } finally {
      installing = null;
      preparing = null;
      cancelling = false;
      // Le bouton « Annuler » disparaît : le focus revient sur le bouton principal (Installer ou Jouer).
      requestAnimationFrame(() => document.querySelector<HTMLElement>('button.primary.focusable')?.focus({ preventScroll: true }));
    }
  }

  function cancelInstall() {
    if (installing === null || cancelling) return;
    cancelling = true;
    void api.cancelInstall(game.id);
  }

  /** Désinstaller : le premier appui arme la confirmation, le second supprime les fichiers du jeu (jamais les sauvegardes). */
  async function uninstall() {
    if (removing || installing !== null) return;
    if (!confirmRemove) {
      confirmRemove = true;
      feedback('err');
      return;
    }
    confirmRemove = false;
    removing = true;
    installError = null;
    try {
      await api.uninstallGame(game.id);
      installState = 'non_installe';
      content = content.map((c) => ({ ...c, installed: false }));
      feedback('saved');
      requestAnimationFrame(() => document.querySelector<HTMLElement>('button.primary.focusable')?.focus({ preventScroll: true }));
    } catch (e) {
      installError = message(e);
      feedback('err');
    } finally {
      removing = false;
    }
  }

  /** Android : jeu absent de l'appareil mais jouable en flux depuis le PC (Moonlight). */
  const streamOnly = $derived(isAndroid && !installed && !!game.stream);

  /** Android : l'app n'installe rien (ROM et contenu arrivent par le PC) ; aucune action d'installation. */
  const pcOnly = $derived(isAndroid && !installed && !game.stream);

  function primary() {
    if (installed) void play(game, undefined, isAndroid ? 'local' : undefined);
    else if (streamOnly) void play(game, undefined, 'flux');
    else if (!pcOnly) void installGame();
  }
</script>

<div class="screen">
  <Ambiance art={game.art} cover={game.cover} background={game.background} x={14} dim />
  <main>
    <aside>
      <div class="art"><Tile {game} size={320} big still hero={router.heroGame === game.id} /></div>
      <h2 class="gname">{game.title}</h2>
      <div class="actions">
        <button class="primary focusable" class:busy={installing !== null} data-autofocus aria-busy={installing !== null} aria-disabled={pcOnly} onclick={primary}>
          {#if installing !== null}<span class="fill" class:waiting={preparing !== null} style:width="{preparing ? 100 : installing * 100}%"></span>{/if}
          <span class="label">{installing !== null ? (preparing ? prepLabel(preparing) : `Téléchargement… ${Math.round(installing * 100)} %`) : installed ? 'Jouer' : streamOnly ? 'Jouer en flux' : pcOnly ? 'Installation depuis le PC' : installError ? 'Réessayer' : 'Installer'}</span>
        </button>
        {#if installing !== null}
          <button class="secondary focusable" data-sound="none" disabled={cancelling} onclick={cancelInstall}>{cancelling ? 'Annulation…' : 'Annuler'}</button>
        {:else if isAndroid}
          {#if installed && game.stream}<button class="secondary focusable" onclick={() => play(game, undefined, 'flux')}>En flux</button>{/if}
        {:else if installed}
          <button class="secondary focusable" class:armed={confirmRemove} data-sound="none" disabled={removing} onclick={uninstall} onblur={() => (confirmRemove = false)}>{removing ? 'Suppression…' : confirmRemove ? 'Confirmer : désinstaller' : 'Désinstaller'}</button>
        {/if}
      </div>
      {#if installError}<p class="error" role="alert">{installError}</p>{/if}
      <dl>
        <dt>Émulateur</dt><dd>{game.emulator}</dd>
        {#if game.version}<dt>Version</dt><dd>{game.version}</dd>{/if}
        {#if game.size}<dt>{installed ? "Sur l'appareil" : 'Taille'}</dt><dd>{game.size}</dd>{/if}
        {#if game.playtime}<dt>Temps de jeu</dt><dd>{game.playtime}</dd>{/if}
      </dl>
    </aside>

    <section class="panel" aria-labelledby="hist">
      {#if content.length > 0}
        <div class="tabs" role="tablist">
          <button class="bumper" tabindex="-1" aria-label="Onglet précédent" onclick={() => showTab('saves')}>L1</button>
          <button class="tab" role="tab" aria-selected={tab === 'saves'} class:on={tab === 'saves'} onclick={() => showTab('saves')}>Sauvegardes</button>
          <button class="tab" role="tab" aria-selected={tab === 'content'} class:on={tab === 'content'} onclick={() => showTab('content')}>Contenu additionnel<span class="count">{content.length}</span></button>
          <button class="bumper" tabindex="-1" aria-label="Onglet suivant" onclick={() => showTab('content')}>R1</button>
        </div>
      {/if}
      {#if tab === 'content'}
        <ul class="content" aria-label="Contenu additionnel">
          {#each content as c, i (c.id)}
            <li class="rise" style:--i={i}>
              <button class="row focusable" class:pop={justInstalled === c.id} class:done={c.installed} data-sound="none" aria-disabled={c.installed || isAndroid} onclick={() => !isAndroid && install(c)}>
                <Icon name={c.role === 'maj' ? 'update' : 'download'} size={22} color="var(--ink-2)" />
                <span class="cname">{c.name}</span>
                <!-- Pastille de type masquée quand le nom la répète (« Mise à jour ») ; sa place reste réservée pour aligner les tailles. -->
                <span class="role" class:blank={c.name === (c.role === 'maj' ? 'Mise à jour' : 'DLC')}>{c.role === 'maj' ? 'Mise à jour' : 'DLC'}</span>
                <span class="csize">{c.size}</span>
                {#if c.installed}
                  <span class="tag ok"><Icon name="check" size={14} color="#8be6b5" />Installé</span>
                {:else if isAndroid}
                  <span class="tag pc">Depuis le PC</span>
                {:else if progress[c.id] !== undefined}
                  <span class="mini" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(progress[c.id] * 100)} aria-label="Installation de {c.name}"><span style:width="{progress[c.id] * 100}%"></span></span>
                {:else}
                  <span class="restore">{failedId === c.id ? 'Réessayer' : 'Installer'}</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
        <span class="policy">{isAndroid ? "Mises à jour et DLC installés depuis le PC : l'émulateur de cet appareil les reçoit avec le jeu." : "Mises à jour et DLC installés dans le dossier de contenu externe de l'émulateur, de la même façon sur tous les appareils."}</span>
      {:else}
      <div class="head">
        <h1 id="hist">Historique des sauvegardes</h1>
        {#if game.sync === 'a_jour'}<span class="state"><Icon name="cloud-ok" size={20} color="var(--ok)" />À jour</span>{/if}
      </div>
      {#if historyError}
        <StateNotice title="Historique indisponible" detail={historyError} autofocus={false} />
      {:else if history === null}
        <p class="empty" aria-busy="true">Chargement de l'historique…</p>
      {:else if history.length === 0}
        <p class="empty">Aucune sauvegarde</p>
      {:else}
        <ul>
          {#each history as r (r.rev)}
            <li class="rise" style:--i={history.indexOf(r)}>
              <button class="row focusable" class:pop={restored === r.rev} disabled={r.current} data-sound="ok" onclick={() => restore(r)}>
                <Icon name={r.deviceKind} size={24} color="var(--ink-2)" />
                <span class="dev">{r.device}</span>
                <span class="when">{r.when}</span>
                <span class="time">{r.playtime}</span>
                {#if r.current}<span class="tag ok">Actuelle</span>
                {:else if r.discarded}<span class="tag discarded">Écartée lors d'un conflit</span>
                {:else}<span class="restore">Restaurer</span>{/if}
              </button>
            </li>
          {/each}
        </ul>
        <span class="policy">20 dernières versions, une par jour sur 30 jours, une par mois ensuite.</span>
      {/if}
      {/if}
    </section>
  </main>
  {#if restoring}
    <RestoreDialog {game} rev={restoring} profile={router.profile!.id} ondone={restoreDone} onfail={() => { restoring = null; requestAnimationFrame(() => document.querySelector<HTMLElement>('button.primary.focusable')?.focus()); }} />
  {/if}
  <ButtonHints hints={restoring ? [] : tab === 'content' && isAndroid ? [{ key: 'B', label: 'Retour' }] : [{ key: 'A', label: tab === 'content' ? 'Installer' : 'Sélectionner' }, { key: 'B', label: 'Retour' }]} note={content.length > 0 && !restoring ? 'L1 / R1 : Sauvegardes, Contenu additionnel' : undefined} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; position: relative; isolation: isolate; }
  main { flex-grow: 1; display: flex; gap: 56px; padding: 44px 56px 0; min-height: 0; }
  /* Panneau sombre translucide : le texte reste lisible sur n'importe quel fond (image floutée claire comprise). */
  .panel { align-self: flex-start; max-height: calc(100% - 24px); box-sizing: border-box; padding: 22px 24px; border-radius: 20px;
    background: rgba(10, 12, 16, 0.62); backdrop-filter: blur(18px) saturate(1.2); box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06), 0 18px 40px rgba(0, 0, 0, 0.3); }
  .gname { margin: -6px 0 0; font-size: 22px; font-weight: 600; line-height: 1.2; text-shadow: 0 2px 12px rgba(0, 0, 0, 0.5); }
  aside { width: 320px; flex-shrink: 0; display: flex; flex-direction: column; gap: 22px; }
  .art { pointer-events: none; }
  .actions { display: flex; gap: 12px; }
  .primary, .secondary { all: unset; cursor: pointer; flex-grow: 1; text-align: center; padding: 14px 0; border-radius: 12px; }
  .primary { position: relative; overflow: hidden; font-size: 18px; font-weight: 600; background: var(--ink); color: var(--ground); }
  .primary .label { position: relative; }
  .primary.busy { cursor: progress; }
  /* Progression sur le bouton lui-même : la partie remplie prend la couleur d'accent. */
  .fill.waiting { animation: attente 1.6s ease-in-out infinite alternate; }
  @keyframes attente { from { opacity: 0.2; } to { opacity: 0.55; } }
  @media (prefers-reduced-motion: reduce) { .fill.waiting { animation: none; } }
  .fill { position: absolute; inset: 0 auto 0 0; background: var(--accent); opacity: 0.55; transition: width 120ms linear; }
  .secondary { font-size: 16px; background: var(--control); color: var(--ink); }
  .secondary.armed { background: rgba(255, 90, 90, 0.16); color: #ff9b9b; }
  .secondary:disabled { cursor: default; opacity: 0.8; }
  .error { margin: 0; font-size: 14px; color: #ff9b9b; }
  dl { padding: 14px 16px; border-radius: 14px; background: rgba(10, 12, 16, 0.5); backdrop-filter: blur(12px);
    margin: 0; display: grid; grid-template-columns: auto 1fr; gap: 10px 18px; font-size: 15px; }
  dt { color: var(--ink-3); }
  dd { margin: 0; }
  section { flex-grow: 1; display: flex; flex-direction: column; gap: 18px; min-width: 0; overflow-y: auto; }
  .head { display: flex; align-items: baseline; justify-content: space-between; }
  h1 { margin: 0; font-size: 28px; font-weight: 600; }
  .state { display: flex; align-items: center; gap: 8px; font-size: 15px; color: var(--ink-2); white-space: nowrap; flex-shrink: 0; }
  ul { list-style: none; margin: 0; padding: 4px; display: flex; flex-direction: column; gap: 8px; }
  .row { all: unset; cursor: pointer; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: 16px; padding: 16px 20px; border-radius: 12px; background: rgba(255, 255, 255, 0.06); font-size: 16px; }
  .row:disabled { cursor: default; }
  .row:focus, .row:hover:not(:disabled) { background: rgba(255, 255, 255, 0.13); }
  .dev { width: 140px; }
  .when { width: 200px; color: var(--ink-2); }
  .time { color: var(--ink-2); }
  .tag { margin-left: auto; font-size: 13px; font-weight: 600; padding: 4px 10px; border-radius: 8px; }
  .tag.ok { background: #23402f; color: #8be6b5; }
  .tag.discarded { background: #33284a; color: #d9b8ff; }
  .restore { margin-left: auto; font-size: 15px; font-weight: 600; padding: 8px 16px; border-radius: var(--radius-control); background: var(--control); color: var(--ink); }
  .row:focus .restore { background: var(--accent); color: var(--ground); }
  .policy, .empty { font-size: 14px; color: var(--ink-2); }
  .tabs { display: flex; align-items: center; gap: 6px; }
  .bumper { all: unset; cursor: pointer; font-size: 13px; font-weight: 700; color: var(--ink); border: 1.5px solid rgba(255, 255, 255, 0.45); border-radius: 6px; padding: 2px 7px; margin: 0 4px; }
  .bumper:hover { background: rgba(255, 255, 255, 0.12); }
  .tabs { padding: 4px; border-radius: 14px; background: rgba(255, 255, 255, 0.06); align-self: flex-start; }
  .tab { all: unset; cursor: pointer; font-size: 20px; font-weight: 600; padding: 8px 14px; border-radius: var(--radius-control); color: var(--ink-2); display: inline-flex; align-items: center; gap: 8px; transition: color var(--t-quick) ease-out, background-color var(--t-std) var(--ease-land); }
  .tab.on { color: var(--ground); background: var(--accent); }
  .count { font-size: 13px; font-weight: 700; min-width: 20px; height: 20px; border-radius: 10px; display: inline-flex; align-items: center; justify-content: center; background: rgba(255, 255, 255, 0.16); }
  .tab.on .count { background: rgba(0, 0, 0, 0.18); }
  .cname { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .role { font-size: 13px; font-weight: 600; letter-spacing: 0.04em; color: var(--ink-2); background: rgba(255, 255, 255, 0.08); padding: 3px 8px; border-radius: 6px; }
  .csize { width: 80px; text-align: right; color: var(--ink-2); font-variant-numeric: tabular-nums; flex-shrink: 0; }
  /* Pastille de type de largeur fixe : la colonne des tailles reste alignée d'une ligne à l'autre. */
  .content .role { width: 96px; box-sizing: border-box; text-align: center; flex-shrink: 0; }
  .content .role.blank { visibility: hidden; }
  .tag.pc { background: rgba(255, 255, 255, 0.08); color: var(--ink-2); }
  /* Colonne de droite de largeur fixe : rien ne bouge quand « Installer » devient une barre puis « Installé ». */
  .content .tag, .content .restore, .content .mini { margin-left: 8px; width: 120px; box-sizing: border-box; justify-content: center; text-align: center; flex-shrink: 0; }
  .content .row.done { cursor: default; }
  .tag { display: inline-flex; align-items: center; gap: 4px; }
  .mini { width: 120px; height: 6px; border-radius: 3px; background: rgba(255, 255, 255, 0.12); overflow: hidden; }
  .mini span { display: block; height: 100%; background: var(--accent); transition: width 90ms linear; }

  /* ——— Écran du bas de la Ayn Thor en double écran (lot thor-ui, 22/11) ———
     Vue ≈ 723×630 px logiques à l'échelle du Deck. La jaquette entière et le titre sont sur l'écran du haut
     (EcranHaut.svelte) : ici, les actions (Jouer en premier) et l'historique, lisibles et touchables. */
  :global(html[data-ecran='bas']) main { gap: 20px; padding: 20px 24px 0; }
  :global(html[data-ecran='bas']) aside { width: 240px; gap: 14px; }
  :global(html[data-ecran='bas']) .art,
  :global(html[data-ecran='bas']) .gname { display: none; }
  :global(html[data-ecran='bas']) .actions { flex-direction: column; gap: 10px; }
  :global(html[data-ecran='bas']) .primary { padding: 16px 0; font-size: 19px; }
  :global(html[data-ecran='bas']) .secondary { padding: 13px 0; }
  :global(html[data-ecran='bas']) dl { font-size: 15px; gap: 8px 14px; }
  :global(html[data-ecran='bas']) .panel { padding: 16px; max-height: calc(100% - 16px); }
  :global(html[data-ecran='bas']) h1 { font-size: 22px; }
  :global(html[data-ecran='bas']) .tab { font-size: 17px; }
  :global(html[data-ecran='bas']) .row { flex-wrap: wrap; gap: 6px 12px; padding: 12px 14px; font-size: 15px; min-height: 52px; }
  :global(html[data-ecran='bas']) .dev,
  :global(html[data-ecran='bas']) .when { width: auto; }
  :global(html[data-ecran='bas']) .content .role,
  :global(html[data-ecran='bas']) .csize { display: none; }
  /* Colonne étroite : jamais de défilement horizontal (barre parasite sous l'historique, capture 723×630). */
  :global(html[data-ecran='bas']) section { overflow-x: hidden; }
  :global(html[data-ecran='bas']) .tabs { max-width: 100%; box-sizing: border-box; }
</style>
