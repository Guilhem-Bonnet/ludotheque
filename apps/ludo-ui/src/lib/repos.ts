// État du repos de l'interface (Repos.svelte), lu par la navigation : l'appui qui réveille n'agit pas.
// Objet ordinaire (pas de rune) : lu au moment d'un événement, jamais suivi.
export const repos = {
  actif: false,
  /** Manette : appuis ignorés jusqu'à cette heure (le réveil vient d'avoir lieu). */
  avaleJusqua: 0,
  /** Le toucher / clic qui a réveillé n'a pas encore produit son `click` : il sera avalé. */
  avaleClic: false,
  /** Touche qui a réveillé : son `keyup` (Entrée, Espace = clic d'un bouton) sera avalé. */
  toucheReveil: '',
};

/** Manette (bureau) : durée pendant laquelle les appuis suivant un réveil sont ignorés. */
export const AVALE_MS = 350;

/** Vrai si une entrée doit être ignorée : repos en cours, ou geste de réveil pas encore terminé. */
export function entreeAvalee(): boolean {
  return repos.actif || Date.now() < repos.avaleJusqua;
}
