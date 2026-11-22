// Menu en jeu (lot menu-jeu du 22/11) : jeu ouvert pendant que la Ludothèque est devant.
//
// Android : pendant une partie, la Ludothèque revient devant (bouton Mode via le service d'accessibilité
// facultatif MenuJeuService.kt, raccourci AYN « Lancer une appli », HOME puis Cocoon) ; l'accueil met
// alors le jeu en tête (carte « En cours » : Reprendre, Fermer le jeu). HOME n'est jamais capté.
// L'état est relu quand la page redevient visible (`visibilitychange`) : un événement, pas une
// scrutation. Bureau : la partie est portée par l'écran « En jeu » (Passage) ; seules `reprendre` et
// `fermer` servent (réduire la fenêtre, terminer l'émulateur).

import { isAndroid, isDemo } from './api';

export interface JeuEnCours {
  game: string;
  paquet?: string;
  /** Heure du lancement (ms depuis l'époque Unix). */
  depuisMs: number;
}

export interface EtatMenu {
  serviceActif: boolean;
  serviceRequis: boolean;
  jeu?: JeuEnCours;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/** Démonstration (navigateur) : `?enCours=<id du jeu>` simule une partie ouverte depuis 42 min. */
function demoEtat(): EtatMenu {
  const id = typeof location !== 'undefined' ? new URLSearchParams(location.search).get('enCours') : null;
  return { serviceActif: true, serviceRequis: true, jeu: id ? { game: id, depuisMs: Date.now() - 42 * 60_000 } : undefined };
}

/** Durée de session lisible : « 42 min », « 1 h 05 ». */
export function dureeSession(depuisMs: number, maintenant = Date.now()): string {
  const min = Math.max(0, Math.floor((maintenant - depuisMs) / 60_000));
  if (min < 1) return 'moins d’une minute';
  if (min < 60) return `${min} min`;
  return `${Math.floor(min / 60)} h ${String(min % 60).padStart(2, '0')}`;
}

class EnCoursStore {
  etat = $state<EtatMenu>({ serviceActif: false, serviceRequis: false });
  /** Le jeu ouvert vient de rendre la main à la Ludothèque (l'accueil met sa carte en avant). */
  retour = $state(0);
  /** Confirmation affichée : fermer le jeu ouvert, puis éventuellement lancer `ensuite`. */
  confirmation = $state<{ ensuite?: () => void; titreEnsuite?: string } | null>(null);
  occupe = $state(false);
  /** Le focus est dans la carte « En cours » (l'accueil adapte sa barre de consignes). */
  carteFocalisee = $state(false);
  private fermetureDemandee = false;
  /** Android : lancement à faire au retour de la page « Infos de l'appli » (« Fermer et lancer »). */
  apresRetour: (() => void) | null = null;
  /** Titre du jeu ouvert (fourni par l'accueil, qui connaît la bibliothèque). */
  titre = $state('Le jeu en cours');

  get jeu(): JeuEnCours | undefined {
    return this.etat.jeu;
  }

  async relire(retourDuJeu = false): Promise<EtatMenu> {
    if (isDemo) {
      this.etat = demoEtat();
      return this.etat;
    }
    if (!isAndroid) return this.etat;
    try {
      this.etat = await invoke<EtatMenu>('jeuEnCours');
      // Retour au premier plan avec un jeu ouvert : l'accueil (où l'app revient toujours après un
      // lancement, Passage.svelte) remet le focus sur « Reprendre ». Aucun changement d'écran ici : un
      // retour des réglages d'Android ou un déverrouillage laisse l'écran consulté tel quel.
      if (this.etat.jeu && retourDuJeu) this.retour++;
    } catch {
      // Pont indisponible (ancienne version du plugin) : pas de menu en jeu, rien d'autre ne change.
    }
    return this.etat;
  }

  async reprendre(): Promise<void> {
    if (isDemo) return;
    await invoke('reprendreJeu');
  }

  /** Ferme le jeu ouvert (Android : après le délai de garde ; bureau : SIGTERM à l'émulateur). */
  async fermer(): Promise<void> {
    this.occupe = true;
    try {
      this.fermetureDemandee = true;
      if (!isDemo) await invoke('fermerJeu');
      this.etat = { ...this.etat, jeu: undefined };
    } finally {
      this.occupe = false;
    }
  }

  /** Bureau : la fin de partie qui suit vient-elle de « Fermer le jeu » ? (lu une fois). */
  consommerFermeture(): boolean {
    const oui = this.fermetureDemandee;
    this.fermetureDemandee = false;
    return oui;
  }

  async reglages(): Promise<void> {
    if (!isDemo) await invoke('menuJeuReglages');
  }

  /** Démarre l'écoute : état relu au démarrage et chaque fois que la Ludothèque redevient visible. */
  ecouter(): () => void {
    const onVisible = () => {
      if (document.visibilityState !== 'visible') return;
      const suite = this.apresRetour;
      this.apresRetour = null;
      void this.relire(true).then(() => suite?.());
    };
    document.addEventListener('visibilitychange', onVisible);
    void this.relire();
    return () => document.removeEventListener('visibilitychange', onVisible);
  }
}

export const enCours = new EnCoursStore();

// Écoute démarrée une fois, au chargement du module (l'accueil l'importe dès le démarrage).
if (typeof document !== 'undefined' && (isAndroid || isDemo)) enCours.ecouter();
