// Lancer un jeu : préparation (profil, téléchargement, sauvegarde) puis conflit éventuel.
//
// Moment phare : si la tuile d'origine est connue, on mémorise sa position pour que l'écran
// suivant la fasse « voler » jusqu'à sa nouvelle place (FLIP), et on laisse 180 ms à la mise en
// scène de l'accueil (les autres tuiles s'estompent, celle-ci se soulève) avant de changer d'écran.

import { api, isAndroid, isDemo } from './api';
import { reducedMotion } from './motion';
import { router } from './router.svelte';
import type { Game, LaunchMode, Preparation } from './types';

/**
 * Étapes affichées pour un lancement réel (hors démonstration). Toutes à venir : l'écran de passage
 * les fait avancer d'après les événements de l'app, dans l'ordre de `LaunchStep`.
 */
export function realPreparation(game: Game, profile: string, profileName?: string, mode?: LaunchMode): Preparation {
  if (isAndroid) {
    // Android : rien à télécharger ni à envoyer ici (le PC synchronise la sauvegarde par adb).
    const stream = mode === 'flux' || (mode === undefined && game.install === 'non_installe' && !!game.stream);
    return {
      game,
      profile,
      mode,
      steps: [
        { label: `Profil : ${profileName ?? profile}`, state: 'a_venir' },
        { label: stream ? 'Flux depuis le PC' : 'Jeu sur l\u2019appareil', state: 'a_venir' },
        { label: 'Sauvegarde : synchronisée par le PC', state: 'a_venir' },
        { label: 'Lancement', state: 'a_venir' },
      ],
    };
  }
  const download = game.install === 'non_installe';
  return {
    game,
    profile,
    steps: [
      { label: `Profil : ${profileName ?? profile}`, state: 'a_venir' },
      download ? { label: 'Téléchargement…', state: 'a_venir', progress: 0 } : { label: 'Jeu installé', state: 'a_venir' },
      { label: 'Sauvegarde : synchronisation', state: 'a_venir' },
      { label: 'Lancement', state: 'a_venir' },
      { label: 'En jeu', state: 'a_venir' },
      { label: 'Envoi de la sauvegarde', state: 'a_venir' },
    ],
  };
}

export async function play(game: Game, from?: HTMLElement, mode?: LaunchMode) {
  const profile = router.profile;
  if (!profile) return;
  router.heroGame = game.id;
  router.heroRect = from ? from.getBoundingClientRect() : null;
  router.enter = from && !reducedMotion() ? 'hero' : 'normal';
  if (!isDemo) {
    // Lancement réel : le conflit éventuel est détecté par `launchGame` (écran de passage).
    await new Promise((r) => setTimeout(r, from && !reducedMotion() ? 180 : 0));
    router.go({ name: 'passage', preparation: realPreparation(game, profile.id, profile.name, mode) });
    return;
  }
  const [result] = await Promise.all([
    api.prepare(profile.id, game.id),
    new Promise((r) => setTimeout(r, from && !reducedMotion() ? 180 : 0)),
  ]);
  if ('conflict' in result) router.go({ name: 'conflit', conflict: result.conflict });
  else router.go({ name: 'passage', preparation: result.preparation });
}
