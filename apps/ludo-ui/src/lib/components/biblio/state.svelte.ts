// État de la bibliothèque gardé hors de l'écran : au retour d'une fiche (l'écran est recréé), onglet,
// filtres, tri, recherche, jeu choisi et position de défilement sont retrouvés tels quels.
// Propre à un profil : un autre profil repart de zéro.

export type Avail = 'installes' | 'flux' | 'serveur';
export type Sort = 'recents' | 'az' | 'temps';

export const SORTS: { id: Sort; label: string }[] = [
  { id: 'recents', label: 'Récents' },
  { id: 'az', label: 'A → Z' },
  { id: 'temps', label: 'Temps de jeu' },
];

class BiblioState {
  profile = '';
  tab = $state(0);
  /** Appareil regardé : « ici » ou l'identifiant d'un autre appareil du foyer. */
  where = $state('ici');
  avail = $state<Avail | null>(null);
  /** Jeux à synchroniser (conflit, envoi en attente, nouvelle sauvegarde). */
  sync = $state(false);
  /** Jeux joués par ce profil (une sauvegarde existe). */
  played = $state(false);
  sort = $state<Sort>('recents');
  query = $state('');
  /** Jeu qui avait le focus en quittant l'écran. */
  focus: string | null = null;
  scroll = 0;

  /** À l'ouverture de l'écran : remet à zéro si le profil a changé. */
  enter(profile: string) {
    if (profile === this.profile) return;
    this.profile = profile;
    this.reset();
    this.where = 'ici';
    this.focus = null;
    this.scroll = 0;
  }

  /** Aucun filtre, tri ni recherche : vue d'accueil (À la une + rangées) sur l'onglet « Tout ». */
  get pristine() {
    return !this.avail && !this.sync && !this.played && this.sort === 'recents' && !this.query.trim();
  }

  reset() {
    this.tab = 0;
    this.avail = null;
    this.sync = false;
    this.played = false;
    this.sort = 'recents';
    this.query = '';
  }
}

export const biblio = new BiblioState();
