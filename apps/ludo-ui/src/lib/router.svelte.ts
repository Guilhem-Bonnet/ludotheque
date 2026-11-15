// État de navigation entre écrans, avec historique pour le bouton B.

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
  private stack: Screen[] = [];

  go(next: Screen) {
    this.stack.push(this.screen);
    this.screen = next;
  }

  /** Remplace l'écran courant sans l'empiler (passage → conflit, par exemple). */
  replace(next: Screen) {
    this.screen = next;
  }

  back() {
    const prev = this.stack.pop();
    if (prev) this.screen = prev;
  }

  reset(to: Screen) {
    this.stack = [];
    this.screen = to;
  }
}

export const router = new Router();
