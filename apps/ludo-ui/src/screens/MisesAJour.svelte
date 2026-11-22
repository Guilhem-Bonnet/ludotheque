<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import ConsoleChip from '../lib/components/ConsoleChip.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api, isAndroid } from '../lib/api';
  import { router } from '../lib/router.svelte';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import StateNotice from '../lib/components/StateNotice.svelte';
  import type { UpdateItem } from '../lib/types';
  import { appUpdate, checkAppUpdate, installAppUpdate } from '../lib/appUpdate.svelte';
  const appCard = $derived(['available', 'downloading', 'restarting', 'error'].includes(appUpdate.status) && !!appUpdate.version);

  let items = $state<UpdateItem[]>([]);
  // Avancement par élément : undefined = pas commencé, 0-100 = en cours, 'fait', ou message d'échec.
  let progress = $state<Record<string, number | 'fait'>>({});
  let failed = $state<Record<string, string>>({});
  let loadError = $state<string | null>(null);
  let loaded = $state(false);

  /**
   * Installations en FILE UNIQUE (un élément après l'autre, bouton par bouton ou « Tout mettre à jour ») :
   * en parallèle, la mise à jour et le DLC d'un même jeu mélangeaient leurs barres (la progression est
   * suivie par jeu) et les sons de fin s'empilaient. Un élément en attente affiche sa barre à 0.
   */
  let queue: Promise<void> = Promise.resolve();
  let mounted = true;

  /** Met l'élément en file (`id` = « <jeu>|<fichier> » côté app réelle). */
  function run(it: UpdateItem) {
    if (progress[it.id] !== undefined) return;
    delete failed[it.id];
    const row = (document.activeElement as HTMLElement | null)?.closest('.item');
    progress[it.id] = 0;
    // Le bouton cède la place à la barre : le focus passe au bouton voisin au lieu de se perdre
    // (sinon la croix repartirait du haut de l'écran).
    requestAnimationFrame(() => keepFocusNear(row));
    queue = queue.then(() => install(it));
  }

  async function install(it: UpdateItem) {
    const [game, file] = it.id.split('|');
    try {
      await api.installContent(game, Number(file), (p) => (progress[it.id] = Math.round(p * 100)));
      progress[it.id] = 'fait';
      feedback(it.action === 'Installer' ? 'downloaded' : 'updated');
    } catch (e) {
      delete progress[it.id];
      failed[it.id] = e instanceof Error ? e.message : String(e);
      feedback('err');
    }
  }

  /** Focus perdu (bouton retiré) : il revient sur le bouton suivant, sinon le dernier, sinon le premier élément. */
  function keepFocusNear(row: Element | null | undefined) {
    if (!mounted) return;
    const a = document.activeElement;
    if (a && a !== document.body && a.isConnected) return;
    const acts = [...document.querySelectorAll<HTMLElement>('.item .act')];
    const after = row?.isConnected ? acts.find((el) => row.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING) : undefined;
    const target = after ?? acts.at(-1);
    if (target) target.focus();
    else focusFirst();
  }

  function load() {
    loadError = null;
    api.updates(router.profile?.id).then((u) => {
      items = u;
      loaded = true;
      focusFirst();
    }).catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
      loaded = true;
      focusFirst();
    });
  }

  /** Tout mettre en file ; le bouton disparaît aussitôt, le focus est repris s'il était dessus. */
  function runAll() {
    for (const it of items) run(it);
    requestAnimationFrame(() => keepFocusNear(null));
  }
  /** Reste-t-il un élément à lancer ? (Sinon le bouton « Tout mettre à jour » n'a plus de sens.) */
  const pending = $derived(items.some((it) => progress[it.id] === undefined));
  onMount(() => {
    load();
    // Ouvrir l'écran relance la recherche d'une nouvelle version de l'app (sauf si déjà trouvée).
    if (appUpdate.status !== 'available' && appUpdate.status !== 'downloading' && appUpdate.status !== 'restarting') void checkAppUpdate();
    // Les installations en file continuent après la sortie de l'écran ; seul le focus n'est plus touché.
    return () => { mounted = false; };
  });

  const GROUPS: { id: UpdateItem['group']; label: string }[] = [
    { id: 'jeux', label: 'Jeux' },
    { id: 'nouveaux', label: 'Nouveaux dans la bibliothèque' },
    { id: 'emulateurs', label: 'Émulateurs et Ludothèque' },
  ];
</script>

<div class="screen">
  <header>
    <h1>Mises à jour</h1>
    {#if items.length && pending}<button class="all focusable" data-autofocus onclick={runAll}>Tout mettre à jour</button>{/if}
  </header>

  <main>
    {#if loadError}
      <StateNotice title="Mises à jour indisponibles" detail={loadError} action="Réessayer" onaction={load} />
    {:else if loaded && items.length === 0 && !appCard}
      {#if isAndroid}
        <!-- Android : l'app n'installe ni jeu ni DLC (contenu des émulateurs dans leur Android/data) ; tout passe par le PC. -->
        <StateNotice icon="check-circle" title="Installation depuis le PC" detail="Les mises à jour et DLC des jeux de cet appareil s'installent depuis le PC." autofocus={false} />
      {:else}
        <StateNotice icon="check-circle" title="Tout est à jour" detail="Aucune mise à jour ni DLC à installer pour les jeux de cet appareil." autofocus={false} />
      {/if}
    {/if}
    {#if appCard}
      <!-- Mise à jour de l'app elle-même : bouton, puis barre de progression, puis redémarrage. -->
      <section>
        <h2>Ludothèque</h2>
        <div class="item app" class:busy={appUpdate.status === 'downloading' || appUpdate.status === 'restarting'}>
          <span class="thumb" style:background="linear-gradient(160deg, #4cc3ff, #1c3546)"><Icon name="update" size={26} color="#fff" /></span>
          <span class="text">
            <span class="title">Ludothèque {appUpdate.version}</span>
            <span class="detail">{appUpdate.status === 'restarting' ? 'Redémarrage…' : appUpdate.status === 'error' ? appUpdate.error : appUpdate.notes || 'Nouvelle version de l\'application'}</span>
            {#if appUpdate.status === 'downloading'}
              <span class="bar" role="progressbar" aria-label="Téléchargement de la mise à jour" aria-valuemin="0" aria-valuemax="100" aria-valuenow={appUpdate.percent ?? undefined}>
                <span class:indet={appUpdate.percent === null} style:width="{appUpdate.percent ?? 30}%"></span>
              </span>
            {/if}
          </span>
          {#if appUpdate.status === 'downloading'}
            <span class="pct">{appUpdate.percent === null ? 'Téléchargement…' : `${appUpdate.percent} %`}</span>
          {:else if appUpdate.status === 'restarting'}
            <span class="pct">Redémarrage…</span>
          {:else}
            <button class="act focusable" data-autofocus data-sound="none" onclick={() => { feedback('ok'); void installAppUpdate(); }}>{appUpdate.status === 'error' ? 'Réessayer' : 'Mettre à jour'}</button>
          {/if}
        </div>
      </section>
    {/if}
    {#each GROUPS as grp (grp.id)}
      {@const list = items.filter((i) => i.group === grp.id)}
      {#if list.length}
        <section>
          <h2>{grp.label}</h2>
          {#each list as it (it.id)}
            <div class="item" class:success={progress[it.id] === 'fait'}>
              <span class="thumb" style:background={it.tile ?? 'var(--control)'}>{#if !it.tile}<Icon name="settings" size={26} color="var(--ink-2)" />{/if}</span>
              <span class="text"><span class="title">{it.title}</span><span class="detail">{it.detail}</span></span>
              {#if it.console}<ConsoleChip console={it.console} size="m" on="ui" />{/if}
              {#if progress[it.id] === 'fait'}
                <span class="done draw" style:--len="16"><Icon name="check-circle" size={24} color="var(--ok)" />{it.action === 'Installer' ? 'Installé' : 'À jour'}</span>
              {:else if progress[it.id] !== undefined}
                <span class="mini" role="progressbar" aria-valuenow={Number(progress[it.id])} aria-valuemin="0" aria-valuemax="100" aria-label="{it.title} : {progress[it.id]} %"><span style:width="{progress[it.id]}%"></span></span>
              {:else}
                {#if failed[it.id]}<span class="fail" title={failed[it.id]}>Échec</span>{/if}
                <button class="act focusable" onclick={() => run(it)}>{failed[it.id] ? 'Réessayer' : it.action}</button>
              {/if}
            </div>
          {/each}
        </section>
      {/if}
    {/each}
  </main>

  <ButtonHints hints={[{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Retour' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; }
  header { padding: 36px 64px 0; display: flex; align-items: center; justify-content: space-between; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; letter-spacing: -0.01em; }
  .all { all: unset; cursor: pointer; white-space: nowrap; flex-shrink: 0; font-size: 16px; font-weight: 600; padding: 12px 22px; border-radius: 12px; background: var(--accent); color: var(--ground); }
  main { flex-grow: 1; overflow-y: auto; padding: 28px 64px 24px; display: flex; flex-direction: column; gap: 26px; }
  section { display: flex; flex-direction: column; gap: 10px; }
  h2 { margin: 0 0 4px; font-size: 17px; font-weight: 600; color: var(--ink-3); }
  .item { display: flex; align-items: center; gap: 18px; padding: 14px 18px; border-radius: 12px; background: var(--surface); }
  .thumb { width: 56px; height: 56px; border-radius: 10px; flex-shrink: 0; display: flex; align-items: center; justify-content: center; }
  .text { display: flex; flex-direction: column; gap: 4px; flex-grow: 1; min-width: 0; }
  .title { font-size: 17px; font-weight: 500; }
  .detail { font-size: 14px; color: var(--ink-3); }
  .act { all: unset; cursor: pointer; font-size: 15px; padding: 9px 16px; border-radius: var(--radius-control); background: var(--control); color: var(--ink); }
  .act:focus { background: var(--accent); color: var(--ground); }
  .item { transition: background-color 600ms ease-out; }
  .item.success { animation: success-flash 900ms ease-out; }
  @keyframes success-flash { 0% { background: color-mix(in srgb, var(--ok) 16%, var(--surface)); } 100% { background: var(--surface); } }
  .done { display: flex; align-items: center; gap: 8px; font-size: 15px; color: var(--ink-2); animation: pop 320ms var(--ease-pop); }
  .mini { width: 120px; height: 6px; border-radius: 3px; background: var(--raised); overflow: hidden; display: block; }
  .mini span { display: block; height: 100%; background: var(--accent); border-radius: 3px; transition: width 120ms linear; }
  .fail { font-size: 13px; color: #ff9b9b; }
  .item.app { background: color-mix(in srgb, var(--accent) 10%, var(--surface)); }
  .bar { display: block; margin-top: 8px; height: 8px; border-radius: 4px; background: var(--raised); overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: 4px; background: var(--accent); transition: width 160ms linear; }
  .bar span.indet { animation: indet 1.2s ease-in-out infinite; }
  @keyframes indet { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  .pct { min-width: 120px; text-align: right; font-size: 15px; font-weight: 600; color: var(--ink-2); font-variant-numeric: tabular-nums; }
</style>
