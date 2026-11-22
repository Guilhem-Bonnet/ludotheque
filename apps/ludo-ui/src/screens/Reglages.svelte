<script lang="ts">
  import { onMount } from 'svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import CloudIcon from '../lib/components/CloudIcon.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api, isAndroid, isDemo } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { syncRhythm, play as playSound, setVolume } from '../lib/sound';
  import { appUpdate, checkAppUpdate, installAppUpdate } from '../lib/appUpdate.svelte';
  import type { DeviceInfo, Emulator } from '../lib/types';

  // Réglages de l'appareil (ceux du profil sont dans l'écran Profil) : son, serveur, synchronisation,
  // appareil, à propos. Enregistrés sur l'appareil, appliqués tout de suite.
  let { device }: { device: DeviceInfo } = $props();

  const load = (k: string, d: string) => { try { return localStorage.getItem(k) ?? d; } catch { return d; } };
  const store = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* stockage indisponible */ } };

  let volume = $state(Number(load('ludo.volume', '7')));
  let syncAtStart = $state(load('ludo.syncAtStart', '1') === '1');
  let syncAtEnd = $state(load('ludo.syncAtEnd', '1') === '1');
  let volFocused = $state(false);
  let test = $state<'idle' | 'busy' | 'ok' | 'ko'>('idle');
  let latency = $state(0);

  let volEl: HTMLButtonElement | undefined = $state();
  onMount(() => {
    focusFirst();
    const onAdjust = (ev: Event) => {
      const d = (ev as CustomEvent<number>).detail;
      const next = Math.max(0, Math.min(10, volume + d));
      if (next === volume) { feedback('err'); return; }
      volume = next;
      setVolume(volume / 10);
      store('ludo.volume', String(volume));
      playSound('move'); // aperçu du niveau choisi
    };
    volEl?.addEventListener('adjust', onAdjust);
    return () => volEl?.removeEventListener('adjust', onAdjust);
  });

  async function testServer() {
    if (test === 'busy') return;
    test = 'busy';
    feedback('syncStart');
    const stopRhythm = syncRhythm();
    try {
      latency = await api.ping();
      test = 'ok';
      feedback('saved');
    } catch {
      test = 'ko';
      feedback('err');
    } finally {
      stopRhythm();
    }
  }

  /* Émulateurs : recherchés à l'ouverture et sur demande (aucun cache : un émulateur installé depuis
     est vu tout de suite ; un émulateur nouveau se déclare par une fiche dans le dossier de fiches). */
  let emus = $state<Emulator[]>([]);
  let emuState = $state<'busy' | 'ok' | 'ko'>('busy');
  let emuError = $state('');
  async function scanEmulators(manual = false) {
    emuState = 'busy';
    if (manual) feedback('syncStart');
    try {
      emus = await api.emulators();
      emuState = 'ok';
      if (manual) feedback('saved');
    } catch (e) {
      emuError = e instanceof Error ? e.message : String(e);
      emuState = 'ko';
      if (manual) feedback('err');
    }
  }
  onMount(() => void scanEmulators());
  const CONSOLE_NAMES: Record<string, string> = { switch: 'Switch', switch2: 'Switch 2', '3ds': '3DS', ds: 'DS', wiiu: 'Wii U', gc: 'GameCube' };

  /* Lancement à l'ouverture de la session (bureau seulement ; masqué sur Android). L'état se lit à
     l'ouverture de l'écran (fichier d'autostart de la session), le changement s'applique tout de suite. */
  let autostart = $state<boolean | null>(null);
  let autostartError = $state('');
  if (!isAndroid) onMount(() => { api.autostart().then((on) => (autostart = on)).catch((e) => (autostartError = e instanceof Error ? e.message : String(e))); });
  let autostartBusy = false;
  async function toggleAutostart() {
    if (autostartBusy) return; // un seul changement à la fois
    if (autostart === null) { feedback('err'); return; }
    autostartBusy = true;
    const want = !autostart;
    autostart = want;
    autostartError = '';
    try {
      autostart = await api.setAutostart(want);
      feedback('tab');
    } catch (e) {
      autostart = !want;
      autostartError = e instanceof Error ? e.message : String(e);
      feedback('err');
    } finally {
      autostartBusy = false;
    }
  }

  function toggle(which: 'start' | 'end') {
    if (which === 'start') { syncAtStart = !syncAtStart; store('ludo.syncAtStart', syncAtStart ? '1' : '0'); }
    else { syncAtEnd = !syncAtEnd; store('ludo.syncAtEnd', syncAtEnd ? '1' : '0'); }
    feedback('tab');
  }
</script>

<div class="screen">
  <header><h1>Réglages</h1><span class="meta">{device.name}</span></header>
  <main>
    <section>
      <h2>Son</h2>
      <button bind:this={volEl} class="row focusable" data-adjust data-autofocus data-sound="none" aria-label="Volume : {volume} sur 10"
        onfocus={() => (volFocused = true)} onblur={() => (volFocused = false)} onclick={() => playSound('ok')}>
        <Icon name="volume" size={22} />Volume
        <span class="bars" aria-hidden="true">
          {#each Array(10) as _, i (i)}<span class:on={i < volume} style:--h="{8 + i * 1.6}px"></span>{/each}
        </span>
        <span class="num">{volume}</span>
      </button>
    </section>

    <section>
      <h2>Serveur</h2>
      <div class="row static"><Icon name="pin" size={22} />Adresse<span class="value">{(device.serverUrl ?? "https://ludotheque.srvdreamer.fr").replace(/^https?:\/\//, "")}</span></div>
      <button class="row focusable" data-sound="none" onclick={testServer} aria-busy={test === 'busy'}>
        <CloudIcon name={test === 'ko' ? 'cloud-off' : test === 'ok' ? 'cloud-ok' : 'cloud-sync'} busy={test === 'busy'} color={test === 'ok' ? 'var(--ok)' : test === 'ko' ? 'var(--offline)' : undefined} />
        Tester la connexion
        <span class="value" class:good={test === 'ok'}>
          {test === 'busy' ? 'Test…' : test === 'ok' ? `Connecté · ${latency} ms` : test === 'ko' ? 'Injoignable' : device.server === 'connecte' ? 'Connecté' : 'Hors ligne'}
        </span>
      </button>
    </section>

    <section>
      <h2>Synchronisation</h2>
      <button class="row focusable" role="switch" aria-checked={syncAtStart} data-sound="none" onclick={() => toggle('start')}>
        <Icon name="cloud-sync" size={22} />Synchroniser au démarrage<span class="switch" class:on={syncAtStart}><span></span></span>
      </button>
      <button class="row focusable" role="switch" aria-checked={syncAtEnd} data-sound="none" onclick={() => toggle('end')}>
        <Icon name="cloud-ok" size={22} />Envoyer la sauvegarde à la fin de chaque partie<span class="switch" class:on={syncAtEnd}><span></span></span>
      </button>
    </section>

    <section>
      <h2>Émulateurs</h2>
      {#each emus as e (e.id)}
        <div class="emu" class:missing={!e.found}>
          <span class="dot" class:on={e.found} aria-hidden="true"></span>
          <span class="emu-name">{e.name}</span>
          <span class="emu-consoles">{e.consoles.map((c) => CONSOLE_NAMES[c] ?? c).join(', ')}{e.defaultFor.length ? ' · par défaut' : ''}</span>
          <span class="emu-src" title={e.found ? `${e.bin}\n${e.data}` : undefined}>{e.found ? e.source : 'Introuvable'}</span>
        </div>
      {/each}
      {#if emuState === 'ko'}<p class="emu-note">Recherche impossible : {emuError}</p>{/if}
      <button class="row focusable" data-sound="none" onclick={() => scanEmulators(true)} aria-busy={emuState === 'busy'}>
        <Icon name="update" size={22} />Rechercher les émulateurs
        <span class="value">{emuState === 'busy' ? 'Recherche…' : `${emus.filter((e) => e.found).length} trouvé${emus.filter((e) => e.found).length > 1 ? 's' : ''} sur ${emus.length}`}</span>
      </button>
      <p class="emu-note">Nouvel émulateur : déposer sa fiche dans le dossier de fiches de l'appareil, puis relancer la recherche.</p>
    </section>

    {#if !isAndroid}
      <section>
        <h2>Démarrage</h2>
        <button class="row focusable" role="switch" aria-checked={autostart === true} data-sound="none" disabled={autostart === null && !autostartError} onclick={toggleAutostart}>
          <Icon name="play" size={22} />Lancer la Ludothèque à l'ouverture de la session<span class="switch" class:on={autostart === true}><span></span></span>
        </button>
        {#if autostartError}<p class="emu-note">{autostartError}</p>{:else if device.kind === 'deck'}<p class="emu-note">Steam Deck : en mode Bureau seulement ; le mode Jeu ne lance pas les applications de démarrage.</p>{/if}
      </section>
    {/if}

    <section>
      <h2>Appareil</h2>
      <div class="row static"><Icon name={device.kind} size={22} />Nom<span class="value">{device.name}</span></div>
      <div class="row static"><Icon name="download" size={22} />Espace libre<span class="value">{device.freeSpace}</span></div>
    </section>

    <section>
      <h2>À propos</h2>
      <div class="row static"><Icon name="grid" size={22} />Ludothèque<span class="value">{device.version ?? '0.1.0'}{isDemo ? ' · démo' : ''}</span></div>
      {#if appUpdate.status === 'available'}
        <button class="row focusable" data-sound="none" onclick={() => void installAppUpdate()}>
          <Icon name="update" size={22} />Mettre à jour vers {appUpdate.version}<span class="value good">Disponible</span>
        </button>
      {:else}
        <button class="row focusable" data-sound="none" onclick={() => void checkAppUpdate()} aria-busy={appUpdate.status === 'checking'} disabled={appUpdate.status === 'downloading' || appUpdate.status === 'restarting'}>
          <Icon name="update" size={22} />Rechercher une mise à jour
          <span class="value">{appUpdate.status === 'checking' ? 'Recherche…' : appUpdate.status === 'none' ? 'À jour' : appUpdate.status === 'downloading' ? `Téléchargement… ${appUpdate.percent ?? ''}${appUpdate.percent === null ? '' : ' %'}` : appUpdate.status === 'restarting' ? 'Redémarrage…' : appUpdate.status === 'error' ? 'Vérification impossible' : ''}</span>
        </button>
      {/if}
    </section>
  </main>
  <ButtonHints hints={[{ key: 'A', label: 'Sélectionner' }, { key: 'B', label: 'Retour' }]} note={volFocused ? 'Gauche / droite : régler le volume' : undefined} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; }
  header { display: flex; align-items: baseline; justify-content: space-between; padding: 28px 48px 0; }
  h1 { margin: 0; font-size: 30px; font-weight: 600; }
  .meta { font-size: 15px; color: var(--ink-3); }
  main { flex: 1; overflow-y: auto; padding: 18px 48px 24px; display: flex; flex-direction: column; gap: 22px; max-width: 760px; scrollbar-width: none; }
  section { display: flex; flex-direction: column; gap: 8px; }
  h2 { margin: 0 0 2px; font-size: 13px; font-weight: 600; letter-spacing: 0.05em; text-transform: uppercase; color: var(--ink-3); }
  .row {
    all: unset; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: 14px; cursor: pointer;
    padding: 13px 18px; border-radius: 12px; background: var(--surface); font-size: 16px; color: var(--ink);
    transition: background-color var(--t-quick) ease-out, transform var(--t-quick) var(--ease-pop), box-shadow var(--t-quick) ease-out;
  }
  .row.static { cursor: default; background: transparent; padding: 8px 18px; color: var(--ink-2); }
  .row.focusable:focus { background: var(--raised); box-shadow: inset 0 0 0 2px var(--accent); transform: translateX(4px); }
  .value { margin-left: auto; color: var(--ink-3); font-size: 15px; }
  .value.good { color: var(--ok); }
  .bars { margin-left: auto; display: flex; align-items: flex-end; gap: 4px; height: 24px; }
  .bars span { width: 8px; height: var(--h); border-radius: 2px; background: rgba(255, 255, 255, 0.16); transition: background-color 120ms ease-out, transform 160ms var(--ease-pop); }
  .bars span.on { background: var(--accent); }
  .num { width: 22px; text-align: right; font-variant-numeric: tabular-nums; color: var(--ink-2); }
  .switch { margin-left: auto; flex-shrink: 0; width: 46px; height: 26px; border-radius: 13px; background: rgba(255, 255, 255, 0.18); position: relative; transition: background-color var(--t-std) ease-out; }
  .switch span { position: absolute; top: 3px; left: 3px; width: 20px; height: 20px; border-radius: 50%; background: #fff; transition: transform var(--t-std) var(--ease-pop); box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3); }
  .switch.on { background: var(--accent); }
  .switch.on span { transform: translateX(20px); }
  .emu { display: grid; grid-template-columns: 12px 120px 1fr auto; align-items: center; gap: 12px; padding: 8px 18px; font-size: 15px; color: var(--ink); }
  .emu.missing { color: var(--ink-3); }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: rgba(255, 255, 255, 0.2); }
  .dot.on { background: var(--ok); box-shadow: 0 0 0 3px rgba(95, 211, 154, 0.25); /* = --ok (#5fd39a) à 25 % ; couleur précalculée : la WebView de la Thor (Chromium 109) ignore le mélange CSS de couleurs */ }
  .emu-name { font-weight: 600; }
  .emu-consoles { color: var(--ink-3); font-size: 14px; }
  .emu-src { color: var(--ink-2); font-size: 13px; max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .emu-note { margin: 2px 18px 0; font-size: 13px; color: var(--ink-3); }
  @media (prefers-reduced-motion: reduce) { .row.focusable:focus { transform: none; } }
</style>
