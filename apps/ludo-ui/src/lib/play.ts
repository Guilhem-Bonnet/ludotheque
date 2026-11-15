// Lancer un jeu : préparation (profil, téléchargement, sauvegarde) puis conflit éventuel.

import { api } from './api';
import { router } from './router.svelte';
import type { Game } from './types';

export async function play(game: Game) {
  const profile = router.profile;
  if (!profile) return;
  const result = await api.prepare(profile.id, game.id);
  if ('conflict' in result) router.go({ name: 'conflit', conflict: result.conflict });
  else router.go({ name: 'passage', preparation: result.preparation });
}
