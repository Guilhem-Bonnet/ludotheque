<script lang="ts">
  // Colonnes latérales des grands formats (lot « ecrans ») : 21:9 → une colonne à droite, 32:9 → une de chaque
  // côté, portrait (Thor) → un panneau sous l'écran. Purement informatives : aucun élément focalisable, la
  // navigation à la manette reste celle de l'écran. Contenu :
  //   - Jeu choisi (accueil) : suit le focus de la rangée (événement `focusin`, aucune scrutation) ;
  //   - Activité récente : dernières parties du profil ;
  //   - Appareils : cet appareil et les autres appareils du foyer (inventaires du serveur).
  // Rechargé à l'arrivée sur l'écran et au retour du focus sur la fenêtre (même règle que les mises à jour).
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import { api } from '../../api';
  import { reducedMotion } from '../../motion';
  import { layout } from '../../layout';
  import { router } from '../../router.svelte';
  import type { DeviceInfo, Game, RemoteDevice, SyncState } from '../../types';
  import CoverArt from '../CoverArt.svelte';
  import Icon from '../Icon.svelte';

  let { device }: { device: DeviceInfo } = $props();

  let recent = $state<Game[]>([]);
  let remotes = $state<RemoteDevice[]>([]);
  let focusedId = $state<string | null>(null);

  const screen = $derived(router.screen.name);
  const fiche = $derived(router.screen.name === 'fiche' ? router.screen.game : null);
  /** Jeu décrit : celui de la fiche, sinon la tuile en focus de l'accueil, sinon la dernière partie. */
  const chosen = $derived(fiche ?? recent.find((g) => g.id === focusedId) ?? recent[0] ?? null);
  const side = $derived($layout.side);

  let loadedFor = '';
  let loadedProfile = '';
  /** Génération du dernier chargement demandé : une réponse plus ancienne arrivée après est ignorée. */
  let generation = 0;
  async function load(force = false) {
    const p = router.profile;
    if (!p) return;
    const key = `${p.id}:${screen}`;
    if (!force && key === loadedFor) return;
    loadedFor = key;
    if (p.id !== loadedProfile) {
      // Autre joueur : rien de l'ancien profil ne reste affiché pendant le chargement.
      loadedProfile = p.id;
      recent = [];
      focusedId = null;
    }
    const gen = ++generation;
    // Échecs silencieux : une colonne vide vaut mieux qu'un message d'erreur en double de l'écran.
    const [r, d] = await Promise.allSettled([api.recent(p.id), api.devices()]);
    if (gen !== generation) return;
    if (r.status === 'fulfilled') recent = r.value;
    if (d.status === 'fulfilled') remotes = d.value;
    if (r.status === 'rejected' && d.status === 'rejected') loadedFor = ''; // nouvel essai au prochain passage
  }

  $effect(() => {
    void router.profile?.id;
    void screen;
    void load();
  });

  onMount(() => {
    // Tuile en focus : `data-game` si l'écran le pose, sinon position de la tuile dans la rangée (`data-slot`),
    // qui suit l'ordre de `api.recent` (Accueil.svelte, load()).
    const onFocusIn = (e: FocusEvent) => {
      const el = e.target instanceof HTMLElement ? e.target : null;
      if (!el) return;
      const id = el.closest<HTMLElement>('[data-game]')?.dataset.game;
      if (id) {
        focusedId = id;
        return;
      }
      const slot = el.closest('[data-slot]');
      if (slot?.parentElement) {
        const i = Array.from(slot.parentElement.querySelectorAll(':scope > [data-slot]')).indexOf(slot);
        if (i >= 0 && recent[i]) focusedId = recent[i].id;
      }
    };
    const onWindowFocus = () => void load(true);
    document.addEventListener('focusin', onFocusIn);
    window.addEventListener('focus', onWindowFocus);
    return () => {
      document.removeEventListener('focusin', onFocusIn);
      window.removeEventListener('focus', onWindowFocus);
    };
  });

  const SYNC: Record<SyncState, { label: string; color: string }> = {
    a_jour: { label: 'À jour', color: 'var(--ok)' },
    synchronisation: { label: 'Synchronisation…', color: 'var(--sync)' },
    envoi_en_attente: { label: 'Envoi en attente', color: 'var(--pending)' },
    conflit: { label: 'Conflit de sauvegarde', color: 'var(--conflict)' },
    hors_ligne: { label: 'Hors ligne', color: 'var(--offline)' },
    aucune: { label: 'Aucune sauvegarde', color: 'var(--ink-3)' },
    a_recuperer: { label: 'Nouvelle sauvegarde', color: 'var(--sync)' },
  };


  /** Ligne secondaire d'une partie récente : l'état s'il demande attention, sinon l'appareil et la date. */
  function activity(g: Game): { text: string; color: string } {
    if (g.sync === 'conflit' || g.sync === 'envoi_en_attente' || g.sync === 'a_recuperer') return { text: SYNC[g.sync].label, color: SYNC[g.sync].color };
    if (g.install === 'mise_a_jour') return { text: 'Mise à jour disponible', color: 'var(--update)' };
    if (g.last) return { text: `${g.last.device} · ${g.last.when}`, color: 'var(--ink-3)' };
    return { text: g.playtime ? `${g.playtime} de jeu` : SYNC[g.sync].label, color: 'var(--ink-3)' };
  }

  /** État du jeu décrit sur un autre appareil. */
  function onDevice(d: RemoteDevice, g: Game | null): { text: string; color: string } {
    if (!g) {
      const n = Object.values(d.games).filter((x) => x.installed).length;
      return { text: n ? `${n} ${n > 1 ? 'jeux' : 'jeu'}` : 'Aucun jeu', color: 'var(--ink-3)' };
    }
    const x = d.games[g.id];
    if (x?.installed) return { text: 'Installé', color: 'var(--ok)' };
    if (x?.stream) return { text: 'Flux depuis le PC', color: 'var(--accent-ink)' };
    if (x?.save) return { text: 'Sauvegarde seule', color: 'var(--ink-3)' };
    return { text: 'Non installé', color: 'var(--ink-4)' };
  }

  function here(g: Game | null): { text: string; color: string } {
    if (!g) return device.server === 'connecte' ? { text: device.freeSpace, color: 'var(--ink-3)' } : { text: 'Hors ligne', color: 'var(--offline)' };
    if (g.install === 'installe' || g.install === 'mise_a_jour') return { text: 'Installé', color: 'var(--ok)' };
    if (g.install === 'telechargement') return { text: 'Téléchargement…', color: 'var(--accent-ink)' };
    if (g.stream) return { text: 'Flux depuis le PC', color: 'var(--accent-ink)' };
    return { text: 'Non installé', color: 'var(--ink-4)' };
  }

  const appear = (node: Element) => fade(node, { duration: reducedMotion() ? 0 : 240, delay: reducedMotion() ? 0 : 110 });
</script>

{#snippet jeuChoisi(g: Game)}
  <section class="panel" aria-label="Jeu choisi">
    <h2>Jeu choisi</h2>
    <p class="title">{g.title}</p>
    <dl>
      {#if g.emulator}<dt>Émulateur</dt><dd>{g.emulator}</dd>{/if}
      {#if g.version}<dt>Version</dt><dd>{g.version}</dd>{/if}
      {#if g.playtime}<dt>Temps de jeu</dt><dd>{g.playtime}</dd>{/if}
      <dt>Sauvegarde</dt><dd style:color={SYNC[g.sync].color}>{SYNC[g.sync].label}</dd>
      {#if g.last}<dt>Dernière partie</dt><dd>{g.last.device} · {g.last.when}</dd>{/if}
    </dl>
  </section>
{/snippet}

{#snippet activite(max: number)}
  <section class="panel" aria-label="Activité récente">
    <h2>Activité récente</h2>
    {#if recent.length === 0}
      <p class="empty">Aucune partie récente</p>
    {:else}
      <ul class="games">
        {#each recent.slice(0, max) as g (g.id)}
          {@const a = activity(g)}
          <li>
            <span class="mini" style:background={g.tile}>
              {#if g.cover}<img src={g.cover} alt="" loading="lazy" />{:else if g.art}<CoverArt art={g.art} />{/if}
            </span>
            <span class="lines">
              <span class="name">{g.title}</span>
              <span class="sub" style:color={a.color}>{a.text}</span>
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/snippet}

{#snippet appareils(g: Game | null)}
  {@const h = here(g)}
  <section class="panel" aria-label="Appareils">
    <h2>Appareils</h2>
    <ul class="devices">
      <li>
        <Icon name={device.kind} size={20} color="var(--ink-3)" />
        <span class="name">{device.name}<span class="this">{' · cet appareil'}</span></span>
        <span class="state" style:color={h.color}>{h.text}</span>
      </li>
      {#each remotes as d (d.id)}
        {@const s = onDevice(d, g)}
        <li>
          <Icon name={d.kind} size={20} color="var(--ink-3)" />
          <span class="name">{d.name}</span>
          <span class="state" style:color={s.color}>{s.text}</span>
        </li>
      {/each}
    </ul>
  </section>
{/snippet}

<div class="colonnes" data-side={side}>
  {#key screen}
    {#if side === 'right' || side === 'both'}
      {#if side === 'both'}
        <aside class="col left" in:appear>{@render activite(10)}</aside>
      {/if}
      <aside class="col right" in:appear>
        {#if screen === 'accueil' && chosen}{@render jeuChoisi(chosen)}{/if}
        {@render appareils(fiche ?? (screen === 'accueil' ? chosen : null))}
        {#if side === 'right'}{@render activite(screen === 'accueil' ? 5 : 8)}{/if}
      </aside>
    {:else if side === 'bottom'}
      <aside class="bottom" in:appear>
        {#if screen === 'accueil' && chosen}{@render jeuChoisi(chosen)}{/if}
        {@render activite(8)}
        {@render appareils(fiche ?? (screen === 'accueil' ? chosen : null))}
      </aside>
    {/if}
  {/key}
</div>

<style>
  .colonnes { position: absolute; inset: 0; z-index: 1; pointer-events: none; transition: opacity var(--t-quick) ease-out, visibility var(--t-quick); }
  .col {
    position: absolute; top: 28px; bottom: 82px; width: var(--side-w);
    display: flex; flex-direction: column; gap: 16px; overflow: hidden;
  }
  .col.right { right: var(--gutter); }
  .col.left { left: var(--gutter); }
  .bottom {
    position: absolute; left: var(--gutter); right: var(--gutter); bottom: 24px; height: calc(var(--bottom-h) - 48px);
    display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); align-content: start; align-items: start; gap: 16px; overflow: hidden;
  }
  /* Pastilles translucides : le fond d'ambiance (couleur du jeu) transparaît, sans flou (coût GPU nul). */
  .panel {
    border-radius: var(--radius-card); background: rgba(10, 14, 18, 0.46); border: 1px solid rgba(255, 255, 255, 0.06);
    padding: 18px 22px 20px; display: flex; flex-direction: column; gap: 12px; min-height: 0;
  }
  h2 { margin: 0; font-size: 13px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--ink-3); }
  .title { margin: 0; font-size: 22px; font-weight: 700; line-height: 1.2; color: var(--ink); overflow-wrap: anywhere; }
  dl { margin: 0; display: grid; grid-template-columns: max-content 1fr; column-gap: 18px; row-gap: 8px; font-size: 15px; }
  dt { color: var(--ink-3); }
  dd { margin: 0; color: var(--ink); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
  .games li { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .mini { position: relative; isolation: isolate; flex-shrink: 0; width: 44px; height: 44px; border-radius: 10px; overflow: hidden; box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08); }
  .mini img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .lines { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .name { font-size: 15px; font-weight: 600; color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .devices li { display: flex; align-items: center; gap: 12px; font-size: 15px; min-width: 0; }
  .devices .name { flex: 1; font-weight: 500; }
  .this { color: var(--ink-3); font-weight: 400; }
  .state { flex-shrink: 0; white-space: nowrap; }
  .empty { margin: 0; font-size: 15px; color: var(--ink-3); }
</style>
