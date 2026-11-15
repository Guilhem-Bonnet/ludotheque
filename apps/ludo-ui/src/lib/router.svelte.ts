// État de navigation entre écrans, avec historique pour le bouton B.
// Chaque changement d'écran passe par une View Transition (aller vers la gauche, retour vers la droite).

import { tick } from 'svelte';
import { feedback } from './feedback';
import { withTransition } from './motion';
import type { Conflict, Game, Preparation, Profile } from './types';

export type Screen =
  | { name: 'qui-joue' }
  | { name: 'accueil' }
  | { name: 'bibliotheque' }
  | { name: 'fiche'; game: Game }
  | { name: 'mises-a-jour' }
  | { name: 'passage'; preparation: Preparation }
  | { name: 'conflit'; conflict: Conflict };

class Router {
  screen = $state<Screen>({ name: 'accueil' });
  profile = $state<Profile | null>(null);
  /** Jeu dont la tuile s'envole vers l'écran suivant (élément partagé de la transition). */
  heroGame = $state<string | null>(null);
  private stack: Screen[] = [];

  private show(next: Screen, dir: 'forward' | 'back') {
    withTransition(dir, async () => {
      this.screen = next;
      await tick();
    });
  }

  go(next: Screen) {
    this.stack.push(this.screen);
    this.show(next, 'forward');
  }

  /** Remplace l'écran courant sans l'empiler. */
  replace(next: Screen) {
    this.show(next, 'forward');
  }

  /** Retour : son « retour » s'il y a un écran précédent, sinon rien (on est déjà à l'accueil). */
  back() {
    const prev = this.stack.pop();
    if (!prev) return false;
    feedback('back');
    this.show(prev, 'back');
    return true;
  }

  /** Premier affichage : sans transition (il n'y a pas d'écran à quitter). */
  start(to: Screen) {
    this.stack = [];
    this.screen = to;
  }

  reset(to: Screen) {
    this.stack = [];
    this.show(to, 'forward');
  }
}

export const router = new Router();
