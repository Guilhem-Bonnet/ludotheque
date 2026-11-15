<script lang="ts">
  import { onMount } from 'svelte';
  import Avatar from '../lib/components/Avatar.svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api } from '../lib/api';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import type { DeviceInfo, Profile } from '../lib/types';

  let { device }: { device: DeviceInfo } = $props();
  let profiles = $state<Profile[]>([]);
  let focused = $state(0);

  onMount(() => {
    api.profiles().then((p) => {
      profiles = p;
      focusFirst();
    });
  });

  function choose(p: Profile) {
    router.profile = p;
    router.reset({ name: 'accueil' });
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
    <h1>Qui joue ?</h1>
    <div class="profiles">
      {#each profiles as p, i (p.id)}
        <button
          class="profile focusable"
          data-autofocus={i === 0 ? '' : undefined}
          onfocus={() => (focused = i)}
          onclick={() => choose(p)}
          style:--accent={p.accent}
        >
          <Avatar profile={p} size={i === focused ? 160 : 134} ring={i === focused} />
          <span class="label">
            <span class="name" class:dim={i !== focused}>{p.name}</span>
            {#if p.status}<span class="status">{p.status}</span>{/if}
          </span>
        </button>
      {/each}
    </div>
  </main>

  <ButtonHints hints={[{ key: 'A', label: 'Choisir' }, { key: 'B', label: 'Retour' }]} />
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
  .label { display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .name { font-size: 24px; font-weight: 600; }
  .name.dim { font-size: 21px; font-weight: 500; color: var(--ink-2); }
  .status { font-size: 14px; color: var(--ink-3); }
</style>
