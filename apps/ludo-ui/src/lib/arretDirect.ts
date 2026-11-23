// Arrêt d'un jeu sans page d'Android (Shizuku, ArretDirect.kt, lot arret du 23/11) : textes de l'interface.

/** Raison lisible de l'absence d'arrêt direct (textes sans pronom), ou `null` s'il est disponible. */
export function raisonSansArretDirect(etat: string | undefined): string | null {
  switch (etat) {
    case undefined:
    case 'pret':
      return null;
    case 'absent':
      return 'Arrêt direct indisponible : Shizuku n’est pas installé.';
    case 'arrete':
      return 'Arrêt direct indisponible : Shizuku est arrêté. Brancher l’appareil au PC le relance.';
    case 'refuse':
      return 'Arrêt direct indisponible : accès à Shizuku non accordé. Brancher l’appareil au PC l’accorde.';
    case 'ancien':
      return 'Arrêt direct indisponible : version de Shizuku trop ancienne.';
    default:
      return 'Arrêt direct indisponible.';
  }
}
