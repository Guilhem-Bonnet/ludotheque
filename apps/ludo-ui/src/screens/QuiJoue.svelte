<script lang="ts">
  import StateNotice from '../lib/components/StateNotice.svelte';
  import { onMount } from 'svelte';
  import Avatar from '../lib/components/Avatar.svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api } from '../lib/api';
  import { feedback } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { DeviceInfo, Profile } from '../lib/types';

  let { device }: { device: DeviceInfo } = $props();
  let profiles = $state<Profile[]>([]);
  let focused = $state(0);
  let loadError = $state<string | null>(null);

  // Le profil en cours (ou retenu sur l'appareil) a le focus d'entrée : A suffit pour le reprendre.
  const current = router.profile?.id;
  const canBack = router.canBack;

  onMount(() => {
    api.profiles().then((p) => {
      profiles = p;
      focused = Math.max(0, p.findIndex((x) => x.id === current));
      focusFirst();
    }).catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
      focusFirst();
    });
  });

  let chosen = $state<string | null>(null);

  function choose(p: Profile) {
    if (chosen) return; // double appui : un seul choix
    // L'avatar choisi grandit, les autres s'effacent, l'accent de l'interface se teinte de sa couleur,
    // puis l'accueil arrive. Son « arrivée du profil ».
    chosen = p.id;
    router.profile = p;
    feedback('arrive');
    // Retenu sur l'appareil : le prochain démarrage ouvre directement ce profil. Échec sans gravité (rien à
    // afficher : le choix vaut pour cette session).
    void api.rememberProfile(p.id).catch(() => {});
    setTimeout(() => router.reset({ name: 'accueil' }), 520);
  }
</script>

<div class="screen">
  <header>
    <span class="where"><Icon name={device.kind} size={20} />{device.name}</span>
    <span class="where">
      {#if device.server === 'connecte'}<Icon name="cloud-ok" size={20} color="var(--ok)" />Serveur connecté{:else}<Icon name="cloud-off" size={20} color="var(--offline)" />Hors ligne{/if}
    </span>
  </header>

  <main>
    <h1 class="rise" style:--i={0}>Qui joue ?</h1>
    {#if loadError}<StateNotice title="Serveur injoignable" detail={loadError} action="Réessayer" onaction={() => location.reload()} />{/if}
    <div class="profiles">
      {#each profiles as p, i (p.id)}
        <button
          class="profile focusable rise"
          style:--i={i + 1}
          class:chosen={chosen === p.id}
          class:faded={chosen !== null && chosen !== p.id}
          data-sound="none"
          data-autofocus={i === focused ? '' : undefined}
          onfocus={() => (focused = i)}
          onclick={() => choose(p)}
          style:--accent={p.accent}
        >
          {#if chosen === p.id}<span class="burst" aria-hidden="true"></span>{/if}
          <Avatar profile={p} size={i === focused ? 160 : 134} ring={i === focused} celebrate={chosen === p.id} />
          <span class="label">
            <span class="name" class:dim={i !== focused}>{p.name}</span>
            {#if p.status}<span class="status">{p.status}</span>{:else if p.id === current}<span class="status">Profil actuel</span>{/if}
          </span>
        </button>
      {/each}
    </div>
  </main>

  <ButtonHints hints={canBack ? [{ key: 'A', label: 'Choisir' }, { key: 'B', label: 'Retour' }] : [{ key: 'A', label: 'Choisir' }]} />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; }
  header { display: flex; justify-content: space-between; align-items: center; padding: 30px 48px 0; font-size: 14px; color: var(--ink-2); }
  .where { display: flex; align-items: center; gap: 12px; }
  main { flex-grow: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 48px; }
  h1 { margin: 0; font-size: 38px; font-weight: 600; letter-spacing: -0.02em; }
  .profiles { display: flex; gap: 64px; align-items: center; }
  .profile { all: unset; cursor: pointer; display: flex; flex-direction: column; align-items: center; gap: 20px; border-radius: 24px; }
  .profile:focus { box-shadow: none; }
  .profile :global(.avatar) { transition: width var(--t-std) var(--ease-pop), height var(--t-std) var(--ease-pop), box-shadow var(--t-std) var(--ease-land), transform var(--t-slow) var(--ease-pop); }
  .profile.chosen :global(.avatar) { transform: scale(1.08); }
  .profile { transition: opacity var(--t-std) ease-out, transform var(--t-std) var(--ease-land); }
  .profile.faded { opacity: 0; transform: translateY(10px); }
  .profile { position: relative; }
  /* Onde de la couleur du joueur qui s'étend depuis l'avatar choisi. */
  .burst { position: absolute; top: 0; left: 50%; width: 160px; height: 160px; margin-left: -80px; border-radius: 50%; pointer-events: none;
    box-shadow: 0 0 0 3px var(--accent); animation: burst 520ms var(--ease-land) both; }
  @keyframes burst { from { transform: scale(1); opacity: 0.9; } to { transform: scale(1.6); opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .burst { display: none; } }
  .label { display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .name { font-size: 24px; font-weight: 600; }
  .name.dim { font-size: 21px; font-weight: 500; color: var(--ink-2); }
  .status { font-size: 14px; color: var(--ink-3); }
</style>
