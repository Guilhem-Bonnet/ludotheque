<script lang="ts">
  import Ambiance from '../lib/components/Ambiance.svelte';
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import Tile from '../lib/components/Tile.svelte';
  import { api, CANCELLED, isDemo, prepLabel } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { reducedMotion } from '../lib/motion';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { LaunchEvent, LaunchResult, LaunchStep } from '../lib/api';
  import type { Preparation } from '../lib/types';

  // Écran de passage : n'apparaît que s'il y a quelque chose à faire (téléchargement, synchro).
  // Récit : le jeu arrive (élément partagé depuis la tuile), chaque étape se coche en se dessinant,
  // la barre avance en continu, puis la montée douce du lancement accompagne un dernier « pop ».
  let { preparation }: { preparation: Preparation } = $props();

  // Copie locale volontaire : l'écran fait avancer ses propres étapes.
  // svelte-ignore state_referenced_locally
  let steps = $state(preparation.steps.map((s) => ({ ...s })));
  let launching = $state(false);
  let art: HTMLElement | undefined = $state();

  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

  /** Avance une barre de progression jusqu'à 100 % en ~1,8 s (démonstration). */
  function fill(i: number) {
    return new Promise<void>((done) => {
      const start = performance.now();
      const from = steps[i].progress ?? 0;
      const tickFn = (t: number) => {
        const k = Math.min(1, (t - start) / 1800);
        steps[i].progress = Math.round(from + (100 - from) * (1 - Math.pow(1 - k, 2)));
        if (k < 1) requestAnimationFrame(tickFn);
        else done();
      };
      requestAnimationFrame(tickFn);
    });
  }

  /** Déroulé des étapes (démonstration ou suite d'un téléchargement réel) ; les étapes faites sont sautées. */
  async function runSteps() {
    for (let i = 0; i < steps.length; i++) {
      const s = steps[i];
      if (s.state === 'fait') continue;
      s.state = 'en_cours';
      if (s.progress !== undefined) {
        await fill(i);
        s.detail = '65,8 Mo · terminé';
        s.label = 'Téléchargement terminé';
        feedback('downloaded');
      } else if (i < steps.length - 1) {
        await wait(700);
        if (s.label.startsWith('Sauvegarde')) s.label = 'Sauvegarde : récupérée';
      }
      if (i === steps.length - 1) {
        launching = true;
        s.label = 'Lancement…';
        feedback('launch');
        await wait(1500);
        s.state = 'fait';
        await wait(500);
        router.back();
        return;
      }
      s.state = 'fait';
      await wait(250);
    }
  }

  let downloading = $state(false);
  let failed = $state(false);
  let cancelling = $state(false);
  /** Lancement réel en cours (téléchargement, sauvegarde, partie, envoi) : B ne quitte pas l'écran. */
  let busy = $state(false);
  /** Partie jouée dont l'envoi a échoué : « Réessayer l'envoi » ne relance pas le jeu. */
  let sendOnly = $state(false);
  /** Partie terminée par une erreur de l'émulateur : il ne reste que « Retour ». */
  let ended = $state(false);
  /** Fermeture de la fenêtre refusée pendant la partie. */
  let closeNote = $state(false);
  let phase = $state<'prep' | 'launching' | 'playing' | 'sending'>('prep');
  /** Étapes d'origine, pour « Réessayer ». */
  // svelte-ignore state_referenced_locally
  const initial = preparation.steps.map((x) => ({ ...x }));
  const dlLabel = preparation.steps.find((x) => x.progress !== undefined)?.label ?? 'Téléchargement…';

  /** Ordre des étapes de `LaunchStep` → position dans la liste. */
  const index: Record<LaunchStep, number> = { profil: 0, telechargement: 1, sauvegarde: 2, lancement: 3, en_jeu: 4, envoi: 5, termine: 5 };
  /** Libellé d'une étape en échec. */
  const failLabel = ['Profil indisponible', 'Téléchargement impossible', 'Sauvegarde impossible à synchroniser', 'Lancement impossible', 'Partie interrompue', 'Envoi de la sauvegarde impossible'];

  /** Applique une étape annoncée par l'app (lancement réel). */
  function onStep(e: LaunchEvent) {
    const s = steps[index[e.step]];
    if (!s) return;
    const done = e.state === 'fait';
    s.state = done ? 'fait' : 'en_cours';
    switch (e.step) {
      case 'profil':
        break;
      case 'telechargement': {
        const real = s.progress !== undefined;
        downloading = !done && real;
        if (done) {
          if (real) { s.progress = 100; s.label = 'Téléchargement terminé'; s.detail = preparation.game.size ?? undefined; feedback('downloaded'); }
          else s.label = 'Jeu installé';
        } else if (real) { s.label = dlLabel; }
        break;
      }
      case 'sauvegarde':
        s.label = done ? 'Sauvegarde : à jour' : 'Sauvegarde : synchronisation';
        if (done) s.detail = e.detail;
        break;
      case 'lancement':
        s.label = done ? 'Émulateur lancé' : 'Lancement…';
        if (!done) { phase = 'launching'; launching = true; feedback('launch'); }
        break;
      case 'en_jeu':
        s.label = done ? 'Partie terminée' : 'En jeu';
        s.detail = done ? undefined : 'Partie en cours. La sauvegarde partira à la fermeture de l\u2019émulateur.';
        if (!done) phase = 'playing';
        break;
      case 'envoi':
        s.label = done ? 'Sauvegarde envoyée' : 'Envoi de la sauvegarde…';
        s.detail = done ? e.detail : undefined;
        if (!done) { phase = 'sending'; launching = false; }
        break;
      case 'termine':
        break;
    }
  }

  /** Une erreur du lancement s'écrit sur l'étape en cours, avec « Réessayer » et « Retour ». */
  function onFailure(msg: string) {
    let i = steps.findLastIndex((x) => x.state === 'en_cours');
    if (i < 0) i = steps.findIndex((x) => x.state !== 'fait');
    i = Math.max(i, 0);
    const s = steps[i];
    s.label = /^En cours sur /.test(msg) ? 'Partie en cours sur un autre appareil' : failLabel[i] ?? 'Échec';
    s.detail = msg;
    s.progress = s.progress !== undefined ? 0 : undefined;
    s.state = 'a_venir';
    failed = true;
    // La partie est jouée : seul l'envoi se réessaie, jamais le lancement du jeu.
    sendOnly = i === 5;
    launching = false;
    downloading = false;
    phase = 'prep';
    feedback('err');
    requestAnimationFrame(() => focusFirst());
  }

  /** Fin d'un lancement ou d'un renvoi : conflit à trancher, ou sauvegarde envoyée (retour à l'accueil). */
  async function finish(r: LaunchResult) {
    if (r.result === 'conflit') {
      router.replace({ name: 'conflit', conflict: { game: preparation.game, profile: preparation.profile, local: r.local, server: r.server, played: r.played, rev: r.rev } });
      return;
    }
    for (const st of steps) st.state = 'fait';
    phase = 'sending';
    launching = false;
    if (r.emulatorError) {
      // Sortie en erreur : la sauvegarde est synchronisée, mais ce n'est pas un succès à fêter.
      steps[4].label = 'Partie terminée';
      steps[5].label = 'L\u2019émulateur s\u2019est arrêté avec une erreur · sauvegarde synchronisée';
      steps[5].detail = r.detail;
      steps[5].state = 'a_venir';
      ended = true;
      feedback('err');
      requestAnimationFrame(() => focusFirst());
      return;
    }
    steps[5].label = 'Sauvegarde envoyée';
    steps[5].detail = r.detail;
    feedback('saved');
    await wait(1200);
    router.reset({ name: 'accueil' });
  }

  /** Lancement réel, ou seulement le renvoi de la sauvegarde (`sendOnly`, partie déjà jouée). */
  async function runReal() {
    failed = false;
    cancelling = false;
    busy = true;
    try {
      if (sendOnly) {
        steps[5].label = 'Envoi de la sauvegarde…';
        steps[5].detail = undefined;
        steps[5].state = 'en_cours';
        phase = 'sending';
        await finish(await api.retrySend(preparation.profile, preparation.game.id));
      } else {
        steps = initial.map((x) => ({ ...x }));
        await finish(
          await api.launchGame(preparation.profile, preparation.game.id, onStep, (p, prep) => {
            const s = steps[1];
            if (s.progress === undefined) return;
            // Le serveur convertit d'abord le fichier (NSZ → NSP) : l'étape le dit, la barre suit le convertisseur.
            s.label = prep ? prepLabel(prep) : dlLabel;
            s.progress = prep ? (prep.percent ?? 0) : Math.round(p * 100);
          }),
        );
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (cancelling || msg === CANCELLED) {
        router.back();
        return;
      }
      onFailure(msg);
    } finally {
      busy = false;
    }
  }

  function cancel() {
    if (!downloading || cancelling) return;
    cancelling = true;
    void api.cancelInstall(preparation.game.id);
  }

  /** Vol de la tuile depuis l'accueil (FLIP), avec un léger arc vers le haut à mi-course. */
  function flyIn() {
    const from = router.heroRect;
    router.heroRect = null;
    const tile = art?.querySelector<HTMLElement>('.tile');
    if (!from || !tile || reducedMotion()) return;
    const to = tile.getBoundingClientRect();
    const zoom = parseFloat(getComputedStyle(document.querySelector('.app') as Element).zoom || '1') || 1;
    const dx = (from.left - to.left) / zoom;
    const dy = (from.top - to.top) / zoom;
    const s = from.width / to.width;
    art?.animate(
      [
        { transform: `translate(${dx}px, ${dy}px) scale(${s})` },
        { transform: `translate(${dx * 0.45}px, ${dy * 0.45 - 18}px) scale(${(s + 1) / 2})`, offset: 0.5 },
        { transform: 'none' },
      ],
      { duration: 560, easing: 'cubic-bezier(.16, 1, .3, 1)' },
    );
  }

  onMount(() => {
    flyIn();
    focusFirst();
    feedback('loading');
    if (isDemo) void runSteps();
    else void runReal();
    // B pendant le téléchargement : annuler (l'écran se déclare « modal » le temps du transfert).
    window.addEventListener('ludo:modal-back', cancel);
    const unlisten = isDemo ? Promise.resolve(() => {}) : api.onCloseRefused(() => { closeNote = true; });
    return () => {
      window.removeEventListener('ludo:modal-back', cancel);
      void unlisten.then((u) => u());
    };
  });
</script>

<div class="screen" class:launching data-modal={busy ? "" : undefined}>
  <Ambiance art={preparation.game.art} cover={preparation.game.cover} background={preparation.game.background} x={14} dim />
  <main>
    <div class="art" bind:this={art}><Tile game={preparation.game} size={340} big still hero shine={launching} lifting={launching} /></div>
    <div class="side" class:dim={launching}>
      <div class="head rise" style:--i={1}>
        <h1>{phase === 'playing' ? 'En jeu' : phase === 'sending' ? 'Sauvegarde' : launching ? 'Lancement' : 'Préparation'}</h1>
        <span>{preparation.game.title} · {preparation.game.emulator}</span>
      </div>
      <ol>
        {#each steps as step, i (i)}
          <li class="{step.state} rise" style:--i={i + 2}>
            <span class="line">
              {#if step.state === 'fait'}
                <span class="check draw" style:--len="16"><Icon name="check-circle" size={26} color="var(--ok)" /></span>
              {:else if step.state === 'en_cours'}
                <span class="spin"><Icon name="spinner" size={26} color="var(--accent)" /></span>
              {:else}
                <Icon name="circle" size={26} color="#5b6170" />
              {/if}
              <span class="label">{step.label}</span>
              {#if step.progress !== undefined}<span class="pct">{step.progress} %</span>{/if}
            </span>
            {#if step.progress !== undefined}
              <span class="bar" role="progressbar" aria-valuenow={step.progress} aria-valuemin="0" aria-valuemax="100" aria-label={step.label}>
                <span style:width="{step.progress}%"></span>
              </span>
            {/if}
            {#if step.detail}<span class="detail">{step.detail}</span>{/if}
          </li>
        {/each}
      </ol>
      {#if downloading}
        <button class="act focusable" data-autofocus data-sound="none" disabled={cancelling} onclick={cancel}>{cancelling ? 'Annulation…' : 'Annuler'}</button>
      {:else if failed}
        <span class="acts">
          <button class="act focusable" data-autofocus onclick={() => void runReal()}>{sendOnly ? 'Réessayer l\u2019envoi' : 'Réessayer'}</button>
          <button class="act focusable" onclick={() => router.back()}>Retour</button>
        </span>
      {:else if ended}
        <button class="act focusable" data-autofocus onclick={() => router.reset({ name: 'accueil' })}>Retour</button>
      {/if}
      {#if closeNote && busy}<p class="note" role="status">Partie en cours : fermer le jeu d\u2019abord</p>{/if}
    </div>
  </main>
  <ButtonHints note="Sauvegardes conservées sur le serveur" hints={[{ key: 'B', label: 'Annuler' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; position: relative; isolation: isolate; }
  main { flex-grow: 1; display: flex; align-items: center; gap: 72px; padding: 0 96px; }
  .art { pointer-events: none; transform-origin: top left; }
  /* Le panneau arrive juste après l'atterrissage de la tuile (follow-through), depuis la droite. */
  .side { animation: side-in 380ms var(--ease-land) 200ms both; transition: opacity var(--t-slow) ease-out, filter var(--t-slow) ease-out; }
  @keyframes side-in { from { opacity: 0; transform: translateX(14px); } }
  /* Au lancement, le panneau s'efface un peu : toute l'attention va à la tuile qui s'illumine. */
  .side.dim { opacity: 0.55; filter: saturate(0.8); }
  /* Coche réussie : la pastille grandit avec un léger dépassement, puis le trait se dessine. */
  .check { animation: check-pop 260ms var(--ease-pop) both; }
  @keyframes check-pop { from { transform: scale(0.7); opacity: 0.4; } to { transform: scale(1); opacity: 1; } }
  /* Reflet régulier sur la partie remplie de la barre : la progression paraît vivante. */
  .bar span { position: relative; overflow: hidden; }
  .bar span::after { content: ''; position: absolute; inset: 0; width: 40%; background: linear-gradient(90deg, transparent, rgba(255,255,255,0.28), transparent); animation: bar-gleam 1.8s ease-in-out infinite; }
  @keyframes bar-gleam { from { transform: translateX(-120%); } to { transform: translateX(260%); } }
  .side { flex-grow: 1; display: flex; flex-direction: column; gap: 36px; }
  @media (prefers-reduced-motion: reduce) { .side, .check { animation: none; } .bar span::after { display: none; } }
  .head { display: flex; flex-direction: column; gap: 10px; }
  h1 { margin: 0; font-size: 36px; font-weight: 600; letter-spacing: -0.01em; }
  .head span { font-size: 17px; color: var(--ink-3); }
  ol { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 22px; font-size: 18px; }
  li { display: flex; flex-direction: column; gap: 12px; color: var(--ink-2); transition: color var(--t-std) ease-out; }
  li.a_venir { color: var(--ink-4); }
  li.en_cours .label { font-weight: 500; color: var(--ink); }
  .line { display: flex; align-items: center; gap: 16px; }
  .line > span:first-child { display: flex; }
  .pct { margin-left: auto; color: var(--ink-2); min-width: 4ch; text-align: right; }
  .bar { margin-left: 42px; height: 8px; border-radius: 4px; background: var(--raised); display: block; overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: 4px; background: var(--accent); transition: width 120ms linear; }
  .detail { margin-left: 42px; font-size: 14px; color: var(--ink-4); }
  .act { all: unset; cursor: pointer; align-self: flex-start; margin-left: 42px; padding: 12px 24px; border-radius: 12px; font-size: 16px; font-weight: 600; background: var(--control); color: var(--ink); }
  .act:focus { box-shadow: inset 0 0 0 2px var(--accent); }
  .act:disabled { cursor: default; opacity: 0.8; }
  .note { margin: 0 0 0 42px; font-size: 15px; color: var(--ink-3); }
  .acts { display: flex; gap: 12px; margin-left: 42px; }
  .acts .act { margin-left: 0; }
  .spin { animation: spin 900ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
