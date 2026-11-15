<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isDemo } from './lib/api';
  import { installNav } from './lib/nav';
  import { router } from './lib/router.svelte';
  import type { DeviceInfo, Profile } from './lib/types';
  import Accueil from './screens/Accueil.svelte';
  import Bibliotheque from './screens/Bibliotheque.svelte';
  import Conflit from './screens/Conflit.svelte';
  import FicheJeu from './screens/FicheJeu.svelte';
  import MisesAJour from './screens/MisesAJour.svelte';
  import Passage from './screens/Passage.svelte';
  import QuiJoue from './screens/QuiJoue.svelte';

  let device = $state<DeviceInfo | null>(null);

  // L'interface est dessinée pour 1280×800 (Steam Deck) et s'agrandit d'un bloc sur une TV (1920×1080 → ×1,35).
  let scale = $state(1);
  function rescale() {
    scale = Math.max(0.75, Math.min(window.innerWidth / 1280, window.innerHeight / 800));
  }

  /** Démonstration seulement : `?ecran=bibliotheque|fiche|maj|passage|conflit` ouvre un écran directement. */
  async function openDemoScreen(profiles: Profile[]) {
    const ecran = new URLSearchParams(location.search).get('ecran');
    if (!ecran) return;
    router.profile ??= profiles[0];
    const games = await api.library(router.profile.id);
    const byId = (id: string) => games.find((g) => g.id === id) ?? games[0];
    if (ecran === 'bibliotheque') router.go({ name: 'bibliotheque' });
    else if (ecran === 'maj') router.go({ name: 'mises-a-jour' });
    else if (ecran === 'fiche') router.go({ name: 'fiche', game: byId('switch-lm3') });
    else if (ecran === 'passage' || ecran === 'conflit') {
      const r = await api.prepare(router.profile.id, ecran === 'conflit' ? 'switch-botw' : 'nds-diamant');
      if ('conflict' in r) router.go({ name: 'conflit', conflict: r.conflict });
      else router.go({ name: 'passage', preparation: r.preparation });
    }
  }

  onMount(() => {
    installNav();
    rescale();
    window.addEventListener('resize', rescale);
    const onBack = () => router.back();
    window.addEventListener('ludo:back', onBack);

    Promise.all([api.device(), api.profiles()]).then(([d, profiles]) => {
      device = d;
      const mine = profiles.find((p) => p.id === d.defaultProfile);
      if (mine) {
        router.profile = mine;
        router.reset({ name: 'accueil' });
      } else {
        router.reset({ name: 'qui-joue' });
      }
      if (isDemo) openDemoScreen(profiles);
    });
    return () => window.removeEventListener('ludo:back', onBack);
  });

  const p = $derived(router.profile);
</script>

<div
  class="app"
  style:zoom={scale}
  style:--z={scale}
  style:--accent={p?.accent ?? '#4cc3ff'}
  style:--accent-soft={p?.accentSoft ?? '#1c3546'}
  style:--accent-ink={p?.accentInk ?? '#8ad8ff'}
  data-demo={isDemo ? '' : undefined}
>
  {#if device}
    {#key router.screen}
      {#if router.screen.name === 'qui-joue'}
        <QuiJoue {device} />
      {:else if router.screen.name === 'accueil' && router.profile}
        <Accueil {device} />
      {:else if router.screen.name === 'bibliotheque'}
        <Bibliotheque {device} />
      {:else if router.screen.name === 'fiche'}
        <FicheJeu game={router.screen.game} />
      {:else if router.screen.name === 'mises-a-jour'}
        <MisesAJour />
      {:else if router.screen.name === 'passage'}
        <Passage preparation={router.screen.preparation} />
      {:else if router.screen.name === 'conflit'}
        <Conflit conflict={router.screen.conflict} />
      {/if}
    {/key}
  {/if}
</div>

<style>
  .app { height: calc(100vh / var(--z, 1)); }
</style>
