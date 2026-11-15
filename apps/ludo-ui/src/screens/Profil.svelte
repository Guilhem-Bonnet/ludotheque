<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Ambiance from '../lib/components/Ambiance.svelte';
  import Avatar from '../lib/components/Avatar.svelte';
  import AvatarArt from '../lib/components/AvatarArt.svelte';
  import ButtonHints from '../lib/components/ButtonHints.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { api } from '../lib/api';
  import { AVATARS, withAvatar } from '../lib/avatars';
  import { feedback, setRumble } from '../lib/feedback';
  import { focusFirst } from '../lib/nav';
  import { router } from '../lib/router.svelte';
  import { setMuted } from '../lib/sound';
  import type { AvatarKind, DeviceInfo, Profile } from '../lib/types';

  // Profil de la personne : image (et donc couleur de l'interface), nom, préférences.
  // Chaque changement est enregistré tout de suite (pas de bouton « Enregistrer »), comme sur la Switch.
  let { device }: { device: DeviceInfo } = $props();

  const p = $derived(router.profile!);
  const prefs = $derived(p.prefs ?? { sons: true, vibrations: true });
  let editing = $state(false);
  let draft = $state('');
  let input: HTMLInputElement | undefined = $state();
  let saved = $state<string | null>(null);

  let failed = $state<string | null>(null);
  async function save(next: Profile, what: string) {
    const before = router.profile!;
    router.profile = next;
    failed = null;
    try {
      await api.saveProfile(next);
      saved = what;
      setTimeout(() => saved === what && (saved = null), 1600);
    } catch {
      // Refusé ou serveur injoignable : on revient à l'état enregistré, sans faux « Enregistré ».
      router.profile = before;
      failed = what;
      feedback('err');
    }
  }

  function pick(kind: AvatarKind) {
    if (kind === p.avatarKind) return;
    void save(withAvatar(p, kind), 'avatar');
    feedback('arrive');
  }

  async function startEdit() {
    draft = p.name;
    editing = true;
    await tick();
    input?.focus();
    input?.select();
  }

  function commit() {
    const name = draft.trim().slice(0, 16);
    editing = false;
    if (name && name !== p.name) {
      void save({ ...p, name }, 'nom');
      feedback('saved');
    }
    requestAnimationFrame(() => document.querySelector<HTMLElement>('.field')?.focus());
  }

  function toggle(key: 'sons' | 'vibrations') {
    const next = { ...prefs, [key]: !prefs[key] };
    if (key === 'sons') setMuted(!next.sons);
    if (key === 'vibrations') setRumble(next.vibrations);
    void save({ ...p, prefs: next }, key);
    feedback('tab');
  }

  onMount(() => focusFirst());

  // L'ambiance prend la couleur du profil ; elle change en fondu avec l'image.
  const art = $derived({ motif: 'manoir' as const, from: p.accent, to: p.accentSoft });
</script>

<div class="screen">
  <Ambiance {art} x={16} key={p.avatarKind} />
  <main>
    <aside>
      {#key p.avatarKind}
        <div class="big pop"><Avatar profile={p} size={200} ring /></div>
      {/key}
      <h1>{p.name}</h1>
      <span class="device">{device.name}</span>
    </aside>

    <section class="settings">
      <div class="group">
        <h2>Image de profil {#if saved === 'avatar'}<span class="ok"><Icon name="check" size={16} color="var(--ok)" />Enregistré</span>{:else if failed === 'avatar'}<span class="ko">Non enregistré</span>{/if}</h2>
        <div class="avatars">
          {#each AVATARS as a (a.kind)}
            <button class="av focusable" class:chosen={a.kind === p.avatarKind} style:--c={a.accent} aria-label={a.label} aria-pressed={a.kind === p.avatarKind}
              data-autofocus={a.kind === p.avatarKind ? '' : undefined} data-sound="none" onclick={() => pick(a.kind)}>
              <span class="disc" style:background={a.accent}><AvatarArt kind={a.kind} /></span>
              {#if a.kind === p.avatarKind}<span class="tick"><Icon name="check" size={14} color="#0b0f14" /></span>{/if}
            </button>
          {/each}
        </div>
      </div>

      <div class="group">
        <h2>Nom {#if saved === 'nom'}<span class="ok"><Icon name="check" size={16} color="var(--ok)" />Enregistré</span>{:else if failed === 'nom'}<span class="ko">Non enregistré</span>{/if}</h2>
        {#if editing}
          <div class="field editing">
            <input bind:this={input} bind:value={draft} maxlength="16" aria-label="Nom du profil" onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); commit(); } else if (e.key === 'Escape') { editing = false; requestAnimationFrame(() => document.querySelector<HTMLElement>('.field')?.focus()); } }} onblur={() => editing && commit()} />
            <span class="hint">Entrée pour valider</span>
          </div>
        {:else}
          <button class="field focusable" onclick={startEdit}><span>{p.name}</span><span class="edit"><Icon name="pencil" size={18} />Modifier</span></button>
        {/if}
      </div>

      <div class="group">
        <h2>Préférences</h2>
        <button class="row focusable" role="switch" aria-checked={prefs.sons} data-sound="none" onclick={() => toggle('sons')}>
          <Icon name="volume" size={22} />Sons de l'interface<span class="switch" class:on={prefs.sons}><span></span></span>
        </button>
        <button class="row focusable" role="switch" aria-checked={prefs.vibrations} data-sound="none" onclick={() => toggle('vibrations')}>
          <Icon name="rumble" size={22} />Vibrations de la manette<span class="switch" class:on={prefs.vibrations}><span></span></span>
        </button>
      </div>

      <div class="group">
        <button class="row focusable" onclick={() => router.go({ name: 'qui-joue' })}><Icon name="people" size={22} />Changer de joueur<span class="chev">›</span></button>
      </div>
    </section>
  </main>
  <ButtonHints hints={[{ key: 'A', label: editing ? 'Valider' : 'Sélectionner' }, { key: 'B', label: 'Retour' }]} note="Enregistré automatiquement sur tous les appareils" />
</div>

<style>
  .screen { height: 100%; display: flex; flex-direction: column; position: relative; isolation: isolate; }
  main { flex: 1; display: flex; gap: 72px; padding: 56px 72px 0; min-height: 0; }
  aside { width: 260px; display: flex; flex-direction: column; align-items: center; gap: 12px; padding-top: 12px; }
  h1 { margin: 14px 0 0; font-size: 32px; font-weight: 600; }
  .device { font-size: 14px; color: rgba(255, 255, 255, 0.72); }
  .settings { flex: 1; display: flex; flex-direction: column; gap: 26px; max-width: 640px; }
  .group { display: flex; flex-direction: column; gap: 10px; }
  h2 { margin: 0; font-size: 14px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: rgba(255, 255, 255, 0.7); display: flex; align-items: center; gap: 12px; }
  .ok { display: inline-flex; align-items: center; gap: 4px; text-transform: none; letter-spacing: 0; color: var(--ok); font-weight: 500; animation: fadein 200ms ease-out; }
  @keyframes fadein { from { opacity: 0; } }
  .ko { text-transform: none; letter-spacing: 0; color: #ff9b9b; font-weight: 500; }

  .avatars { display: flex; gap: 16px; padding: 6px 0; }
  .av { all: unset; cursor: pointer; position: relative; border-radius: 50%; transition: transform var(--t-std) var(--ease-pop); }
  .disc { display: block; width: 76px; height: 76px; border-radius: 50%; overflow: hidden; box-shadow: 0 6px 14px rgba(0, 0, 0, 0.3); }
  .av.focusable:focus { transform: translateY(-4px) scale(1.08); }
  .av.focusable:focus .disc, .av.chosen .disc { box-shadow: 0 0 0 3px #0b0f14, 0 0 0 6px var(--c), 0 10px 20px rgba(0, 0, 0, 0.35); }
  .tick { position: absolute; right: -2px; bottom: -2px; width: 24px; height: 24px; border-radius: 50%; background: var(--c); display: flex; align-items: center; justify-content: center; box-shadow: 0 0 0 3px #0b0f14; animation: pop 320ms var(--ease-pop); }

  .field, .row {
    all: unset; cursor: pointer; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: 14px;
    padding: 14px 18px; border-radius: 12px; background: rgba(255, 255, 255, 0.08); font-size: 17px; color: var(--ink);
    transition: background-color var(--t-quick) ease-out, transform var(--t-quick) var(--ease-pop), box-shadow var(--t-quick) ease-out;
  }
  .field.focusable:focus, .row.focusable:focus { background: rgba(255, 255, 255, 0.14); box-shadow: inset 0 0 0 2px var(--accent); transform: translateX(4px); }
  .field .edit { margin-left: auto; display: inline-flex; align-items: center; gap: 6px; font-size: 14px; color: rgba(255, 255, 255, 0.7); }
  .field.editing { box-shadow: inset 0 0 0 2px var(--accent); background: rgba(255, 255, 255, 0.14); cursor: text; }
  input { all: unset; flex: 1; font: inherit; color: var(--ink); caret-color: var(--accent); }
  .hint { font-size: 13px; color: rgba(255, 255, 255, 0.6); }
  .chev { margin-left: auto; color: rgba(255, 255, 255, 0.6); font-size: 20px; }

  .switch { margin-left: auto; width: 46px; height: 26px; border-radius: 13px; background: rgba(255, 255, 255, 0.18); position: relative; transition: background-color var(--t-std) ease-out; }
  .switch span { position: absolute; top: 3px; left: 3px; width: 20px; height: 20px; border-radius: 50%; background: #fff; transition: transform var(--t-std) var(--ease-pop); box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3); }
  .switch.on { background: var(--accent); }
  .switch.on span { transform: translateX(20px); }

  @media (prefers-reduced-motion: no-preference) {
    /* Nouvelle image : le grand avatar arrive avec un rebond. */
    .pop { animation: pop 420ms var(--ease-pop); }
    @keyframes pop { from { transform: scale(0.82); opacity: 0.4; } to { transform: none; opacity: 1; } }
  }
  @media (prefers-reduced-motion: reduce) { .av.focusable:focus, .field.focusable:focus, .row.focusable:focus { transform: none; } }
</style>
