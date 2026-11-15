// État de navigation entre écrans, avec historique pour le bouton B.
// Le changement d'écran est un fondu doux unique, joué par App.svelte (voir screenIn / screenOut).

import { feedback } from './feedback';
import type { Conflict, Game, Preparation, Profile } from './types';

export type Screen =
  | { name: 'qui-joue' }
  | { name: 'accueil' }
  | { name: 'bibliotheque' }
  | { name: 'fiche'; game: Game }
  | { name: 'mises-a-jour' }
  | { name: 'passage'; preparation: Preparation }
  | { name: 'conflit'; conflict: Conflict }
  | { name: 'profil' };

class Router {
  screen = $state<Screen>({ name: 'accueil' });
  profile = $state<Profile | null>(null);
  /** Jeu dont la tuile s'envole vers l'écran suivant (élément partagé de la transition). */
  heroGame = $state<string | null>(null);
  /** Position à l'écran de la tuile d'origine, pour la faire voler jusqu'à l'écran suivant (FLIP). */
  heroRect: DOMRect | null = null;
  /** Jeu en cours de lancement depuis l'accueil (mise en scène : les autres tuiles s'estompent). */
  launching = $state<string | null>(null);
  /** Entrée d'écran : 'hero' = fondu court, l'écran suivant fait voler la tuile lui-même. */
  enter: 'normal' | 'hero' = 'normal';
  private stack: Screen[] = [];

  /** Sens du dernier changement d'écran (la transition d'App en tient compte). */
  dir = $state<'forward' | 'back'>('forward');

  private show(next: Screen, dir: 'forward' | 'back') {
    this.dir = dir;
    this.screen = next;
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
