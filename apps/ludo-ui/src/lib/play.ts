// Lancer un jeu : préparation (profil, téléchargement, sauvegarde) puis conflit éventuel.
//
// Moment phare : si la tuile d'origine est connue, on mémorise sa position pour que l'écran
// suivant la fasse « voler » jusqu'à sa nouvelle place (FLIP), et on laisse 180 ms à la mise en
// scène de l'accueil (les autres tuiles s'estompent, celle-ci se soulève) avant de changer d'écran.

import { api } from './api';
import { reducedMotion } from './motion';
import { router } from './router.svelte';
import type { Game } from './types';

export async function play(game: Game, from?: HTMLElement) {
  const profile = router.profile;
  if (!profile) return;
  router.heroGame = game.id;
  router.heroRect = from ? from.getBoundingClientRect() : null;
  router.enter = from && !reducedMotion() ? 'hero' : 'normal';
  const [result] = await Promise.all([
    api.prepare(profile.id, game.id),
    new Promise((r) => setTimeout(r, from && !reducedMotion() ? 180 : 0)),
  ]);
  if ('conflict' in result) router.go({ name: 'conflit', conflict: result.conflict });
  else router.go({ name: 'passage', preparation: result.preparation });
}
